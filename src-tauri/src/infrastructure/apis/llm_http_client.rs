use crate::application::dto::{ChatMessage, PresetConfig};
use crate::infrastructure::provider_registry::{get_provider, resolve_url, ApiFormat, AuthType};
use futures::channel::mpsc;
use futures::{SinkExt, Stream, StreamExt};
use serde_json::Value;
use std::pin::Pin;

type ChunkStream = Pin<Box<dyn Stream<Item = Result<String, String>> + Send>>;

const ANTHROPIC_MODELS: &[&str] = &[
    "claude-opus-4-7",
    "claude-sonnet-4-6",
    "claude-haiku-4-5",
    "claude-opus-4-5",
    "claude-sonnet-4-5",
    "claude-haiku-3-5",
    "claude-3-5-sonnet",
    "claude-3-5-haiku",
];

pub struct LlmHttpClient {
    client: reqwest::Client,
}

impl LlmHttpClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Fetch available model IDs from the provider.
    pub async fn fetch_models(
        &self,
        provider_id: &str,
        api_key: &str,
        api_url: &str,
    ) -> Result<Vec<String>, String> {
        let info = get_provider(provider_id)
            .ok_or_else(|| format!("Unknown provider: {}", provider_id))?;

        match info.api_format {
            ApiFormat::Anthropic => {
                // Anthropic has no public models API — return known models
                Ok(ANTHROPIC_MODELS.iter().map(|s| s.to_string()).collect())
            }
            ApiFormat::Gemini => self.fetch_gemini_models(api_key, api_url).await,
            ApiFormat::OpenAiCompatible => {
                // OpenRouter has its own models endpoint
                if provider_id == "openrouter" {
                    self.fetch_openrouter_models(api_key).await
                } else {
                    self.fetch_openai_compatible_models(api_key, api_url).await
                }
            }
        }
    }

    /// Test connection by sending a minimal chat request.
    pub async fn test_connection(
        &self,
        provider_id: &str,
        api_key: &str,
        api_url: &str,
        model: &str,
    ) -> Result<(), String> {
        let info = get_provider(provider_id)
            .ok_or_else(|| format!("Unknown provider: {}", provider_id))?;

        match info.api_format {
            ApiFormat::Anthropic => {
                self.test_anthropic_connection(api_key, api_url, model).await
            }
            ApiFormat::Gemini => {
                self.test_gemini_connection(api_key, api_url, model).await
            }
            ApiFormat::OpenAiCompatible => {
                self.test_openai_compatible_connection(api_key, api_url, model, provider_id).await
            }
        }
    }

    // ── Model fetching helpers ──────────────────────

    async fn fetch_openai_compatible_models(
        &self,
        api_key: &str,
        api_url: &str,
    ) -> Result<Vec<String>, String> {
        let models_url = derive_models_url(api_url);
        let resp = self
            .client
            .get(&models_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("HTTP {}: {}", status, text));
        }

        let json: Value = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;
        let models: Vec<String> = json["data"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|m| m["id"].as_str().map(String::from))
            .collect();

        if models.is_empty() {
            return Err("No models returned from API".into());
        }
        Ok(models)
    }

    async fn fetch_openrouter_models(&self, api_key: &str) -> Result<Vec<String>, String> {
        let resp = self
            .client
            .get("https://openrouter.ai/api/v1/models")
            .header("Authorization", format!("Bearer {}", api_key))
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("HTTP {}: {}", status, text));
        }

        let json: Value = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;
        let models: Vec<String> = json["data"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|m| m["id"].as_str().map(String::from))
            .collect();

        if models.is_empty() {
            return Err("No models returned from OpenRouter".into());
        }
        Ok(models)
    }

    async fn fetch_gemini_models(
        &self,
        api_key: &str,
        api_url: &str,
    ) -> Result<Vec<String>, String> {
        let url = format!("{}?key={}", api_url.trim_end_matches('/'), api_key);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("HTTP {}: {}", status, text));
        }

        let json: Value = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;
        let models: Vec<String> = json["models"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|m| {
                m["name"]
                    .as_str()
                    .map(|n| n.strip_prefix("models/").unwrap_or(n).to_string())
            })
            .collect();

        if models.is_empty() {
            return Err("No models returned from Gemini".into());
        }
        Ok(models)
    }

    // ── Connection test helpers ────────────────────

    async fn test_openai_compatible_connection(
        &self,
        api_key: &str,
        api_url: &str,
        model: &str,
        provider_id: &str,
    ) -> Result<(), String> {
        let info = get_provider(provider_id);
        let auth_type = info
            .as_ref()
            .map(|i| i.auth_type.clone())
            .unwrap_or(AuthType::Bearer);

        let body = serde_json::json!({
            "model": model,
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 5,
            "stream": false,
        });

        let mut req = self
            .client
            .post(api_url)
            .header("Content-Type", "application/json")
            .json(&body);

        req = match auth_type {
            AuthType::Bearer => req.header("Authorization", format!("Bearer {}", api_key)),
            AuthType::ApiKey => req.header("x-api-key", api_key),
            AuthType::GoogleApiKey => req.header("x-goog-api-key", api_key),
        };

        let resp = req.send().await.map_err(|e| format!("Connection failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("HTTP {}: {}", status, text));
        }

        Ok(())
    }

    async fn test_anthropic_connection(
        &self,
        api_key: &str,
        api_url: &str,
        model: &str,
    ) -> Result<(), String> {
        let body = serde_json::json!({
            "model": model,
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 5,
            "stream": false,
        });

        let resp = self
            .client
            .post(api_url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Connection failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("HTTP {}: {}", status, text));
        }

        Ok(())
    }

    async fn test_gemini_connection(
        &self,
        api_key: &str,
        api_url: &str,
        model: &str,
    ) -> Result<(), String> {
        let base = api_url
            .trim_end_matches('/')
            .rsplit_once("/models")
            .map(|(prefix, _)| prefix)
            .unwrap_or(api_url.trim_end_matches('/'));
        let url = format!(
            "{}/models/{}:generateContent?key={}",
            base, model, api_key
        );

        let body = serde_json::json!({
            "contents": [{"parts": [{"text": "Hi"}]}],
            "generationConfig": {"maxOutputTokens": 5},
        });

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Connection failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("HTTP {}: {}", status, text));
        }

        Ok(())
    }

    pub fn build_payload_preview(
        preset: &PresetConfig,
        messages: &[ChatMessage],
    ) -> Value {
        let provider = preset
            .provider
            .clone()
            .unwrap_or_else(|| "openai".to_string());
        let info = get_provider(&provider);
        let format = info
            .as_ref()
            .map(|i| i.api_format.clone())
            .unwrap_or(ApiFormat::OpenAiCompatible);

        match format {
            ApiFormat::Anthropic => Self::build_anthropic_body(preset, messages, true),
            ApiFormat::Gemini => Self::build_gemini_body(preset, messages),
            ApiFormat::OpenAiCompatible => {
                Self::build_openai_compatible_body(preset, messages, true)
            }
        }
    }

    pub fn serialize_messages_for_debug(
        preset: &PresetConfig,
        messages: &[ChatMessage],
    ) -> String {
        if preset
            .chat_format
            .as_deref()
            .unwrap_or("chatml")
            .eq_ignore_ascii_case("alpaca")
        {
            return serialize_alpaca(messages);
        }

        messages
            .iter()
            .map(|m| format!("<|{}|>\n{}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn stream_chat(
        &self,
        preset: &PresetConfig,
        messages: &[ChatMessage],
        api_key: &str,
        global_url: &Option<String>,
    ) -> ChunkStream {
        let provider = preset
            .provider
            .clone()
            .unwrap_or_else(|| "openai".to_string());
        let info = get_provider(&provider);
        let format = info
            .as_ref()
            .map(|i| i.api_format.clone())
            .unwrap_or(ApiFormat::OpenAiCompatible);

        match format {
            ApiFormat::Anthropic => self.stream_anthropic(preset, messages, api_key, global_url),
            ApiFormat::Gemini => self.stream_gemini(preset, messages, api_key, global_url),
            ApiFormat::OpenAiCompatible => {
                self.stream_openai_compatible(preset, messages, api_key, global_url)
            }
        }
    }

    fn resolve_request_url(
        provider_id: &str,
        preset: &PresetConfig,
        global_url: &Option<String>,
    ) -> String {
        resolve_url(provider_id, &preset.provider_url, global_url)
    }

    fn apply_auth(request: reqwest::RequestBuilder, provider: &str, api_key: &str) -> reqwest::RequestBuilder {
        let info = get_provider(provider);
        let auth_type = info
            .as_ref()
            .map(|i| i.auth_type.clone())
            .unwrap_or(AuthType::Bearer);

        match auth_type {
            AuthType::Bearer => request.header("Authorization", format!("Bearer {}", api_key)),
            AuthType::ApiKey => request.header("x-api-key", api_key),
            AuthType::GoogleApiKey => request.header("x-goog-api-key", api_key),
        }
    }

    // ── OpenAI-compatible streaming ─────────────────────

    fn stream_openai_compatible(
        &self,
        preset: &PresetConfig,
        messages: &[ChatMessage],
        api_key: &str,
        global_url: &Option<String>,
    ) -> ChunkStream {
        let client = self.client.clone();
        let provider = preset
            .provider
            .clone()
            .unwrap_or_else(|| "openai".to_string());
        let url = Self::resolve_request_url(&provider, preset, global_url);
        let body = Self::build_openai_compatible_body(preset, messages, true);

        let (mut tx, rx) = mpsc::channel::<Result<String, String>>(64);
        let api_key = api_key.to_string();

        tokio::spawn(async move {
            let req = client
                .post(&url)
                .header("Content-Type", "application/json")
                .json(&body);
            let req = Self::apply_auth(req, &provider, &api_key);

            let result = req.send().await;

            let response = match result {
                Ok(r) => r,
                Err(e) => {
                    tx.send(Err(format!("Request failed: {}", e))).await.ok();
                    tx.close_channel();
                    return;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                tx.send(Err(format!("HTTP {}: {}", status, text)))
                    .await
                    .ok();
                tx.close_channel();
                return;
            }

            Self::parse_openai_sse(response, &mut tx).await;
        });

        Box::pin(rx)
    }

    async fn parse_openai_sse(
        response: reqwest::Response,
        tx: &mut mpsc::Sender<Result<String, String>>,
    ) {
        let mut byte_stream = response.bytes_stream();
        let mut buffer: Vec<u8> = Vec::new();

        while let Some(chunk_result) = byte_stream.next().await {
            match chunk_result {
                Ok(chunk) => {
                    buffer.extend_from_slice(&chunk);

                    while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                        let line_bytes = &buffer[..pos];
                        let rest = buffer[pos + 1..].to_vec();

                        let line = String::from_utf8_lossy(line_bytes);
                        let line = line.trim().to_string();
                        buffer = rest;

                        if line.is_empty() {
                            continue;
                        }

                        if line == "data: [DONE]" {
                            tx.close_channel();
                            return;
                        }

                        if let Some(data) = line.strip_prefix("data: ") {
                            if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                                if let Some(content) =
                                    parsed["choices"][0]["delta"]["content"].as_str()
                                {
                                    if tx.send(Ok(content.to_string())).await.is_err() {
                                        return;
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    tx.send(Err(format!("Stream error: {}", e))).await.ok();
                    tx.close_channel();
                    return;
                }
            }
        }

        // Flush remaining bytes
        if !buffer.is_empty() {
            let line = String::from_utf8_lossy(&buffer).trim().to_string();
            if !line.is_empty() {
                if let Some(data) = line.strip_prefix("data: ") {
                    if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                        if let Some(content) = parsed["choices"][0]["delta"]["content"].as_str() {
                            tx.send(Ok(content.to_string())).await.ok();
                        }
                    }
                }
            }
        }

        tx.close_channel();
    }

    // ── Anthropic streaming ─────────────────────────────

    fn stream_anthropic(
        &self,
        preset: &PresetConfig,
        messages: &[ChatMessage],
        api_key: &str,
        global_url: &Option<String>,
    ) -> ChunkStream {
        let client = self.client.clone();
        let url = Self::resolve_request_url("anthropic", preset, global_url);
        let body = Self::build_anthropic_body(preset, messages, true);

        let (mut tx, rx) = mpsc::channel::<Result<String, String>>(64);
        let api_key = api_key.to_string();

        tokio::spawn(async move {
            let result = client
                .post(&url)
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await;

            let response = match result {
                Ok(r) => r,
                Err(e) => {
                    tx.send(Err(format!("Request failed: {}", e))).await.ok();
                    tx.close_channel();
                    return;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                tx.send(Err(format!("HTTP {}: {}", status, text)))
                    .await
                    .ok();
                tx.close_channel();
                return;
            }

            let mut byte_stream = response.bytes_stream();
            let mut buffer: Vec<u8> = Vec::new();

            while let Some(chunk_result) = byte_stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        buffer.extend_from_slice(&chunk);

                        while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                            let line_bytes = &buffer[..pos];
                            let rest = buffer[pos + 1..].to_vec();

                            let line = String::from_utf8_lossy(line_bytes);
                            let line = line.trim().to_string();
                            buffer = rest;

                            if line.is_empty() {
                                continue;
                            }

                            if let Some(data) = line.strip_prefix("data: ") {
                                if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                                    if parsed["type"] == "content_block_delta" {
                                        if let Some(text) = parsed["delta"]["text"].as_str() {
                                            if tx.send(Ok(text.to_string())).await.is_err() {
                                                return;
                                            }
                                        }
                                    }
                                    if parsed["type"] == "message_stop" {
                                        tx.close_channel();
                                        return;
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tx.send(Err(format!("Stream error: {}", e))).await.ok();
                        tx.close_channel();
                        return;
                    }
                }
            }

            tx.close_channel();
        });

        Box::pin(rx)
    }

    // ── Gemini streaming ────────────────────────────────

    fn stream_gemini(
        &self,
        preset: &PresetConfig,
        messages: &[ChatMessage],
        api_key: &str,
        global_url: &Option<String>,
    ) -> ChunkStream {
        let client = self.client.clone();
        let base_url = Self::resolve_request_url("google_gemini", preset, global_url);
        let model = preset
            .model
            .clone()
            .unwrap_or_else(|| "gemini-2.0-flash".to_string());
        let url = format!(
            "{}/{}:streamGenerateContent?alt=sse",
            base_url.trim_end_matches('/'),
            model
        );
        let body = Self::build_gemini_body(preset, messages);

        let (mut tx, rx) = mpsc::channel::<Result<String, String>>(64);
        let api_key = api_key.to_string();

        tokio::spawn(async move {
            let result = client
                .post(&url)
                .header("x-goog-api-key", &api_key)
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await;

            let response = match result {
                Ok(r) => r,
                Err(e) => {
                    tx.send(Err(format!("Request failed: {}", e))).await.ok();
                    tx.close_channel();
                    return;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                tx.send(Err(format!("HTTP {}: {}", status, text)))
                    .await
                    .ok();
                tx.close_channel();
                return;
            }

            let mut byte_stream = response.bytes_stream();
            let mut buffer: Vec<u8> = Vec::new();

            while let Some(chunk_result) = byte_stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        buffer.extend_from_slice(&chunk);

                        while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                            let line_bytes = &buffer[..pos];
                            let rest = buffer[pos + 1..].to_vec();

                            let line = String::from_utf8_lossy(line_bytes);
                            let line = line.trim().to_string();
                            buffer = rest;

                            if line.is_empty() {
                                continue;
                            }

                            if let Some(data) = line.strip_prefix("data: ") {
                                if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                                    if let Some(candidates) = parsed["candidates"].as_array() {
                                        for candidate in candidates {
                                            if let Some(parts) = candidate["content"]["parts"]
                                                .as_array()
                                            {
                                                for part in parts {
                                                    if let Some(text) = part["text"].as_str() {
                                                        if tx
                                                            .send(Ok(text.to_string()))
                                                            .await
                                                            .is_err()
                                                        {
                                                            return;
                                                        }
                                                    }
                                                }
                                            }
                                            // Non-standard stop reasons (SAFETY, RECITATION, etc.)
                                            let reason = candidate["finishReason"].as_str().unwrap_or("");
                                            if !reason.is_empty()
                                                && reason != "STOP"
                                                && reason != "MAX_TOKENS"
                                            {
                                                let _ = tx
                                                    .send(Err(format!(
                                                        "Gemini stream stopped: {}",
                                                        reason
                                                    )))
                                                    .await;
                                                tx.close_channel();
                                                return;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tx.send(Err(format!("Stream error: {}", e))).await.ok();
                        tx.close_channel();
                        return;
                    }
                }
            }

            tx.close_channel();
        });

        Box::pin(rx)
    }

    // ── Body builders ───────────────────────────────────

    fn build_openai_compatible_body(
        preset: &PresetConfig,
        messages: &[ChatMessage],
        stream: bool,
    ) -> Value {
        let provider = preset
            .provider
            .clone()
            .unwrap_or_else(|| "openai".to_string());
        let info = get_provider(&provider);
        let default_model = info
            .as_ref()
            .map(|i| i.default_model.clone())
            .unwrap_or_else(|| "gpt-4o".to_string());
        let model = preset.model.clone().unwrap_or(default_model);
        let temperature = preset.temperature.unwrap_or(0.7);
        let max_tokens = preset.max_tokens.unwrap_or(4096);
        let msgs = openai_messages_for_format(preset, messages);

        serde_json::json!({
            "model": model,
            "messages": msgs,
            "temperature": temperature,
            "max_tokens": max_tokens,
            "stream": stream,
        })
    }

    fn build_anthropic_body(
        preset: &PresetConfig,
        messages: &[ChatMessage],
        stream: bool,
    ) -> Value {
        let model = preset
            .model
            .clone()
            .unwrap_or_else(|| "claude-sonnet-4-6".to_string());
        let temperature = preset.temperature.unwrap_or(0.7);
        let max_tokens = preset.max_tokens.unwrap_or(4096);

        let system_prompt = messages
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.content.clone())
            .collect::<Vec<_>>()
            .join("\n\n");

        let conversation: Vec<Value> = messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content,
                })
            })
            .collect();

        let mut body = serde_json::json!({
            "model": model,
            "messages": conversation,
            "temperature": temperature,
            "max_tokens": max_tokens,
            "stream": stream,
        });

        if !system_prompt.is_empty() {
            body["system"] = serde_json::json!(system_prompt);
        }

        body
    }

    fn build_gemini_body(preset: &PresetConfig, messages: &[ChatMessage]) -> Value {
        let temperature = preset.temperature.unwrap_or(0.7);
        let max_tokens = preset.max_tokens.unwrap_or(4096);

        let system_prompt = messages
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.content.clone())
            .collect::<Vec<_>>()
            .join("\n\n");

        let contents: Vec<Value> = messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                let role = match m.role.as_str() {
                    "assistant" => "model",
                    "user" => "user",
                    _ => "user", // Gemini only accepts "user" / "model"
                };
                serde_json::json!({
                    "role": role,
                    "parts": [{"text": m.content}],
                })
            })
            .collect();

        let mut body = serde_json::json!({
            "contents": contents,
            "generationConfig": {
                "temperature": temperature,
                "maxOutputTokens": max_tokens,
            },
        });

        if !system_prompt.is_empty() {
            body["systemInstruction"] = serde_json::json!({
                "parts": [{"text": system_prompt}]
            });
        }

        body
    }
}

fn openai_messages_for_format(preset: &PresetConfig, messages: &[ChatMessage]) -> Vec<Value> {
    if preset
        .chat_format
        .as_deref()
        .unwrap_or("chatml")
        .eq_ignore_ascii_case("alpaca")
    {
        return vec![serde_json::json!({
            "role": "user",
            "content": serialize_alpaca(messages),
        })];
    }

    messages
        .iter()
        .map(|m| {
            serde_json::json!({
                "role": m.role,
                "content": m.content,
            })
        })
        .collect()
}

fn serialize_alpaca(messages: &[ChatMessage]) -> String {
    let system = messages
        .iter()
        .filter(|m| m.role == "system")
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");

    let conversation = messages
        .iter()
        .filter(|m| m.role != "system")
        .map(|m| match m.role.as_str() {
            "assistant" => format!("### Response:\n{}", m.content),
            _ => format!("### Instruction:\n{}", m.content),
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    if system.is_empty() {
        conversation
    } else if conversation.is_empty() {
        format!("### System:\n{}", system)
    } else {
        format!("### System:\n{}\n\n{}", system, conversation)
    }
}

/// Derive the /models URL from a chat completions endpoint.
/// e.g. "https://api.openai.com/v1/chat/completions" → "https://api.openai.com/v1/models"
fn derive_models_url(chat_url: &str) -> String {
    if chat_url.ends_with("/chat/completions") {
        chat_url.replace("/chat/completions", "/models")
    } else if chat_url.ends_with('/') {
        format!("{}models", chat_url)
    } else {
        format!("{}/models", chat_url)
    }
}
