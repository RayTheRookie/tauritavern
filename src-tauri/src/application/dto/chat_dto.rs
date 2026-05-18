use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetConfig {
    pub system_prompt: String,
    #[serde(default)]
    pub prompt_entries: Vec<PresetPromptEntry>,
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

impl PresetConfig {
    pub fn effective_prompt_entries(&self) -> Vec<PresetPromptEntry> {
        if !self.prompt_entries.is_empty() {
            return self.prompt_entries.clone();
        }

        let mut entries = default_prompt_entries();
        if let Some(main) = entries.iter_mut().find(|entry| entry.id == "main_prompt") {
            main.content = self.system_prompt.clone();
            main.enabled = !main.content.trim().is_empty();
        }
        if let Some(note) = entries.iter_mut().find(|entry| entry.id == "authors_note") {
            note.content = self.authors_note.clone().unwrap_or_default();
            note.enabled = !note.content.trim().is_empty();
            note.depth = self.authors_note_depth;
        }
        entries
    }

    pub fn ensure_prompt_entries(&mut self) {
        if self.prompt_entries.is_empty() {
            self.prompt_entries = self.effective_prompt_entries();
        }
        if let Some(main) = self
            .prompt_entries
            .iter()
            .find(|entry| entry.id == "main_prompt")
        {
            self.system_prompt = main.content.clone();
        }
        if let Some(note) = self
            .prompt_entries
            .iter()
            .find(|entry| entry.id == "authors_note")
        {
            self.authors_note = if note.content.trim().is_empty() {
                None
            } else {
                Some(note.content.clone())
            };
            self.authors_note_depth = note.depth;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetPromptEntry {
    pub id: String,
    pub name: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_system_role")]
    pub role: String,
    #[serde(default)]
    pub content: String,
    #[serde(default = "default_prompt_position")]
    pub position: String,
    #[serde(default)]
    pub depth: Option<usize>,
    #[serde(default)]
    pub order: i32,
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub pinned: bool,
}

pub fn default_prompt_entries() -> Vec<PresetPromptEntry> {
    vec![
        PresetPromptEntry {
            id: "main_prompt".to_string(),
            name: "Main Prompt".to_string(),
            enabled: true,
            role: "system".to_string(),
            content: "You are a helpful assistant.".to_string(),
            position: "relative".to_string(),
            depth: None,
            order: 0,
            triggers: Vec::new(),
            pinned: true,
        },
        PresetPromptEntry {
            id: "auxiliary_prompt".to_string(),
            name: "Auxiliary Prompt".to_string(),
            enabled: false,
            role: "system".to_string(),
            content: String::new(),
            position: "relative".to_string(),
            depth: None,
            order: 100,
            triggers: Vec::new(),
            pinned: true,
        },
        PresetPromptEntry {
            id: "authors_note".to_string(),
            name: "Author's Note".to_string(),
            enabled: false,
            role: "system".to_string(),
            content: String::new(),
            position: "in_chat".to_string(),
            depth: Some(2),
            order: 200,
            triggers: Vec::new(),
            pinned: true,
        },
        PresetPromptEntry {
            id: "post_history_instructions".to_string(),
            name: "Post-History Instructions".to_string(),
            enabled: false,
            role: "system".to_string(),
            content: String::new(),
            position: "in_chat".to_string(),
            depth: Some(0),
            order: 300,
            triggers: Vec::new(),
            pinned: true,
        },
    ]
}

fn default_prompt_position() -> String {
    "relative".to_string()
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
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub keys: Vec<String>,
    pub content: String,
    #[serde(default)]
    pub secondary_keys: Vec<String>,
    #[serde(default)]
    pub enable_semantic_search: bool,
    #[serde(default)]
    pub insertion_depth: Option<usize>,
    #[serde(default = "default_world_position")]
    pub position: String,
    #[serde(default)]
    pub order: i32,
    #[serde(default = "default_system_role")]
    pub role: String,
}

fn default_enabled() -> bool {
    true
}

fn default_system_role() -> String {
    "system".to_string()
}

fn default_world_position() -> String {
    "auto".to_string()
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
    #[serde(default = "default_regex_enabled")]
    pub enabled: bool,
    #[serde(default = "default_history_target")]
    pub target: String,
    #[serde(default)]
    pub depth_range: Vec<usize>,
    pub pattern: String,
    pub replacement: String,
    #[serde(default)]
    pub flags: String,
    #[serde(default)]
    pub sample: String,
    #[serde(default)]
    pub description: String,
}

fn default_regex_enabled() -> bool {
    true
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
