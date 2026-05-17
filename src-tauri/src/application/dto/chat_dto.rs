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
    /// Maximum tokens for the LLM completion (output). Defaults to 4096.
    pub max_tokens: Option<usize>,
    /// Context window size for truncating history (input budget). Defaults to 8192.
    pub context_window_size: Option<usize>,
    pub provider: Option<String>,
    pub provider_url: Option<String>,
    /// API/chat serialization style. Supported values: "chatml" (default) and "alpaca".
    pub chat_format: Option<String>,
    /// Creator-authored note that should be inserted near the bottom of the prompt.
    #[serde(default, alias = "author_note")]
    pub authors_note: Option<String>,
    /// Distance from the newest history message where Author's Note is inserted.
    pub authors_note_depth: Option<usize>,
    #[serde(default)]
    pub user_name: Option<String>,
    #[serde(default)]
    pub char_name: Option<String>,
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
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub keys: Vec<String>,
    pub content: String,
    #[serde(default)]
    pub secondary_keys: Vec<String>,
    #[serde(default)]
    pub enable_semantic_search: bool,
    #[serde(default)]
    pub insertion_depth: Option<usize>,
    #[serde(default = "default_system_role")]
    pub role: String,
}

fn default_system_role() -> String {
    "system".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldInfoBook {
    #[serde(default)]
    pub entries: Vec<WorldEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    #[serde(default)]
    pub context_strategy: ContextStrategy,
    #[serde(default)]
    pub regex_mutators: Vec<RegexMutator>,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            context_strategy: ContextStrategy::default(),
            regex_mutators: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextStrategy {
    #[serde(default = "default_max_context_tokens")]
    pub max_context_tokens: usize,
    #[serde(default = "default_rag_fetch_count")]
    pub rag_fetch_count: usize,
    #[serde(default = "default_rag_similarity_threshold")]
    pub rag_similarity_threshold: f32,
    #[serde(default = "default_history_fetch_limit")]
    pub history_fetch_limit: usize,
}

impl Default for ContextStrategy {
    fn default() -> Self {
        Self {
            max_context_tokens: default_max_context_tokens(),
            rag_fetch_count: default_rag_fetch_count(),
            rag_similarity_threshold: default_rag_similarity_threshold(),
            history_fetch_limit: default_history_fetch_limit(),
        }
    }
}

fn default_max_context_tokens() -> usize {
    8_000
}

fn default_rag_fetch_count() -> usize {
    3
}

fn default_rag_similarity_threshold() -> f32 {
    0.75
}

fn default_history_fetch_limit() -> usize {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegexMutator {
    pub id: String,
    #[serde(default = "default_history_target")]
    pub target: String,
    #[serde(default)]
    pub depth_range: Vec<usize>,
    pub pattern: String,
    pub replacement: String,
    #[serde(default)]
    pub description: String,
}

fn default_history_target() -> String {
    "history".to_string()
}

#[derive(Debug, Clone, Serialize)]
pub struct PromptDryRunResult {
    pub messages: Vec<ChatMessage>,
    pub payload: serde_json::Value,
    pub final_text: String,
    pub budget: PromptBudgetReport,
    pub rag_recalls: Vec<RagRecallDebug>,
    pub world_triggers: Vec<WorldTriggerDebug>,
    pub regex_mutations: Vec<RegexMutationDebug>,
    pub insertions: Vec<PromptInsertionDebug>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PromptBudgetReport {
    pub max_context_tokens: usize,
    pub system_tokens: usize,
    pub lore_tokens: usize,
    pub rag_tokens: usize,
    pub history_tokens: usize,
    pub total_tokens: usize,
    pub dropped_history_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RagRecallDebug {
    pub id: String,
    pub source_type: String,
    pub source_id: Option<String>,
    pub similarity: f32,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorldTriggerDebug {
    pub id: Option<String>,
    pub keys: Vec<String>,
    pub trigger: String,
    pub similarity: Option<f32>,
    pub insertion_depth: Option<usize>,
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegexMutationDebug {
    pub mutator_id: String,
    pub message_id: Option<String>,
    pub depth: usize,
    pub role: String,
    pub before_tokens: usize,
    pub after_tokens: usize,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PromptInsertionDebug {
    pub label: String,
    pub depth: usize,
    pub index: usize,
    pub role: String,
    pub content: String,
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
