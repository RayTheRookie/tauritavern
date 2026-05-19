use crate::application::dto::{
    default_prompt_entries, CartridgeInfo, Manifest, PipelineConfig, PresetConfig, RegexMutator,
    WorldEntry,
};
use crate::application::services::prompt_engine;
use crate::infrastructure::database::{CartridgeRow, SqliteRepo};
use crate::infrastructure::fs;
use base64::Engine;
use serde_json::Value;
use std::path::Path;
use uuid::Uuid;
use zip::ZipArchive;

pub async fn import_cartridge(
    repo: &SqliteRepo,
    file_path: &str,
    data_dir: &Path,
) -> Result<CartridgeInfo, String> {
    if file_path.to_ascii_lowercase().ends_with(".png") {
        return import_sillytavern_png(repo, file_path, data_dir).await;
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

async fn import_sillytavern_png(
    repo: &SqliteRepo,
    file_path: &str,
    data_dir: &Path,
) -> Result<CartridgeInfo, String> {
    let png_bytes =
        std::fs::read(file_path).map_err(|e| format!("Failed to read PNG card: {}", e))?;
    let card_json = extract_sillytavern_json_from_png(&png_bytes)?;
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

    std::fs::write(dest_dir.join("assets").join("avatar.png"), &png_bytes)
        .map_err(|e| format!("Failed to copy avatar PNG: {}", e))?;
    fs::save_json(&dest_dir.join("sillytavern_card.json"), &card_json)?;

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
        cover_image: "assets/avatar.png".to_string(),
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
        cover_image: fs::encode_data_uri(&png_bytes, "assets/avatar.png"),
        installed_at: now,
    })
}

fn extract_sillytavern_json_from_png(bytes: &[u8]) -> Result<Value, String> {
    const PNG_SIG: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 8 || &bytes[..8] != PNG_SIG {
        return Err("Not a PNG file".to_string());
    }

    let mut offset = 8usize;
    let mut text_values = Vec::new();
    while offset + 12 <= bytes.len() {
        let length = u32::from_be_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .map_err(|_| "Invalid PNG chunk length".to_string())?,
        ) as usize;
        offset += 4;
        if offset + 4 + length + 4 > bytes.len() {
            return Err("Truncated PNG chunk".to_string());
        }
        let chunk_type = &bytes[offset..offset + 4];
        offset += 4;
        let data = &bytes[offset..offset + length];
        offset += length + 4;

        if chunk_type == b"tEXt" {
            if let Some((keyword, text)) = split_png_text(data) {
                if keyword.eq_ignore_ascii_case("chara") {
                    text_values.insert(0, text);
                } else {
                    text_values.push(text);
                }
            }
        } else if chunk_type == b"iTXt" {
            if let Some((keyword, text)) = split_png_itxt(data) {
                if keyword.eq_ignore_ascii_case("chara") {
                    text_values.insert(0, text);
                } else {
                    text_values.push(text);
                }
            }
        }

        if chunk_type == b"IEND" {
            break;
        }
    }

    for value in text_values {
        if let Ok(json) = parse_card_json_text(&value) {
            return Ok(json);
        }
    }
    Err("No SillyTavern character metadata found in PNG tEXt/iTXt chunks".to_string())
}

fn split_png_text(data: &[u8]) -> Option<(String, String)> {
    let nul = data.iter().position(|b| *b == 0)?;
    let keyword = String::from_utf8_lossy(&data[..nul]).to_string();
    let text = String::from_utf8_lossy(&data[nul + 1..]).to_string();
    Some((keyword, text))
}

fn split_png_itxt(data: &[u8]) -> Option<(String, String)> {
    let keyword_end = data.iter().position(|b| *b == 0)?;
    let keyword = String::from_utf8_lossy(&data[..keyword_end]).to_string();
    let mut idx = keyword_end + 1;
    if idx + 2 > data.len() {
        return None;
    }
    let compression_flag = data[idx];
    idx += 2; // flag + method
    let lang_end = data[idx..].iter().position(|b| *b == 0)? + idx;
    idx = lang_end + 1;
    let translated_end = data[idx..].iter().position(|b| *b == 0)? + idx;
    idx = translated_end + 1;
    if compression_flag != 0 {
        return None;
    }
    let text = String::from_utf8_lossy(&data[idx..]).to_string();
    Some((keyword, text))
}

fn parse_card_json_text(text: &str) -> Result<Value, String> {
    let trimmed = text.trim();
    if trimmed.starts_with('{') {
        return serde_json::from_str(trimmed).map_err(|e| e.to_string());
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(trimmed))
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&decoded).map_err(|e| e.to_string())
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
                || script
                    .get("promptOnly")
                    .or_else(|| script.get("prompt_only"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            {
                return None;
            }

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
                target: "display".to_string(),
                depth_range: Vec::new(),
                pattern,
                replacement,
                flags,
                sample,
                description,
            })
        })
        .collect()
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

#[derive(serde::Serialize)]
struct ClassicCardData {
    name: String,
    description: String,
    personality: String,
    scenario: String,
    first_mes: String,
    mes_example: String,
    creator_notes: String,
    alternate_greetings: Vec<String>,
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
        <button id="toggle-lore">Card</button>
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
</body>
</html>
"#;

const CLASSIC_SCRIPT: &str = r#"const SDK = window.TavernSDK;

let card = null;
let pipeline = { regex_mutators: [] };
let currentChat = null;
let busy = false;

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
  renderWelcome();
  await loadChats();
}

async function openChat(chat) {
  currentChat = chat;
  const messages = await SDK.getMessages(chat.id);
  el("messages").innerHTML = "";
  if (messages.length === 0) {
    renderWelcome();
  } else {
    messages.forEach((message) => addMessage(message.role, message.content));
  }
  await loadChats();
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

function handleGuiCommand(command) {
  const raw = String(command || "").trim();
  if (!raw) return Promise.resolve();
  const parts = raw.split(/\|(?=\/)/g).map((part) => part.trim()).filter(Boolean);
  let pending = "";
  let shouldTrigger = false;

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
  const displayContent = role === "assistant" ? applySillyTavernMacros(applyDisplayRegex(rawContent)) : rawContent;
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

function applyDisplayRegex(content) {
  let output = String(content || "");
  for (const mutator of pipeline.regex_mutators || []) {
    if (mutator.enabled === false || !["display", "frontend", "message_display"].includes(mutator.target)) {
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

function extractSillyTavernRegexScripts(rawCard) {
  const data = rawCard?.data || rawCard || {};
  const scripts = data.extensions?.regex_scripts || data.regex_scripts || rawCard?.extensions?.regex_scripts || [];
  if (!Array.isArray(scripts)) return [];
  return scripts
    .filter((script) => !(script.disabled || script.disable || script.promptOnly || script.prompt_only))
    .map((script, index) => ({
      id: script.scriptName || script.name || `st_display_regex_${index + 1}`,
      enabled: true,
      target: "display",
      depth_range: [],
      pattern: script.findRegex || script.find_regex || script.pattern || "",
      replacement: script.replaceString || script.replacement || "",
      flags: script.flags || "gs",
      sample: script.sample || "",
      description: script.description || "",
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
  const replacements = [
    ["{{user}}", userName], ["{{User}}", userName], ["{{USER}}", userName],
    ["<user>", userName], ["<User>", userName], ["<USER>", userName],
    ["{{char}}", charName], ["{{Char}}", charName], ["{{CHAR}}", charName],
    ["<char>", charName], ["<Char>", charName], ["<CHAR>", charName],
    ["{{input}}", input], ["{{Input}}", input], ["{{INPUT}}", input],
    ["<input>", input], ["<Input>", input], ["<INPUT>", input],
  ];
  let output = String(content || "");
  for (const [from, to] of replacements) output = output.split(from).join(to);
  return output;
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
  try {
    avatar.src = await SDK.loadAsset("assets/avatar.png");
  } catch (error) {
    console.warn("Avatar data URI failed, falling back to tavern URL", error);
    avatar.src = SDK.getAssetUrl("assets/avatar.png");
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
}
"#;
