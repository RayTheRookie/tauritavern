use crate::application::services::prompt_engine;
use crate::infrastructure::apis::LlmHttpClient;
use crate::infrastructure::credentials::CredentialService;
use crate::infrastructure::database::{MessageRow, MessageSwipeRow};
use crate::infrastructure::fs;
use crate::AppState;
use chrono::Utc;
use futures::StreamExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct ScriptCommand {
    name: String,
    args: String,
}

#[derive(Debug, Clone)]
struct IfCommand {
    condition: String,
    then_script: String,
    else_script: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ScriptContext {
    pipe: String,
    locals: HashMap<String, String>,
    args: Vec<String>,
}

pub async fn execute_slash_script(
    state: &AppState,
    cartridge_id: &str,
    chat_id: &str,
    input: &str,
) -> Option<Result<String, String>> {
    if !input.trim_start().starts_with('/') {
        return None;
    }

    Some(
        run_script(
            state,
            cartridge_id,
            chat_id,
            input,
            &mut ScriptContext::default(),
        )
        .await,
    )
}

async fn run_script(
    state: &AppState,
    cartridge_id: &str,
    chat_id: &str,
    input: &str,
    context: &mut ScriptContext,
) -> Result<String, String> {
    let commands = parse_script(input)?;
    let mut outputs = Vec::new();

    for command in commands {
        let args = expand_runtime_macros(state, chat_id, &command.args, context).await?;
        let effective_args = if args.trim().is_empty() {
            context.pipe.clone()
        } else {
            args
        };
        context.pipe = execute_command(
            state,
            cartridge_id,
            chat_id,
            &command.name,
            &effective_args,
            context,
        )
        .await?;
        if !context.pipe.trim().is_empty() {
            outputs.push(context.pipe.clone());
        }
    }

    Ok(outputs.join("\n"))
}

async fn execute_command(
    state: &AppState,
    cartridge_id: &str,
    chat_id: &str,
    name: &str,
    args: &str,
    context: &mut ScriptContext,
) -> Result<String, String> {
    match normalize_command_name(name).as_str() {
        "help" => Ok(help_text()),
        "echo" | "return" => Ok(args.to_string()),
        "let" | "setlocal" => set_local(context, args),
        "getlocal" => get_local(context, args),
        "setvar" | "setglobalvar" => set_var(state, chat_id, args).await,
        "addvar" | "incvar" | "decvar" => add_var(state, chat_id, name, args).await,
        "appendvar" => append_var(state, chat_id, args).await,
        "pushvar" | "arraypush" => push_var(state, chat_id, args).await,
        "popvar" | "arraypop" => pop_var(state, chat_id, args).await,
        "setjson" | "setprop" => set_json_path_var(state, chat_id, args).await,
        "getvar" | "getglobalvar" => get_var(state, chat_id, args).await,
        "delvar" | "unsetvar" | "deletevar" => del_var(state, chat_id, args).await,
        "listvar" | "vars" => list_vars(state, chat_id).await,
        "sys" | "system" => insert_script_message(state, chat_id, "system", args).await,
        "user" => insert_script_message(state, chat_id, "user", args).await,
        "assistant" | "char" | "bot" => {
            insert_script_message(state, chat_id, "assistant", args).await
        }
        "if" => run_if(state, cartridge_id, chat_id, args, context).await,
        "run" | "call" => run_closure(state, cartridge_id, chat_id, args, context).await,
        "each" | "foreach" => run_each(state, cartridge_id, chat_id, args, context).await,
        "send" => send_message(state, cartridge_id, chat_id, args).await,
        "trigger" | "dryrun" => dry_run(state, cartridge_id, chat_id, args).await,
        "swipes" | "swipe" => swipes(state, cartridge_id, chat_id, args).await,
        "lastmessage" | "lastmsg" | "last" => last_message(state, chat_id, args).await,
        other => Err(format!("Unsupported slash command: /{}", other)),
    }
}

fn parse_script(input: &str) -> Result<Vec<ScriptCommand>, String> {
    let parts = split_pipeline(input);
    let mut commands = Vec::new();

    for part in parts {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        let command = trimmed.strip_prefix('/').unwrap_or(trimmed);
        let mut pieces = command.splitn(2, char::is_whitespace);
        let name = pieces.next().unwrap_or("").trim();
        if name.is_empty() {
            return Err("Slash command name cannot be empty".to_string());
        }
        commands.push(ScriptCommand {
            name: name.to_string(),
            args: pieces.next().unwrap_or("").trim().to_string(),
        });
    }

    if commands.is_empty() {
        return Err("No slash command found".to_string());
    }
    Ok(commands)
}

fn split_pipeline(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    let mut quote: Option<char> = None;
    let mut brace_depth = 0usize;

    for ch in input.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if matches!(ch, '"' | '\'') {
            if quote == Some(ch) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(ch);
            }
            current.push(ch);
            continue;
        }
        if quote.is_none() {
            if ch == '{' {
                brace_depth += 1;
            } else if ch == '}' {
                brace_depth = brace_depth.saturating_sub(1);
            }
        }
        if ch == '|' && quote.is_none() && brace_depth == 0 {
            parts.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(ch);
        }
    }
    if escaped {
        current.push('\\');
    }
    parts.push(current.trim().to_string());
    parts
}

fn apply_pipe_macros(text: &str, pipe: &str) -> String {
    text.replace("{{pipe}}", pipe)
        .replace("{{Pipe}}", pipe)
        .replace("{{PIPE}}", pipe)
}

async fn expand_runtime_macros(
    state: &AppState,
    chat_id: &str,
    text: &str,
    context: &ScriptContext,
) -> Result<String, String> {
    let mut output = apply_pipe_macros(text, &context.pipe)
        .replace(
            "{{arg}}",
            context.args.first().map(String::as_str).unwrap_or(""),
        )
        .replace("{{args}}", &context.args.join(" "))
        .replace(
            "{{index}}",
            context
                .locals
                .get("index")
                .map(String::as_str)
                .unwrap_or(""),
        );
    output = replace_prefixed_macros(&output, "local", |name| {
        context.locals.get(name).cloned().unwrap_or_default()
    });
    output = replace_async_var_macros(state, chat_id, &output, "var").await?;
    output = replace_async_var_macros(state, chat_id, &output, "getvar").await?;
    Ok(output)
}

fn replace_prefixed_macros<F>(text: &str, prefix: &str, mut resolver: F) -> String
where
    F: FnMut(&str) -> String,
{
    let needle = format!("{{{{{}::", prefix);
    let mut output = text.to_string();
    while let Some(start) = output.find(&needle) {
        let Some(end) = output[start..].find("}}").map(|idx| start + idx) else {
            break;
        };
        let name = output[start + needle.len()..end].to_string();
        output.replace_range(start..end + 2, &resolver(&name));
    }
    output
}

async fn replace_async_var_macros(
    state: &AppState,
    chat_id: &str,
    text: &str,
    prefix: &str,
) -> Result<String, String> {
    let needle = format!("{{{{{}::", prefix);
    let mut output = text.to_string();
    while let Some(start) = output.find(&needle) {
        let Some(end) = output[start..].find("}}").map(|idx| start + idx) else {
            break;
        };
        let reference = output[start + needle.len()..end].to_string();
        let value = get_var_reference(state, chat_id, &reference).await?;
        output.replace_range(start..end + 2, &value);
    }
    Ok(output)
}

fn normalize_command_name(name: &str) -> String {
    name.trim()
        .trim_start_matches('/')
        .to_ascii_lowercase()
        .replace('-', "")
        .replace('_', "")
}

async fn set_var(state: &AppState, chat_id: &str, args: &str) -> Result<String, String> {
    let (name, value) = parse_name_value(args)?;
    state
        .repo
        .set_chat_variable(chat_id, &name, &value, &Utc::now().to_rfc3339())
        .await
        .map_err(|e| e.to_string())?;
    Ok(value)
}

async fn get_var(state: &AppState, chat_id: &str, args: &str) -> Result<String, String> {
    get_var_reference(state, chat_id, args.trim()).await
}

async fn get_var_reference(
    state: &AppState,
    chat_id: &str,
    reference: &str,
) -> Result<String, String> {
    let (name, path) = parse_var_reference(reference)?;
    let raw = state
        .repo
        .get_chat_variable(chat_id, &name)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    if path.is_empty() {
        return Ok(raw);
    }
    let value = parse_json_value(&raw);
    Ok(json_path_get(&value, &path)
        .map(json_to_pipe_string)
        .unwrap_or_default())
}

async fn add_var(
    state: &AppState,
    chat_id: &str,
    command: &str,
    args: &str,
) -> Result<String, String> {
    let (name, delta_text) = parse_name_value(args)?;
    let current = state
        .repo
        .get_chat_variable(chat_id, &name)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "0".to_string())
        .parse::<f64>()
        .unwrap_or(0.0);
    let mut delta = delta_text.parse::<f64>().unwrap_or(1.0);
    if normalize_command_name(command) == "decvar" {
        delta = -delta;
    }
    let value = format_number(current + delta);
    state
        .repo
        .set_chat_variable(chat_id, &name, &value, &Utc::now().to_rfc3339())
        .await
        .map_err(|e| e.to_string())?;
    Ok(value)
}

async fn append_var(state: &AppState, chat_id: &str, args: &str) -> Result<String, String> {
    let (name, suffix) = parse_name_value(args)?;
    let mut value = state
        .repo
        .get_chat_variable(chat_id, &name)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    value.push_str(&suffix);
    state
        .repo
        .set_chat_variable(chat_id, &name, &value, &Utc::now().to_rfc3339())
        .await
        .map_err(|e| e.to_string())?;
    Ok(value)
}

async fn push_var(state: &AppState, chat_id: &str, args: &str) -> Result<String, String> {
    let (name, item) = parse_name_value(args)?;
    let raw = state
        .repo
        .get_chat_variable(chat_id, &name)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "[]".to_string());
    let mut value = parse_json_value(&raw);
    if !value.is_array() {
        value = json!([]);
    }
    value
        .as_array_mut()
        .unwrap()
        .push(parse_json_or_string(&item));
    let serialized = value.to_string();
    state
        .repo
        .set_chat_variable(chat_id, &name, &serialized, &Utc::now().to_rfc3339())
        .await
        .map_err(|e| e.to_string())?;
    Ok(serialized)
}

async fn pop_var(state: &AppState, chat_id: &str, args: &str) -> Result<String, String> {
    let name = normalize_variable_name(args.trim())?;
    let raw = state
        .repo
        .get_chat_variable(chat_id, &name)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "[]".to_string());
    let mut value = parse_json_value(&raw);
    let popped = value
        .as_array_mut()
        .and_then(|items| items.pop())
        .unwrap_or(Value::Null);
    state
        .repo
        .set_chat_variable(chat_id, &name, &value.to_string(), &Utc::now().to_rfc3339())
        .await
        .map_err(|e| e.to_string())?;
    Ok(json_to_pipe_string(&popped))
}

async fn set_json_path_var(state: &AppState, chat_id: &str, args: &str) -> Result<String, String> {
    let (reference, raw_value) = parse_raw_name_value(args)?;
    let (name, path) = parse_var_reference(&reference)?;
    if path.is_empty() {
        return set_var(state, chat_id, args).await;
    }
    let raw = state
        .repo
        .get_chat_variable(chat_id, &name)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "{}".to_string());
    let mut value = parse_json_value(&raw);
    json_path_set(&mut value, &path, parse_json_or_string(&raw_value));
    let serialized = value.to_string();
    state
        .repo
        .set_chat_variable(chat_id, &name, &serialized, &Utc::now().to_rfc3339())
        .await
        .map_err(|e| e.to_string())?;
    Ok(serialized)
}

fn set_local(context: &mut ScriptContext, args: &str) -> Result<String, String> {
    let (name, value) = parse_name_value(args)?;
    context.locals.insert(name, value.clone());
    Ok(value)
}

fn get_local(context: &ScriptContext, args: &str) -> Result<String, String> {
    let name = normalize_variable_name(args.trim())?;
    Ok(context.locals.get(&name).cloned().unwrap_or_default())
}

async fn del_var(state: &AppState, chat_id: &str, args: &str) -> Result<String, String> {
    let name = normalize_variable_name(args.trim())?;
    state
        .repo
        .delete_chat_variable(chat_id, &name)
        .await
        .map_err(|e| e.to_string())?;
    Ok(String::new())
}

async fn list_vars(state: &AppState, chat_id: &str) -> Result<String, String> {
    let vars = state
        .repo
        .list_chat_variables(chat_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(vars
        .into_iter()
        .map(|row| format!("{}={}", row.name, row.value))
        .collect::<Vec<_>>()
        .join("\n"))
}

async fn insert_script_message(
    state: &AppState,
    chat_id: &str,
    role: &str,
    content: &str,
) -> Result<String, String> {
    let content = content.trim();
    if content.is_empty() {
        return Err(format!("/{role} requires message content"));
    }
    let now = Utc::now().to_rfc3339();
    state
        .repo
        .insert_message(&MessageRow {
            id: Uuid::new_v4().to_string(),
            chat_id: chat_id.to_string(),
            role: role.to_string(),
            content: strip_outer_quotes(content).to_string(),
            created_at: now.clone(),
        })
        .await
        .map_err(|e| e.to_string())?;
    state
        .repo
        .touch_chat(chat_id, &now)
        .await
        .map_err(|e| e.to_string())?;
    if role == "assistant" {
        if let Some(row) = state
            .repo
            .get_last_message_by_role(chat_id, "assistant")
            .await
            .map_err(|e| e.to_string())?
        {
            ensure_default_swipe(state, &row).await?;
        }
    }
    Ok(String::new())
}

async fn run_if(
    state: &AppState,
    cartridge_id: &str,
    chat_id: &str,
    args: &str,
    context: &mut ScriptContext,
) -> Result<String, String> {
    let conditional = parse_if(args)?;
    let condition = evaluate_condition(&conditional.condition, state, chat_id, context).await?;
    let branch = if condition {
        conditional.then_script
    } else {
        conditional.else_script.unwrap_or_default()
    };
    if branch.trim().is_empty() {
        return Ok(String::new());
    }
    Box::pin(run_script(state, cartridge_id, chat_id, &branch, context)).await
}

async fn run_closure(
    state: &AppState,
    cartridge_id: &str,
    chat_id: &str,
    args: &str,
    context: &mut ScriptContext,
) -> Result<String, String> {
    let (_, script, rest) = extract_leading_closure(args)
        .or_else(|| extract_leading_closure(&format!("{{{}}}", args)))
        .ok_or_else(|| "/run requires { commands }".to_string())?;
    let mut child = context.clone();
    child.args = split_args(&rest);
    Box::pin(run_script(
        state,
        cartridge_id,
        chat_id,
        &script,
        &mut child,
    ))
    .await
}

async fn run_each(
    state: &AppState,
    cartridge_id: &str,
    chat_id: &str,
    args: &str,
    context: &mut ScriptContext,
) -> Result<String, String> {
    let (source, script, _) = extract_leading_closure(args)
        .ok_or_else(|| "/each requires a value or variable followed by { commands }".to_string())?;
    let items = resolve_iterable(state, chat_id, source.trim()).await?;
    let mut outputs = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        let mut child = context.clone();
        child.args = vec![json_to_pipe_string(&item)];
        child
            .locals
            .insert("arg".to_string(), json_to_pipe_string(&item));
        child.locals.insert("index".to_string(), index.to_string());
        let output = Box::pin(run_script(
            state,
            cartridge_id,
            chat_id,
            &script,
            &mut child,
        ))
        .await?;
        if !output.trim().is_empty() {
            outputs.push(output);
        }
    }
    let joined = outputs.join("\n");
    context.pipe = joined.clone();
    Ok(joined)
}

async fn send_message(
    state: &AppState,
    cartridge_id: &str,
    chat_id: &str,
    args: &str,
) -> Result<String, String> {
    let message = strip_outer_quotes(args).trim().to_string();
    let user_message = if message.is_empty() {
        state
            .repo
            .get_last_message_by_role(chat_id, "user")
            .await
            .map_err(|e| e.to_string())?
            .map(|row| row.content)
            .unwrap_or_default()
    } else {
        insert_script_message(state, chat_id, "user", &message).await?;
        message
    };

    if user_message.trim().is_empty() {
        return Err("/send requires text or an existing user message".to_string());
    }
    generate_assistant_reply(state, cartridge_id, chat_id, &user_message).await
}

async fn generate_assistant_reply(
    state: &AppState,
    cartridge_id: &str,
    chat_id: &str,
    user_message: &str,
) -> Result<String, String> {
    let cartridge = state
        .repo
        .get_cartridge(cartridge_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cartridge not found".to_string())?;
    let dir = std::path::PathBuf::from(&cartridge.directory_path);
    let preset = fs::load_preset(&dir)?;
    let active_pid = state.active_profile_id.lock().unwrap().clone();
    let (provider, model, api_key, api_url_override) =
        resolve_script_chat_config(state, &preset, active_pid.as_deref()).await?;
    let preset = crate::application::dto::PresetConfig {
        provider: Some(provider),
        model: Some(model),
        provider_url: api_url_override.clone(),
        prompt_entries: preset.prompt_entries.clone(),
        ..preset
    };
    let rendered = prompt_engine::render_prompt(
        &state.repo,
        cartridge_id,
        chat_id,
        &dir,
        &preset,
        &cartridge.name,
        user_message,
        None,
    )
    .await?;

    let client = LlmHttpClient::new();
    let mut stream = client.stream_chat(&preset, &rendered.messages, &api_key, &api_url_override);
    let mut full_response = String::new();
    while let Some(chunk) = stream.next().await {
        full_response.push_str(&chunk?);
    }

    let now = Utc::now().to_rfc3339();
    let message_id = Uuid::new_v4().to_string();
    state
        .repo
        .insert_message(&MessageRow {
            id: message_id.clone(),
            chat_id: chat_id.to_string(),
            role: "assistant".to_string(),
            content: full_response.clone(),
            created_at: now.clone(),
        })
        .await
        .map_err(|e| e.to_string())?;
    state
        .repo
        .touch_chat(chat_id, &now)
        .await
        .map_err(|e| e.to_string())?;
    state
        .repo
        .insert_message_swipe(&MessageSwipeRow {
            id: Uuid::new_v4().to_string(),
            message_id,
            swipe_index: 0,
            content: full_response.clone(),
            created_at: now,
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(full_response)
}

async fn resolve_script_chat_config(
    state: &AppState,
    preset: &crate::application::dto::PresetConfig,
    active_pid: Option<&str>,
) -> Result<(String, String, String, Option<String>), String> {
    if let Some(pid) = active_pid {
        if let Some(profile) = state
            .repo
            .get_profile(pid)
            .await
            .map_err(|e| e.to_string())?
        {
            if let Some(key) = CredentialService::get(&state.repo, &profile.provider_id).await? {
                return Ok((profile.provider_id, profile.model, key, profile.api_url));
            }
        }
    }

    let provider = preset
        .provider
        .clone()
        .unwrap_or_else(|| "openai".to_string());
    let key = CredentialService::get(&state.repo, &provider)
        .await?
        .ok_or_else(|| format!("Provider '{}' has no API key configured.", provider))?;
    let url = CredentialService::get_url(&state.repo, &provider)
        .await
        .unwrap_or(None);
    let model = preset.model.clone().unwrap_or_else(|| "gpt-4o".to_string());
    Ok((provider, model, key, url))
}

async fn swipes(
    state: &AppState,
    cartridge_id: &str,
    chat_id: &str,
    args: &str,
) -> Result<String, String> {
    let mut pieces = args.splitn(2, char::is_whitespace);
    let action = pieces.next().unwrap_or("").trim().to_ascii_lowercase();
    let rest = pieces.next().unwrap_or("").trim();

    match action.as_str() {
        "" | "current" | "list" => {
            let row = state
                .repo
                .get_last_message_by_role(chat_id, "assistant")
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "No assistant swipe found.".to_string())?;
            ensure_default_swipe(state, &row).await?;
            let swipes = state
                .repo
                .list_message_swipes(&row.id)
                .await
                .map_err(|e| e.to_string())?;
            Ok(swipes
                .into_iter()
                .map(|swipe| {
                    let marker = if swipe.content == row.content {
                        "*"
                    } else {
                        " "
                    };
                    format!("{}{}: {}", marker, swipe.swipe_index + 1, swipe.content)
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }
        "add" | "append" => {
            let row = state
                .repo
                .get_last_message_by_role(chat_id, "assistant")
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "No assistant swipe found.".to_string())?;
            ensure_default_swipe(state, &row).await?;
            let content = strip_outer_quotes(rest).to_string();
            if content.trim().is_empty() {
                return Err("/swipes add requires content".to_string());
            }
            let index = state
                .repo
                .next_swipe_index(&row.id)
                .await
                .map_err(|e| e.to_string())?;
            state
                .repo
                .insert_message_swipe(&MessageSwipeRow {
                    id: Uuid::new_v4().to_string(),
                    message_id: row.id.clone(),
                    swipe_index: index,
                    content: content.clone(),
                    created_at: Utc::now().to_rfc3339(),
                })
                .await
                .map_err(|e| e.to_string())?;
            state
                .repo
                .update_message_content(&row.id, &content)
                .await
                .map_err(|e| e.to_string())?;
            Ok(content)
        }
        "select" | "use" | "set" => {
            let row = state
                .repo
                .get_last_message_by_role(chat_id, "assistant")
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "No assistant swipe found.".to_string())?;
            ensure_default_swipe(state, &row).await?;
            let selected = rest
                .parse::<i64>()
                .map_err(|_| "/swipes select requires a 1-based index".to_string())?
                .saturating_sub(1);
            let swipe = state
                .repo
                .list_message_swipes(&row.id)
                .await
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|swipe| swipe.swipe_index == selected)
                .ok_or_else(|| format!("Swipe {} not found", selected + 1))?;
            state
                .repo
                .update_message_content(&row.id, &swipe.content)
                .await
                .map_err(|e| e.to_string())?;
            Ok(swipe.content)
        }
        "delete" | "del" | "remove" => {
            let row = state
                .repo
                .get_last_message_by_role(chat_id, "assistant")
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "No assistant swipe found.".to_string())?;
            state
                .repo
                .delete_message(&row.id)
                .await
                .map_err(|e| e.to_string())?;
            Ok(String::new())
        }
        "regenerate" | "regen" | "next" => {
            let previous = state
                .repo
                .get_last_message_by_role(chat_id, "assistant")
                .await
                .map_err(|e| e.to_string())?;
            let previous_id = previous.as_ref().map(|row| row.id.clone());
            let reply = send_message(state, cartridge_id, chat_id, rest).await?;
            if let (Some(old_id), Some(new_row)) = (
                previous_id,
                state
                    .repo
                    .get_last_message_by_role(chat_id, "assistant")
                    .await
                    .map_err(|e| e.to_string())?,
            ) {
                if old_id != new_row.id {
                    state
                        .repo
                        .delete_message(&new_row.id)
                        .await
                        .map_err(|e| e.to_string())?;
                    let old_row = state
                        .repo
                        .get_messages_by_chat(chat_id, 10_000)
                        .await
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .find(|message| message.id == old_id);
                    if let Some(old_row) = old_row {
                        ensure_default_swipe(state, &old_row).await?;
                        let index = state
                            .repo
                            .next_swipe_index(&old_row.id)
                            .await
                            .map_err(|e| e.to_string())?;
                        state
                            .repo
                            .insert_message_swipe(&MessageSwipeRow {
                                id: Uuid::new_v4().to_string(),
                                message_id: old_row.id.clone(),
                                swipe_index: index,
                                content: reply.clone(),
                                created_at: Utc::now().to_rfc3339(),
                            })
                            .await
                            .map_err(|e| e.to_string())?;
                        state
                            .repo
                            .update_message_content(&old_row.id, &reply)
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
            Ok(reply)
        }
        "replace" => {
            let row = state
                .repo
                .get_last_message_by_role(chat_id, "assistant")
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "No assistant swipe found.".to_string())?;
            let content = strip_outer_quotes(rest).to_string();
            state
                .repo
                .update_message_content(&row.id, &content)
                .await
                .map_err(|e| e.to_string())?;
            ensure_default_swipe(state, &row).await?;
            let index = state
                .repo
                .next_swipe_index(&row.id)
                .await
                .map_err(|e| e.to_string())?;
            state
                .repo
                .insert_message_swipe(&MessageSwipeRow {
                    id: Uuid::new_v4().to_string(),
                    message_id: row.id,
                    swipe_index: index,
                    content: content.clone(),
                    created_at: Utc::now().to_rfc3339(),
                })
                .await
                .map_err(|e| e.to_string())?;
            Ok(content)
        }
        "dropcurrent" => {
            if let Some(row) = state
                .repo
                .get_last_message_by_role(chat_id, "assistant")
                .await
                .map_err(|e| e.to_string())?
            {
                state
                    .repo
                    .delete_message(&row.id)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            Ok(String::new())
        }
        other => Err(format!("Unsupported /swipes action: {}", other)),
    }
}

async fn ensure_default_swipe(state: &AppState, row: &MessageRow) -> Result<(), String> {
    if row.role != "assistant" {
        return Ok(());
    }
    let existing = state
        .repo
        .list_message_swipes(&row.id)
        .await
        .map_err(|e| e.to_string())?;
    if existing.is_empty() {
        state
            .repo
            .insert_message_swipe(&MessageSwipeRow {
                id: Uuid::new_v4().to_string(),
                message_id: row.id.clone(),
                swipe_index: 0,
                content: row.content.clone(),
                created_at: row.created_at.clone(),
            })
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn last_message(state: &AppState, chat_id: &str, args: &str) -> Result<String, String> {
    let role = match args.trim().to_ascii_lowercase().as_str() {
        "user" => "user",
        "system" => "system",
        _ => "assistant",
    };
    Ok(state
        .repo
        .get_last_message_by_role(chat_id, role)
        .await
        .map_err(|e| e.to_string())?
        .map(|row| row.content)
        .unwrap_or_default())
}

async fn dry_run(
    state: &AppState,
    cartridge_id: &str,
    chat_id: &str,
    args: &str,
) -> Result<String, String> {
    let cartridge = state
        .repo
        .get_cartridge(cartridge_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cartridge not found".to_string())?;
    let dir = std::path::PathBuf::from(&cartridge.directory_path);
    let preset = fs::load_preset(&dir)?;
    let rendered = prompt_engine::render_prompt(
        &state.repo,
        cartridge_id,
        chat_id,
        &dir,
        &preset,
        &cartridge.name,
        args,
        Some(args.to_string()),
    )
    .await?;
    let included = rendered
        .dry_run
        .world_triggers
        .iter()
        .filter(|item| item.included)
        .map(|item| {
            item.id
                .clone()
                .unwrap_or_else(|| item.keys.join(","))
                .trim()
                .to_string()
        })
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    Ok(format!(
        "messages={}\nworld_info={}",
        rendered.dry_run.messages.len(),
        included.join(", ")
    ))
}

fn parse_if(args: &str) -> Result<IfCommand, String> {
    let (condition, then_script, rest) = extract_leading_closure(args)
        .ok_or_else(|| "/if requires a condition followed by { commands }".to_string())?;
    let else_script = parse_else_closure(rest);
    Ok(IfCommand {
        condition: condition.trim().to_string(),
        then_script,
        else_script,
    })
}

fn extract_leading_closure(input: &str) -> Option<(String, String, String)> {
    let mut escaped = false;
    let mut quote: Option<char> = None;
    let mut open = None;

    for (idx, ch) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if matches!(ch, '"' | '\'') {
            if quote == Some(ch) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(ch);
            }
            continue;
        }
        if ch == '{' && quote.is_none() {
            open = Some(idx);
            break;
        }
    }

    let open = open?;
    let close = find_matching_brace(input, open)?;
    Some((
        input[..open].trim().to_string(),
        input[open + 1..close].trim().to_string(),
        input[close + 1..].trim().to_string(),
    ))
}

fn parse_else_closure(rest: String) -> Option<String> {
    let trimmed = rest.trim();
    let rest = trimmed.strip_prefix("else")?.trim();
    let (_, script, _) = extract_leading_closure(rest)?;
    Some(script)
}

fn find_matching_brace(input: &str, open: usize) -> Option<usize> {
    let mut escaped = false;
    let mut quote: Option<char> = None;
    let mut depth = 0usize;

    for (idx, ch) in input.char_indices().skip_while(|(idx, _)| *idx < open) {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if matches!(ch, '"' | '\'') {
            if quote == Some(ch) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(ch);
            }
            continue;
        }
        if quote.is_some() {
            continue;
        }
        if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(idx);
            }
        }
    }
    None
}

async fn evaluate_condition(
    condition: &str,
    state: &AppState,
    chat_id: &str,
    context: &ScriptContext,
) -> Result<bool, String> {
    let expanded = expand_runtime_macros(state, chat_id, condition, context).await?;
    let trimmed = expanded.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }

    for op in ["==", "!=", ">=", "<=", ">", "<", " contains ", " in "] {
        if let Some((left, right)) = split_condition(trimmed, op) {
            return Ok(compare_values(left, right, op.trim()));
        }
    }

    Ok(truthy(trimmed))
}

fn split_condition<'a>(condition: &'a str, op: &str) -> Option<(&'a str, &'a str)> {
    condition
        .find(op)
        .map(|idx| (&condition[..idx], &condition[idx + op.len()..]))
}

fn compare_values(left: &str, right: &str, op: &str) -> bool {
    let left = strip_outer_quotes(left).trim();
    let right = strip_outer_quotes(right).trim();
    match op {
        "==" => left == right,
        "!=" => left != right,
        "contains" => left.contains(right),
        "in" => right.contains(left),
        ">" | "<" | ">=" | "<=" => {
            let l = left.parse::<f64>().unwrap_or(f64::NAN);
            let r = right.parse::<f64>().unwrap_or(f64::NAN);
            match op {
                ">" => l > r,
                "<" => l < r,
                ">=" => l >= r,
                "<=" => l <= r,
                _ => false,
            }
        }
        _ => false,
    }
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

fn truthy(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "0" | "false" | "null" | "none" | "undefined"
    )
}

async fn resolve_iterable(
    state: &AppState,
    chat_id: &str,
    source: &str,
) -> Result<Vec<Value>, String> {
    let raw = if let Some(name) = source.strip_prefix('$') {
        get_var_reference(state, chat_id, name).await?
    } else if source.starts_with("{{") {
        replace_async_var_macros(state, chat_id, source, "var").await?
    } else if source.starts_with('[') || source.starts_with('{') {
        source.to_string()
    } else {
        get_var_reference(state, chat_id, source)
            .await
            .unwrap_or_else(|_| source.to_string())
    };
    let value = parse_json_value(&raw);
    Ok(match value {
        Value::Array(items) => items,
        Value::Object(map) => map
            .into_iter()
            .map(|(key, value)| json!({ "key": key, "value": value }))
            .collect(),
        Value::Null => Vec::new(),
        other => vec![other],
    })
}

fn parse_var_reference(reference: &str) -> Result<(String, Vec<PathSegment>), String> {
    let trimmed = reference.trim();
    let mut split_at = trimmed.len();
    for (idx, ch) in trimmed.char_indices() {
        if ch == '.' || ch == '[' {
            split_at = idx;
            break;
        }
    }
    let name = normalize_variable_name(&trimmed[..split_at])?;
    let path = parse_json_path(&trimmed[split_at..])?;
    Ok((name, path))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PathSegment {
    Key(String),
    Index(usize),
}

fn parse_json_path(path: &str) -> Result<Vec<PathSegment>, String> {
    let mut result = Vec::new();
    let mut chars = path.char_indices().peekable();
    while let Some((_, ch)) = chars.next() {
        match ch {
            '.' => {
                let mut key = String::new();
                while let Some((_, next)) = chars.peek().copied() {
                    if next == '.' || next == '[' {
                        break;
                    }
                    key.push(next);
                    chars.next();
                }
                if !key.is_empty() {
                    result.push(PathSegment::Key(key));
                }
            }
            '[' => {
                let mut token = String::new();
                for (_, next) in chars.by_ref() {
                    if next == ']' {
                        break;
                    }
                    token.push(next);
                }
                let token = token.trim().trim_matches('"').trim_matches('\'');
                if let Ok(index) = token.parse::<usize>() {
                    result.push(PathSegment::Index(index));
                } else if !token.is_empty() {
                    result.push(PathSegment::Key(token.to_string()));
                }
            }
            _ => return Err(format!("Invalid JSON path segment '{}'", ch)),
        }
    }
    Ok(result)
}

fn parse_json_value(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
}

fn parse_json_or_string(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(strip_outer_quotes(raw).to_string()))
}

fn json_to_pipe_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn json_path_get<'a>(value: &'a Value, path: &[PathSegment]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        current = match segment {
            PathSegment::Key(key) => current.get(key)?,
            PathSegment::Index(index) => current.get(*index)?,
        };
    }
    Some(current)
}

fn json_path_set(value: &mut Value, path: &[PathSegment], new_value: Value) {
    if path.is_empty() {
        *value = new_value;
        return;
    }
    if value.is_null() || value.is_string() {
        *value = json!({});
    }

    let mut current = value;
    for segment in &path[..path.len() - 1] {
        match segment {
            PathSegment::Key(key) => {
                if !current.is_object() {
                    *current = json!({});
                }
                current = current
                    .as_object_mut()
                    .unwrap()
                    .entry(key.clone())
                    .or_insert_with(|| json!({}));
            }
            PathSegment::Index(index) => {
                if !current.is_array() {
                    *current = json!([]);
                }
                let array = current.as_array_mut().unwrap();
                while array.len() <= *index {
                    array.push(Value::Null);
                }
                current = &mut array[*index];
            }
        }
    }

    match path.last().unwrap() {
        PathSegment::Key(key) => {
            if !current.is_object() {
                *current = json!({});
            }
            current
                .as_object_mut()
                .unwrap()
                .insert(key.clone(), new_value);
        }
        PathSegment::Index(index) => {
            if !current.is_array() {
                *current = json!([]);
            }
            let array = current.as_array_mut().unwrap();
            while array.len() <= *index {
                array.push(Value::Null);
            }
            array[*index] = new_value;
        }
    }
}

fn split_args(args: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    let mut quote: Option<char> = None;
    for ch in args.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if matches!(ch, '"' | '\'') {
            if quote == Some(ch) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(ch);
            } else {
                current.push(ch);
            }
            continue;
        }
        if ch.is_whitespace() && quote.is_none() {
            if !current.is_empty() {
                result.push(current.clone());
                current.clear();
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

fn parse_name_value(args: &str) -> Result<(String, String), String> {
    let (name, value) = parse_raw_name_value(args)?;
    let name = normalize_variable_name(&name)?;
    Ok((name, value))
}

fn parse_raw_name_value(args: &str) -> Result<(String, String), String> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Err("/setvar requires a name and value".to_string());
    }

    let (name, value) = if let Some((name, value)) = trimmed.split_once('=') {
        (name.trim(), value.trim())
    } else {
        let mut pieces = trimmed.splitn(2, char::is_whitespace);
        (
            pieces.next().unwrap_or("").trim(),
            pieces.next().unwrap_or("").trim(),
        )
    };

    Ok((name.to_string(), strip_outer_quotes(value).to_string()))
}

fn normalize_variable_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Variable name cannot be empty".to_string());
    }
    if trimmed.len() > 128 {
        return Err("Variable name is too long".to_string());
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':'))
    {
        return Err("Variable name contains unsupported characters".to_string());
    }
    Ok(trimmed.to_string())
}

fn strip_outer_quotes(value: &str) -> &str {
    let trimmed = value.trim();
    if trimmed.len() >= 2 {
        let first = trimmed.as_bytes()[0] as char;
        let last = trimmed.as_bytes()[trimmed.len() - 1] as char;
        if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
            return &trimmed[1..trimmed.len() - 1];
        }
    }
    trimmed
}

fn help_text() -> String {
    [
        "Supported slash commands:",
        "/setvar name=value",
        "/setjson object.path=value",
        "/addvar name=1",
        "/appendvar name=text",
        "/pushvar list=value",
        "/popvar list",
        "/let name=value",
        "/getlocal name",
        "/getvar name",
        "/getvar object.path",
        "/delvar name",
        "/listvar",
        "/sys text",
        "/user text",
        "/assistant text",
        "/if condition { commands } else { commands }",
        "/run { commands } arg1 arg2",
        "/each list { commands with {{arg}} and {{index}} }",
        "/send text",
        "/echo text",
        "/trigger text",
        "/swipes current|add|select|replace|regenerate|delete",
        "Pipes are supported: /getvar name | /echo {{pipe}}",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pipeline_with_quotes_and_escaped_pipe() {
        let commands = parse_script(
            r#"/echo "a|b" | /if mood == soft { /echo a|b } | /setvar mood=soft\|bright"#,
        )
        .unwrap();
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0].name, "echo");
        assert_eq!(commands[0].args, r#""a|b""#);
        assert_eq!(commands[1].name, "if");
        assert_eq!(commands[1].args, "mood == soft { /echo a|b }");
        assert_eq!(commands[2].name, "setvar");
        assert_eq!(commands[2].args, "mood=soft|bright");
    }

    #[test]
    fn parses_setvar_name_value_forms() {
        assert_eq!(
            parse_name_value("mood=happy").unwrap(),
            ("mood".to_string(), "happy".to_string())
        );
        assert_eq!(
            parse_name_value("mood \"very happy\"").unwrap(),
            ("mood".to_string(), "very happy".to_string())
        );
    }

    #[test]
    fn parses_if_closures_with_else_branch() {
        let parsed = parse_if(r#"mood == "happy" { /echo yes } else { /echo no }"#).unwrap();
        assert_eq!(parsed.condition, r#"mood == "happy""#);
        assert_eq!(parsed.then_script, "/echo yes");
        assert_eq!(parsed.else_script.as_deref(), Some("/echo no"));
    }

    #[test]
    fn compares_condition_values() {
        assert!(compare_values("5", "3", ">"));
        assert!(compare_values("hello world", "world", "contains"));
        assert!(compare_values("world", "hello world", "in"));
        assert!(!truthy("false"));
        assert!(truthy("yes"));
    }

    #[test]
    fn json_path_get_and_set_support_arrays_and_objects() {
        let mut value = json!({});
        let path = parse_json_path(".stats.affection").unwrap();
        json_path_set(&mut value, &path, json!(7));
        let list_path = parse_json_path(".items[0]").unwrap();
        json_path_set(&mut value, &list_path, json!("tea"));

        assert_eq!(json_path_get(&value, &path), Some(&json!(7)));
        assert_eq!(json_path_get(&value, &list_path), Some(&json!("tea")));
    }

    #[test]
    fn split_args_respects_quotes() {
        assert_eq!(
            split_args(r#"one "two words" three"#),
            vec![
                "one".to_string(),
                "two words".to_string(),
                "three".to_string()
            ]
        );
    }
}
