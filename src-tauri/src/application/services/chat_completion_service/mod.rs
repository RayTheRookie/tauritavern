use crate::application::dto::{ChatChunkPayload, ChatMessage, WorldEntry};
use crate::application::services::memory_service;
use crate::infrastructure::apis::LlmHttpClient;
use crate::infrastructure::credentials::CredentialService;
use crate::infrastructure::database::MessageRow;
use crate::infrastructure::fs;
use crate::AppState;
use chrono::Utc;
use futures::StreamExt;
use tauri::Emitter;
use uuid::Uuid;

pub async fn handle_chat(
    state: &AppState,
    cartridge_id: &str,
    chat_id: &str,
    user_message: &str,
    window: &tauri::Window,
) -> Result<(), String> {
    let cartridge = state
        .repo
        .get_cartridge(cartridge_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cartridge not found".to_string())?;

    let dir = std::path::PathBuf::from(&cartridge.directory_path);

    // Load preset
    let preset = fs::load_preset(&dir)?;

    // Save user message
    let now = Utc::now().to_rfc3339();
    let user_msg_id = Uuid::new_v4().to_string();
    state
        .repo
        .insert_message(&MessageRow {
            id: user_msg_id.clone(),
            chat_id: chat_id.to_string(),
            role: "user".to_string(),
            content: user_message.to_string(),
            created_at: now.clone(),
        })
        .await
        .map_err(|e| e.to_string())?;

    // Update chat timestamp
    state
        .repo
        .touch_chat(chat_id, &now)
        .await
        .map_err(|e| e.to_string())?;

    // Load history
    let history = state
        .repo
        .get_messages_by_chat(chat_id, 50)
        .await
        .map_err(|e| e.to_string())?;

    // Load and match world info
    let world_entries = fs::load_world_info(&dir).unwrap_or_default();
    let matched_entries = match_world_entries(&world_entries, user_message);

    // Build messages: system prompt + world entries + history (user message already saved)
    let mut messages: Vec<ChatMessage> = Vec::new();
    messages.push(ChatMessage {
        role: "system".to_string(),
        content: preset.system_prompt.clone(),
    });
    for entry in &matched_entries {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: entry.content.clone(),
        });
    }
    for msg in &history {
        messages.push(ChatMessage {
            role: msg.role.clone(),
            content: msg.content.clone(),
        });
    }

    // Truncate using context_window_size (separate from max_tokens which controls output)
    let context_budget = preset.context_window_size.unwrap_or(8192);
    let model_name = preset.model.as_deref().unwrap_or("gpt-4");
    messages = memory_service::truncate_messages(&messages, context_budget, model_name);

    // Get API key from OS credential store
    let provider = preset.provider.clone().unwrap_or_else(|| "openai".to_string());
    let api_key = CredentialService::get(&provider)?
        .ok_or_else(|| format!("API key not set for provider '{}'", provider))?;

    // Stream from LLM
    let client = LlmHttpClient::new();
    let mut stream = client.stream_chat(&preset, &messages, &api_key);

    let mut full_response = String::new();
    let response_msg_id = Uuid::new_v4().to_string();
    let response_ts = Utc::now().to_rfc3339();

    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(text) => {
                full_response.push_str(&text);
                let _ = window.emit(
                    "chat-chunk",
                    ChatChunkPayload {
                        chat_id: chat_id.to_string(),
                        content: text,
                        done: false,
                        error: false,
                    },
                );
            }
            Err(e) => {
                let _ = window.emit(
                    "chat-chunk",
                    ChatChunkPayload {
                        chat_id: chat_id.to_string(),
                        content: e.clone(),
                        done: true,
                        error: true,
                    },
                );
                return Err(e);
            }
        }
    }

    // Save assistant response
    state
        .repo
        .insert_message(&MessageRow {
            id: response_msg_id,
            chat_id: chat_id.to_string(),
            role: "assistant".to_string(),
            content: full_response,
            created_at: response_ts,
        })
        .await
        .map_err(|e| e.to_string())?;

    // Emit done
    let _ = window.emit(
        "chat-chunk",
        ChatChunkPayload {
            chat_id: chat_id.to_string(),
            content: String::new(),
            done: true,
            error: false,
        },
    );

    Ok(())
}

fn match_world_entries(entries: &[WorldEntry], message: &str) -> Vec<WorldEntry> {
    let lower_msg = message.to_lowercase();
    entries
        .iter()
        .filter(|entry| {
            entry
                .keys
                .iter()
                .any(|key| lower_msg.contains(&key.to_lowercase()))
        })
        .cloned()
        .collect()
}
