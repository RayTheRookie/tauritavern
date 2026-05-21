use crate::application::dto::ChatMessage;

/// Token counting strategies for different model families.
pub fn count_tokens(text: &str, model: &str) -> usize {
    match TokenCountingStrategy::for_model(model) {
        TokenCountingStrategy::Tiktoken(model_name) => tiktoken_count(text, model_name),
        TokenCountingStrategy::ConservativeBpeLike => conservative_bpe_like_count(text),
        TokenCountingStrategy::RedundantEstimate => redundant_estimate_count(text),
    }
}

enum TokenCountingStrategy<'a> {
    Tiktoken(&'a str),
    ConservativeBpeLike,
    RedundantEstimate,
}

impl<'a> TokenCountingStrategy<'a> {
    fn for_model(model: &'a str) -> Self {
        let model_lower = model.to_lowercase();

        if is_openai_model(&model_lower) {
            return Self::Tiktoken(model);
        }

        if is_bpe_family_model(&model_lower) {
            return Self::ConservativeBpeLike;
        }

        Self::RedundantEstimate
    }
}

fn is_openai_model(model_lower: &str) -> bool {
    if model_lower.starts_with("gpt-")
        || model_lower.starts_with("o1")
        || model_lower.starts_with("o3")
        || model_lower.starts_with("o4")
        || model_lower.contains("text-embedding")
    {
        return true;
    }

    false
}

fn is_bpe_family_model(model_lower: &str) -> bool {
    model_lower.starts_with("llama")
        || model_lower.contains("llama-")
        || model_lower.starts_with("qwen")
        || model_lower.contains("qwen-")
        || model_lower.starts_with("mistral")
        || model_lower.contains("mistral-")
}

fn tiktoken_count(text: &str, model: &str) -> usize {
    let bpe = match tiktoken_rs::get_bpe_from_model(model) {
        Ok(bpe) => bpe,
        Err(_) => tiktoken_rs::cl100k_base().unwrap(),
    };
    bpe.encode_with_special_tokens(text).len()
}

fn redundant_estimate_count(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }

    // Claude and unknown model families deliberately use a safety margin until
    // provider-specific tokenizers can be added without increasing build risk.
    let chars = text.chars().count();
    let bytes = text.len();
    ceil_div(chars, 2).max(ceil_div(bytes, 4)).saturating_add(8)
}

fn conservative_bpe_like_count(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }

    // TODO: Add an optional tokenizer-backed implementation behind a feature
    // flag once Windows build impact is verified. This facade keeps the call
    // site stable for Llama/Qwen/Mistral families.
    let mut tokens = 0usize;
    let mut ascii_run_len = 0usize;

    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            ascii_run_len += 1;
            continue;
        }

        tokens += ascii_run_tokens(ascii_run_len);
        ascii_run_len = 0;

        if ch.is_whitespace() {
            continue;
        }

        if is_cjk_char(ch) {
            tokens += 1;
        } else if ch.is_ascii_punctuation() {
            tokens += 1;
        } else {
            tokens += ceil_div(ch.len_utf8(), 2);
        }
    }

    tokens += ascii_run_tokens(ascii_run_len);
    ceil_div(tokens.saturating_mul(6), 5).saturating_add(4)
}

fn ascii_run_tokens(run_len: usize) -> usize {
    if run_len == 0 {
        0
    } else {
        ceil_div(run_len, 4)
    }
}

fn ceil_div(value: usize, divisor: usize) -> usize {
    value.div_ceil(divisor)
}

fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
            | 0x2CEB0..=0x2EBEF
    )
}

#[allow(dead_code)]
/// Truncates conversation history to fit within `budget_tokens`.
///
/// Guarantees:
/// - System messages are always retained.
/// - The latest message (user's new message) is always included, even if over budget.
/// - Pairs (user → assistant) are kept or dropped together — no orphaned assistants.
/// - The final sequence always starts with `user` (or `system`) after system messages.
pub fn truncate_messages(
    messages: &[ChatMessage],
    budget_tokens: usize,
    model: &str,
) -> Vec<ChatMessage> {
    if messages.is_empty() {
        return vec![];
    }

    let system_msgs: Vec<&ChatMessage> = messages.iter().filter(|m| m.role == "system").collect();
    let conversation: Vec<&ChatMessage> = messages.iter().filter(|m| m.role != "system").collect();

    let system_tokens: usize = system_msgs
        .iter()
        .map(|m| count_tokens(&m.content, model))
        .sum();

    let mut available = budget_tokens.saturating_sub(system_tokens);
    let mut kept: Vec<ChatMessage> = Vec::new();

    let len = conversation.len();
    if len == 0 {
        let result: Vec<ChatMessage> = system_msgs.into_iter().cloned().collect();
        return result;
    }

    // Always keep the latest message (the just-sent user message)
    let last = conversation[len - 1];
    let last_tokens = count_tokens(&last.content, model);
    kept.push((*last).clone());
    available = available.saturating_sub(last_tokens);

    // Walk backwards in (user, assistant) pairs, keeping or dropping both together
    let mut i = len.saturating_sub(1); // skip the last message, already kept
    while i >= 2 {
        let assistant_candidate = conversation[i - 1];
        let user_candidate = conversation[i - 2];

        // We only handle proper (user, assistant) pairs
        if user_candidate.role != "user" || assistant_candidate.role != "assistant" {
            i -= 1;
            continue;
        }

        let pair_tokens = count_tokens(&user_candidate.content, model)
            + count_tokens(&assistant_candidate.content, model);

        if pair_tokens <= available {
            kept.push(user_candidate.clone());
            kept.push(assistant_candidate.clone());
            available = available.saturating_sub(pair_tokens);
            i -= 2;
        } else {
            break;
        }
    }

    // Reverse to chronological order
    kept.reverse();

    // Safety: ensure first non-system message is user
    if let Some(first) = kept.first() {
        if first.role != "user" {
            if let Some(pos) = kept.iter().position(|m| m.role == "user") {
                kept.drain(..pos);
            } else {
                kept.clear();
            }
        }
    }

    let mut result: Vec<ChatMessage> = system_msgs.into_iter().cloned().collect();
    result.extend(kept);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.to_string(),
            content: content.to_string(),
        }
    }

    #[test]
    fn test_truncate_keeps_system_prompt() {
        let messages = vec![
            msg("system", "You are a helpful assistant."),
            msg("user", "Hi"),
        ];
        let result = truncate_messages(&messages, 4096, "gpt-4");
        assert_eq!(result[0].role, "system");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_empty_messages() {
        let result = truncate_messages(&[], 4096, "gpt-4");
        assert!(result.is_empty());
    }

    #[test]
    fn test_starts_with_user_after_system() {
        let messages = vec![
            msg("system", "System prompt here."),
            msg("user", "First question"),
            msg("assistant", "First answer"),
            msg("user", "Second question"),
            msg("assistant", "Second answer"),
            msg("user", "Third question"),
        ];
        let result = truncate_messages(&messages, 4096, "gpt-4");
        let non_system: Vec<_> = result.iter().filter(|m| m.role != "system").collect();
        assert_eq!(non_system[0].role, "user");
    }

    #[test]
    fn test_latest_user_always_kept() {
        let messages = vec![
            msg("system", "You are helpful."),
            msg("user", "Hi"),
            msg("assistant", "Hello"),
            msg("user", "Latest message"),
        ];
        let result = truncate_messages(&messages, 50, "gpt-4");
        let non_system: Vec<_> = result.iter().filter(|m| m.role != "system").collect();
        assert!(non_system.iter().any(|m| m.content == "Latest message"));
    }

    #[test]
    fn test_no_orphaned_assistant() {
        let messages = vec![
            msg("system", "You are helpful."),
            msg("user", "Q1"),
            msg("assistant", "A1"),
            msg("user", &"x".repeat(5_000)), // very long, will be dropped with its assistant
            msg("assistant", "A2"),
            msg("user", "Q3"),
        ];
        let result = truncate_messages(&messages, 100, "gpt-4");
        let non_system: Vec<_> = result.iter().filter(|m| m.role != "system").collect();
        // First non-system message should be user
        assert_eq!(non_system[0].role, "user");
        // "A2" should NOT appear without its user "x...x"
        let has_orphaned = non_system
            .windows(2)
            .any(|w| w[0].role == "assistant" && w[1].role == "assistant");
        assert!(!has_orphaned);
    }

    #[test]
    fn test_claude_model_uses_redundant_estimate() {
        let tokens = count_tokens("Hello world", "claude-sonnet-4-6");
        assert!(tokens >= 13);
    }

    #[test]
    fn test_gpt_model_uses_tiktoken() {
        let tokens = count_tokens("Hello world", "gpt-4o");
        assert!(tokens > 0 && tokens <= 20);
    }

    #[test]
    fn test_llama_model_uses_conservative_bpe_like_fallback() {
        let text = "Hello world, this is a local Llama memory test.";
        let tokens = count_tokens(text, "llama-3.1-8b");

        assert!(tokens > 8);
        assert!(tokens < text.chars().count());
    }

    #[test]
    fn test_qwen_and_mistral_use_conservative_bpe_like_fallback() {
        let text = "Memory retrieval with mixed 中文 context.";

        assert_eq!(
            count_tokens(text, "qwen2.5-32b"),
            count_tokens(text, "mistral-large-latest")
        );
    }

    #[test]
    fn test_mixed_chinese_long_text_is_counted_conservatively() {
        let text = "角色记忆包含中文、English names, numbers 12345, and punctuation! ".repeat(120);

        let gpt_tokens = count_tokens(&text, "gpt-4o");
        let claude_tokens = count_tokens(&text, "claude-3-5-sonnet");
        let llama_tokens = count_tokens(&text, "llama-3.1-70b");
        let unknown_tokens = count_tokens(&text, "some-new-provider-model");

        assert!(gpt_tokens > 0);
        assert!(claude_tokens > gpt_tokens / 2);
        assert!(llama_tokens > gpt_tokens / 2);
        assert!(unknown_tokens >= claude_tokens);
    }
}
