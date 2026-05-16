use crate::application::dto::{Manifest, PresetConfig, WorldEntry};
use std::path::Path;

pub fn guess_mime(path: &str) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "html" => "text/html",
        "css" => "text/css",
        "js" => "application/javascript",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

/// Encodes raw bytes into a `data:` URI string with the correct MIME type.
pub fn encode_data_uri(data: &[u8], path: &str) -> String {
    use base64::Engine;
    let mime = guess_mime(path);
    let b64 = base64::engine::general_purpose::STANDARD.encode(data);
    format!("data:{};base64,{}", mime, b64)
}

pub fn load_asset(cartridge_dir: &Path, relative_path: &str) -> Result<Vec<u8>, String> {
    let resolved = cartridge_dir.join(relative_path);

    let canonical_base = cartridge_dir
        .canonicalize()
        .map_err(|e| format!("Failed to resolve cartridge directory: {}", e))?;

    let canonical = resolved
        .canonicalize()
        .map_err(|e| format!("Asset not found: {}", e))?;

    if !canonical.starts_with(&canonical_base) {
        return Err("Path traversal detected — access denied".to_string());
    }

    std::fs::read(&canonical).map_err(|e| format!("Failed to read asset: {}", e))
}

pub fn load_manifest(cartridge_dir: &Path) -> Result<Manifest, String> {
    let path = cartridge_dir.join("manifest.json");
    let content =
        std::fs::read_to_string(&path).map_err(|e| format!("Failed to read manifest.json: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("Invalid manifest.json: {}", e))
}

pub fn load_preset(cartridge_dir: &Path) -> Result<PresetConfig, String> {
    let path = cartridge_dir.join("preset.json");
    let content =
        std::fs::read_to_string(&path).map_err(|e| format!("Failed to read preset.json: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("Invalid preset.json: {}", e))
}

pub fn load_world_info(cartridge_dir: &Path) -> Result<Vec<WorldEntry>, String> {
    let path = cartridge_dir.join("world_info.json");
    if !path.exists() {
        return Ok(vec![]);
    }
    let content =
        std::fs::read_to_string(&path).map_err(|e| format!("Failed to read world_info.json: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("Invalid world_info.json: {}", e))
}
