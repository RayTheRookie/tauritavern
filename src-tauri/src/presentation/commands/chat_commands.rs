use crate::application::dto::{
    ChatInfo, ChatMessage, PresetConfig, PromptDryRunResult, WorldEntry,
};
use crate::application::services::{chat_completion_service, st_script_engine};
use crate::infrastructure::database::{ChatRow, ChatVariableRow};
use crate::infrastructure::fs;
use crate::AppState;
use chrono::Utc;
use std::path::PathBuf;
use uuid::Uuid;

#[tauri::command]
pub async fn send_chat(
    window: tauri::Window,
    state: tauri::State<'_, AppState>,
    cartridge_id: String,
    chat_id: String,
    message: String,
) -> Result<String, String> {
    assert_chat_scope(&state, &cartridge_id, &chat_id).await?;
    chat_completion_service::handle_chat(&state, &cartridge_id, &chat_id, &message, &window).await
}

#[tauri::command]
pub async fn execute_st_command(
    state: tauri::State<'_, AppState>,
    cartridge_id: String,
    chat_id: String,
    command: String,
) -> Result<String, String> {
    assert_chat_scope(&state, &cartridge_id, &chat_id).await?;
    match st_script_engine::execute_slash_script(&state, &cartridge_id, &chat_id, &command).await {
        Some(result) => result,
        None => Err("ST command must start with '/'".to_string()),
    }
}

#[tauri::command]
pub async fn dry_run_prompt_pipeline(
    state: tauri::State<'_, AppState>,
    cartridge_id: String,
    chat_id: Option<String>,
    message: String,
) -> Result<PromptDryRunResult, String> {
    if let Some(chat_id) = chat_id.as_deref() {
        assert_chat_scope(&state, &cartridge_id, chat_id).await?;
    }
    chat_completion_service::dry_run_prompt_pipeline(
        &state,
        &cartridge_id,
        chat_id.as_deref(),
        &message,
    )
    .await
}

#[tauri::command]
pub async fn create_chat(
    state: tauri::State<'_, AppState>,
    cartridge_id: String,
    title: Option<String>,
) -> Result<ChatInfo, String> {
    state
        .repo
        .get_cartridge(&cartridge_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cartridge not found".to_string())?;

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let chat_title = title.unwrap_or_else(|| "New Chat".to_string());

    let row = ChatRow {
        id: id.clone(),
        cartridge_id: cartridge_id.clone(),
        title: chat_title.clone(),
        created_at: now.clone(),
        updated_at: now.clone(),
    };

    state
        .repo
        .create_chat(&row)
        .await
        .map_err(|e| e.to_string())?;

    Ok(ChatInfo {
        id,
        cartridge_id,
        title: chat_title,
        created_at: now.clone(),
        updated_at: now,
    })
}

#[tauri::command]
pub async fn list_chats(
    state: tauri::State<'_, AppState>,
    cartridge_id: String,
) -> Result<Vec<ChatInfo>, String> {
    let rows = state
        .repo
        .list_chats(&cartridge_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(rows
        .into_iter()
        .map(|r| ChatInfo {
            id: r.id,
            cartridge_id: r.cartridge_id,
            title: r.title,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
}

#[tauri::command]
pub async fn get_messages(
    state: tauri::State<'_, AppState>,
    cartridge_id: Option<String>,
    chat_id: String,
) -> Result<Vec<ChatMessage>, String> {
    if let Some(cartridge_id) = cartridge_id.as_deref() {
        assert_chat_scope(&state, cartridge_id, &chat_id).await?;
    }
    let rows = state
        .repo
        .get_messages_by_chat(&chat_id, 200)
        .await
        .map_err(|e| e.to_string())?;

    Ok(rows
        .into_iter()
        .map(|r| ChatMessage {
            role: r.role,
            content: r.content,
        })
        .collect())
}

#[tauri::command]
pub async fn delete_chat(
    state: tauri::State<'_, AppState>,
    cartridge_id: Option<String>,
    chat_id: String,
) -> Result<(), String> {
    if let Some(cartridge_id) = cartridge_id.as_deref() {
        assert_chat_scope(&state, cartridge_id, &chat_id).await?;
    }
    state
        .repo
        .delete_chat(&chat_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_chat_variable(
    state: tauri::State<'_, AppState>,
    cartridge_id: Option<String>,
    chat_id: String,
    name: String,
    value: String,
) -> Result<(), String> {
    if let Some(cartridge_id) = cartridge_id.as_deref() {
        assert_chat_scope(&state, cartridge_id, &chat_id).await?;
    }
    let name = normalize_variable_name(&name)?;
    state
        .repo
        .set_chat_variable(&chat_id, &name, &value, &Utc::now().to_rfc3339())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_chat_variable(
    state: tauri::State<'_, AppState>,
    cartridge_id: Option<String>,
    chat_id: String,
    name: String,
) -> Result<Option<String>, String> {
    if let Some(cartridge_id) = cartridge_id.as_deref() {
        assert_chat_scope(&state, cartridge_id, &chat_id).await?;
    }
    let name = normalize_variable_name(&name)?;
    state
        .repo
        .get_chat_variable(&chat_id, &name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_chat_variables(
    state: tauri::State<'_, AppState>,
    cartridge_id: Option<String>,
    chat_id: String,
) -> Result<Vec<ChatVariableRow>, String> {
    if let Some(cartridge_id) = cartridge_id.as_deref() {
        assert_chat_scope(&state, cartridge_id, &chat_id).await?;
    }
    state
        .repo
        .list_chat_variables(&chat_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_chat_variable(
    state: tauri::State<'_, AppState>,
    cartridge_id: Option<String>,
    chat_id: String,
    name: String,
) -> Result<(), String> {
    if let Some(cartridge_id) = cartridge_id.as_deref() {
        assert_chat_scope(&state, cartridge_id, &chat_id).await?;
    }
    let name = normalize_variable_name(&name)?;
    state
        .repo
        .delete_chat_variable(&chat_id, &name)
        .await
        .map_err(|e| e.to_string())
}

fn normalize_variable_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Variable name cannot be empty".to_string());
    }
    if trimmed.len() > 128 {
        return Err("Variable name is too long".to_string());
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':'))
    {
        return Err("Variable name contains unsupported characters".to_string());
    }
    Ok(trimmed.to_string())
}

async fn assert_chat_scope(
    state: &tauri::State<'_, AppState>,
    cartridge_id: &str,
    chat_id: &str,
) -> Result<(), String> {
    let chat = state
        .repo
        .get_chat(chat_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Chat not found".to_string())?;
    if chat.cartridge_id != cartridge_id {
        return Err("Chat does not belong to this cartridge".to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn load_asset(
    state: tauri::State<'_, AppState>,
    cartridge_id: String,
    path: String,
) -> Result<String, String> {
    let cartridge = state
        .repo
        .get_cartridge(&cartridge_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cartridge not found".to_string())?;

    let dir = PathBuf::from(&cartridge.directory_path);
    let data = fs::load_asset(&dir, &path)?;
    Ok(fs::encode_data_uri(&data, &path))
}

#[tauri::command]
pub async fn get_preset(
    state: tauri::State<'_, AppState>,
    cartridge_id: String,
) -> Result<PresetConfig, String> {
    let cartridge = state
        .repo
        .get_cartridge(&cartridge_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cartridge not found".to_string())?;

    let dir = PathBuf::from(&cartridge.directory_path);
    fs::load_preset(&dir)
}

#[tauri::command]
pub async fn match_world_info(
    state: tauri::State<'_, AppState>,
    cartridge_id: String,
    message: String,
) -> Result<Vec<WorldEntry>, String> {
    let cartridge = state
        .repo
        .get_cartridge(&cartridge_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cartridge not found".to_string())?;

    let dir = PathBuf::from(&cartridge.directory_path);
    let entries = fs::load_world_info(&dir).unwrap_or_default();

    let lower_msg = message.to_lowercase();
    Ok(entries
        .into_iter()
        .filter(|entry| {
            entry
                .keys
                .iter()
                .any(|key| lower_msg.contains(&key.to_lowercase()))
        })
        .collect())
}
