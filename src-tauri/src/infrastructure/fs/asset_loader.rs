use crate::application::dto::{Manifest, PipelineConfig, PresetConfig, WorldEntry, WorldInfoBook};
use std::path::Path;

/// Save any serializable value as pretty-printed JSON.
pub fn save_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| format!("JSON serialization error: {}", e))?;
    std::fs::write(path, &json).map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}

/// Load and deserialize a JSON file.
pub fn load_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let data = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    serde_json::from_str(&data).map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
}

/// Load world info entries, supporting both object {entries:[]} and array formats.
pub fn load_world_info_entries(dir: &Path) -> Result<Vec<WorldEntry>, String> {
    let path = dir.join("world_info.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read world_info.json: {}", e))?;
    if let Ok(book) = serde_json::from_str::<serde_json::Value>(&data) {
        if let Some(entries) = book.get("entries").and_then(|e| e.as_array()) {
            return entries
                .iter()
                .map(|v| serde_json::from_value::<WorldEntry>(v.clone()).map_err(|e| e.to_string()))
                .collect();
        }
    }
    serde_json::from_str::<Vec<WorldEntry>>(&data)
        .map_err(|e| format!("Failed to parse world_info.json: {}", e))
}

pub fn guess_mime(path: &str) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "html" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "application/javascript",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "wasm" => "application/wasm",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "txt" => "text/plain",
        "xml" => "application/xml",
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
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read manifest.json: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("Invalid manifest.json: {}", e))
}

pub fn load_preset(cartridge_dir: &Path) -> Result<PresetConfig, String> {
    let path = cartridge_dir.join("preset.json");
    let content =
        std::fs::read_to_string(&path).map_err(|e| format!("Failed to read preset.json: {}", e))?;
    let mut preset: PresetConfig =
        serde_json::from_str(&content).map_err(|e| format!("Invalid preset.json: {}", e))?;
    preset.ensure_prompt_entries();
    Ok(preset)
}

pub fn load_world_info(cartridge_dir: &Path) -> Result<Vec<WorldEntry>, String> {
    let path = cartridge_dir.join("world_info.json");
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read world_info.json: {}", e))?;
    let value: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("Invalid world_info.json: {}", e))?;

    if value.is_array() {
        serde_json::from_value(value).map_err(|e| format!("Invalid world_info.json: {}", e))
    } else {
        let book: WorldInfoBook =
            serde_json::from_value(value).map_err(|e| format!("Invalid world_info.json: {}", e))?;
        Ok(book.entries)
    }
}

pub fn load_pipeline(cartridge_dir: &Path) -> Result<PipelineConfig, String> {
    let path = cartridge_dir.join("pipeline.json");
    if !path.exists() {
        return Ok(PipelineConfig::default());
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read pipeline.json: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("Invalid pipeline.json: {}", e))
}
