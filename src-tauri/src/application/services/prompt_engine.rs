use crate::application::dto::{
    ChatMessage, PipelineConfig, PresetConfig, PresetPromptEntry, PromptBudgetReport,
    PromptDryRunResult, PromptInsertionDebug, RagRecallDebug, RegexMutationDebug, RegexMutator,
    WorldEntry, WorldTriggerDebug,
};
use crate::application::services::{memory_service, st_macro_engine};
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
    recursion_depth: usize,
    index: usize,
    included: bool,
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
    let repo_for_history = repo.clone();
    let cartridge_for_rag = cartridge_id.to_string();
    let chat_for_rag = chat_id.to_string();
    let chat_for_history = chat_id.to_string();
    let user_for_rag = user_message.to_string();
    let rag_threshold = strategy.rag_similarity_threshold;
    let rag_fetch_count = strategy.rag_fetch_count;
    let world_threshold = strategy.rag_similarity_threshold;
    let model_name = preset.model.as_deref().unwrap_or("gpt-4");

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

    let history_task = tokio::spawn(async move {
        repo_for_history
            .get_recent_messages_by_chat(&chat_for_history, history_limit)
            .await
            .map_err(|e| e.to_string())
    });

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

    let mut regex_mutations = Vec::new();
    let mut history = build_history(history_rows);
    apply_prompt_regex_to_history(&mut history, &pipeline, model_name, &mut regex_mutations)?;

    let variables = repo
        .list_chat_variables(chat_id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|row| (row.name, row.value))
        .collect();
    let last_message_id = history.iter().rev().find_map(|message| message.id.clone());
    let macro_context = st_macro_engine::MacroContext {
        user_name: preset
            .user_name
            .clone()
            .unwrap_or_else(|| "User".to_string()),
        char_name: preset
            .char_name
            .clone()
            .unwrap_or_else(|| cartridge_name.to_string()),
        input: user_message.to_string(),
        last_message_id,
        variables,
    };

    let world_scan_text = build_world_scan_buffer(&history, &strategy, &macro_context);
    let world_triggers = trigger_world_entries(
        repo,
        cartridge_id,
        &world_entries,
        &world_scan_text,
        &strategy,
        max_context_tokens,
        model_name,
        world_threshold,
    )
    .await?;
    let rag_memories = rag_task
        .await
        .map_err(|e| format!("RAG task failed: {}", e))??;

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
            content: apply_prompt_regex_to_text(
                entry.content,
                &pipeline,
                "system_prompt",
                0,
                &normalize_role(&entry.role),
                None,
                model_name,
                &mut regex_mutations,
            )?,
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

    for trigger in world_triggers.iter().filter(|trigger| trigger.included) {
        let entry = &trigger.entry;
        let item = PromptInjection {
            label: format!(
                "world_info:{}",
                entry.id.clone().unwrap_or_else(|| entry.keys.join(","))
            ),
            role: normalize_role(&entry.role),
            content: apply_prompt_regex_to_text(
                entry.content.clone(),
                &pipeline,
                "world_info",
                entry.insertion_depth.unwrap_or(0),
                &normalize_role(&entry.role),
                entry.id.as_deref(),
                model_name,
                &mut regex_mutations,
            )?,
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
            &macro_context,
        )
    }));

    let selected_history_messages: Vec<ChatMessage> = selected_history
        .into_iter()
        .map(|msg| ChatMessage {
            role: normalize_role(&msg.role),
            content: apply_macros(&msg.content, &macro_context),
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
            &macro_context,
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
            recursion_depth: trigger.recursion_depth,
            included: trigger.included,
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
    scan_text: &str,
    strategy: &crate::application::dto::ContextStrategy,
    max_context_tokens: usize,
    model: &str,
    threshold: f32,
) -> Result<Vec<WorldTrigger>, String> {
    let query_vector = embed_text(scan_text);
    let indexed_scores = score_indexed_world_entries(repo, cartridge_id, &query_vector, threshold)
        .await
        .unwrap_or_default();
    let mut seen = HashSet::new();
    let mut triggers = Vec::new();
    let mut current_buffers = vec![scan_text.to_string()];
    let max_depth = strategy.world_max_recursion_steps;

    for recursion_depth in 0..=max_depth {
        if current_buffers.is_empty() {
            break;
        }
        let buffer = current_buffers.join("\n\n");
        let mut next_buffers = Vec::new();

        for (idx, entry) in entries.iter().enumerate() {
            if !world_entry_can_scan(entry, recursion_depth) {
                continue;
            }
            let identity = world_entry_identity(cartridge_id, entry);
            if seen.contains(&identity) {
                continue;
            }

            if let Some(mut trigger) = evaluate_world_entry(
                entry,
                idx,
                &buffer,
                &query_vector,
                &indexed_scores,
                strategy,
                threshold,
                recursion_depth,
            ) {
                if !world_probability_passes(&identity, scan_text, entry.probability) {
                    continue;
                }
                seen.insert(identity);
                if recursion_depth > 0 && !trigger.trigger.starts_with("recursive:") {
                    trigger.trigger = format!("recursive:{}", trigger.trigger);
                }
                if !entry.prevent_recursion {
                    next_buffers.push(entry.content.clone());
                }
                triggers.push(trigger);
            }
        }

        current_buffers = next_buffers;
    }

    apply_world_budget(triggers, max_context_tokens, strategy, model)
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

fn world_entry_can_scan(entry: &WorldEntry, recursion_depth: usize) -> bool {
    entry.enabled
        && !entry.content.trim().is_empty()
        && (recursion_depth > 0 || !entry.delay_until_recursion)
        && (recursion_depth == 0 || entry.recursive || entry.constant)
}

fn evaluate_world_entry(
    entry: &WorldEntry,
    index: usize,
    scan_text: &str,
    query_vector: &[f32],
    indexed_scores: &HashMap<String, f32>,
    strategy: &crate::application::dto::ContextStrategy,
    threshold: f32,
    recursion_depth: usize,
) -> Option<WorldTrigger> {
    let scoped_scan;
    let scan_text = if let Some(depth) = entry.scan_depth {
        scoped_scan = take_recent_lines(scan_text, depth.max(1));
        scoped_scan.as_str()
    } else {
        scan_text
    };

    if entry.constant {
        return Some(WorldTrigger {
            entry: entry.clone(),
            trigger: "constant".to_string(),
            similarity: None,
            recursion_depth,
            index,
            included: true,
        });
    }

    let options = world_match_options(entry, strategy);
    let primary_hit = entry
        .keys
        .iter()
        .find(|key| key_matches_with_options(scan_text, key, options));

    let mut trigger = primary_hit.map(|key| format!("keyword:{}", key));
    let mut similarity = None;

    if trigger.is_none() && entry.enable_semantic_search {
        let source_id = world_entry_source_id(entry, index);
        let score = indexed_scores.get(&source_id).copied().unwrap_or_else(|| {
            let index_text = format!("{}\n{}", entry.keys.join("\n"), entry.content);
            let entry_vector = embed_text(&index_text);
            cosine_similarity(query_vector, &entry_vector)
        });
        if score >= threshold {
            trigger = Some("semantic".to_string());
            similarity = Some(score);
        }
    }

    let mut trigger = trigger?;
    if !secondary_keys_pass(entry, scan_text, options) {
        return None;
    }
    if entry.selective && !entry.secondary_keys.is_empty() {
        trigger.push_str(":secondary");
    }

    Some(WorldTrigger {
        entry: entry.clone(),
        trigger,
        similarity,
        recursion_depth,
        index,
        included: true,
    })
}

fn take_recent_lines(text: &str, depth: usize) -> String {
    let mut lines = text.lines().rev().take(depth).collect::<Vec<_>>();
    lines.reverse();
    lines.join("\n")
}

#[derive(Debug, Clone, Copy)]
struct WorldMatchOptions {
    case_sensitive: bool,
    match_whole_words: bool,
}

fn world_match_options(
    entry: &WorldEntry,
    strategy: &crate::application::dto::ContextStrategy,
) -> WorldMatchOptions {
    WorldMatchOptions {
        case_sensitive: entry
            .case_sensitive
            .unwrap_or(strategy.world_case_sensitive),
        match_whole_words: entry
            .match_whole_words
            .unwrap_or(strategy.world_match_whole_words),
    }
}

fn secondary_keys_pass(entry: &WorldEntry, scan_text: &str, options: WorldMatchOptions) -> bool {
    if !entry.selective || entry.secondary_keys.is_empty() {
        return true;
    }

    let matches: Vec<bool> = entry
        .secondary_keys
        .iter()
        .map(|key| key_matches_with_options(scan_text, key, options))
        .collect();
    match normalize_selective_logic(&entry.selective_logic) {
        "and_all" => matches.iter().all(|hit| *hit),
        "not_any" => matches.iter().all(|hit| !*hit),
        "not_all" => !matches.iter().all(|hit| *hit),
        _ => matches.iter().any(|hit| *hit),
    }
}

fn normalize_selective_logic(logic: &str) -> &'static str {
    match logic.to_ascii_lowercase().as_str() {
        "and_all" | "all" | "1" => "and_all",
        "not_any" | "none" | "2" => "not_any",
        "not_all" | "3" => "not_all",
        _ => "and_any",
    }
}

fn world_probability_passes(identity: &str, scan_text: &str, probability: f32) -> bool {
    if probability <= 0.0 {
        return false;
    }
    if probability >= 100.0 {
        return true;
    }

    let mut hash = 14695981039346656037u64;
    for byte in identity.bytes().chain(scan_text.bytes()) {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    let roll = (hash % 10_000) as f32 / 100.0;
    roll < probability
}

fn apply_world_budget(
    mut triggers: Vec<WorldTrigger>,
    max_context_tokens: usize,
    strategy: &crate::application::dto::ContextStrategy,
    model: &str,
) -> Result<Vec<WorldTrigger>, String> {
    let budget = strategy
        .world_budget_tokens
        .unwrap_or_else(|| max_context_tokens.saturating_mul(strategy.world_budget_percent) / 100);
    triggers.sort_by(|a, b| {
        world_budget_rank(a)
            .cmp(&world_budget_rank(b))
            .then_with(|| a.entry.order.cmp(&b.entry.order))
            .then_with(|| a.recursion_depth.cmp(&b.recursion_depth))
            .then_with(|| a.index.cmp(&b.index))
    });

    let mut used = 0usize;
    for trigger in triggers.iter_mut() {
        let tokens = memory_service::count_tokens(&trigger.entry.content, model);
        if used.saturating_add(tokens) <= budget {
            used = used.saturating_add(tokens);
        } else {
            trigger.included = false;
            trigger.trigger = format!("budget_skipped:{}", trigger.trigger);
        }
    }

    Ok(triggers)
}

fn world_budget_rank(trigger: &WorldTrigger) -> usize {
    if trigger.entry.constant {
        0
    } else {
        1
    }
}

fn build_world_scan_buffer(
    history: &[HistoryMessage],
    strategy: &crate::application::dto::ContextStrategy,
    context: &st_macro_engine::MacroContext,
) -> String {
    let scan_depth = strategy.world_scan_depth.max(1);
    let mut lines = Vec::new();

    for message in history.iter().rev().take(scan_depth).rev() {
        let content = apply_macros(&message.content, context);
        if strategy.world_include_names {
            let name = match message.role.as_str() {
                "user" => context.user_name.as_str(),
                "assistant" => context.char_name.as_str(),
                "system" => "System",
                _ => message.role.as_str(),
            };
            lines.push(format!("{}: {}", name, content));
        } else {
            lines.push(content);
        }
    }

    lines.join("\n")
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

fn apply_prompt_regex_to_history(
    history: &mut [HistoryMessage],
    pipeline: &PipelineConfig,
    model: &str,
    debug: &mut Vec<RegexMutationDebug>,
) -> Result<(), String> {
    for message in history.iter_mut() {
        let target = role_regex_target(&message.role);
        message.content = apply_prompt_regex_to_text(
            std::mem::take(&mut message.content),
            pipeline,
            target,
            message.depth,
            &message.role,
            message.id.as_deref(),
            model,
            debug,
        )?;
    }

    Ok(())
}

fn apply_prompt_regex_to_text(
    content: String,
    pipeline: &PipelineConfig,
    target: &str,
    depth: usize,
    role: &str,
    message_id: Option<&str>,
    model: &str,
    debug: &mut Vec<RegexMutationDebug>,
) -> Result<String, String> {
    let mut output = content;
    for mutator in &pipeline.regex_mutators {
        if !mutator_applies(mutator, "prompt", target, role, depth) {
            continue;
        }

        let regex = Regex::new(&mutator.pattern)
            .map_err(|e| format!("Invalid regex mutator '{}': {}", mutator.id, e))?;
        let before = output.clone();
        let after = regex
            .replace_all(&output, mutator.replacement.as_str())
            .to_string();

        if before != after {
            let before_tokens = memory_service::count_tokens(&before, model);
            let after_tokens = memory_service::count_tokens(&after, model);
            output = after.clone();
            debug.push(RegexMutationDebug {
                mutator_id: mutator.id.clone(),
                message_id: message_id.map(str::to_string),
                depth,
                role: role.to_string(),
                before_tokens,
                after_tokens,
                before,
                after,
            });
        }
    }
    Ok(output)
}

fn mutator_applies(
    mutator: &RegexMutator,
    placement: &str,
    target: &str,
    role: &str,
    depth: usize,
) -> bool {
    if !mutator.enabled {
        return false;
    }

    if normalize_regex_placement(mutator) != placement {
        return false;
    }

    if !regex_target_matches(&mutator.target, target, role) {
        return false;
    }

    let start = mutator.depth_range.first().copied().unwrap_or(0);
    let end = mutator.depth_range.get(1).copied().unwrap_or(usize::MAX);
    depth >= start && depth <= end
}

fn normalize_regex_placement(mutator: &RegexMutator) -> &'static str {
    if mutator.prompt_only {
        return "prompt";
    }
    if mutator.markdown_only {
        return "display";
    }
    match mutator
        .placement
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "display" | "ui_display" | "message_display" | "frontend" => "display",
        "prompt" | "llm_prompt" | "format_prompt" => "prompt",
        _ if legacy_display_target(&mutator.target) => "display",
        _ => "prompt",
    }
}

fn regex_target_matches(mutator_target: &str, target: &str, role: &str) -> bool {
    let normalized = normalize_regex_target(mutator_target);
    normalized == "history"
        || normalized == target
        || (normalized == "user_input" && role == "user")
        || (normalized == "bot_output" && role == "assistant")
        || (normalized == "system_prompt" && role == "system")
}

fn normalize_regex_target(target: &str) -> &'static str {
    match target.to_ascii_lowercase().as_str() {
        "display" | "frontend" | "message_display" | "bot" | "bot_output" | "assistant" => {
            "bot_output"
        }
        "user" | "user_input" | "input" => "user_input",
        "system" | "system_prompt" | "preset" | "prompt" => "system_prompt",
        "world" | "world_info" | "lorebook" | "lore" => "world_info",
        "history" | "chat_history" => "history",
        _ => "history",
    }
}

fn legacy_display_target(target: &str) -> bool {
    matches!(
        target.to_ascii_lowercase().as_str(),
        "display" | "frontend" | "message_display"
    )
}

fn role_regex_target(role: &str) -> &'static str {
    match role {
        "user" => "user_input",
        "assistant" => "bot_output",
        "system" => "system_prompt",
        _ => "history",
    }
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

fn apply_macros_to_message(
    mut message: ChatMessage,
    context: &st_macro_engine::MacroContext,
) -> ChatMessage {
    message.content = apply_macros(&message.content, context);
    message
}

fn apply_macros(text: &str, context: &st_macro_engine::MacroContext) -> String {
    st_macro_engine::apply_macros(text, context)
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
    key_matches_with_options(
        query,
        key,
        WorldMatchOptions {
            case_sensitive: false,
            match_whole_words: false,
        },
    )
}

fn key_matches_with_options(query: &str, key: &str, options: WorldMatchOptions) -> bool {
    let key = key.trim();
    if key.is_empty() {
        return false;
    }

    if key == "*" {
        return true;
    }

    if let Some(pattern) = key.strip_prefix("re:") {
        return compile_st_regex(pattern, options.case_sensitive)
            .map(|regex| regex.is_match(query))
            .unwrap_or(false);
    }

    if key.starts_with('/') && key.ends_with('/') && key.len() > 2 {
        return compile_st_regex(&key[1..key.len() - 1], options.case_sensitive)
            .map(|regex| regex.is_match(query))
            .unwrap_or(false);
    }

    if let Some((pattern, flags)) = parse_slash_regex(key) {
        let case_sensitive = options.case_sensitive && !flags.contains('i');
        return compile_st_regex(pattern, case_sensitive)
            .map(|regex| regex.is_match(query))
            .unwrap_or(false);
    }

    text_contains_key(query, key, options)
}

fn parse_slash_regex(key: &str) -> Option<(&str, &str)> {
    if !key.starts_with('/') {
        return None;
    }
    let last = key.rfind('/')?;
    if last == 0 {
        return None;
    }
    Some((&key[1..last], &key[last + 1..]))
}

fn compile_st_regex(pattern: &str, case_sensitive: bool) -> Result<Regex, regex::Error> {
    if case_sensitive {
        Regex::new(pattern)
    } else {
        Regex::new(&format!("(?i:{})", pattern))
    }
}

fn text_contains_key(query: &str, key: &str, options: WorldMatchOptions) -> bool {
    if options.case_sensitive {
        if options.match_whole_words {
            contains_whole_word(query, key)
        } else {
            query.contains(key)
        }
    } else {
        let query = query.to_lowercase();
        let key = key.to_lowercase();
        if options.match_whole_words {
            contains_whole_word(&query, &key)
        } else {
            query.contains(&key)
        }
    }
}

fn contains_whole_word(query: &str, key: &str) -> bool {
    if key.chars().any(|ch| !is_word_boundary_char(ch)) {
        return query.contains(key);
    }

    for (start, _) in query.match_indices(key) {
        let end = start + key.len();
        let before = query[..start].chars().next_back();
        let after = query[end..].chars().next();
        if before.map(is_word_boundary_char).unwrap_or(false)
            || after.map(is_word_boundary_char).unwrap_or(false)
        {
            continue;
        }
        return true;
    }
    false
}

fn is_word_boundary_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
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
    use crate::application::dto::{default_prompt_entries, ContextStrategy, PresetPromptEntry};

    fn history_msg(id: &str, depth: usize, content: &str) -> HistoryMessage {
        HistoryMessage {
            id: Some(id.to_string()),
            role: "assistant".to_string(),
            content: content.to_string(),
            depth,
        }
    }

    fn world_entry(id: &str, keys: Vec<&str>, content: &str) -> WorldEntry {
        WorldEntry {
            id: Some(id.to_string()),
            enabled: true,
            keys: keys.into_iter().map(str::to_string).collect(),
            content: content.to_string(),
            secondary_keys: Vec::new(),
            enable_semantic_search: false,
            constant: false,
            selective: false,
            selective_logic: "and_any".to_string(),
            case_sensitive: None,
            match_whole_words: None,
            scan_depth: None,
            probability: 100.0,
            recursive: true,
            prevent_recursion: false,
            delay_until_recursion: false,
            insertion_depth: None,
            position: "auto".to_string(),
            order: 0,
            role: "system".to_string(),
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
                placement: Some("prompt".to_string()),
                target: "history".to_string(),
                depth_range: vec![15, 999],
                pattern: "<text>[\\s\\S]*?</text>\\n<zongjie>([\\s\\S]*?)</zongjie>".to_string(),
                replacement: "<zongjie>$1</zongjie>".to_string(),
                flags: String::new(),
                sample: String::new(),
                description: String::new(),
                markdown_only: false,
                prompt_only: false,
                run_on_edit: false,
            }],
            ..PipelineConfig::default()
        };

        let mut debug = Vec::new();
        apply_prompt_regex_to_history(&mut history, &pipeline, "gpt-4", &mut debug).unwrap();
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
        let mut entry = world_entry("lore", Vec::new(), "lore");
        assert_eq!(world_position(&entry), "relative");
        entry.insertion_depth = Some(2);
        assert_eq!(world_position(&entry), "in_chat");
        entry.position = "top".to_string();
        assert_eq!(world_position(&entry), "relative");
    }

    #[test]
    fn world_key_matching_supports_regex_case_and_whole_words() {
        let loose = WorldMatchOptions {
            case_sensitive: false,
            match_whole_words: false,
        };
        assert!(key_matches_with_options("Draw the Sword", "sword", loose));
        assert!(key_matches_with_options(
            "alpha 123",
            r"/alpha\s+\d+/i",
            loose
        ));

        let strict = WorldMatchOptions {
            case_sensitive: true,
            match_whole_words: true,
        };
        assert!(!key_matches_with_options("Draw the Sword", "sword", strict));
        assert!(!key_matches_with_options("swordsman", "sword", strict));
        assert!(key_matches_with_options("take sword now", "sword", strict));
    }

    #[test]
    fn world_secondary_logic_matches_st_filters() {
        let mut entry = world_entry("lore", vec!["alpha"], "lore");
        entry.selective = true;
        entry.secondary_keys = vec!["beta".to_string(), "gamma".to_string()];
        let options = WorldMatchOptions {
            case_sensitive: false,
            match_whole_words: false,
        };

        entry.selective_logic = "and_any".to_string();
        assert!(secondary_keys_pass(&entry, "alpha beta", options));
        entry.selective_logic = "and_all".to_string();
        assert!(!secondary_keys_pass(&entry, "alpha beta", options));
        assert!(secondary_keys_pass(&entry, "alpha beta gamma", options));
        entry.selective_logic = "not_any".to_string();
        assert!(!secondary_keys_pass(&entry, "alpha beta", options));
        assert!(secondary_keys_pass(&entry, "alpha delta", options));
        entry.selective_logic = "not_all".to_string();
        assert!(secondary_keys_pass(&entry, "alpha beta", options));
        assert!(!secondary_keys_pass(&entry, "alpha beta gamma", options));
    }

    #[test]
    fn world_budget_keeps_constants_then_order_and_marks_skips() {
        let mut constant = world_entry("constant", Vec::new(), "one");
        constant.constant = true;
        constant.order = 100;
        let mut early = world_entry("early", vec!["a"], "two");
        early.order = 0;
        let mut late = world_entry("late", vec!["b"], "three four five six seven eight");
        late.order = 200;

        let triggers = vec![
            WorldTrigger {
                entry: late,
                trigger: "keyword:b".to_string(),
                similarity: None,
                recursion_depth: 0,
                index: 2,
                included: true,
            },
            WorldTrigger {
                entry: early,
                trigger: "keyword:a".to_string(),
                similarity: None,
                recursion_depth: 0,
                index: 1,
                included: true,
            },
            WorldTrigger {
                entry: constant,
                trigger: "constant".to_string(),
                similarity: None,
                recursion_depth: 0,
                index: 0,
                included: true,
            },
        ];
        let strategy = ContextStrategy {
            world_budget_tokens: Some(2),
            ..ContextStrategy::default()
        };
        let result = apply_world_budget(triggers, 100, &strategy, "gpt-4").unwrap();

        assert_eq!(result[0].entry.id.as_deref(), Some("constant"));
        assert!(result[0].included);
        assert_eq!(result[1].entry.id.as_deref(), Some("early"));
        assert!(result[1].included);
        assert_eq!(result[2].entry.id.as_deref(), Some("late"));
        assert!(!result[2].included);
        assert!(result[2].trigger.starts_with("budget_skipped:"));
    }

    #[tokio::test]
    async fn world_entries_trigger_recursively_and_respect_prevent_recursion() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        let repo = SqliteRepo::new(pool);
        let strategy = ContextStrategy {
            world_budget_tokens: Some(1_000),
            ..ContextStrategy::default()
        };
        let first = world_entry("a", vec!["alpha"], "beta appears here");
        let second = world_entry("b", vec!["beta"], "recursive lore");

        let result = trigger_world_entries(
            &repo,
            "cart",
            &[first.clone(), second.clone()],
            "alpha",
            &strategy,
            4_000,
            "gpt-4",
            0.75,
        )
        .await
        .unwrap();
        assert_eq!(result.iter().filter(|item| item.included).count(), 2);
        assert!(result
            .iter()
            .any(|item| item.entry.id.as_deref() == Some("b")
                && item.trigger.starts_with("recursive:")));

        let mut blocked = first;
        blocked.prevent_recursion = true;
        let result = trigger_world_entries(
            &repo,
            "cart",
            &[blocked, second],
            "alpha",
            &strategy,
            4_000,
            "gpt-4",
            0.75,
        )
        .await
        .unwrap();
        assert_eq!(result.iter().filter(|item| item.included).count(), 1);
    }

    #[tokio::test]
    async fn world_trigger_skips_disabled_probability_zero_and_delay_until_recursion() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        let repo = SqliteRepo::new(pool);
        let strategy = ContextStrategy {
            world_budget_tokens: Some(1_000),
            ..ContextStrategy::default()
        };
        let mut disabled = world_entry("disabled", vec!["alpha"], "disabled");
        disabled.enabled = false;
        let mut never = world_entry("never", vec!["alpha"], "never");
        never.probability = 0.0;
        let mut delayed = world_entry("delayed", vec!["alpha"], "delayed");
        delayed.delay_until_recursion = true;

        let result = trigger_world_entries(
            &repo,
            "cart",
            &[disabled, never, delayed],
            "alpha",
            &strategy,
            4_000,
            "gpt-4",
            0.75,
        )
        .await
        .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn sillytavern_macros_are_replaced() {
        let vars = st_macro_engine::MacroContext {
            user_name: "Alice".to_string(),
            char_name: "Soyo".to_string(),
            input: "hello".to_string(),
            last_message_id: None,
            variables: HashMap::new(),
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
