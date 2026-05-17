use crate::application::dto::CartridgeInfo;
use crate::application::services::prompt_engine;
use crate::infrastructure::database::{CartridgeRow, SqliteRepo};
use crate::infrastructure::fs;
use std::path::Path;
use uuid::Uuid;
use zip::ZipArchive;

pub async fn import_cartridge(
    repo: &SqliteRepo,
    file_path: &str,
    data_dir: &Path,
) -> Result<CartridgeInfo, String> {
    let file = std::fs::File::open(file_path).map_err(|e| format!("Failed to open file: {}", e))?;

    let mut archive =
        ZipArchive::new(file).map_err(|e| format!("Failed to read .taurichar archive: {}", e))?;

    let cartridge_id = Uuid::new_v4().to_string();
    let dest_dir = data_dir.join("cartridges").join(&cartridge_id);
    std::fs::create_dir_all(&dest_dir)
        .map_err(|e| format!("Failed to create cartridge directory: {}", e))?;

    // Extract all files
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read entry in archive: {}", e))?;

        let out_path = match file.enclosed_name() {
            Some(path) => dest_dir.join(path),
            None => continue,
        };

        if file.is_dir() {
            std::fs::create_dir_all(&out_path).ok();
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            let mut outfile = std::fs::File::create(&out_path)
                .map_err(|e| format!("Failed to create file: {}", e))?;
            std::io::copy(&mut file, &mut outfile)
                .map_err(|e| format!("Failed to extract file: {}", e))?;
        }
    }

    // Validate required files
    let manifest_path = dest_dir.join("manifest.json");
    if !manifest_path.exists() {
        let _ = std::fs::remove_dir_all(&dest_dir);
        return Err("Invalid .taurichar: missing manifest.json".to_string());
    }

    let preset_path = dest_dir.join("preset.json");
    if !preset_path.exists() {
        let _ = std::fs::remove_dir_all(&dest_dir);
        return Err("Invalid .taurichar: missing preset.json".to_string());
    }

    let entry_file = {
        let manifest = fs::load_manifest(&dest_dir)?;
        let entry = dest_dir.join(&manifest.entry_file);
        if !entry.exists() {
            let _ = std::fs::remove_dir_all(&dest_dir);
            return Err(format!(
                "Invalid .taurichar: entry file '{}' not found",
                manifest.entry_file
            ));
        }
        manifest
    };

    // Copy SDK into cartridge directory
    copy_sdk_to_cartridge(&dest_dir)?;

    // Insert into database
    let now = chrono::Utc::now().to_rfc3339();
    let row = CartridgeRow {
        id: cartridge_id.clone(),
        name: entry_file.name.clone(),
        author: entry_file.author.clone(),
        description: entry_file.description.clone(),
        version: entry_file.version.clone(),
        entry_file: entry_file.entry_file.clone(),
        cover_image: entry_file.cover_image.clone(),
        installed_at: now.clone(),
        directory_path: dest_dir.to_string_lossy().to_string(),
    };

    repo.insert_cartridge(&row)
        .await
        .map_err(|e| format!("Database error: {}", e))?;

    prompt_engine::spawn_static_world_index(repo.clone(), cartridge_id.clone(), dest_dir.clone());

    // Read cover image if present
    let mut cover_uri = String::new();
    if !entry_file.cover_image.is_empty() {
        if let Ok(data) = fs::load_asset(&dest_dir, &entry_file.cover_image) {
            cover_uri = fs::encode_data_uri(&data, &entry_file.cover_image);
        }
    }

    Ok(CartridgeInfo {
        id: cartridge_id,
        name: entry_file.name,
        author: entry_file.author,
        description: entry_file.description,
        version: entry_file.version,
        cover_image: cover_uri,
        installed_at: now,
    })
}

/// SDK content baked into the binary at compile time.
const SDK_CONTENT: &str = include_str!("../../../../tauri-tavern-sdk.js");

fn copy_sdk_to_cartridge(cartridge_dir: &Path) -> Result<(), String> {
    let dest = cartridge_dir.join("tauri-tavern-sdk.js");
    std::fs::write(&dest, SDK_CONTENT).map_err(|e| format!("Failed to copy SDK: {}", e))?;
    Ok(())
}
