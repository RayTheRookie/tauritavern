use crate::application::dto::{
    ChatMessage, PipelineConfig, PresetConfig, PresetPromptEntry, PromptBudgetReport,
    PromptDryRunResult, PromptInsertionDebug, RagRecallDebug, RegexMutationDebug, RegexMutator,
    WorldEntry, WorldTriggerDebug,
};
use crate::application::services::memory_service;
use crate::infrastructure::apis::LlmHttpClient;
use crate::infrastructure::database::{MessageRow, RagMemoryRow, SqliteRepo};
use crate::infrastructure::fs;
use chrono::Utc;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const EMBEDDING_DIM: usize = 1024;

#[derive(Debug, Clone)]
pub struct PromptRenderOutput {
    pub messages: Vec<ChatMessage>,
    pub dry_run: PromptDryRunResult,
}

#[derive(Debug, Clone)]
struct HistoryMessage {
    id: Option<String>,
    role: String,
    content: String,
    depth: usize,
}

#[derive(Debug, Clone)]
struct ScoredMemory {
    row: RagMemoryRow,
    similarity: f32,
}

#[derive(Debug, Clone)]
struct WorldTrigger {
    entry: WorldEntry,
    trigger: String,
    similarity: Option<f32>,
}

#[derive(Debug, Clone)]
struct MacroVars {
    user_name: String,
    char_name: String,
    input: String,
}

#[derive(Debug, Clone)]
struct PromptInjection {
    label: String,
    role: String,
    content: String,
    depth: usize,
    order: i32,
    seq: usize,
    source: InjectionSource,
}

#[derive(Debug, Clone, PartialEq)]
enum InjectionSource {
    Preset,
    World,
    Rag,
}

pub async fn render_prompt(
    repo: &SqliteRepo,
    cartridge_id: &str,
    chat_id: &str,
    cartridge_dir: &Path,
    preset: &PresetConfig,
    cartridge_name: &str,
    user_message: &str,
    pending_user_message: Option<String>,
) -> Result<PromptRenderOutput, String> {
    let pipeline = fs::load_pipeline(cartridge_dir)?;
    let world_entries = fs::load_world_info(cartridge_dir).unwrap_or_default();
    let strategy = pipeline.context_strategy.clone();
    let max_context_tokens = preset
        .context_window_size
        .unwrap_or(strategy.max_context_tokens);
    let history_limit = strategy.history_fetch_limit as i64;

    let repo_for_rag = repo.clone();
    let repo_for_world = repo.clone();
    let repo_for_history = repo.clone();
    let cartridge_for_rag = cartridge_id.to_string();
    let cartridge_for_world = cartridge_id.to_string();
    let chat_for_rag = chat_id.to_string();
    let chat_for_history = chat_id.to_string();
    let user_for_rag = user_message.to_string();
    let user_for_world = user_message.to_string();
    let world_for_task = world_entries.clone();
    let rag_threshold = strategy.rag_similarity_threshold;
    let rag_fetch_count = strategy.rag_fetch_count;
    let world_threshold = strategy.rag_similarity_threshold;

    let rag_task = tokio::spawn(async move {
        retrieve_rag_memories(
            &repo_for_rag,
            &cartridge_for_rag,
            &chat_for_rag,
            &user_for_rag,
            rag_threshold,
            rag_fetch_count,
        )
        .await
    });

    let world_task = tokio::spawn(async move {
        trigger_world_entries(
            &repo_for_world,
            &cartridge_for_world,
            &world_for_task,
            &user_for_world,
            world_threshold,
        )
        .await
    });

    let history_task = tokio::spawn(async move {
        repo_for_history
            .get_recent_messages_by_chat(&chat_for_history, history_limit)
            .await
            .map_err(|e| e.to_string())
    });

    let rag_memories = rag_task
        .await
        .map_err(|e| format!("RAG task failed: {}", e))??;
    let world_triggers = world_task
        .await
        .map_err(|e| format!("World info task failed: {}", e))??;
    let mut history_rows = history_task
        .await
        .map_err(|e| format!("History task failed: {}", e))??;

    if let Some(content) = pending_user_message {
        history_rows.push(MessageRow {
            id: "__dry_run_user__".to_string(),
            chat_id: chat_id.to_string(),
            role: "user".to_string(),
            content,
            created_at: Utc::now().to_rfc3339(),
        });
    }

    let mut history = build_history(history_rows);
    let model_name = preset.model.as_deref().unwrap_or("gpt-4");
    let regex_mutations = apply_regex_mutators(&mut history, &pipeline, model_name)?;

    let macro_vars = MacroVars {
        user_name: preset
            .user_name
            .clone()
            .unwrap_or_else(|| "User".to_string()),
        char_name: preset
            .char_name
            .clone()
            .unwrap_or_else(|| cartridge_name.to_string()),
        input: user_message.to_string(),
    };

    let mut relative_injections = Vec::new();
    let mut in_chat_injections = Vec::new();
    let mut seq = 0usize;

    for entry in preset.effective_prompt_entries() {
        if !preset_entry_is_active(&entry, user_message) {
            continue;
        }
        let item = PromptInjection {
            label: format!("preset:{}", entry.id),
            role: normalize_role(&entry.role),
            content: entry.content,
            depth: entry.depth.unwrap_or(0),
            order: entry.order,
            seq,
            source: InjectionSource::Preset,
        };
        seq += 1;
        if entry.position == "in_chat" {
            in_chat_injections.push(item);
        } else {
            relative_injections.push(item);
        }
    }

    for trigger in &world_triggers {
        let entry = &trigger.entry;
        let item = PromptInjection {
            label: format!(
                "world_info:{}",
                entry.id.clone().unwrap_or_else(|| entry.keys.join(","))
            ),
            role: normalize_role(&entry.role),
            content: entry.content.clone(),
            depth: entry.insertion_depth.unwrap_or(0),
            order: entry.order,
            seq,
            source: InjectionSource::World,
        };
        seq += 1;
        match world_position(entry) {
            "in_chat" => in_chat_injections.push(item),
            _ => relative_injections.push(item),
        }
    }

    for (idx, memory) in rag_memories.iter().enumerate() {
        relative_injections.push(PromptInjection {
            label: format!("rag:{}", memory.row.id),
            role: "system".to_string(),
            content: format!("[Recall: {}]", memory.row.content),
            depth: 0,
            order: 20_000 + idx as i32,
            seq,
            source: InjectionSource::Rag,
        });
        seq += 1;
    }

    sort_relative_injections(&mut relative_injections);

    let system_tokens = relative_injections
        .iter()
        .filter(|item| item.source == InjectionSource::Preset)
        .map(|item| memory_service::count_tokens(&item.content, model_name))
        .sum();
    let lore_tokens = relative_injections
        .iter()
        .chain(in_chat_injections.iter())
        .filter(|item| item.source != InjectionSource::Rag)
        .map(|item| memory_service::count_tokens(&item.content, model_name))
        .sum::<usize>()
        .saturating_sub(system_tokens);
    let rag_tokens = relative_injections
        .iter()
        .filter(|item| item.source == InjectionSource::Rag)
        .map(|item| memory_service::count_tokens(&item.content, model_name))
        .sum();
    let history_budget = max_context_tokens
        .saturating_sub(system_tokens)
        .saturating_sub(lore_tokens)
        .saturating_sub(rag_tokens);

    let (selected_history, dropped_history_count) =
        select_recent_history(&history, history_budget, model_name);

    let mut messages = Vec::new();
    messages.extend(relative_injections.into_iter().map(|item| {
        apply_macros_to_message(
            ChatMessage {
                role: item.role,
                content: item.content,
            },
            &macro_vars,
        )
    }));

    let selected_history_messages: Vec<ChatMessage> = selected_history
        .into_iter()
        .map(|msg| ChatMessage {
            role: normalize_role(&msg.role),
            content: apply_macros(&msg.content, &macro_vars),
        })
        .collect();
    let selected_history_tokens = message_tokens(&selected_history_messages, model_name);
    let mut lower_history = selected_history_messages;

    let mut insertions = Vec::new();
    sort_in_chat_injections(&mut in_chat_injections);
    for item in in_chat_injections {
        let message = apply_macros_to_message(
            ChatMessage {
                role: item.role,
                content: item.content,
            },
            &macro_vars,
        );
        insert_with_depth(
            &mut lower_history,
            &mut insertions,
            item.depth,
            item.label,
            message,
        );
    }

    messages.extend(lower_history.clone());

    let total_tokens = message_tokens(&messages, model_name);
    let budget = PromptBudgetReport {
        max_context_tokens,
        system_tokens,
        lore_tokens,
        rag_tokens,
        history_tokens: selected_history_tokens,
        total_tokens,
        dropped_history_count,
    };

    let rag_recalls = rag_memories
        .iter()
        .map(|memory| RagRecallDebug {
            id: memory.row.id.clone(),
            source_type: memory.row.source_type.clone(),
            source_id: memory.row.source_id.clone(),
            similarity: memory.similarity,
            content: memory.row.content.clone(),
        })
        .collect();

    let world_triggers_debug = world_triggers
        .iter()
        .map(|trigger| WorldTriggerDebug {
            id: trigger.entry.id.clone(),
            keys: trigger.entry.keys.clone(),
            trigger: trigger.trigger.clone(),
            similarity: trigger.similarity,
            insertion_depth: trigger.entry.insertion_depth,
            role: normalize_role(&trigger.entry.role),
            content: trigger.entry.content.clone(),
        })
        .collect();

    let payload = LlmHttpClient::build_payload_preview(preset, &messages);
    let final_text = LlmHttpClient::serialize_messages_for_debug(preset, &messages);
    let dry_run = PromptDryRunResult {
        messages: messages.clone(),
        payload,
        final_text,
        budget,
        rag_recalls,
        world_triggers: world_triggers_debug,
        regex_mutations,
        insertions,
    };

    Ok(PromptRenderOutput { messages, dry_run })
}

pub fn spawn_static_world_index(repo: SqliteRepo, cartridge_id: String, cartridge_dir: PathBuf) {
    tokio::spawn(async move {
        let entries = match fs::load_world_info(&cartridge_dir) {
            Ok(entries) => entries,
            Err(e) => {
                log::warn!("failed to load world_info for indexing: {}", e);
                return;
            }
        };

        if let Err(e) = repo
            .delete_rag_memories_by_source_type(&cartridge_id, "world_info")
            .await
        {
            log::warn!("failed to clear old world_info vectors: {}", e);
            return;
        }

        for (idx, entry) in entries
            .into_iter()
            .filter(|entry| entry.enabled && entry.enable_semantic_search)
            .enumerate()
        {
            let source_id = entry
                .id
                .clone()
                .unwrap_or_else(|| format!("world_entry_{}", idx));
            let index_text = format!("{}\n{}", entry.keys.join("\n"), entry.content);
            let vector = embed_text(&index_text);
            let vector_json = match serde_json::to_string(&vector) {
                Ok(json) => json,
                Err(e) => {
                    log::warn!("failed to serialize world_info vector: {}", e);
                    continue;
                }
            };
            let row = RagMemoryRow {
                id: format!("world:{}:{}", cartridge_id, source_id),
                cartridge_id: cartridge_id.clone(),
                chat_id: None,
                message_id: None,
                source_type: "world_info".to_string(),
                source_id: Some(source_id),
                content: entry.content,
                vector_json,
                created_at: Utc::now().to_rfc3339(),
            };
            if let Err(e) = repo.insert_rag_memory(&row).await {
                log::warn!("failed to insert world_info vector: {}", e);
            }
        }
    });
}

pub fn spawn_turn_index(
    repo: SqliteRepo,
    cartridge_id: String,
    chat_id: String,
    message_id: String,
    user_text: String,
    assistant_text: String,
) {
    tokio::spawn(async move {
        let content = format!("用户说：{}\n角色回答：{}", user_text, assistant_text);
        let vector = embed_text(&content);
        let vector_json = match serde_json::to_string(&vector) {
            Ok(json) => json,
            Err(e) => {
                log::warn!("failed to serialize turn vector: {}", e);
                return;
            }
        };
        let row = RagMemoryRow {
            id: Uuid::new_v4().to_string(),
            cartridge_id,
            chat_id: Some(chat_id),
            message_id: Some(message_id.clone()),
            source_type: "turn".to_string(),
            source_id: Some(message_id),
            content,
            vector_json,
            created_at: Utc::now().to_rfc3339(),
        };
        if let Err(e) = repo.insert_rag_memory(&row).await {
            log::warn!("failed to insert rolling RAG memory: {}", e);
        }
    });
}

async fn retrieve_rag_memories(
    repo: &SqliteRepo,
    cartridge_id: &str,
    chat_id: &str,
    user_message: &str,
    threshold: f32,
    fetch_count: usize,
) -> Result<Vec<ScoredMemory>, String> {
    let query = build_rag_query(repo, chat_id, user_message).await;
    let query_vector = embed_text(&query);
    let candidates = repo
        .get_rag_memories(cartridge_id, Some("turn"))
        .await
        .map_err(|e| e.to_string())?;

    let mut scored = Vec::new();
    for row in candidates {
        let vector: Vec<f32> = match serde_json::from_str(&row.vector_json) {
            Ok(vector) => vector,
            Err(_) => continue,
        };
        let similarity = cosine_similarity(&query_vector, &vector);
        if similarity >= threshold {
            scored.push(ScoredMemory { row, similarity });
        }
    }

    scored.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(fetch_count);
    Ok(scored)
}

async fn build_rag_query(repo: &SqliteRepo, chat_id: &str, user_message: &str) -> String {
    let recent = repo
        .get_recent_messages_by_chat(chat_id, 3)
        .await
        .unwrap_or_default();
    let previous = recent
        .iter()
        .rev()
        .find(|msg| !(msg.role == "user" && msg.content == user_message))
        .map(|msg| msg.content.as_str())
        .unwrap_or("");

    if previous.is_empty() {
        user_message.to_string()
    } else {
        format!("{}\n{}", user_message, previous)
    }
}

async fn trigger_world_entries(
    repo: &SqliteRepo,
    cartridge_id: &str,
    entries: &[WorldEntry],
    query: &str,
    threshold: f32,
) -> Result<Vec<WorldTrigger>, String> {
    let query_vector = embed_text(query);
    let indexed_scores = score_indexed_world_entries(repo, cartridge_id, &query_vector, threshold)
        .await
        .unwrap_or_default();
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for (idx, entry) in entries.iter().enumerate() {
        if !entry.enabled {
            continue;
        }

        let identity = world_entry_identity(cartridge_id, entry);
        let keyword_hit = entry
            .keys
            .iter()
            .chain(entry.secondary_keys.iter())
            .find(|key| key_matches(query, key));

        if let Some(key) = keyword_hit {
            if seen.insert(identity.clone()) {
                result.push(WorldTrigger {
                    entry: entry.clone(),
                    trigger: format!("keyword:{}", key),
                    similarity: None,
                });
            }
            continue;
        }

        if entry.enable_semantic_search {
            let source_id = world_entry_source_id(entry, idx);
            let similarity = indexed_scores.get(&source_id).copied().unwrap_or_else(|| {
                let index_text = format!("{}\n{}", entry.keys.join("\n"), entry.content);
                let entry_vector = embed_text(&index_text);
                cosine_similarity(&query_vector, &entry_vector)
            });
            if similarity >= threshold && seen.insert(identity) {
                result.push(WorldTrigger {
                    entry: entry.clone(),
                    trigger: "semantic".to_string(),
                    similarity: Some(similarity),
                });
            }
        }
    }

    Ok(result)
}

async fn score_indexed_world_entries(
    repo: &SqliteRepo,
    cartridge_id: &str,
    query_vector: &[f32],
    threshold: f32,
) -> Result<HashMap<String, f32>, String> {
    let rows = repo
        .get_rag_memories(cartridge_id, Some("world_info"))
        .await
        .map_err(|e| e.to_string())?;
    let mut scores = HashMap::new();

    for row in rows {
        let source_id = match row.source_id {
            Some(source_id) => source_id,
            None => continue,
        };
        let vector: Vec<f32> = match serde_json::from_str(&row.vector_json) {
            Ok(vector) => vector,
            Err(_) => continue,
        };
        let similarity = cosine_similarity(query_vector, &vector);
        if similarity >= threshold {
            scores.insert(source_id, similarity);
        }
    }

    Ok(scores)
}

fn build_history(rows: Vec<MessageRow>) -> Vec<HistoryMessage> {
    let len = rows.len();
    rows.into_iter()
        .enumerate()
        .map(|(idx, row)| HistoryMessage {
            id: Some(row.id),
            role: row.role,
            content: row.content,
            depth: len.saturating_sub(idx + 1),
        })
        .collect()
}

fn apply_regex_mutators(
    history: &mut [HistoryMessage],
    pipeline: &PipelineConfig,
    model: &str,
) -> Result<Vec<RegexMutationDebug>, String> {
    let mut debug = Vec::new();

    for message in history.iter_mut() {
        for mutator in &pipeline.regex_mutators {
            if !mutator_applies(mutator, message.depth) {
                continue;
            }

            let regex = Regex::new(&mutator.pattern)
                .map_err(|e| format!("Invalid regex mutator '{}': {}", mutator.id, e))?;
            let before = message.content.clone();
            let after = regex
                .replace_all(&message.content, mutator.replacement.as_str())
                .to_string();

            if before != after {
                let before_tokens = memory_service::count_tokens(&before, model);
                let after_tokens = memory_service::count_tokens(&after, model);
                message.content = after.clone();
                debug.push(RegexMutationDebug {
                    mutator_id: mutator.id.clone(),
                    message_id: message.id.clone(),
                    depth: message.depth,
                    role: message.role.clone(),
                    before_tokens,
                    after_tokens,
                    before,
                    after,
                });
            }
        }
    }

    Ok(debug)
}

fn mutator_applies(mutator: &RegexMutator, depth: usize) -> bool {
    if !mutator.enabled {
        return false;
    }

    if mutator.target != "history" {
        return false;
    }

    let start = mutator.depth_range.first().copied().unwrap_or(0);
    let end = mutator.depth_range.get(1).copied().unwrap_or(usize::MAX);
    depth >= start && depth <= end
}

fn select_recent_history(
    history: &[HistoryMessage],
    budget_tokens: usize,
    model: &str,
) -> (Vec<HistoryMessage>, usize) {
    let mut selected = Vec::new();
    let mut used_tokens = 0usize;

    for message in history.iter().rev() {
        let tokens = memory_service::count_tokens(&message.content, model);
        if selected.is_empty() || used_tokens + tokens <= budget_tokens {
            selected.push(message.clone());
            used_tokens = used_tokens.saturating_add(tokens);
        } else {
            break;
        }
    }

    selected.reverse();
    let dropped = history.len().saturating_sub(selected.len());
    (selected, dropped)
}

fn insert_with_depth(
    history: &mut Vec<ChatMessage>,
    debug: &mut Vec<PromptInsertionDebug>,
    depth: usize,
    label: String,
    message: ChatMessage,
) {
    let index = history.len().saturating_sub(depth);
    let debug_item = PromptInsertionDebug {
        label,
        depth,
        index,
        role: message.role.clone(),
        content: message.content.clone(),
    };
    history.insert(index, message);
    debug.push(debug_item);
}

fn preset_entry_is_active(entry: &PresetPromptEntry, query: &str) -> bool {
    if !entry.enabled || entry.content.trim().is_empty() {
        return false;
    }
    entry.triggers.is_empty() || entry.triggers.iter().any(|key| key_matches(query, key))
}

fn world_position(entry: &WorldEntry) -> &'static str {
    match entry.position.as_str() {
        "top" | "relative" => "relative",
        "in_chat" => "in_chat",
        _ if entry.insertion_depth.is_some() => "in_chat",
        _ => "relative",
    }
}

fn sort_relative_injections(items: &mut [PromptInjection]) {
    items.sort_by(|a, b| {
        a.order
            .cmp(&b.order)
            .then_with(|| role_rank(&a.role).cmp(&role_rank(&b.role)))
            .then_with(|| a.seq.cmp(&b.seq))
    });
}

fn sort_in_chat_injections(items: &mut [PromptInjection]) {
    items.sort_by(|a, b| {
        b.depth
            .cmp(&a.depth)
            .then_with(|| a.order.cmp(&b.order))
            .then_with(|| role_rank(&a.role).cmp(&role_rank(&b.role)))
            .then_with(|| a.seq.cmp(&b.seq))
    });
}

fn normalize_role(role: &str) -> String {
    match role {
        "user" | "assistant" | "system" => role.to_string(),
        _ => "system".to_string(),
    }
}

fn role_rank(role: &str) -> usize {
    match role {
        "system" => 0,
        "user" => 1,
        "assistant" => 2,
        _ => 3,
    }
}

fn message_tokens(messages: &[ChatMessage], model: &str) -> usize {
    messages
        .iter()
        .map(|message| memory_service::count_tokens(&message.content, model))
        .sum()
}

fn apply_macros_to_message(mut message: ChatMessage, vars: &MacroVars) -> ChatMessage {
    message.content = apply_macros(&message.content, vars);
    message
}

fn apply_macros(text: &str, vars: &MacroVars) -> String {
    let replacements = [
        ("{{user}}", vars.user_name.as_str()),
        ("{{User}}", vars.user_name.as_str()),
        ("{{USER}}", vars.user_name.as_str()),
        ("<user>", vars.user_name.as_str()),
        ("<User>", vars.user_name.as_str()),
        ("<USER>", vars.user_name.as_str()),
        ("{{char}}", vars.char_name.as_str()),
        ("{{Char}}", vars.char_name.as_str()),
        ("{{CHAR}}", vars.char_name.as_str()),
        ("<char>", vars.char_name.as_str()),
        ("<Char>", vars.char_name.as_str()),
        ("<CHAR>", vars.char_name.as_str()),
        ("{{input}}", vars.input.as_str()),
        ("{{Input}}", vars.input.as_str()),
        ("{{INPUT}}", vars.input.as_str()),
        ("<input>", vars.input.as_str()),
        ("<Input>", vars.input.as_str()),
        ("<INPUT>", vars.input.as_str()),
    ];
    let mut output = text.to_string();
    for (from, to) in replacements {
        output = output.replace(from, to);
    }
    output
}

fn world_entry_identity(cartridge_id: &str, entry: &WorldEntry) -> String {
    entry
        .id
        .clone()
        .unwrap_or_else(|| format!("{}:{}", cartridge_id, entry.content))
}

fn world_entry_source_id(entry: &WorldEntry, index: usize) -> String {
    entry
        .id
        .clone()
        .unwrap_or_else(|| format!("world_entry_{}", index))
}

fn key_matches(query: &str, key: &str) -> bool {
    let key = key.trim();
    if key.is_empty() {
        return false;
    }

    if key == "*" {
        return true;
    }

    if let Some(pattern) = key.strip_prefix("re:") {
        return Regex::new(pattern)
            .map(|regex| regex.is_match(query))
            .unwrap_or(false);
    }

    if key.starts_with('/') && key.ends_with('/') && key.len() > 2 {
        return Regex::new(&key[1..key.len() - 1])
            .map(|regex| regex.is_match(query))
            .unwrap_or(false);
    }

    query.to_lowercase().contains(&key.to_lowercase())
}

fn embed_text(text: &str) -> Vec<f32> {
    let mut vector = vec![0.0f32; EMBEDDING_DIM];
    let lowered = text.to_lowercase();

    for token in lowered.split_whitespace() {
        add_feature(&mut vector, token);
    }

    let chars: Vec<char> = lowered.chars().filter(|c| !c.is_whitespace()).collect();
    for n in 1..=3 {
        if chars.len() < n {
            continue;
        }
        for window in chars.windows(n) {
            let feature: String = window.iter().collect();
            add_feature(&mut vector, &feature);
        }
    }

    normalize_vector(vector)
}

fn add_feature(vector: &mut [f32], feature: &str) {
    if feature.is_empty() {
        return;
    }

    // Deterministic FNV-1a 64-bit — stable across process restarts,
    // unlike DefaultHasher which uses per-process random SipHash keys.
    let hash = fnv1a_64(feature);
    let index = (hash as usize) % vector.len();
    let sign = if (hash >> 63) == 0 { 1.0 } else { -1.0 };
    vector[index] += sign;
}

fn fnv1a_64(data: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for byte in data.bytes() {
        h ^= byte as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn normalize_vector(mut vector: Vec<f32>) -> Vec<f32> {
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm == 0.0 {
        return vector;
    }
    for value in &mut vector {
        *value /= norm;
    }
    vector
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    a.iter()
        .zip(b.iter())
        .map(|(left, right)| left * right)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::dto::{default_prompt_entries, PresetPromptEntry};

    fn history_msg(id: &str, depth: usize, content: &str) -> HistoryMessage {
        HistoryMessage {
            id: Some(id.to_string()),
            role: "assistant".to_string(),
            content: content.to_string(),
            depth,
        }
    }

    #[test]
    fn regex_mutator_applies_by_depth() {
        let mut history = vec![history_msg(
            "1",
            20,
            "<text>long</text>\n<zongjie>short</zongjie>",
        )];
        let pipeline = PipelineConfig {
            regex_mutators: vec![RegexMutator {
                id: "summary".to_string(),
                enabled: true,
                target: "history".to_string(),
                depth_range: vec![15, 999],
                pattern: "<text>[\\s\\S]*?</text>\\n<zongjie>([\\s\\S]*?)</zongjie>".to_string(),
                replacement: "<zongjie>$1</zongjie>".to_string(),
                flags: String::new(),
                sample: String::new(),
                description: String::new(),
            }],
            ..PipelineConfig::default()
        };

        let debug = apply_regex_mutators(&mut history, &pipeline, "gpt-4").unwrap();
        assert_eq!(history[0].content, "<zongjie>short</zongjie>");
        assert_eq!(debug.len(), 1);
    }

    #[test]
    fn latest_history_is_kept_even_when_over_budget() {
        let history = vec![
            history_msg("old", 1, "old message"),
            history_msg("latest", 0, &"x".repeat(10_000)),
        ];
        let (selected, dropped) = select_recent_history(&history, 1, "gpt-4");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id.as_deref(), Some("latest"));
        assert_eq!(dropped, 1);
    }

    #[test]
    fn local_embeddings_score_related_text_higher() {
        let a = embed_text("地下城 哥布林");
        let b = embed_text("哥布林住在地下城");
        let c = embed_text("月亮和酒杯");
        assert!(cosine_similarity(&a, &b) > cosine_similarity(&a, &c));
    }

    #[test]
    fn old_preset_migrates_to_prompt_entries() {
        let preset = PresetConfig {
            system_prompt: "main".to_string(),
            prompt_entries: Vec::new(),
            model: None,
            temperature: None,
            max_tokens: None,
            context_window_size: None,
            provider: None,
            provider_url: None,
            chat_format: None,
            authors_note: Some("note".to_string()),
            authors_note_depth: Some(4),
            user_name: None,
            char_name: None,
        };

        let entries = preset.effective_prompt_entries();
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.id == "main_prompt")
                .unwrap()
                .content,
            "main"
        );
        let note = entries
            .iter()
            .find(|entry| entry.id == "authors_note")
            .unwrap();
        assert_eq!(note.content, "note");
        assert_eq!(note.depth, Some(4));
    }

    #[test]
    fn in_chat_injections_sort_by_depth_order_role_then_sequence() {
        let mut items = vec![
            injection("late_user", "user", 1, 10, 0),
            injection("older", "system", 3, 99, 1),
            injection("same_order_assistant", "assistant", 1, 10, 2),
            injection("same_order_system", "system", 1, 10, 3),
        ];

        sort_in_chat_injections(&mut items);
        let labels: Vec<_> = items.into_iter().map(|item| item.label).collect();
        assert_eq!(
            labels,
            vec![
                "older",
                "same_order_system",
                "late_user",
                "same_order_assistant"
            ]
        );
    }

    #[test]
    fn disabled_and_triggered_preset_entries_are_filtered() {
        let mut active = default_prompt_entries().remove(0);
        active.content = "active".to_string();
        active.triggers = vec!["sword".to_string()];

        let disabled = PresetPromptEntry {
            enabled: false,
            content: "disabled".to_string(),
            ..active.clone()
        };

        assert!(preset_entry_is_active(&active, "draw the sword"));
        assert!(!preset_entry_is_active(&active, "drink tea"));
        assert!(!preset_entry_is_active(&disabled, "draw the sword"));
    }

    #[test]
    fn world_position_defaults_match_legacy_depth_behavior() {
        let mut entry = WorldEntry {
            id: None,
            enabled: true,
            keys: Vec::new(),
            content: "lore".to_string(),
            secondary_keys: Vec::new(),
            enable_semantic_search: false,
            insertion_depth: None,
            position: "auto".to_string(),
            order: 0,
            role: "system".to_string(),
        };
        assert_eq!(world_position(&entry), "relative");
        entry.insertion_depth = Some(2);
        assert_eq!(world_position(&entry), "in_chat");
        entry.position = "top".to_string();
        assert_eq!(world_position(&entry), "relative");
    }

    #[test]
    fn sillytavern_macros_are_replaced() {
        let vars = MacroVars {
            user_name: "Alice".to_string(),
            char_name: "Soyo".to_string(),
            input: "hello".to_string(),
        };

        assert_eq!(
            apply_macros("<user> meets <char>: {{input}} / {{User}} / <CHAR>", &vars),
            "Alice meets Soyo: hello / Alice / Soyo"
        );
    }

    fn injection(label: &str, role: &str, depth: usize, order: i32, seq: usize) -> PromptInjection {
        PromptInjection {
            label: label.to_string(),
            role: role.to_string(),
            content: label.to_string(),
            depth,
            order,
            seq,
            source: InjectionSource::Preset,
        }
    }
}
