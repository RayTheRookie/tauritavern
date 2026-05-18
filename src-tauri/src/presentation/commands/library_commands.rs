use crate::application::dto::CartridgeInfo;
use crate::application::services::package_service;
use crate::infrastructure::fs;
use crate::AppState;
use std::path::PathBuf;
use tauri::Manager;
use tauri::WebviewWindowBuilder;

#[tauri::command]
pub async fn list_cartridges(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<CartridgeInfo>, String> {
    let rows = state
        .repo
        .get_all_cartridges()
        .await
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        let mut cover_uri = String::new();
        if !row.cover_image.is_empty() {
            let dir = PathBuf::from(&row.directory_path);
            if let Ok(data) = fs::load_asset(&dir, &row.cover_image) {
                cover_uri = fs::encode_data_uri(&data, &row.cover_image);
            }
        }
        result.push(CartridgeInfo {
            id: row.id,
            name: row.name,
            author: row.author,
            description: row.description,
            version: row.version,
            cover_image: cover_uri,
            installed_at: row.installed_at,
        });
    }

    Ok(result)
}

#[tauri::command]
pub async fn import_cartridge(
    state: tauri::State<'_, AppState>,
    file_path: String,
) -> Result<CartridgeInfo, String> {
    package_service::import_cartridge(&state.repo, &file_path, &state.data_dir).await
}

#[tauri::command]
pub async fn delete_cartridge(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let cartridge = state
        .repo
        .get_cartridge(&id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cartridge not found".to_string())?;

    // Remove from database first (cascades to chats and messages).
    // If this fails, the filesystem is untouched — no partial state.
    state
        .repo
        .delete_cartridge(&id)
        .await
        .map_err(|e| e.to_string())?;

    // Remove from filesystem
    let dir = PathBuf::from(&cartridge.directory_path);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("Failed to remove cartridge: {}", e))?;
    }

    Ok(())
}

#[tauri::command]
pub async fn open_cartridge(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    cartridge_id: String,
) -> Result<(), String> {
    let cartridge = state
        .repo
        .get_cartridge(&cartridge_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cartridge not found".to_string())?;

    let label = format!("cartridge_{}", cartridge_id);

    // Check if window already exists
    if let Some(window) = app.get_webview_window(&label) {
        window.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    let url = format!(
        "tavern://localhost/cartridge/{}/{}",
        cartridge_id, cartridge.entry_file
    );

    WebviewWindowBuilder::new(
        &app,
        &label,
        tauri::WebviewUrl::External(url.parse().map_err(|e| format!("Invalid URL: {}", e))?),
    )
    .title(&cartridge.name)
    .inner_size(900.0, 700.0)
    .resizable(true)
    .build()
    .map_err(|e| format!("Failed to create window: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn get_cartridge_cover(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<String, String> {
    let cartridge = state
        .repo
        .get_cartridge(&id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cartridge not found".to_string())?;

    if cartridge.cover_image.is_empty() {
        return Ok(String::new());
    }

    let dir = PathBuf::from(&cartridge.directory_path);
    let data = fs::load_asset(&dir, &cartridge.cover_image)?;
    Ok(fs::encode_data_uri(&data, &cartridge.cover_image))
}
