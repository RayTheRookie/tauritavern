use crate::application::dto::ChatMessage;

/// Token counting strategies for different model families.
pub fn count_tokens(text: &str, model: &str) -> usize {
    let model_lower = model.to_lowercase();

    if model_lower.starts_with("gpt-")
        || model_lower.starts_with("o1")
        || model_lower.starts_with("o3")
        || model_lower.starts_with("o4")
        || model_lower.contains("text-embedding")
    {
        return tiktoken_count(text, &model_lower);
    }

    if model_lower.contains("claude") {
        return char_based_count(text);
    }

    tiktoken_count(text, "gpt-4")
}

fn tiktoken_count(text: &str, model: &str) -> usize {
    let bpe = match tiktoken_rs::get_bpe_from_model(model) {
        Ok(bpe) => bpe,
        Err(_) => tiktoken_rs::cl100k_base().unwrap(),
    };
    bpe.encode_with_special_tokens(text).len()
}

fn char_based_count(text: &str) -> usize {
    (text.chars().count() as f64 / 3.0).ceil() as usize
}

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

    let system_msgs: Vec<&ChatMessage> =
        messages.iter().filter(|m| m.role == "system").collect();
    let conversation: Vec<&ChatMessage> =
        messages.iter().filter(|m| m.role != "system").collect();

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
        let has_orphaned = non_system.windows(2).any(|w| {
            w[0].role == "assistant" && w[1].role == "assistant"
        });
        assert!(!has_orphaned);
    }

    #[test]
    fn test_char_count_fallback() {
        let tokens = count_tokens("Hello world", "claude-sonnet-4-6");
        assert!(tokens > 0);
    }

    #[test]
    fn test_gpt_model_uses_tiktoken() {
        let tokens = count_tokens("Hello world", "gpt-4o");
        assert!(tokens > 0 && tokens <= 20);
    }
}
