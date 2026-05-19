use crate::application::dto::{
    default_prompt_entries, Manifest, PipelineConfig, PresetConfig, PromptDryRunResult, WorldEntry,
};
use crate::application::services::chat_completion_service;
use crate::infrastructure::credentials::CredentialService;
use crate::infrastructure::database::{CartridgeRow, ChatRow, MessageRow};
use crate::infrastructure::fs;
use crate::infrastructure::provider_registry::get_provider;
use crate::AppState;
use chrono::Utc;
use futures::StreamExt;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::WebviewWindowBuilder;
use uuid::Uuid;

// ── Open / Close ─────────────────────────────────────────

#[tauri::command]
pub async fn open_creator(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let workbench_id = Uuid::new_v4().to_string();
    let wb_dir = state.data_dir.join("workbench").join(&workbench_id);

    // Create directory structure
    std::fs::create_dir_all(wb_dir.join("ui"))
        .map_err(|e| format!("Failed to create workbench dir: {}", e))?;
    std::fs::create_dir_all(wb_dir.join("assets"))
        .map_err(|e| format!("Failed to create assets dir: {}", e))?;

    // Write default files
    let default_manifest = Manifest {
        name: "My Character".to_string(),
        author: "Anonymous".to_string(),
        version: "1.0.0".to_string(),
        entry_file: "ui/index.html".to_string(),
        description: String::new(),
        cover_image: String::new(),
    };
    let default_preset = PresetConfig {
        system_prompt: "You are a helpful assistant.".to_string(),
        prompt_entries: default_prompt_entries(),
        model: None,
        temperature: None,
        max_tokens: None,
        context_window_size: None,
        provider: None,
        provider_url: None,
        chat_format: None,
        authors_note: None,
        authors_note_depth: None,
        user_name: Some("User".to_string()),
        char_name: Some("Character".to_string()),
    };
    let default_pipeline = PipelineConfig::default();
    let default_agent = CreatorAgentConfig::default();

    fs::save_json(&wb_dir.join("manifest.json"), &default_manifest)?;
    fs::save_json(&wb_dir.join("preset.json"), &default_preset)?;
    fs::save_json(&wb_dir.join("pipeline.json"), &default_pipeline)?;
    fs::save_json(
        &wb_dir.join("world_info.json"),
        &serde_json::json!({"entries": []}),
    )?;
    fs::save_json(&wb_dir.join("creator_agent.json"), &default_agent)?;

    // Write default UI files
    let ui_html = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <title>My Character</title>
  <link rel="stylesheet" href="style.css" />
  <script src="../tauri-tavern-sdk.js"></script>
  <script defer src="script.js"></script>
</head>
<body>
  <h1>Hello!</h1>
</body>
</html>"#;
    std::fs::write(wb_dir.join("ui").join("index.html"), ui_html)
        .map_err(|e| format!("Failed to write index.html: {}", e))?;
    std::fs::write(
        wb_dir.join("ui").join("script.js"),
        "const SDK = window.TavernSDK;\n\n// Your chat logic here\n",
    )
    .map_err(|e| format!("Failed to write script.js: {}", e))?;
    std::fs::write(
        wb_dir.join("ui").join("style.css"),
        "body {\n  font-family: sans-serif;\n  background: #1a1a2e;\n  color: #e0e0e0;\n  margin: 0;\n  padding: 16px;\n}\n",
    )
    .map_err(|e| format!("Failed to write style.css: {}", e))?;

    // Open creator window via App URL (Vite-bundled, full IPC access)
    let label = format!("creator_{}", workbench_id);
    let url = format!("creator/index.html?workbench={}", workbench_id);

    WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::App(url.into()))
        .title("Character Card Creator")
        .inner_size(1200.0, 800.0)
        .resizable(true)
        .build()
        .map_err(|e| format!("Failed to create creator window: {}", e))?;

    Ok(workbench_id)
}

#[tauri::command]
pub async fn open_creator_for_cartridge(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    cartridge_id: String,
) -> Result<String, String> {
    let cartridge = state
        .repo
        .get_cartridge(&cartridge_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cartridge not found".to_string())?;

    let source_dir = PathBuf::from(&cartridge.directory_path);
    if !source_dir.exists() {
        return Err("Cartridge directory not found".to_string());
    }

    let workbench_id = Uuid::new_v4().to_string();
    let wb_dir = state.data_dir.join("workbench").join(&workbench_id);
    std::fs::create_dir_all(&wb_dir)
        .map_err(|e| format!("Failed to create workbench dir: {}", e))?;
    copy_dir_recursive(&source_dir, &wb_dir)?;
    std::fs::create_dir_all(wb_dir.join("ui"))
        .map_err(|e| format!("Failed to create ui dir: {}", e))?;
    std::fs::create_dir_all(wb_dir.join("assets"))
        .map_err(|e| format!("Failed to create assets dir: {}", e))?;

    if !wb_dir.join("pipeline.json").exists() {
        fs::save_json(&wb_dir.join("pipeline.json"), &PipelineConfig::default())?;
    }
    if !wb_dir.join("world_info.json").exists() {
        fs::save_json(
            &wb_dir.join("world_info.json"),
            &serde_json::json!({ "entries": [] }),
        )?;
    }
    if !wb_dir.join("creator_agent.json").exists() {
        fs::save_json(
            &wb_dir.join("creator_agent.json"),
            &CreatorAgentConfig::default(),
        )?;
    }

    let mut preset = fs::load_preset(&wb_dir).map_err(|e| format!("preset.json: {}", e))?;
    preset.ensure_prompt_entries();
    fs::save_json(&wb_dir.join("preset.json"), &preset)?;

    open_creator_window(&app, &workbench_id)?;
    Ok(workbench_id)
}

#[tauri::command]
pub async fn delete_workbench(
    state: tauri::State<'_, AppState>,
    workbench_id: String,
) -> Result<(), String> {
    let wb_dir = state.data_dir.join("workbench").join(&workbench_id);
    if wb_dir.exists() {
        std::fs::remove_dir_all(&wb_dir)
            .map_err(|e| format!("Failed to delete workbench: {}", e))?;
    }
    // Clean up temp cartridge if exists
    let temp_cart_id = format!("__wb_{}__", workbench_id);
    let _ = state.repo.delete_cartridge(&temp_cart_id).await;
    Ok(())
}

// ── Workbench I/O ────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
pub struct WorkbenchBundle {
    pub manifest: Manifest,
    pub preset: PresetConfig,
    pub world_info: Vec<WorldEntry>,
    pub pipeline: PipelineConfig,
    pub ui_files: HashMap<String, String>,
    pub agent_config: CreatorAgentConfig,
}

#[tauri::command]
pub async fn get_workbench(
    state: tauri::State<'_, AppState>,
    workbench_id: String,
) -> Result<WorkbenchBundle, String> {
    let wb_dir = state.data_dir.join("workbench").join(&workbench_id);

    let manifest = fs::load_json::<Manifest>(&wb_dir.join("manifest.json"))
        .map_err(|e| format!("Failed to load manifest: {}", e))?;
    let preset = fs::load_json::<PresetConfig>(&wb_dir.join("preset.json"))
        .map_err(|e| format!("Failed to load preset: {}", e))?;
    let mut preset = preset;
    preset.ensure_prompt_entries();
    let world_info = fs::load_world_info_entries(&wb_dir)?;
    let pipeline = fs::load_pipeline(&wb_dir).unwrap_or_default();
    let agent_config = load_agent_config(&wb_dir)?;

    let mut ui_files = HashMap::new();
    for filename in &["index.html", "script.js", "style.css"] {
        let path = wb_dir.join("ui").join(filename);
        if path.exists() {
            ui_files.insert(
                filename.to_string(),
                std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read {}: {}", filename, e))?,
            );
        }
    }

    Ok(WorkbenchBundle {
        manifest,
        preset,
        world_info,
        pipeline,
        ui_files,
        agent_config,
    })
}

#[tauri::command]
pub async fn save_workbench_manifest(
    state: tauri::State<'_, AppState>,
    workbench_id: String,
    manifest: Manifest,
) -> Result<(), String> {
    let path = state
        .data_dir
        .join("workbench")
        .join(&workbench_id)
        .join("manifest.json");
    fs::save_json(&path, &manifest)
}

#[tauri::command]
pub async fn save_workbench_preset(
    state: tauri::State<'_, AppState>,
    workbench_id: String,
    mut preset: PresetConfig,
) -> Result<(), String> {
    preset.ensure_prompt_entries();
    let path = state
        .data_dir
        .join("workbench")
        .join(&workbench_id)
        .join("preset.json");
    fs::save_json(&path, &preset)
}

#[tauri::command]
pub async fn save_workbench_world_info(
    state: tauri::State<'_, AppState>,
    workbench_id: String,
    entries: Vec<WorldEntry>,
) -> Result<(), String> {
    let path = state
        .data_dir
        .join("workbench")
        .join(&workbench_id)
        .join("world_info.json");
    // Filter out _disabled internal field before saving
    let clean: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            let mut v = serde_json::to_value(e).unwrap_or_default();
            v.as_object_mut().map(|o| {
                o.remove("_disabled");
            });
            v
        })
        .collect();
    let json_str = serde_json::to_string_pretty(&serde_json::json!({"entries": clean}))
        .map_err(|e| format!("JSON error: {}", e))?;
    std::fs::write(&path, json_str).map_err(|e| format!("Write error: {}", e))
}

#[tauri::command]
pub async fn save_workbench_pipeline(
    state: tauri::State<'_, AppState>,
    workbench_id: String,
    pipeline: PipelineConfig,
) -> Result<(), String> {
    let path = state
        .data_dir
        .join("workbench")
        .join(&workbench_id)
        .join("pipeline.json");
    fs::save_json(&path, &pipeline)
}

#[tauri::command]
pub async fn save_workbench_ui_file(
    state: tauri::State<'_, AppState>,
    workbench_id: String,
    filename: String,
    content: String,
) -> Result<(), String> {
    // Validate filename
    if !["index.html", "script.js", "style.css"].contains(&filename.as_str()) {
        return Err(format!("Invalid UI filename: {}", filename));
    }
    let path = state
        .data_dir
        .join("workbench")
        .join(&workbench_id)
        .join("ui")
        .join(&filename);
    std::fs::write(&path, &content).map_err(|e| format!("Write error: {}", e))
}

#[tauri::command]
pub async fn import_workbench_cover_image(
    state: tauri::State<'_, AppState>,
    workbench_id: String,
    source_path: String,
) -> Result<Manifest, String> {
    let wb_dir = state.data_dir.join("workbench").join(&workbench_id);
    if !wb_dir.exists() {
        return Err("Workbench not found".to_string());
    }

    let source = parse_dialog_path(&source_path)?;
    let ext = source
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .ok_or_else(|| "Selected file has no extension".to_string())?;
    if !is_supported_cover_extension(&ext) {
        return Err(format!("Unsupported cover image type: {}", ext));
    }

    let assets_dir = wb_dir.join("assets");
    std::fs::create_dir_all(&assets_dir)
        .map_err(|e| format!("Failed to create assets dir: {}", e))?;

    let dest = unique_asset_path(&assets_dir, "cover", &ext);
    if source != dest {
        std::fs::copy(&source, &dest).map_err(|e| {
            format!(
                "Failed to copy cover image from {}: {}",
                source.display(),
                e
            )
        })?;
    }

    let mut manifest: Manifest = fs::load_json(&wb_dir.join("manifest.json"))?;
    let filename = dest
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Invalid copied cover filename".to_string())?;
    manifest.cover_image = format!("assets/{}", filename);
    fs::save_json(&wb_dir.join("manifest.json"), &manifest)?;
    Ok(manifest)
}

// ── Creator Agent ────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CreatorAgentConfig {
    pub provider: String,
    pub model: String,
    pub provider_url: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<usize>,
}

impl Default for CreatorAgentConfig {
    fn default() -> Self {
        let provider = "openai".to_string();
        let model = get_provider(&provider)
            .map(|info| info.default_model)
            .unwrap_or_else(|| "gpt-4o".to_string());
        Self {
            provider,
            model,
            provider_url: None,
            temperature: Some(0.2),
            max_tokens: Some(4096),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct CreatorAgentChatRequest {
    pub workbench_id: String,
    pub message: String,
    #[serde(default)]
    pub history: Vec<crate::application::dto::ChatMessage>,
}

#[derive(Debug, serde::Serialize)]
pub struct CreatorAgentChatResponse {
    pub reply: String,
    pub applied: Vec<String>,
    pub workbench: WorkbenchBundle,
}

#[derive(Debug, serde::Deserialize)]
struct CreatorAgentModelOutput {
    reply: String,
    #[serde(default)]
    updates: CreatorAgentUpdates,
}

#[derive(Debug, Default, serde::Deserialize)]
struct CreatorAgentUpdates {
    manifest: Option<Manifest>,
    preset: Option<PresetConfig>,
    world_info: Option<Vec<WorldEntry>>,
    pipeline: Option<PipelineConfig>,
    ui_files: Option<HashMap<String, String>>,
}

#[tauri::command]
pub async fn save_creator_agent_config(
    state: tauri::State<'_, AppState>,
    workbench_id: String,
    config: CreatorAgentConfig,
) -> Result<(), String> {
    validate_agent_config(&config)?;
    let path = state
        .data_dir
        .join("workbench")
        .join(&workbench_id)
        .join("creator_agent.json");
    fs::save_json(&path, &config)
}

#[tauri::command]
pub async fn creator_agent_chat(
    state: tauri::State<'_, AppState>,
    request: CreatorAgentChatRequest,
) -> Result<CreatorAgentChatResponse, String> {
    let wb_dir = state.data_dir.join("workbench").join(&request.workbench_id);
    if !wb_dir.exists() {
        return Err("Workbench not found".to_string());
    }

    let config = load_agent_config(&wb_dir)?;
    validate_agent_config(&config)?;
    let api_key = CredentialService::get(&state.repo, &config.provider)
        .await?
        .ok_or_else(|| {
            format!(
                "No API key configured for '{}'. Open Agent Settings and save a key.",
                config.provider
            )
        })?;

    let manifest = fs::load_json::<Manifest>(&wb_dir.join("manifest.json"))?;
    let mut preset = fs::load_json::<PresetConfig>(&wb_dir.join("preset.json"))?;
    preset.ensure_prompt_entries();
    let world_info = fs::load_world_info_entries(&wb_dir)?;
    let pipeline = fs::load_pipeline(&wb_dir).unwrap_or_default();
    let ui_files = read_ui_files(&wb_dir)?;

    let system_prompt = build_creator_agent_system_prompt();
    let snapshot = serde_json::json!({
        "manifest": manifest,
        "preset": preset,
        "world_info": world_info,
        "pipeline": pipeline,
        "ui_files": ui_files,
    });

    let mut messages = vec![crate::application::dto::ChatMessage {
        role: "system".to_string(),
        content: system_prompt,
    }];

    for item in request
        .history
        .into_iter()
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        if (item.role == "user" || item.role == "assistant") && !item.content.trim().is_empty() {
            messages.push(item);
        }
    }
    messages.push(crate::application::dto::ChatMessage {
        role: "user".to_string(),
        content: format!(
            "Current workbench JSON:\n{}\n\nUser request:\n{}",
            serde_json::to_string_pretty(&snapshot).map_err(|e| e.to_string())?,
            request.message
        ),
    });

    let agent_preset = PresetConfig {
        system_prompt: String::new(),
        prompt_entries: Vec::new(),
        provider: Some(config.provider.clone()),
        model: Some(config.model.clone()),
        provider_url: config.provider_url.clone(),
        temperature: config.temperature,
        max_tokens: config.max_tokens,
        context_window_size: None,
        chat_format: None,
        authors_note: None,
        authors_note_depth: None,
        user_name: None,
        char_name: None,
    };

    let client = crate::infrastructure::apis::LlmHttpClient::new();
    let mut stream = client.stream_chat(&agent_preset, &messages, &api_key, &config.provider_url);
    let mut raw = String::new();
    while let Some(chunk) = stream.next().await {
        raw.push_str(&chunk?);
    }

    let parsed = parse_creator_agent_output(&raw)?;
    let applied = apply_creator_agent_updates(&wb_dir, parsed.updates)?;
    let workbench = load_workbench_bundle_from_dir(&wb_dir)?;

    Ok(CreatorAgentChatResponse {
        reply: parsed.reply,
        applied,
        workbench,
    })
}

// ── Test Chat ────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
pub struct TestChatWorkbenchResponse {
    pub reply: String,
    pub dry_run: PromptDryRunResult,
}

#[tauri::command]
pub async fn test_chat_workbench(
    state: tauri::State<'_, AppState>,
    workbench_id: String,
    message: String,
) -> Result<TestChatWorkbenchResponse, String> {
    let wb_dir = state.data_dir.join("workbench").join(&workbench_id);
    let temp_cart_id = format!("__wb_{}__", workbench_id);

    // Register temp cartridge if not already done
    if state
        .repo
        .get_cartridge(&temp_cart_id)
        .await
        .map_err(|e| e.to_string())?
        .is_none()
    {
        let manifest = fs::load_json::<Manifest>(&wb_dir.join("manifest.json"))
            .map_err(|e| format!("manifest.json: {}", e))?;
        let now = Utc::now().to_rfc3339();
        let row = CartridgeRow {
            id: temp_cart_id.clone(),
            name: manifest.name.clone(),
            author: manifest.author.clone(),
            description: manifest.description.clone(),
            version: manifest.version.clone(),
            entry_file: manifest.entry_file.clone(),
            cover_image: String::new(),
            installed_at: now.clone(),
            directory_path: wb_dir.to_string_lossy().to_string(),
        };
        state
            .repo
            .insert_cartridge(&row)
            .await
            .map_err(|e| e.to_string())?;
    }

    // Create a chat for this test session (or reuse)
    let chat_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    state
        .repo
        .create_chat(&ChatRow {
            id: chat_id.clone(),
            cartridge_id: temp_cart_id.clone(),
            title: "Test Chat".to_string(),
            created_at: now.clone(),
            updated_at: now,
        })
        .await
        .map_err(|e| e.to_string())?;

    // Save user message
    state
        .repo
        .insert_message(&MessageRow {
            id: Uuid::new_v4().to_string(),
            chat_id: chat_id.clone(),
            role: "user".to_string(),
            content: message.clone(),
            created_at: Utc::now().to_rfc3339(),
        })
        .await
        .map_err(|e| e.to_string())?;

    // We can't easily get the window reference here for streaming,
    // so we call chat_completion_service::handle_chat directly but without event emission.
    // For now, use a simplified approach: read the active profile, make the LLM call,
    // and return the full response.

    let wb_dir_clone = wb_dir.clone();
    let temp_cart_id_clone = temp_cart_id.clone();
    let chat_id_clone = chat_id.clone();

    // Use the existing handle_chat infrastructure
    // Need a window for event emission — create a dummy approach
    // Actually, let's build the prompt manually and call the LLM client

    let preset = fs::load_preset(&wb_dir_clone).map_err(|e| format!("preset.json: {}", e))?;

    // Resolve active profile
    let active_pid = state.active_profile_id.lock().unwrap().clone();
    let (provider, model, api_key, api_url_override) =
        chat_completion_service::resolve_chat_config(&state, &preset, active_pid.as_deref())
            .await?;

    // Apply profile overrides to preset
    let preset = PresetConfig {
        provider: Some(provider.clone()),
        model: Some(model),
        provider_url: api_url_override.clone(),
        prompt_entries: preset.prompt_entries.clone(),
        ..preset
    };

    // Build messages
    let rendered = crate::application::services::prompt_engine::render_prompt(
        &state.repo,
        &temp_cart_id_clone,
        &chat_id_clone,
        &wb_dir_clone,
        &preset,
        &preset
            .char_name
            .clone()
            .unwrap_or_else(|| "Character".to_string()),
        &message,
        None,
    )
    .await?;

    // Make the LLM call
    let client = crate::infrastructure::apis::LlmHttpClient::new();
    let api_url_ref = &api_url_override;
    let mut stream = client.stream_chat(&preset, &rendered.messages, &api_key, api_url_ref);

    let mut full_response = String::new();
    use futures::StreamExt;
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(text) => full_response.push_str(&text),
            Err(e) => return Err(e),
        }
    }

    // Save assistant response
    let _ = state
        .repo
        .insert_message(&MessageRow {
            id: Uuid::new_v4().to_string(),
            chat_id: chat_id_clone,
            role: "assistant".to_string(),
            content: full_response.clone(),
            created_at: Utc::now().to_rfc3339(),
        })
        .await;

    Ok(TestChatWorkbenchResponse {
        reply: full_response,
        dry_run: rendered.dry_run,
    })
}

// ── Export ───────────────────────────────────────────────

#[tauri::command]
pub async fn export_workbench(
    state: tauri::State<'_, AppState>,
    workbench_id: String,
) -> Result<String, String> {
    let wb_dir = state.data_dir.join("workbench").join(&workbench_id);

    // Validate required files
    if !wb_dir.join("manifest.json").exists() {
        return Err("manifest.json is missing".to_string());
    }
    if !wb_dir.join("preset.json").exists() {
        return Err("preset.json is missing".to_string());
    }

    let manifest: Manifest = fs::load_json(&wb_dir.join("manifest.json"))?;
    let safe_name = sanitize_filename(&manifest.name);

    // Copy SDK into workbench
    let sdk_content = include_str!("../../../../tauri-tavern-sdk.js");
    std::fs::write(wb_dir.join("tauri-tavern-sdk.js"), sdk_content)
        .map_err(|e| format!("Failed to copy SDK: {}", e))?;

    // Create .taurichar (zip) in the workbench directory
    let output_path = state.data_dir.join(format!("{}.taurichar", safe_name));
    create_zip(&wb_dir, &output_path)?;

    Ok(output_path.to_string_lossy().to_string())
}

// ── Helpers ──────────────────────────────────────────────

fn load_agent_config(wb_dir: &PathBuf) -> Result<CreatorAgentConfig, String> {
    let path = wb_dir.join("creator_agent.json");
    if path.exists() {
        fs::load_json::<CreatorAgentConfig>(&path)
    } else {
        Ok(CreatorAgentConfig::default())
    }
}

fn validate_agent_config(config: &CreatorAgentConfig) -> Result<(), String> {
    if get_provider(&config.provider).is_none() {
        return Err(format!("Unknown provider: {}", config.provider));
    }
    if config.model.trim().is_empty() {
        return Err("Agent model is required".to_string());
    }
    if let Some(url) = &config.provider_url {
        if url.trim().is_empty() {
            return Err("Provider URL must be omitted or non-empty".to_string());
        }
    }
    Ok(())
}

fn read_ui_files(wb_dir: &PathBuf) -> Result<HashMap<String, String>, String> {
    let mut ui_files = HashMap::new();
    for filename in &["index.html", "script.js", "style.css"] {
        let path = wb_dir.join("ui").join(filename);
        if path.exists() {
            ui_files.insert(
                filename.to_string(),
                std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read {}: {}", filename, e))?,
            );
        }
    }
    Ok(ui_files)
}

fn load_workbench_bundle_from_dir(wb_dir: &PathBuf) -> Result<WorkbenchBundle, String> {
    let mut preset = fs::load_json::<PresetConfig>(&wb_dir.join("preset.json"))?;
    preset.ensure_prompt_entries();
    Ok(WorkbenchBundle {
        manifest: fs::load_json::<Manifest>(&wb_dir.join("manifest.json"))?,
        preset,
        world_info: fs::load_world_info_entries(wb_dir)?,
        pipeline: fs::load_pipeline(wb_dir).unwrap_or_default(),
        ui_files: read_ui_files(wb_dir)?,
        agent_config: load_agent_config(wb_dir)?,
    })
}

fn open_creator_window(app: &tauri::AppHandle, workbench_id: &str) -> Result<(), String> {
    let label = format!("creator_{}", workbench_id);
    let url = format!("creator/index.html?workbench={}", workbench_id);

    WebviewWindowBuilder::new(app, &label, tauri::WebviewUrl::App(url.into()))
        .title("Character Card Creator")
        .inner_size(1200.0, 800.0)
        .resizable(true)
        .build()
        .map_err(|e| format!("Failed to create creator window: {}", e))?;
    Ok(())
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<(), String> {
    let canonical_source = source
        .canonicalize()
        .map_err(|e| format!("Failed to resolve source dir: {}", e))?;
    std::fs::create_dir_all(target).map_err(|e| format!("Failed to create target dir: {}", e))?;

    fn copy_inner(base: &Path, current: &Path, target: &Path) -> Result<(), String> {
        let canonical_current = current
            .canonicalize()
            .map_err(|e| format!("Failed to resolve source path: {}", e))?;
        if !canonical_current.starts_with(base) {
            return Err("Source path traversal detected".to_string());
        }

        for entry in std::fs::read_dir(current).map_err(|e| format!("Read dir error: {}", e))? {
            let entry = entry.map_err(|e| format!("Dir entry error: {}", e))?;
            let path = entry.path();
            let dest = target.join(entry.file_name());
            if path.is_dir() {
                std::fs::create_dir_all(&dest)
                    .map_err(|e| format!("Failed to create copied dir: {}", e))?;
                copy_inner(base, &path, &dest)?;
            } else if path.is_file() {
                std::fs::copy(&path, &dest).map_err(|e| {
                    format!(
                        "Failed to copy {} to {}: {}",
                        path.display(),
                        dest.display(),
                        e
                    )
                })?;
            }
        }
        Ok(())
    }

    copy_inner(&canonical_source, &canonical_source, target)
}

fn parse_dialog_path(raw: &str) -> Result<PathBuf, String> {
    if let Ok(url) = url::Url::parse(raw) {
        if url.scheme() == "file" {
            return url
                .to_file_path()
                .map_err(|_| format!("Invalid file URL: {}", raw));
        }
    }
    Ok(PathBuf::from(raw))
}

fn unique_asset_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{}.{}", stem, ext));
    if !first.exists() {
        return first;
    }
    for idx in 1..10_000 {
        let candidate = dir.join(format!("{}-{}.{}", stem, idx, ext));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{}-{}.{}", stem, Uuid::new_v4(), ext))
}

fn is_supported_cover_extension(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg")
}

fn build_creator_agent_system_prompt() -> String {
    r#"You are the Creator Mode agent inside TauriTavern Console. Help the user edit the current character card workbench.

You may directly update:
- manifest metadata
- preset prompt_entries and model-facing character settings
- world_info entries, including keys, secondary_keys, insertion_depth, position, order, enable_semantic_search, enabled, role, and content
- pipeline context strategy and regex mutators
- UI files: index.html, script.js, style.css

Return exactly one JSON object. No markdown fences, no prose outside JSON.
Schema:
{
  "reply": "short user-facing explanation",
  "updates": {
    "manifest": null or full manifest object,
    "preset": null or full preset object,
    "world_info": null or full array of world entries,
    "pipeline": null or full pipeline object,
    "ui_files": null or object with any of index.html, script.js, style.css
  }
}

Only include changed full objects. Preserve existing fields unless the user asked to change them.
Preset prompt_entries use: id, name, enabled, role, content, position ("relative" or "in_chat"), depth, order, triggers, pinned.
World info position uses "auto", "top", "relative", or "in_chat".
Every world entry must include enabled true or false. Disable entries by setting enabled to false.
Regex mutators use: id, enabled, target ("history" for prompt cleanup or "display" for frontend replacement), depth_range, pattern, replacement, flags, sample, description.
Use target "display" for SillyTavern-style frontend regex that converts message tags into HTML/GUI code. Use target "history" only when the replacement should be sent to the model.
Do not invent unsupported files or commands. Keep UI code self-contained and compatible with the TavernSDK already referenced by the preview."#.to_string()
}

fn parse_creator_agent_output(raw: &str) -> Result<CreatorAgentModelOutput, String> {
    let trimmed = raw.trim();
    let json_text = if trimmed.starts_with("```") {
        let without_start = trimmed
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim();
        without_start
            .strip_suffix("```")
            .unwrap_or(without_start)
            .trim()
            .to_string()
    } else if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        trimmed[start..=end].to_string()
    } else {
        trimmed.to_string()
    };

    serde_json::from_str::<CreatorAgentModelOutput>(&json_text)
        .map_err(|e| format!("Agent returned invalid JSON: {}\nRaw response: {}", e, raw))
}

fn apply_creator_agent_updates(
    wb_dir: &PathBuf,
    updates: CreatorAgentUpdates,
) -> Result<Vec<String>, String> {
    let mut applied = Vec::new();

    if let Some(manifest) = updates.manifest {
        fs::save_json(&wb_dir.join("manifest.json"), &manifest)?;
        applied.push("metadata".to_string());
    }
    if let Some(mut preset) = updates.preset {
        preset.ensure_prompt_entries();
        fs::save_json(&wb_dir.join("preset.json"), &preset)?;
        applied.push("preset".to_string());
    }
    if let Some(entries) = updates.world_info {
        let value = serde_json::json!({ "entries": entries });
        fs::save_json(&wb_dir.join("world_info.json"), &value)?;
        applied.push("world_info".to_string());
    }
    if let Some(pipeline) = updates.pipeline {
        fs::save_json(&wb_dir.join("pipeline.json"), &pipeline)?;
        applied.push("pipeline".to_string());
    }
    if let Some(ui_files) = updates.ui_files {
        for (filename, content) in ui_files {
            if !["index.html", "script.js", "style.css"].contains(&filename.as_str()) {
                return Err(format!(
                    "Agent tried to write unsupported UI file: {}",
                    filename
                ));
            }
            std::fs::write(wb_dir.join("ui").join(&filename), content)
                .map_err(|e| format!("Write error for {}: {}", filename, e))?;
        }
        applied.push("ui_code".to_string());
    }

    Ok(applied)
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .replace(' ', "_")
}

fn create_zip(dir: &PathBuf, output: &PathBuf) -> Result<(), String> {
    use std::io::Write;
    let file = std::fs::File::create(output).map_err(|e| format!("Failed to create zip: {}", e))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();

    fn add_dir(
        zip: &mut zip::ZipWriter<std::fs::File>,
        base: &PathBuf,
        rel: &PathBuf,
        options: zip::write::SimpleFileOptions,
    ) -> Result<(), String> {
        let full = base.join(rel);
        for entry in std::fs::read_dir(&full).map_err(|e| format!("Read dir error: {}", e))? {
            let entry = entry.map_err(|e| format!("Entry error: {}", e))?;
            let path = entry.path();
            let rel_path = rel.join(entry.file_name());
            if path.is_dir() {
                zip.add_directory(rel_path.to_string_lossy().replace('\\', "/"), options)
                    .map_err(|e| format!("Zip add dir error: {}", e))?;
                add_dir(zip, base, &rel_path, options)?;
            } else if path.is_file() {
                zip.start_file(rel_path.to_string_lossy().replace('\\', "/"), options)
                    .map_err(|e| format!("Zip start file error: {}", e))?;
                let data = std::fs::read(&path).map_err(|e| format!("Read file error: {}", e))?;
                zip.write_all(&data)
                    .map_err(|e| format!("Zip write error: {}", e))?;
            }
        }
        Ok(())
    }

    add_dir(&mut zip, dir, &PathBuf::new(), options)?;
    zip.finish()
        .map_err(|e| format!("Zip finish error: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_extension_allowlist_accepts_images_only() {
        assert!(is_supported_cover_extension("png"));
        assert!(is_supported_cover_extension("jpeg"));
        assert!(is_supported_cover_extension("svg"));
        assert!(!is_supported_cover_extension("exe"));
        assert!(!is_supported_cover_extension("../png"));
    }

    #[test]
    fn file_url_paths_are_supported_for_dialog_results() {
        let path = if cfg!(windows) {
            "file:///C:/tmp/cover.png"
        } else {
            "file:///tmp/cover.png"
        };
        let parsed = parse_dialog_path(path).unwrap();
        assert_eq!(parsed.extension().and_then(|ext| ext.to_str()), Some("png"));
    }
}
