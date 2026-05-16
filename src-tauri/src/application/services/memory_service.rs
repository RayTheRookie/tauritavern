use crate::application::dto::ChatMessage;

pub fn count_tokens(text: &str, model: &str) -> usize {
    let bpe = match tiktoken_rs::get_bpe_from_model(model) {
        Ok(bpe) => bpe,
        Err(_) => {
            // Fall back to cl100k_base for unknown models
            tiktoken_rs::cl100k_base().unwrap()
        }
    };
    bpe.encode_with_special_tokens(text).len()
}

pub fn truncate_messages(
    messages: &[ChatMessage],
    max_tokens: usize,
    model: &str,
) -> Vec<ChatMessage> {
    if messages.is_empty() {
        return vec![];
    }

    // Reserve some tokens for the response (20% of budget)
    let budget = (max_tokens as f64 * 0.8) as usize;

    // Always keep system messages
    let system_msgs: Vec<&ChatMessage> = messages.iter().filter(|m| m.role == "system").collect();
    let conversation: Vec<&ChatMessage> =
        messages.iter().filter(|m| m.role != "system").collect();

    let system_tokens: usize = system_msgs
        .iter()
        .map(|m| count_tokens(&m.content, model))
        .sum();

    let mut available = budget.saturating_sub(system_tokens);

    // Take messages from the end (most recent), preserving pairs
    let mut kept: Vec<ChatMessage> = vec![];

    for msg in conversation.iter().rev() {
        let tokens = count_tokens(&msg.content, model);
        if tokens <= available {
            kept.push((*msg).clone());
            available = available.saturating_sub(tokens);
        } else {
            break;
        }
    }

    kept.reverse();

    // Prepend system messages
    let mut result: Vec<ChatMessage> = system_msgs.into_iter().cloned().collect();
    result.extend(kept);

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_keeps_system_prompt() {
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "You are a helpful assistant.".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "Hi".to_string(),
            },
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
}
