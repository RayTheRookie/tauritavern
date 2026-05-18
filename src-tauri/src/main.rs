mod app_state;
mod application;
mod infrastructure;
mod presentation;

use app_state::AppState;
use infrastructure::database::SqliteRepo;
use std::path::PathBuf;
use url::Url;

use tauri_plugin_dialog;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Determine data directory
    let data_dir = resolve_data_dir();

    // Ensure directories exist
    let cartridges_dir = data_dir.join("cartridges");
    std::fs::create_dir_all(&cartridges_dir)?;

    // Initialize SQLite
    let db_path = data_dir.join("tavern_data.db");
    let db_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(4)
        .connect(&db_url)
        .await?;

    // Enable foreign key enforcement (SQLite disables it by default)
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await?;

    // Avoid SQLITE_BUSY under concurrent access
    sqlx::query("PRAGMA busy_timeout = 5000")
        .execute(&pool)
        .await?;

    // Run migrations
    infrastructure::database::run_migrations(&pool).await?;

    let repo = SqliteRepo::new(pool);

    // Restore active profile from previous session
    let active_profile_id = repo.get_setting("active_profile_id").await.ok().flatten();

    let app_state = AppState::new(repo, data_dir.clone());
    if let Some(ref pid) = active_profile_id {
        *app_state.active_profile_id.lock().unwrap() = Some(pid.clone());
    }

    let protocol_data_dir = data_dir.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .register_uri_scheme_protocol("tavern", move |_app, request| {
            handle_tavern_protocol(&request, &protocol_data_dir)
        })
        .invoke_handler(tauri::generate_handler![
            presentation::commands::library_commands::list_cartridges,
            presentation::commands::library_commands::import_cartridge,
            presentation::commands::library_commands::delete_cartridge,
            presentation::commands::library_commands::open_cartridge,
            presentation::commands::library_commands::get_cartridge_cover,
            presentation::commands::settings_commands::get_api_key,
            presentation::commands::settings_commands::get_raw_api_key,
            presentation::commands::settings_commands::set_api_key,
            presentation::commands::settings_commands::delete_api_key,
            presentation::commands::settings_commands::get_all_api_keys,
            presentation::commands::settings_commands::get_providers,
            presentation::commands::settings_commands::get_provider_url,
            presentation::commands::settings_commands::set_provider_url,
            presentation::commands::settings_commands::fetch_models,
            presentation::commands::settings_commands::test_connection,
            presentation::commands::settings_commands::save_profile,
            presentation::commands::settings_commands::list_profiles,
            presentation::commands::settings_commands::delete_profile,
            presentation::commands::settings_commands::set_active_profile,
            presentation::commands::settings_commands::get_active_profile,
            presentation::commands::settings_commands::diagnose,
            presentation::commands::settings_commands::view_profile,
            presentation::commands::settings_commands::check_keyring,
            presentation::commands::creator_commands::open_creator,
            presentation::commands::creator_commands::open_creator_for_cartridge,
            presentation::commands::creator_commands::delete_workbench,
            presentation::commands::creator_commands::get_workbench,
            presentation::commands::creator_commands::save_workbench_manifest,
            presentation::commands::creator_commands::save_workbench_preset,
            presentation::commands::creator_commands::save_workbench_world_info,
            presentation::commands::creator_commands::save_workbench_pipeline,
            presentation::commands::creator_commands::save_workbench_ui_file,
            presentation::commands::creator_commands::import_workbench_cover_image,
            presentation::commands::creator_commands::save_creator_agent_config,
            presentation::commands::creator_commands::creator_agent_chat,
            presentation::commands::creator_commands::test_chat_workbench,
            presentation::commands::creator_commands::export_workbench,
            presentation::commands::chat_commands::send_chat,
            presentation::commands::chat_commands::dry_run_prompt_pipeline,
            presentation::commands::chat_commands::create_chat,
            presentation::commands::chat_commands::list_chats,
            presentation::commands::chat_commands::get_messages,
            presentation::commands::chat_commands::delete_chat,
            presentation::commands::chat_commands::load_asset,
            presentation::commands::chat_commands::get_preset,
            presentation::commands::chat_commands::match_world_info,
        ])
        .run(tauri::generate_context!())?;

    Ok(())
}

fn resolve_data_dir() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    // In development, exe is in target/debug or target/release — walk up to project root
    let mut current = exe_dir.clone();
    for _ in 0..4 {
        let candidate = current.join("user_data");
        if candidate.exists() || current.join("src-tauri").exists() {
            return candidate;
        }
        if let Some(parent) = current.parent() {
            current = parent.to_path_buf();
        } else {
            break;
        }
    }

    PathBuf::from("user_data")
}

/// SDK content baked into the binary at compile time — never missing at runtime.
const SDK_CONTENT: &str = include_str!("../../tauri-tavern-sdk.js");

fn handle_tavern_protocol(
    request: &tauri::http::Request<Vec<u8>>,
    data_dir: &PathBuf,
) -> tauri::http::Response<Vec<u8>> {
    // Parse URI properly — handles both tavern://localhost/path and http://tavern.localhost/path
    let uri = request.uri().to_string();
    let url = match Url::parse(&uri) {
        Ok(u) => u,
        Err(_) => {
            return tauri::http::Response::builder()
                .status(400)
                .body(b"Invalid URL".to_vec())
                .unwrap();
        }
    };

    let path = url.path();
    let path = path.trim_start_matches('/');

    // SDK endpoint — served from compile-time embedded content
    // Matches: /sdk, /sdk/, /tauri-tavern-sdk.js, /cartridge/*/tauri-tavern-sdk.js
    if path.starts_with("sdk/") || path == "sdk" || path.ends_with("/tauri-tavern-sdk.js") {
        return tauri::http::Response::builder()
            .status(200)
            .header("Content-Type", "application/javascript")
            .header("Access-Control-Allow-Origin", "*")
            .body(SDK_CONTENT.as_bytes().to_vec())
            .unwrap();
    }

    // Workbench endpoint — serves workbench files during editing (live preview)
    if let Some(rest) = path.strip_prefix("workbench/") {
        let mut parts = rest.splitn(2, '/');
        let wb_id = parts.next().unwrap_or("");
        let file_path = match parts.next() {
            Some("") | None => "ui/index.html",
            Some(p) => p,
        };
        if wb_id.is_empty() {
            return tauri::http::Response::builder()
                .status(400)
                .body(b"Invalid workbench path".to_vec())
                .unwrap();
        }
        let wb_dir = data_dir.join("workbench").join(wb_id);
        let full_path = wb_dir.join(file_path);
        // Path traversal protection
        let canonical_dir = match wb_dir.canonicalize() {
            Ok(d) => d,
            Err(_) => {
                return tauri::http::Response::builder()
                    .status(404)
                    .body(b"Workbench not found".to_vec())
                    .unwrap()
            }
        };
        let canonical_path = match full_path.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                return tauri::http::Response::builder()
                    .status(404)
                    .body(b"File not found".to_vec())
                    .unwrap()
            }
        };
        if !canonical_path.starts_with(&canonical_dir) {
            return tauri::http::Response::builder()
                .status(403)
                .body(b"Access denied".to_vec())
                .unwrap();
        }
        match std::fs::read(&canonical_path) {
            Ok(data) => {
                let content_type = guess_content_type(&canonical_path);
                return tauri::http::Response::builder()
                    .status(200)
                    .header("Content-Type", content_type)
                    .header("Access-Control-Allow-Origin", "*")
                    .body(data)
                    .unwrap();
            }
            Err(_) => {
                return tauri::http::Response::builder()
                    .status(500)
                    .body(b"Failed to read file".to_vec())
                    .unwrap();
            }
        }
    }

    // Cartridge endpoint
    if let Some(rest) = path.strip_prefix("cartridge/") {
        let mut parts = rest.splitn(2, '/');
        let cartridge_id = parts.next().unwrap_or("");
        let file_path = match parts.next() {
            Some("") | None => "ui/index.html",
            Some(p) => p,
        };

        if cartridge_id.is_empty() {
            return tauri::http::Response::builder()
                .status(400)
                .body(b"Invalid cartridge path".to_vec())
                .unwrap();
        }

        let cartridge_dir = data_dir.join("cartridges").join(cartridge_id);
        let full_path = cartridge_dir.join(file_path);

        // Security: ensure resolved path stays within cartridge directory
        let canonical_dir = match cartridge_dir.canonicalize() {
            Ok(d) => d,
            Err(_) => {
                return tauri::http::Response::builder()
                    .status(404)
                    .body(b"Cartridge not found".to_vec())
                    .unwrap();
            }
        };

        let canonical_path = match full_path.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                return tauri::http::Response::builder()
                    .status(404)
                    .body(b"File not found".to_vec())
                    .unwrap();
            }
        };

        if !canonical_path.starts_with(&canonical_dir) {
            return tauri::http::Response::builder()
                .status(403)
                .body(b"Access denied".to_vec())
                .unwrap();
        }

        match std::fs::read(&canonical_path) {
            Ok(data) => {
                let content_type = guess_content_type(&canonical_path);
                tauri::http::Response::builder()
                    .status(200)
                    .header("Content-Type", content_type)
                    .header("Access-Control-Allow-Origin", "*")
                    .body(data)
                    .unwrap()
            }
            Err(_) => tauri::http::Response::builder()
                .status(500)
                .body(b"Failed to read file".to_vec())
                .unwrap(),
        }
    } else {
        tauri::http::Response::builder()
            .status(404)
            .body(b"Not found".to_vec())
            .unwrap()
    }
}

fn guess_content_type(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") | Some("mjs") => "application/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("ogg") => "audio/ogg",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("wasm") => "application/wasm",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("txt") => "text/plain; charset=utf-8",
        Some("xml") => "application/xml; charset=utf-8",
        _ => "application/octet-stream",
    }
}
