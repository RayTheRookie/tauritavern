use crate::application::dto::{Manifest, PresetConfig, WorldEntry};
use std::path::{Path, PathBuf};

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

pub fn resolve_cartridge_dir(data_dir: &Path, cartridge_id: &str) -> PathBuf {
    data_dir.join("cartridges").join(cartridge_id)
}
