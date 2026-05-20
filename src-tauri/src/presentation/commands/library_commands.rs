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
    let cartridge_dir = PathBuf::from(&cartridge.directory_path);
    if let Err(e) = package_service::refresh_sillytavern_classic_runtime(&cartridge_dir) {
        log::warn!("failed to refresh SillyTavern classic runtime: {}", e);
    }

    WebviewWindowBuilder::new(
        &app,
        &label,
        tauri::WebviewUrl::External(url.parse().map_err(|e| format!("Invalid URL: {}", e))?),
    )
    .initialization_script(cartridge_runtime_bootstrap())
    .title(&cartridge.name)
    .inner_size(900.0, 700.0)
    .resizable(true)
    .build()
    .map_err(|e| format!("Failed to create window: {}", e))?;

    Ok(())
}

fn cartridge_runtime_bootstrap() -> &'static str {
    r#"
(() => {
  const rawInvoke = window.__TAURI__?.core?.invoke || window.__TAURI_INTERNALS__?.invoke;
  const rawListen = window.__TAURI__?.event?.listen || window.__TAURI_INTERNALS__?.event?.listen;
  const href = String(window.location.href || "");
  const match = href.match(/\/cartridge\/([^/]+)/);
  const cartridgeId = match ? decodeURIComponent(match[1]) : null;
  const allowed = new Set([
    "send_chat",
    "execute_st_command",
    "dry_run_prompt_pipeline",
    "create_chat",
    "list_chats",
    "get_messages",
    "delete_chat",
    "set_chat_variable",
    "get_chat_variable",
    "list_chat_variables",
    "delete_chat_variable",
    "load_asset",
    "get_preset",
    "match_world_info"
  ]);
  const allowedEvents = new Set(["chat-chunk"]);

  function scopedArgs(command, args) {
    const scoped = Object.assign({}, args || {});
    if (cartridgeId && !scoped.cartridgeId && command !== "delete_chat") {
      scoped.cartridgeId = cartridgeId;
    }
    if (cartridgeId && command === "delete_chat" && !scoped.cartridgeId) {
      scoped.cartridgeId = cartridgeId;
    }
    if (scoped.cartridgeId && cartridgeId && scoped.cartridgeId !== cartridgeId) {
      throw new Error("Cartridge scope mismatch.");
    }
    return scoped;
  }

  const bridge = Object.freeze({
    invoke(command, args) {
      if (!rawInvoke) throw new Error("Tauri IPC unavailable.");
      if (!allowed.has(command)) throw new Error(`Command '${command}' is not exposed to cartridge runtime.`);
      return rawInvoke(command, scopedArgs(command, args));
    },
    listen(event, handler) {
      if (!rawListen) throw new Error("Tauri event bridge unavailable.");
      if (!allowedEvents.has(event)) throw new Error(`Event '${event}' is not exposed to cartridge runtime.`);
      return rawListen(event, handler);
    },
    cartridgeId
  });

  Object.defineProperty(window, "__TAURI_TAVERN_BRIDGE__", {
    value: bridge,
    enumerable: false,
    configurable: false,
    writable: false
  });
  try { delete window.__TAURI__; } catch (_) {}
  try { delete window.__TAURI_INTERNALS__; } catch (_) {}
  try {
    Object.defineProperty(window, "__TAURI__", { value: undefined, configurable: false, writable: false });
  } catch (_) {}
  try {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { value: undefined, configurable: false, writable: false });
  } catch (_) {}
})();
"#
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
