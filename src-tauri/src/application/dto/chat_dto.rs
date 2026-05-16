use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetConfig {
    pub system_prompt: String,
    pub model: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<usize>,
    pub provider: Option<String>,
    pub provider_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub author: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default = "default_entry_file")]
    pub entry_file: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub cover_image: String,
}

fn default_version() -> String {
    "1.0.0".to_string()
}

fn default_entry_file() -> String {
    "ui/index.html".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldEntry {
    pub keys: Vec<String>,
    pub content: String,
    #[serde(default)]
    pub secondary_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartridgeInfo {
    pub id: String,
    pub name: String,
    pub author: String,
    pub description: String,
    pub version: String,
    pub cover_image: String,
    pub installed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatInfo {
    pub id: String,
    pub cartridge_id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatChunkPayload {
    pub chat_id: String,
    pub content: String,
    pub done: bool,
    #[serde(default)]
    pub error: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorPayload {
    pub message: String,
    pub chat_id: Option<String>,
}
