mod app_state;
mod application;
mod infrastructure;
mod presentation;

use app_state::AppState;
use infrastructure::database::SqliteRepo;
use std::path::PathBuf;

use tauri::Manager;
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

    // Run migrations
    infrastructure::database::run_migrations(&pool).await?;

    let repo = SqliteRepo::new(pool);
    let app_state = AppState::new(repo, data_dir.clone());

    let protocol_data_dir = data_dir.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .register_uri_scheme_protocol("tavern", move |_app, request| {
            handle_tavern_protocol(request, &protocol_data_dir)
        })
        .invoke_handler(tauri::generate_handler![
            presentation::commands::library_commands::list_cartridges,
            presentation::commands::library_commands::import_cartridge,
            presentation::commands::library_commands::delete_cartridge,
            presentation::commands::library_commands::open_cartridge,
            presentation::commands::library_commands::get_cartridge_cover,
            presentation::commands::settings_commands::get_api_key,
            presentation::commands::settings_commands::set_api_key,
            presentation::commands::settings_commands::delete_api_key,
            presentation::commands::settings_commands::get_all_api_keys,
            presentation::commands::chat_commands::send_chat,
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

fn handle_tavern_protocol(
    request: &tauri::http::Request<Vec<u8>>,
    data_dir: &PathBuf,
) -> tauri::http::Response<Vec<u8>> {
    let uri = request.uri().to_string();
    let path = uri.strip_prefix("tavern://localhost/").unwrap_or("");

    // SDK endpoint
    if path.starts_with("sdk/") {
        let sdk_paths = [
            PathBuf::from("tauri-tavern-sdk.js"),
            PathBuf::from("../tauri-tavern-sdk.js"),
        ];
        for sdk_path in &sdk_paths {
            if let Ok(data) = std::fs::read(sdk_path) {
                return tauri::http::Response::builder()
                    .status(200)
                    .header("Content-Type", "application/javascript")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(data)
                    .unwrap();
            }
        }
        return tauri::http::Response::builder()
            .status(404)
            .body(b"SDK not found".to_vec())
            .unwrap();
    }

    // Cartridge endpoint
    if let Some(rest) = path.strip_prefix("cartridge/") {
        let mut parts = rest.splitn(2, '/');
        let cartridge_id = parts.next().unwrap_or("");
        let file_path = parts.next().unwrap_or("ui/index.html");

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
        Some("js") => "application/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("ogg") => "audio/ogg",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        _ => "application/octet-stream",
    }
}
