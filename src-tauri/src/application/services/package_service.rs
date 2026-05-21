use crate::application::dto::{
    default_prompt_entries, CartridgeInfo, Manifest, PipelineConfig, PresetConfig, RegexMutator,
    WorldEntry,
};
use crate::application::services::prompt_engine;
use crate::infrastructure::database::{CartridgeRow, SqliteRepo};
use crate::infrastructure::fs;
use serde_json::Value;
use std::path::Path;
use uuid::Uuid;
use zip::ZipArchive;

pub async fn import_cartridge(
    repo: &SqliteRepo,
    file_path: &str,
    data_dir: &Path,
) -> Result<CartridgeInfo, String> {
    let lower_path = file_path.to_ascii_lowercase();
    if lower_path.ends_with(".png") || lower_path.ends_with(".webp") {
        return import_sillytavern_card_image(repo, file_path, data_dir).await;
    }

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

async fn import_sillytavern_card_image(
    repo: &SqliteRepo,
    file_path: &str,
    data_dir: &Path,
) -> Result<CartridgeInfo, String> {
    let image_bytes =
        std::fs::read(file_path).map_err(|e| format!("Failed to read ST card image: {}", e))?;
    let parsed_card = fs::parse_sillytavern_card_image(&image_bytes)?;
    let avatar_path = format!("assets/avatar.{}", parsed_card.avatar_extension());
    let card_json = parsed_card.raw_json;
    let data = card_json.get("data").unwrap_or(&card_json);

    let name = get_string(data, "name")
        .or_else(|| get_string(&card_json, "name"))
        .unwrap_or_else(|| "SillyTavern Character".to_string());
    let description = get_string(data, "description").unwrap_or_default();
    let personality = get_string(data, "personality").unwrap_or_default();
    let scenario = get_string(data, "scenario").unwrap_or_default();
    let first_mes = get_string(data, "first_mes").unwrap_or_else(|| {
        get_string(data, "first_message").unwrap_or_else(|| "Hello.".to_string())
    });
    let mes_example = get_string(data, "mes_example").unwrap_or_default();
    let creator_notes = get_string(data, "creator_notes").unwrap_or_default();
    let system_prompt = get_string(data, "system_prompt")
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            build_classic_system_prompt(&name, &description, &personality, &scenario, &mes_example)
        });
    let post_history = get_string(data, "post_history_instructions").unwrap_or_default();
    let alternate_greetings = data
        .get("alternate_greetings")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let cartridge_id = Uuid::new_v4().to_string();
    let dest_dir = data_dir.join("cartridges").join(&cartridge_id);
    std::fs::create_dir_all(dest_dir.join("assets"))
        .map_err(|e| format!("Failed to create cartridge directory: {}", e))?;
    std::fs::create_dir_all(dest_dir.join("ui"))
        .map_err(|e| format!("Failed to create UI directory: {}", e))?;

    std::fs::write(dest_dir.join(&avatar_path), &parsed_card.avatar_bytes)
        .map_err(|e| format!("Failed to copy avatar image: {}", e))?;
    fs::save_json(&dest_dir.join("sillytavern_card.json"), &card_json)?;
    if let Some(version_hint) = parsed_card.version_hint.as_deref() {
        log::debug!(
            "Imported SillyTavern card with version hint: {}",
            version_hint
        );
    }

    let manifest = Manifest {
        name: name.clone(),
        author: get_string(data, "creator").unwrap_or_else(|| "SillyTavern".to_string()),
        version: get_string(data, "character_version").unwrap_or_else(|| "1.0.0".to_string()),
        entry_file: "ui/index.html".to_string(),
        description: if creator_notes.trim().is_empty() {
            description.clone()
        } else {
            creator_notes.clone()
        },
        cover_image: avatar_path.clone(),
    };

    let mut prompt_entries = default_prompt_entries();
    if let Some(main) = prompt_entries
        .iter_mut()
        .find(|entry| entry.id == "main_prompt")
    {
        main.content = system_prompt.clone();
        main.enabled = true;
    }
    if let Some(post) = prompt_entries
        .iter_mut()
        .find(|entry| entry.id == "post_history_instructions")
    {
        post.content = post_history.clone();
        post.enabled = !post.content.trim().is_empty();
        post.depth = Some(0);
    }

    let preset = PresetConfig {
        system_prompt: system_prompt.clone(),
        prompt_entries,
        model: None,
        temperature: None,
        max_tokens: None,
        context_window_size: None,
        provider: None,
        provider_url: None,
        chat_format: None,
        authors_note: None,
        authors_note_depth: None,
        user_name: Some("You".to_string()),
        char_name: Some(name.clone()),
    };

    let world_entries = extract_character_book_entries(data);
    fs::save_json(&dest_dir.join("manifest.json"), &manifest)?;
    fs::save_json(&dest_dir.join("preset.json"), &preset)?;
    let pipeline = PipelineConfig {
        regex_mutators: extract_display_regex_mutators(data),
        ..PipelineConfig::default()
    };
    fs::save_json(&dest_dir.join("pipeline.json"), &pipeline)?;
    fs::save_json(
        &dest_dir.join("world_info.json"),
        &serde_json::json!({ "entries": world_entries }),
    )?;
    write_classic_ui(
        &dest_dir,
        &ClassicCardData {
            name: name.clone(),
            description,
            personality,
            scenario,
            first_mes,
            mes_example,
            creator_notes,
            alternate_greetings,
            avatar_path: avatar_path.clone(),
        },
    )?;
    copy_sdk_to_cartridge(&dest_dir)?;

    let now = chrono::Utc::now().to_rfc3339();
    let row = CartridgeRow {
        id: cartridge_id.clone(),
        name: manifest.name.clone(),
        author: manifest.author.clone(),
        description: manifest.description.clone(),
        version: manifest.version.clone(),
        entry_file: manifest.entry_file.clone(),
        cover_image: manifest.cover_image.clone(),
        installed_at: now.clone(),
        directory_path: dest_dir.to_string_lossy().to_string(),
    };
    repo.insert_cartridge(&row)
        .await
        .map_err(|e| format!("Database error: {}", e))?;
    prompt_engine::spawn_static_world_index(repo.clone(), cartridge_id.clone(), dest_dir.clone());

    Ok(CartridgeInfo {
        id: cartridge_id,
        name: manifest.name,
        author: manifest.author,
        description: manifest.description,
        version: manifest.version,
        cover_image: fs::encode_data_uri(&parsed_card.avatar_bytes, &avatar_path),
        installed_at: now,
    })
}

fn build_classic_system_prompt(
    name: &str,
    description: &str,
    personality: &str,
    scenario: &str,
    mes_example: &str,
) -> String {
    [
        format!("You are roleplaying as {}.", name),
        non_empty_section("Description", description),
        non_empty_section("Personality", personality),
        non_empty_section("Scenario", scenario),
        non_empty_section("Example Dialogue", mes_example),
    ]
    .into_iter()
    .filter(|s| !s.trim().is_empty())
    .collect::<Vec<_>>()
    .join("\n\n")
}

fn non_empty_section(label: &str, content: &str) -> String {
    if content.trim().is_empty() {
        String::new()
    } else {
        format!("[{}]\n{}", label, content)
    }
}

fn get_string(data: &Value, key: &str) -> Option<String> {
    data.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn get_raw_string(data: &Value, key: &str) -> Option<String> {
    data.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

fn extract_character_book_entries(data: &Value) -> Vec<WorldEntry> {
    let Some(entries) = data
        .get("character_book")
        .or_else(|| data.get("world_info"))
        .and_then(|book| book.get("entries"))
        .and_then(|entries| entries.as_array())
    else {
        return Vec::new();
    };

    entries
        .iter()
        .enumerate()
        .filter_map(|(idx, entry)| {
            let content = get_string(entry, "content")?;
            let is_constant = entry
                .get("constant")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let mut keys = string_array(entry, "keys")
                .or_else(|| string_array(entry, "key"))
                .unwrap_or_default();
            if is_constant && keys.is_empty() {
                keys.push("*".to_string());
            }
            let extensions = entry.get("extensions");
            Some(WorldEntry {
                id: get_string(entry, "id").or_else(|| Some(format!("st_world_{}", idx + 1))),
                enabled: !entry
                    .get("disable")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                keys,
                content,
                secondary_keys: string_array(entry, "secondary_keys").unwrap_or_default(),
                enable_semantic_search: false,
                constant: is_constant,
                selective: entry
                    .get("selective")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                selective_logic: st_selective_logic(
                    extensions
                        .and_then(|ext| ext.get("selectiveLogic"))
                        .or_else(|| entry.get("selectiveLogic"))
                        .or_else(|| entry.get("selective_logic")),
                ),
                case_sensitive: optional_bool(
                    extensions
                        .and_then(|ext| ext.get("case_sensitive"))
                        .or_else(|| extensions.and_then(|ext| ext.get("caseSensitive")))
                        .or_else(|| entry.get("case_sensitive")),
                ),
                match_whole_words: optional_bool(
                    extensions
                        .and_then(|ext| ext.get("match_whole_words"))
                        .or_else(|| extensions.and_then(|ext| ext.get("matchWholeWords")))
                        .or_else(|| entry.get("match_whole_words")),
                ),
                scan_depth: optional_usize(
                    extensions
                        .and_then(|ext| ext.get("scan_depth"))
                        .or_else(|| entry.get("scan_depth")),
                ),
                probability: optional_f32(
                    entry
                        .get("probability")
                        .or_else(|| entry.get("probability_percent")),
                )
                .unwrap_or(100.0),
                recursive: optional_bool(
                    extensions
                        .and_then(|ext| ext.get("recursive"))
                        .or_else(|| entry.get("recursive")),
                )
                .unwrap_or(true),
                prevent_recursion: optional_bool(
                    extensions
                        .and_then(|ext| ext.get("prevent_recursion"))
                        .or_else(|| entry.get("prevent_recursion")),
                )
                .unwrap_or(false),
                delay_until_recursion: optional_bool(
                    extensions
                        .and_then(|ext| ext.get("delay_until_recursion"))
                        .or_else(|| entry.get("delay_until_recursion")),
                )
                .unwrap_or(false),
                insertion_depth: entry
                    .get("extensions")
                    .and_then(|ext| ext.get("depth"))
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize),
                position: "auto".to_string(),
                order: entry
                    .get("insertion_order")
                    .or_else(|| entry.get("order"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(idx as i64 * 100) as i32,
                role: "system".to_string(),
            })
        })
        .collect()
}

fn extract_display_regex_mutators(data: &Value) -> Vec<RegexMutator> {
    let scripts = data
        .get("extensions")
        .and_then(|ext| ext.get("regex_scripts"))
        .or_else(|| data.get("regex_scripts"))
        .and_then(|items| items.as_array());

    let Some(scripts) = scripts else {
        return Vec::new();
    };

    scripts
        .iter()
        .enumerate()
        .filter_map(|(idx, script)| {
            if script
                .get("disabled")
                .or_else(|| script.get("disable"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                return None;
            }

            let markdown_only = script
                .get("markdownOnly")
                .or_else(|| script.get("markdown_only"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let prompt_only = script
                .get("promptOnly")
                .or_else(|| script.get("prompt_only"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let run_on_edit = script
                .get("runOnEdit")
                .or_else(|| script.get("run_on_edit"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let pattern = get_string(script, "findRegex")
                .or_else(|| get_string(script, "find_regex"))
                .or_else(|| get_string(script, "pattern"))?;
            let replacement = get_raw_string(script, "replaceString")
                .or_else(|| get_raw_string(script, "replacement"))
                .unwrap_or_default();
            let id = get_string(script, "scriptName")
                .or_else(|| get_string(script, "name"))
                .unwrap_or_else(|| format!("st_display_regex_{}", idx + 1));
            let flags = get_string(script, "flags").unwrap_or_else(|| "gs".to_string());
            let sample = get_raw_string(script, "sample").unwrap_or_default();
            let description = get_string(script, "description").unwrap_or_default();

            Some(RegexMutator {
                id,
                enabled: true,
                placement: Some(if prompt_only {
                    "prompt".to_string()
                } else {
                    "display".to_string()
                }),
                target: if prompt_only {
                    st_prompt_regex_target(&pattern)
                } else {
                    "bot_output".to_string()
                },
                depth_range: Vec::new(),
                pattern,
                replacement,
                flags,
                sample,
                description,
                markdown_only,
                prompt_only,
                run_on_edit,
            })
        })
        .collect()
}

fn st_prompt_regex_target(pattern: &str) -> String {
    let lowered = pattern.to_ascii_lowercase();
    if lowered.contains("<gui") || lowered.contains("<ztl") || lowered.contains("<xuan") {
        "bot_output".to_string()
    } else {
        "history".to_string()
    }
}

fn st_selective_logic(value: Option<&Value>) -> String {
    match value {
        Some(Value::Number(n)) => match n.as_i64().unwrap_or(0) {
            1 => "and_all",
            2 => "not_any",
            3 => "not_all",
            _ => "and_any",
        },
        Some(Value::String(s)) => match s.to_ascii_lowercase().as_str() {
            "and_all" | "all" | "1" => "and_all",
            "not_any" | "none" | "2" => "not_any",
            "not_all" | "3" => "not_all",
            _ => "and_any",
        },
        _ => "and_any",
    }
    .to_string()
}

fn optional_bool(value: Option<&Value>) -> Option<bool> {
    match value? {
        Value::Bool(v) => Some(*v),
        Value::Number(n) => Some(n.as_i64().unwrap_or(0) != 0),
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn optional_usize(value: Option<&Value>) -> Option<usize> {
    match value? {
        Value::Number(n) => n.as_u64().map(|v| v as usize),
        Value::String(s) => s.parse::<usize>().ok(),
        _ => None,
    }
}

fn optional_f32(value: Option<&Value>) -> Option<f32> {
    match value? {
        Value::Number(n) => n.as_f64().map(|v| v as f32),
        Value::String(s) => s.parse::<f32>().ok(),
        _ => None,
    }
}

fn string_array(data: &Value, key: &str) -> Option<Vec<String>> {
    data.get(key).and_then(|v| {
        if let Some(items) = v.as_array() {
            Some(
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::trim))
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect(),
            )
        } else {
            v.as_str().map(|s| {
                s.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn st_character_book_fields_are_preserved_on_import_mapping() {
        let card = json!({
            "character_book": {
                "entries": [{
                    "content": "secret lore",
                    "keys": ["alpha"],
                    "secondary_keys": ["beta"],
                    "constant": true,
                    "selective": true,
                    "insertion_order": 42,
                    "extensions": {
                        "depth": 3,
                        "selectiveLogic": 1,
                        "case_sensitive": true,
                        "matchWholeWords": true,
                        "scan_depth": 2,
                        "recursive": false,
                        "prevent_recursion": true,
                        "delay_until_recursion": true
                    },
                    "probability": 25
                }]
            }
        });

        let entries = extract_character_book_entries(&card);
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert!(entry.constant);
        assert!(entry.selective);
        assert_eq!(entry.selective_logic, "and_all");
        assert_eq!(entry.case_sensitive, Some(true));
        assert_eq!(entry.match_whole_words, Some(true));
        assert_eq!(entry.scan_depth, Some(2));
        assert_eq!(entry.probability, 25.0);
        assert!(!entry.recursive);
        assert!(entry.prevent_recursion);
        assert!(entry.delay_until_recursion);
        assert_eq!(entry.insertion_depth, Some(3));
        assert_eq!(entry.order, 42);
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ClassicCardData {
    name: String,
    description: String,
    personality: String,
    scenario: String,
    first_mes: String,
    mes_example: String,
    creator_notes: String,
    alternate_greetings: Vec<String>,
    #[serde(default = "default_avatar_path")]
    avatar_path: String,
}

fn default_avatar_path() -> String {
    "assets/avatar.png".to_string()
}

pub fn refresh_sillytavern_classic_runtime(dest_dir: &Path) -> Result<(), String> {
    if !dest_dir.join("sillytavern_card.json").exists() {
        return Ok(());
    }
    let card_data_path = dest_dir.join("ui").join("card-data.json");
    if !card_data_path.exists() {
        return Ok(());
    }
    let card_data = fs::load_json::<ClassicCardData>(&card_data_path)?;
    write_classic_ui(dest_dir, &card_data)?;
    copy_sdk_to_cartridge(dest_dir)?;
    Ok(())
}

fn write_classic_ui(dest_dir: &Path, card_data: &ClassicCardData) -> Result<(), String> {
    fs::save_json(&dest_dir.join("ui").join("card-data.json"), card_data)?;
    std::fs::write(dest_dir.join("ui").join("index.html"), CLASSIC_INDEX)
        .map_err(|e| format!("Failed to write classic index.html: {}", e))?;
    std::fs::write(dest_dir.join("ui").join("style.css"), CLASSIC_STYLE)
        .map_err(|e| format!("Failed to write classic style.css: {}", e))?;
    std::fs::write(dest_dir.join("ui").join("script.js"), CLASSIC_SCRIPT)
        .map_err(|e| format!("Failed to write classic script.js: {}", e))?;
    Ok(())
}

const CLASSIC_INDEX: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>SillyTavern Classic</title>
  <link rel="stylesheet" href="style.css" />
  <script src="../tauri-tavern-sdk.js"></script>
  <script defer src="script.js"></script>
</head>
<body>
  <div class="shell">
    <aside class="left">
      <img id="avatar" class="avatar" alt="Character avatar" />
      <h1 id="char-name">Character</h1>
      <p id="char-desc"></p>
      <button id="new-chat">New Chat</button>
      <div id="chat-list" class="chat-list"></div>
    </aside>
    <main class="chat">
      <header class="topbar">
        <div>
          <strong id="top-name">Character</strong>
          <span>SillyTavern Classic</span>
        </div>
        <div class="top-actions">
          <button id="open-prompt-viewer">Prompt</button>
          <button id="toggle-lore">Card</button>
        </div>
      </header>
      <section id="chat" class="messages"><div id="messages" class="messages-inner"></div></section>
      <footer class="composer">
        <textarea id="send_textarea" rows="1" placeholder="Send a message..."></textarea>
        <button id="send_but">Send</button>
      </footer>
    </main>
    <aside id="lore" class="right">
      <h2>Character Card</h2>
      <h3>Personality</h3><p id="personality"></p>
      <h3>Scenario</h3><p id="scenario"></p>
      <h3>Example Dialogue</h3><pre id="examples"></pre>
    </aside>
  </div>
  <div id="prompt-viewer" class="prompt-viewer hidden" role="dialog" aria-modal="true" aria-label="Prompt Viewer">
    <div class="prompt-viewer-panel">
      <header class="prompt-viewer-head">
        <div>
          <h2>Prompt Viewer</h2>
          <span>Dry-run the next prompt without sending it to the model.</span>
        </div>
        <button id="close-prompt-viewer">Close</button>
      </header>
      <div class="prompt-viewer-controls">
        <textarea id="prompt-viewer-input" rows="3" placeholder="Simulated next user message..."></textarea>
        <button id="refresh-prompt-viewer">Refresh</button>
      </div>
      <div id="prompt-viewer-summary" class="prompt-viewer-summary"></div>
      <div id="prompt-viewer-body" class="prompt-viewer-body"></div>
    </div>
  </div>
</body>
</html>
"#;

const CLASSIC_SCRIPT: &str = r#"const SDK = window.TavernSDK;

let card = null;
let pipeline = { regex_mutators: [] };
let currentChat = null;
let busy = false;
let chatVariables = {};

const el = (id) => document.getElementById(id);
const inputEl = () => el("send_textarea") || el("input");
const sendButtonEl = () => el("send_but") || el("send");

const event_types = {
  APP_READY: "app_ready",
  CHAT_CHANGED: "chat_changed",
  MESSAGE_SENT: "message_sent",
  MESSAGE_RECEIVED: "message_received",
  USER_MESSAGE_RENDERED: "user_message_rendered",
  CHARACTER_MESSAGE_RENDERED: "character_message_rendered",
  GENERATION_STARTED: "generation_started",
  GENERATION_ENDED: "generation_ended",
};
const eventSource = createEventSource();

window.event_types = event_types;
window.eventSource = eventSource;
window.triggerSlash = handleGuiCommand;
window.getContext = getContext;
window.SillyTavern = { getContext, eventSource, event_types };
window.TavernHelper = {
  setInput: (text) => setInputText(text),
  appendInput: (text) => appendInputText(text),
  send: (text) => text == null ? send() : sendText(text),
};
window.toastr = {
  info: console.info.bind(console),
  success: console.info.bind(console),
  warning: console.warn.bind(console),
  error: console.error.bind(console),
};

window.addEventListener("message", (event) => {
  if (event.data?.type === "tt-classic-command") {
    handleGuiCommand(event.data.command);
  } else if (event.data?.type === "tt-classic-input") {
    if (event.data.mode === "append") appendInputText(event.data.text);
    else setInputText(event.data.text);
    if (event.data.send) send();
  } else if (event.data?.type === "tt-classic-resize") {
    resizeGuiFrame(event.source, event.data.height);
  } else if (event.data?.type === "tt-classic-event") {
    eventSource.emit(event.data.name, ...(event.data.args || []));
  }
});

init();

async function init() {
  card = await fetch("card-data.json").then((r) => r.json());
  pipeline = await loadPipeline();
  el("char-name").textContent = card.name;
  el("top-name").textContent = card.name;
  el("char-desc").textContent = card.description || card.creator_notes || "";
  el("personality").textContent = card.personality || "";
  el("scenario").textContent = card.scenario || "";
  el("examples").textContent = card.mes_example || "";
  loadAvatar();

  el("new-chat").addEventListener("click", newChat);
  sendButtonEl().addEventListener("click", send);
  el("toggle-lore").addEventListener("click", () => el("lore").classList.toggle("hidden"));
  el("open-prompt-viewer").addEventListener("click", openPromptViewer);
  el("close-prompt-viewer").addEventListener("click", closePromptViewer);
  el("refresh-prompt-viewer").addEventListener("click", refreshPromptViewer);
  el("prompt-viewer").addEventListener("click", (event) => {
    if (event.target === el("prompt-viewer")) closePromptViewer();
  });
  inputEl().addEventListener("keydown", (event) => {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      send();
    }
  });

  await loadChats();
  if (!currentChat) await newChat();
  eventSource.emit(event_types.APP_READY);
}

async function loadPipeline() {
  const loaded = await fetch("../pipeline.json").then((r) => r.json()).catch(() => ({ regex_mutators: [] }));
  loaded.regex_mutators = Array.isArray(loaded.regex_mutators) ? loaded.regex_mutators : [];
  const rawCard = await fetch("../sillytavern_card.json").then((r) => r.json()).catch(() => null);
  for (const mutator of extractSillyTavernRegexScripts(rawCard)) {
    if (!loaded.regex_mutators.some((item) => item.id === mutator.id && item.pattern === mutator.pattern)) {
      loaded.regex_mutators.push(mutator);
    }
  }
  return loaded;
}

async function loadChats() {
  const chats = await SDK.listChats();
  el("chat-list").innerHTML = "";
  chats.forEach((chat) => {
    const button = document.createElement("button");
    button.textContent = chat.title;
    button.className = chat.id === currentChat?.id ? "active" : "";
    button.onclick = () => openChat(chat);
    el("chat-list").appendChild(button);
  });
  if (chats[0] && !currentChat) await openChat(chats[0]);
}

async function newChat() {
  currentChat = await SDK.createChat(`Chat with ${card.name}`);
  chatVariables = {};
  renderWelcome();
  await loadChats();
}

async function openChat(chat) {
  currentChat = chat;
  chatVariables = await loadChatVariables(chat.id);
  const messages = await SDK.getMessages(chat.id);
  el("messages").innerHTML = "";
  if (messages.length === 0) {
    renderWelcome();
  } else {
    messages.forEach((message) => addMessage(message.role, message.content));
  }
  await loadChats();
}

async function loadChatVariables(chatId) {
  try {
    const rows = await SDK.listChatVariables(chatId);
    return Object.fromEntries(rows.map((row) => [row.name, row.value]));
  } catch (error) {
    console.warn("Failed to load chat variables", error);
    return {};
  }
}

function renderWelcome() {
  el("messages").innerHTML = "";
  addMessage("assistant", card.first_mes || `Hello, I am ${card.name}.`);
  (card.alternate_greetings || []).slice(0, 3).forEach((greeting) => {
    const chip = document.createElement("button");
    chip.className = "greeting";
    chip.textContent = greeting.slice(0, 120);
    chip.onclick = () => {
      el("messages").innerHTML = "";
      addMessage("assistant", greeting);
    };
    el("messages").appendChild(chip);
  });
}

async function send() {
  if (busy || !currentChat) return;
  const text = inputEl().value.trim();
  if (!text) return;
  inputEl().value = "";
  await sendText(text);
}

async function sendText(text) {
  if (busy || !currentChat) return;
  const message = String(text || "").trim();
  if (!message) return;
  addMessage("user", message);
  eventSource.emit(event_types.MESSAGE_SENT, message);
  eventSource.emit(event_types.USER_MESSAGE_RENDERED, message);
  const assistant = addMessage("assistant", "...");
  busy = true;
  eventSource.emit(event_types.GENERATION_STARTED);
  try {
    await SDK.sendMessage(
      currentChat.id,
      message,
      (response) => replaceMessageContent(assistant, "assistant", response),
      () => {},
      (error) => replaceMessageContent(assistant, "assistant", `Error: ${error}`),
    );
    eventSource.emit(event_types.MESSAGE_RECEIVED, assistant.textContent || "");
    eventSource.emit(event_types.CHARACTER_MESSAGE_RENDERED, assistant.textContent || "");
  } finally {
    busy = false;
    eventSource.emit(event_types.GENERATION_ENDED);
    await loadChats();
  }
}

async function openPromptViewer() {
  const viewer = el("prompt-viewer");
  el("prompt-viewer-input").value = inputEl().value.trim();
  viewer.classList.remove("hidden");
  await refreshPromptViewer();
}

function closePromptViewer() {
  el("prompt-viewer").classList.add("hidden");
}

async function refreshPromptViewer() {
  if (!currentChat) return;
  const summary = el("prompt-viewer-summary");
  const body = el("prompt-viewer-body");
  const input = el("prompt-viewer-input").value.trim() || inputEl().value.trim() || " ";
  summary.textContent = "Building prompt dry run...";
  body.innerHTML = "";
  try {
    const dryRun = await SDK.dryRunPromptPipeline(currentChat.id, input);
    renderPromptViewer(dryRun);
  } catch (error) {
    summary.textContent = "Prompt dry run failed.";
    body.innerHTML = `<pre class="prompt-error">${escapeHtml(String(error))}</pre>`;
  }
}

function renderPromptViewer(dryRun) {
  const messages = dryRun?.messages || [];
  const triggers = dryRun?.world_triggers || [];
  const includedWorld = triggers.filter((trigger) => trigger.included);
  const regexMutations = dryRun?.regex_mutations || [];
  const insertions = dryRun?.insertions || [];
  const recalls = dryRun?.rag_recalls || [];
  const budget = dryRun?.budget || {};

  el("prompt-viewer-summary").innerHTML = [
    promptChip(`${messages.length} messages`),
    promptChip(`${includedWorld.length}/${triggers.length} world entries`),
    promptChip(`${regexMutations.length} regex changes`),
    promptChip(`${budget.total_tokens ?? 0}/${budget.max_context_tokens ?? "?"} tokens`),
  ].join("");

  const sections = [
    promptSection("Final Messages", renderPromptMessages(messages), true),
    promptSection("World Info", renderWorldTriggers(triggers), true),
    promptSection("Insertions", renderInsertions(insertions), false),
    promptSection("Regex Mutations", renderRegexMutations(regexMutations), false),
    promptSection("RAG Recalls", renderRagRecalls(recalls), false),
    promptSection("Budget", `<pre>${escapeHtml(JSON.stringify(budget, null, 2))}</pre>`, false),
    promptSection("Raw Payload", `<pre>${escapeHtml(JSON.stringify(dryRun?.payload || {}, null, 2))}</pre>`, false),
    promptSection("Final Text", `<pre>${escapeHtml(dryRun?.final_text || "")}</pre>`, false),
  ];
  el("prompt-viewer-body").innerHTML = sections.join("");
}

function promptChip(text) {
  return `<span class="prompt-chip">${escapeHtml(text)}</span>`;
}

function promptSection(title, content, open) {
  return `<details class="prompt-section" ${open ? "open" : ""}><summary>${escapeHtml(title)}</summary>${content}</details>`;
}

function renderPromptMessages(messages) {
  if (!messages.length) return `<div class="prompt-empty">No messages.</div>`;
  return messages.map((message, index) => `
    <article class="prompt-message">
      <div class="prompt-message-head"><span>${index + 1}</span><strong>${escapeHtml(message.role || "message")}</strong></div>
      <pre>${escapeHtml(message.content || "")}</pre>
    </article>
  `).join("");
}

function renderWorldTriggers(triggers) {
  if (!triggers.length) return `<div class="prompt-empty">No world info entries were evaluated.</div>`;
  return triggers.map((trigger) => {
    const status = trigger.included ? "included" : "skipped";
    const label = trigger.id || (trigger.keys || []).join(", ") || "world entry";
    const depth = trigger.insertion_depth == null ? "top" : `depth ${trigger.insertion_depth}`;
    return `
      <article class="prompt-row ${trigger.included ? "included" : "skipped"}">
        <div><strong>${escapeHtml(status)}</strong> ${escapeHtml(label)}</div>
        <div class="prompt-meta">${escapeHtml(trigger.trigger || "")} | ${escapeHtml(trigger.role || "system")} | ${escapeHtml(depth)} | recursion ${trigger.recursion_depth ?? 0}</div>
        <pre>${escapeHtml(trigger.content || "")}</pre>
      </article>
    `;
  }).join("");
}

function renderInsertions(insertions) {
  if (!insertions.length) return `<div class="prompt-empty">No explicit insertion records.</div>`;
  return insertions.map((insertion) => `
    <article class="prompt-row included">
      <div><strong>${escapeHtml(insertion.label || "insertion")}</strong></div>
      <div class="prompt-meta">index ${insertion.index ?? 0} | depth ${insertion.depth ?? 0} | ${escapeHtml(insertion.role || "system")}</div>
      <pre>${escapeHtml(insertion.content || "")}</pre>
    </article>
  `).join("");
}

function renderRegexMutations(mutations) {
  if (!mutations.length) return `<div class="prompt-empty">No prompt regex mutation changed text.</div>`;
  return mutations.map((mutation) => `
    <article class="prompt-row included">
      <div><strong>${escapeHtml(mutation.mutator_id || "regex")}</strong></div>
      <div class="prompt-meta">${escapeHtml(mutation.role || "")} | depth ${mutation.depth ?? 0} | ${mutation.before_tokens ?? 0} -> ${mutation.after_tokens ?? 0} tokens</div>
      <div class="prompt-diff">
        <pre>${escapeHtml(mutation.before || "")}</pre>
        <pre>${escapeHtml(mutation.after || "")}</pre>
      </div>
    </article>
  `).join("");
}

function renderRagRecalls(recalls) {
  if (!recalls.length) return `<div class="prompt-empty">No RAG recalls.</div>`;
  return recalls.map((recall) => `
    <article class="prompt-row included">
      <div><strong>${escapeHtml(recall.id || "recall")}</strong></div>
      <div class="prompt-meta">${escapeHtml(recall.source_type || "")} | similarity ${Number(recall.similarity || 0).toFixed(3)}</div>
      <pre>${escapeHtml(recall.content || "")}</pre>
    </article>
  `).join("");
}

async function handleGuiCommand(command) {
  const raw = String(command || "").trim();
  if (!raw) return Promise.resolve();
  const parts = raw.split(/\|(?=\/)/g).map((part) => part.trim()).filter(Boolean);
  let pending = "";
  let shouldTrigger = false;
  let pipe = "";

  for (const part of parts.length ? parts : [raw]) {
    if (/^\/send\b/i.test(part)) {
      pending = part.replace(/^\/send\b/i, "").trim();
      shouldTrigger = true;
    } else if (/^\/setinput\b/i.test(part)) {
      pending = part.replace(/^\/setinput\b/i, "").trim();
      setInputText(pending);
    } else if (/^\/append\b/i.test(part)) {
      const text = part.replace(/^\/append\b/i, "").trim();
      appendInputText(text);
      pending = inputEl().value.trim();
    } else if (/^\/trigger\b/i.test(part) || /^\/gen\b/i.test(part)) {
      shouldTrigger = true;
    } else if (/^\//.test(part) && currentChat) {
      pipe = await SDK.executeCommand(currentChat.id, part.replace(/\{\{pipe\}\}/gi, pipe));
    }
  }

  if (shouldTrigger) {
    const text = pending || inputEl().value.trim();
    if (text) return sendText(text);
  }
  return Promise.resolve();
}

function addMessage(role, content) {
  const div = document.createElement("div");
  div.className = `mes message ${role}`;
  div.dataset.mesRole = role;
  div.dataset.messageId = String(Date.now());
  const text = document.createElement("div");
  text.className = "mes_text";
  div.appendChild(text);
  replaceMessageContent(div, role, content);
  el("messages").appendChild(div);
  scrollChatToBottom();
  return div;
}

function replaceMessageContent(container, role, content) {
  container.className = `mes message ${role}`;
  container.dataset.mesRole = role;
  let target = container.querySelector(".mes_text");
  if (!target) {
    target = document.createElement("div");
    target.className = "mes_text";
    container.replaceChildren(target);
  }
  target.replaceChildren();
  const rawContent = applySillyTavernMacros(String(content || ""));
  const displayContent = role === "assistant"
    ? applySillyTavernMacros(applyDisplayRegex(rawContent, "assistant"))
    : applySillyTavernMacros(applyDisplayRegex(rawContent, "user"));
  const parts = role === "assistant" ? splitDisplayParts(displayContent) : [{ type: "text", content: displayContent }];
  const hasGui = parts.some((part) => part.type === "html");
  container.classList.toggle("gui-message", hasGui);

  for (const part of parts) {
    if (part.type === "html") appendGuiPart(target, part.content);
    else appendTextPart(target, part.content);
  }
}

function appendTextPart(target, content) {
  const text = String(content || "");
  if (!text.trim()) return;
  const block = document.createElement("div");
  block.className = "mes_plain";
  block.textContent = text;
  target.appendChild(block);
}

function appendGuiPart(target, html) {
  const iframe = document.createElement("iframe");
  iframe.className = "gui-frame";
  iframe.setAttribute("sandbox", "allow-scripts allow-forms allow-popups allow-same-origin");
  iframe.setAttribute("scrolling", "no");
  iframe.srcdoc = buildGuiSrcdoc(html);
  target.appendChild(iframe);
}

function applyDisplayRegex(content, role = "assistant") {
  let output = String(content || "");
  for (const mutator of pipeline.regex_mutators || []) {
    if (mutator.enabled === false || !regexRunsOnDisplay(mutator, role)) {
      continue;
    }
    try {
      const { pattern, flags } = normalizeRegex(mutator.pattern, mutator.flags || "gs");
      output = output.replace(new RegExp(pattern, flags), mutator.replacement || "");
    } catch (error) {
      console.warn("Invalid display regex", mutator.id, error);
    }
  }
  return output;
}

function regexRunsOnDisplay(mutator, role = "assistant") {
  const placement = String(mutator.placement || "").toLowerCase();
  const target = String(mutator.target || "").toLowerCase();
  if (mutator.prompt_only || placement === "prompt") return false;
  if (role === "user" && mutator.run_on_edit) return true;
  if (role === "user") return ["user", "user_input", "input"].includes(target);
  if (mutator.markdown_only || placement === "display" || placement === "ui_display") return true;
  return ["display", "frontend", "message_display", "bot_output"].includes(target);
}

function extractSillyTavernRegexScripts(rawCard) {
  const data = rawCard?.data || rawCard || {};
  const scripts = data.extensions?.regex_scripts || data.regex_scripts || rawCard?.extensions?.regex_scripts || [];
  if (!Array.isArray(scripts)) return [];
  return scripts
    .filter((script) => !(script.disabled || script.disable || script.promptOnly || script.prompt_only))
    .map((script, index) => ({
      id: script.scriptName || script.name || `st_display_regex_${index + 1}`,
      enabled: true,
      placement: "display",
      target: "display",
      depth_range: [],
      pattern: script.findRegex || script.find_regex || script.pattern || "",
      replacement: script.replaceString || script.replacement || "",
      flags: script.flags || "gs",
      sample: script.sample || "",
      description: script.description || "",
      markdown_only: !!(script.markdownOnly || script.markdown_only),
      prompt_only: false,
      run_on_edit: !!(script.runOnEdit || script.run_on_edit),
    }))
    .filter((script) => script.pattern);
}

function normalizeRegex(pattern, fallbackFlags) {
  const match = String(pattern || "").match(/^\/([\s\S]*)\/([a-z]*)$/);
  if (!match) return { pattern: String(pattern || ""), flags: uniqueFlags(fallbackFlags || "gs") };
  return { pattern: match[1], flags: uniqueFlags(match[2] || fallbackFlags || "gs") };
}

function uniqueFlags(flags) {
  return Array.from(new Set(String(flags || "").replace(/[^dgimsuvy]/g, "").split(""))).join("");
}

function applySillyTavernMacros(content) {
  const userName = "You";
  const charName = card?.name || "Character";
  const input = inputEl()?.value || "";
  let output = String(content || "");
  const replacements = [
    ["{{user}}", userName], ["{{User}}", userName], ["{{USER}}", userName],
    ["<user>", userName], ["<User>", userName], ["<USER>", userName],
    ["{{char}}", charName], ["{{Char}}", charName], ["{{CHAR}}", charName],
    ["<char>", charName], ["<Char>", charName], ["<CHAR>", charName],
    ["{{input}}", input], ["{{Input}}", input], ["{{INPUT}}", input],
    ["<input>", input], ["<Input>", input], ["<INPUT>", input],
  ];
  for (const [from, to] of replacements) output = output.split(from).join(to);
  return output.replace(/\{\{([^{}]+)\}\}/g, (match, token) => resolveSillyTavernMacro(token.trim(), match));
}

function resolveSillyTavernMacro(token, fallback) {
  const now = new Date();
  if (token === "time" || token === "Time") {
    return now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }
  if (token === "date" || token === "Date") {
    return now.toLocaleDateString();
  }
  if (token === "lastMessageId" || token === "lastmessageid") {
    return document.querySelector(".mes:last-child")?.dataset?.messageId || "";
  }
  const getVar = token.match(/^getvar(?:::|:)(.+)$/);
  if (getVar) return chatVariables[getVar[1].trim()] || "";
  const random = token.match(/^(?:random|pick)(?:::|:)([\s\S]+)$/);
  if (random) {
    const options = random[1].split(",").map((item) => item.trim()).filter(Boolean);
    return options.length ? options[Math.floor(Math.random() * options.length)] : "";
  }
  const roll = token.match(/^roll:([0-9]*)d?([0-9]+)$/i);
  if (roll) {
    const count = Math.max(1, Number(roll[1] || 1));
    const sides = Math.max(1, Number(roll[2]));
    let total = 0;
    for (let i = 0; i < count; i += 1) total += 1 + Math.floor(Math.random() * sides);
    return String(total);
  }
  const calc = token.match(/^calc:([0-9+\-*/().\s]+)$/);
  if (calc) {
    try {
      const value = Function(`"use strict"; return (${calc[1]});`)();
      return Number.isFinite(value) ? String(Number(value.toFixed(6))) : fallback;
    } catch (_error) {
      return fallback;
    }
  }
  return fallback;
}

function extractGuiHtml(content) {
  let text = String(content || "").trim();
  const fence = text.match(/^`{3,}(?:html)?\s*([\s\S]*?)\s*`{3,}$/i);
  if (fence) text = fence[1].trim();

  const guiMatch = text.match(/^<Gui\b[^>]*>([\s\S]*?)<\/Gui>$/i);
  if (guiMatch) return guiMatch[1].trim();

  if (/^(?:<!doctype\s+html>|<html\b)/i.test(text)) return text;
  return null;
}

function splitDisplayParts(content) {
  const direct = extractGuiHtml(content);
  if (direct) return [{ type: "html", content: direct }];

  const parts = [];
  const source = String(content || "");
  const matcher = /<Gui\b[^>]*>[\s\S]*?<\/Gui>|`{3,}(?:html)?\s*[\s\S]*?`{3,}/gi;
  let cursor = 0;
  let match;
  while ((match = matcher.exec(source))) {
    if (match.index > cursor) {
      parts.push({ type: "text", content: source.slice(cursor, match.index) });
    }
    const html = extractGuiHtml(match[0]);
    parts.push(html ? { type: "html", content: html } : { type: "text", content: match[0] });
    cursor = matcher.lastIndex;
  }
  if (cursor < source.length) parts.push({ type: "text", content: source.slice(cursor) });
  return parts.length ? parts : [{ type: "text", content: source }];
}

async function loadAvatar() {
  const avatar = el("avatar");
  const avatarPath = card?.avatar_path || "assets/avatar.png";
  try {
    avatar.src = await SDK.loadAsset(avatarPath);
  } catch (error) {
    console.warn("Avatar data URI failed, falling back to tavern URL", error);
    avatar.src = SDK.getAssetUrl(avatarPath);
  }
}

function buildGuiSrcdoc(html) {
  const bridgeStyle = `<style id="tt-classic-host-style">
html, body { overflow: visible !important; min-height: 0 !important; }
body { scrollbar-width: none; }
body::-webkit-scrollbar { display: none; }
</style>`;
  const bridge = `${bridgeStyle}<script>
window.triggerSlash = function(command) {
  parent.postMessage({ type: "tt-classic-command", command: String(command || "") }, "*");
};
window.event_types = parent.event_types || {};
window.eventSource = {
  on: function() {},
  once: function() {},
  makeFirst: function() {},
  removeListener: function() {},
  emit: function(name) {
    parent.postMessage({ type: "tt-classic-event", name: name, args: Array.prototype.slice.call(arguments, 1) }, "*");
  },
};
window.getContext = function() {
  return parent.getContext ? parent.getContext() : {};
};
window.SillyTavern = { getContext: window.getContext, eventSource: window.eventSource, event_types: window.event_types };
window.TavernHelper = {
  setInput: function(text) { parent.postMessage({ type: "tt-classic-input", mode: "set", text: String(text || "") }, "*"); },
  appendInput: function(text) { parent.postMessage({ type: "tt-classic-input", mode: "append", text: String(text || "") }, "*"); },
  send: function(text) { parent.postMessage({ type: "tt-classic-input", mode: "set", text: String(text || ""), send: true }, "*"); },
};
window.toastr = {
  info: console.info.bind(console),
  success: console.info.bind(console),
  warning: console.warn.bind(console),
  error: console.error.bind(console),
};
(function() {
  function reportSize() {
    var body = document.body;
    var doc = document.documentElement;
    var height = Math.max(body ? body.scrollHeight : 0, doc ? doc.scrollHeight : 0, body ? body.offsetHeight : 0, doc ? doc.offsetHeight : 0);
    parent.postMessage({ type: "tt-classic-resize", height: height }, "*");
  }
  window.addEventListener("load", reportSize);
  document.addEventListener("DOMContentLoaded", reportSize);
  if (window.ResizeObserver) {
    new ResizeObserver(reportSize).observe(document.documentElement);
  }
  setTimeout(reportSize, 50);
  setTimeout(reportSize, 350);
  setInterval(reportSize, 1000);
})();
<\/script>`;
  if (/<head\b[^>]*>/i.test(html)) {
    return html.replace(/<head\b[^>]*>/i, (match) => `${match}${bridge}`);
  }
  return `${bridge}${html}`;
}

function setInputText(text) {
  const input = inputEl();
  input.value = String(text || "");
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.focus();
}

function appendInputText(text) {
  const input = inputEl();
  const value = String(text || "").trim();
  if (!value) return;
  const current = input.value.trim();
  input.value = current ? `${current}\n${value}` : value;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.focus();
}

function resizeGuiFrame(source, rawHeight) {
  const contentHeight = Number(rawHeight) || 0;
  const viewportMin = Math.max(320, window.innerHeight - 154);
  const height = Math.max(viewportMin, Math.min(12000, contentHeight + 8));
  for (const frame of document.querySelectorAll(".gui-frame")) {
    if (frame.contentWindow === source) {
      frame.style.height = `${height}px`;
      scrollChatToBottom(false);
      return;
    }
  }
}

function scrollChatToBottom(force = true) {
  const scroller = el("chat") || el("messages");
  if (!scroller) return;
  if (force || scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 160) {
    scroller.scrollTop = scroller.scrollHeight;
  }
}

function escapeHtml(value) {
  return String(value ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

function getContext() {
  return {
    name1: "You",
    name2: card?.name || "Character",
    characterId: card?.name || "",
    chatId: currentChat?.id || "",
    chat: currentChat,
    eventSource,
    event_types,
    extensionSettings: {},
    sendSystemMessage: (_type, text) => addMessage("system", text),
    generate: () => send(),
    triggerSlash: handleGuiCommand,
  };
}

function createEventSource() {
  const listeners = new Map();
  const on = (name, callback) => {
    if (!listeners.has(name)) listeners.set(name, new Set());
    listeners.get(name).add(callback);
    return callback;
  };
  const removeListener = (name, callback) => listeners.get(name)?.delete(callback);
  const emit = (name, ...args) => {
    for (const callback of listeners.get(name) || []) {
      try { callback(...args); } catch (error) { console.error(error); }
    }
  };
  return {
    on,
    makeFirst: on,
    removeListener,
    emit,
    once(name, callback) {
      const wrapped = (...args) => {
        removeListener(name, wrapped);
        callback(...args);
      };
      return on(name, wrapped);
    },
  };
}
"#;

const CLASSIC_STYLE: &str = r#"* { box-sizing: border-box; }
body {
  margin: 0;
  width: 100vw;
  height: 100vh;
  overflow: hidden;
  background: #111016;
  color: #e8e1ef;
  font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}
button, textarea { font: inherit; }
.shell {
  display: grid;
  grid-template-columns: minmax(220px, 280px) minmax(0, 1fr) minmax(240px, 300px);
  height: 100vh;
  overflow: hidden;
}
.left, .right {
  background: #191720;
  border-color: #2d2938;
  overflow: auto;
}
.left { border-right: 1px solid #2d2938; padding: 18px; }
.right { border-left: 1px solid #2d2938; padding: 18px; }
.avatar {
  width: 168px;
  max-width: 100%;
  aspect-ratio: 1;
  object-fit: cover;
  object-position: center;
  border-radius: 50%;
  border: 1px solid #3b3549;
  display: block;
  margin: 0 auto;
}
h1 { margin: 14px 0 6px; font-size: 1.3rem; }
h2 { margin: 0 0 16px; }
h3 { margin: 18px 0 6px; color: #c9b6ff; font-size: .86rem; }
p, pre { color: #bdb3c8; line-height: 1.55; white-space: pre-wrap; }
.chat {
  display: grid;
  grid-template-rows: 58px minmax(0, 1fr) auto;
  min-width: 0;
  min-height: 0;
  height: 100vh;
  background: #111016;
}
.topbar {
  height: 58px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 16px;
  border-bottom: 1px solid #2d2938;
  background: #17151d;
}
.topbar span { display: block; color: #8d829c; font-size: .75rem; margin-top: 2px; }
.top-actions { display: flex; gap: 8px; align-items: center; }
button {
  border: 1px solid #3b3549;
  background: #24202f;
  color: #f3eef8;
  border-radius: 7px;
  padding: 8px 12px;
  cursor: pointer;
}
button:hover, .chat-list button.active { border-color: #9d7cff; background: #302845; }
.chat-list { display: flex; flex-direction: column; gap: 8px; margin-top: 18px; }
.chat-list button { text-align: left; color: #bdb3c8; }
.messages {
  position: relative;
  min-height: 0;
  height: 100%;
  overflow-y: auto;
  overflow-x: hidden;
  padding: clamp(12px, 2.4vw, 24px);
  padding-bottom: clamp(18px, 3vw, 32px);
  scrollbar-gutter: stable;
  overscroll-behavior: contain;
  -webkit-overflow-scrolling: touch;
}
.messages-inner {
  display: flex;
  flex-direction: column;
  gap: 14px;
  min-height: min-content;
}
.message {
  max-width: min(760px, 86%);
  padding: 12px 14px;
  border-radius: 10px;
  white-space: pre-wrap;
  line-height: 1.55;
  overflow: visible;
}
.mes_text {
  min-width: 0;
  overflow: visible;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.mes_plain {
  white-space: pre-wrap;
}
.message.user {
  align-self: flex-end;
  background: #2d2450;
  border: 1px solid #4d3d83;
}
.message.assistant {
  align-self: flex-start;
  background: #1b1922;
  border: 1px solid #302b3c;
}
.message.gui-message {
  width: min(1040px, 100%);
  max-width: min(1040px, 100%);
  padding: 0;
  background: transparent;
  border: 0;
  white-space: normal;
}
.gui-frame {
  display: block;
  width: 100%;
  min-height: 260px;
  height: 360px;
  max-height: none;
  border: 0;
  border-radius: 12px;
  background: transparent;
  overflow: hidden;
}
.greeting {
  max-width: min(680px, 86%);
  text-align: left;
  color: #bfaeff;
  background: transparent;
  border-style: dashed;
}
.composer {
  position: sticky;
  bottom: 0;
  z-index: 20;
  display: flex;
  gap: 10px;
  padding: 14px;
  border-top: 1px solid #2d2938;
  background: #17151d;
  box-shadow: 0 -10px 24px rgba(0, 0, 0, 0.28);
}
textarea {
  flex: 1;
  resize: none;
  min-height: 44px;
  max-height: 140px;
  padding: 11px 12px;
  color: #f3eef8;
  background: #0f0e14;
  border: 1px solid #383244;
  border-radius: 8px;
  outline: none;
}
textarea:focus { border-color: #9d7cff; }
.prompt-viewer {
  position: fixed;
  inset: 0;
  z-index: 100;
  display: grid;
  place-items: center;
  padding: clamp(10px, 2vw, 24px);
  background: rgba(8, 7, 12, 0.72);
}
.prompt-viewer-panel {
  width: min(1180px, 100%);
  height: min(860px, 100%);
  min-height: 0;
  display: grid;
  grid-template-rows: auto auto auto minmax(0, 1fr);
  background: #14121a;
  border: 1px solid #3b3549;
  border-radius: 10px;
  box-shadow: 0 24px 80px rgba(0, 0, 0, 0.45);
  overflow: hidden;
}
.prompt-viewer-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 16px;
  border-bottom: 1px solid #2d2938;
  background: #1a1722;
}
.prompt-viewer-head h2 { margin: 0 0 4px; }
.prompt-viewer-head span { color: #8d829c; font-size: .78rem; }
.prompt-viewer-controls {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 10px;
  padding: 14px 16px;
  border-bottom: 1px solid #2d2938;
}
.prompt-viewer-controls textarea { min-height: 74px; max-height: 160px; }
.prompt-viewer-summary {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  padding: 12px 16px;
  border-bottom: 1px solid #2d2938;
}
.prompt-chip {
  border: 1px solid #3b3549;
  border-radius: 999px;
  padding: 5px 9px;
  color: #dcd3e8;
  background: #201c2a;
  font-size: .78rem;
}
.prompt-viewer-body {
  min-height: 0;
  overflow: auto;
  padding: 16px;
  scrollbar-gutter: stable;
}
.prompt-section {
  border: 1px solid #302b3c;
  border-radius: 8px;
  margin-bottom: 12px;
  background: #181620;
  overflow: hidden;
}
.prompt-section summary {
  cursor: pointer;
  padding: 11px 13px;
  color: #f3eef8;
  background: #1f1b29;
  border-bottom: 1px solid #302b3c;
}
.prompt-message,
.prompt-row {
  margin: 12px;
  padding: 12px;
  border: 1px solid #302b3c;
  border-radius: 8px;
  background: #111016;
}
.prompt-row.included { border-color: #4c7a58; }
.prompt-row.skipped { opacity: .68; }
.prompt-message-head {
  display: flex;
  gap: 8px;
  align-items: center;
  margin-bottom: 8px;
  color: #c9b6ff;
}
.prompt-message-head span {
  min-width: 24px;
  height: 24px;
  display: grid;
  place-items: center;
  border-radius: 999px;
  background: #2a2437;
  color: #f3eef8;
  font-size: .72rem;
}
.prompt-meta {
  color: #8d829c;
  font-size: .76rem;
  margin-top: 4px;
}
.prompt-message pre,
.prompt-row pre,
.prompt-section > pre,
.prompt-error {
  margin: 9px 0 0;
  max-height: 380px;
  overflow: auto;
  padding: 10px;
  border-radius: 7px;
  background: #0c0b10;
  color: #d8cedf;
  font-family: "Cascadia Code", "SFMono-Regular", Consolas, monospace;
  font-size: .78rem;
  line-height: 1.5;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.prompt-diff {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 10px;
}
.prompt-empty {
  margin: 12px;
  color: #8d829c;
}
.hidden { display: none; }
@media (max-width: 1180px) {
  .shell { grid-template-columns: minmax(200px, 240px) minmax(0, 1fr); }
  .right { display: none; }
}
@media (max-width: 760px) {
  .shell { grid-template-columns: 1fr; }
  .left { display: none; }
  .messages { padding: 10px; }
  .message { max-width: 100%; }
  .gui-frame { min-height: 360px; }
  .topbar { padding: 0 10px; }
  .top-actions button { padding: 7px 9px; }
  .prompt-viewer-controls { grid-template-columns: 1fr; }
  .prompt-diff { grid-template-columns: 1fr; }
}
"#;
