use crate::application::dto::{ChatChunkPayload, PromptDryRunResult};
use crate::application::services::prompt_engine;
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

    let rendered = prompt_engine::render_prompt(
        &state.repo,
        cartridge_id,
        chat_id,
        &dir,
        &preset,
        &cartridge.name,
        user_message,
        None,
    )
    .await?;
    let messages = rendered.messages;

    // Get API key from OS credential store
    let provider = preset
        .provider
        .clone()
        .unwrap_or_else(|| "openai".to_string());
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
            id: response_msg_id.clone(),
            chat_id: chat_id.to_string(),
            role: "assistant".to_string(),
            content: full_response.clone(),
            created_at: response_ts,
        })
        .await
        .map_err(|e| e.to_string())?;

    prompt_engine::spawn_turn_index(
        state.repo.clone(),
        cartridge_id.to_string(),
        chat_id.to_string(),
        response_msg_id,
        user_message.to_string(),
        full_response,
    );

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

pub async fn dry_run_prompt_pipeline(
    state: &AppState,
    cartridge_id: &str,
    chat_id: Option<&str>,
    user_message: &str,
) -> Result<PromptDryRunResult, String> {
    let cartridge = state
        .repo
        .get_cartridge(cartridge_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cartridge not found".to_string())?;

    let dir = std::path::PathBuf::from(&cartridge.directory_path);
    let preset = fs::load_preset(&dir)?;
    let render_chat_id = chat_id.unwrap_or("__dry_run_chat__");

    let rendered = prompt_engine::render_prompt(
        &state.repo,
        cartridge_id,
        render_chat_id,
        &dir,
        &preset,
        &cartridge.name,
        user_message,
        Some(user_message.to_string()),
    )
    .await?;

    Ok(rendered.dry_run)
}
