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
) -> Result<String, String> {
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

    // Resolve provider, model, key, and URL — active profile overrides preset
    let active_pid = state.active_profile_id.lock().unwrap().clone();
    let (provider, model, api_key, api_url_override) =
        resolve_chat_config(state, &preset, active_pid.as_deref()).await?;

    // Apply active profile overrides to preset for the request
    let preset = crate::application::dto::PresetConfig {
        provider: Some(provider.clone()),
        model: Some(model),
        provider_url: api_url_override.clone(),
        ..preset
    };

    // Stream from LLM
    let client = LlmHttpClient::new();
    let api_url_ref = &api_url_override;
    let stream_url = crate::infrastructure::provider_registry::resolve_url(
        &provider,
        api_url_ref,
        api_url_ref,
    );
    let total_input_chars: usize = messages.iter().map(|m| m.content.chars().count()).sum();
    log::info!(
        "→ API request: provider={} model={} url={} messages={} input_chars={}",
        provider,
        preset.model.as_deref().unwrap_or("?"),
        stream_url,
        messages.len(),
        total_input_chars,
    );

    let mut stream = client.stream_chat(&preset, &messages, &api_key, api_url_ref);

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

    let output_chars = full_response.chars().count();
    log::info!(
        "← API response: output_chars={} (est. ~{} tokens)",
        output_chars,
        output_chars / 2  // rough: CJK ~1 char/token, EN ~4 char/token → avg ~2
    );

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
        full_response.clone(),
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

    Ok(full_response)
}

async fn resolve_chat_config(
    state: &AppState,
    preset: &crate::application::dto::PresetConfig,
    active_pid: Option<&str>,
) -> Result<(String, String, String, Option<String>), String> {
    // Try active profile first
    let skip_reason;

    if let Some(pid) = active_pid {
        match state.repo.get_profile(pid).await {
            Ok(Some(profile)) => match CredentialService::get(&state.repo, &profile.provider_id).await {
                Ok(Some(key)) => {
                    log::info!(
                        "Using active profile '{}' ({} / {})",
                        profile.name,
                        profile.provider_id,
                        profile.model
                    );
                    return Ok((
                        profile.provider_id,
                        profile.model,
                        key,
                        profile.api_url,
                    ));
                }
                Ok(None) => {
                    skip_reason = Some(format!(
                        "Active profile '{}' has no API key for '{}' in the credential store. Re-configure your API key.",
                        profile.name, profile.provider_id
                    ));
                }
                Err(e) => {
                    skip_reason = Some(format!(
                        "Active profile '{}' keyring read error for '{}': {}",
                        profile.name, profile.provider_id, e
                    ));
                }
            },
            Ok(None) => {
                skip_reason = Some(format!(
                    "Active profile ID '{}' not found in database (may have been deleted)",
                    pid
                ));
            }
            Err(e) => {
                skip_reason = Some(format!("Database error reading active profile: {}", e));
            }
        }
    } else {
        skip_reason = Some("No active profile selected. Use Settings to select one.".to_string());
    }

    // Fall back to preset config
    let provider = preset
        .provider
        .clone()
        .unwrap_or_else(|| "openai".to_string());
    let key = CredentialService::get(&state.repo, &provider).await?
        .ok_or_else(|| {
            let hint = skip_reason.unwrap_or_else(|| "unknown reason".to_string());
            format!(
                "Active profile skipped ({})\n  → Fallback provider '{}' has no API key configured.\n  → Go to Settings → select a provider → Fetch Models → Connect to create a profile, then click it to activate.",
                hint, provider
            )
        })?;
    let url = CredentialService::get_url(&state.repo, &provider).await.unwrap_or(None);
    let model = preset
        .model
        .clone()
        .unwrap_or_else(|| "gpt-4o".to_string());
    Ok((provider, model, key, url))
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
