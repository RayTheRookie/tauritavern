use crate::application::dto::{ChatInfo, ChatMessage, PresetConfig, WorldEntry};
use crate::application::services::chat_completion_service;
use crate::infrastructure::database::ChatRow;
use crate::infrastructure::fs;
use crate::AppState;
use std::path::PathBuf;
use uuid::Uuid;

#[tauri::command]
pub async fn send_chat(
    window: tauri::Window,
    state: tauri::State<'_, AppState>,
    cartridge_id: String,
    chat_id: String,
    message: String,
) -> Result<(), String> {
    chat_completion_service::handle_chat(&state, &cartridge_id, &chat_id, &message, &window).await
}

#[tauri::command]
pub async fn create_chat(
    state: tauri::State<'_, AppState>,
    cartridge_id: String,
    title: Option<String>,
) -> Result<ChatInfo, String> {
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
    chat_id: String,
) -> Result<Vec<ChatMessage>, String> {
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
    chat_id: String,
) -> Result<(), String> {
    state
        .repo
        .delete_chat(&chat_id)
        .await
        .map_err(|e| e.to_string())
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
