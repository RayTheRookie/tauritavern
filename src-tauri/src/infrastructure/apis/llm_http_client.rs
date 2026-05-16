use crate::application::dto::{ChatMessage, PresetConfig};
use futures::channel::mpsc;
use futures::{SinkExt, Stream, StreamExt};
use serde_json::Value;
use std::pin::Pin;

type ChunkStream = Pin<Box<dyn Stream<Item = Result<String, String>> + Send>>;

pub struct LlmHttpClient {
    client: reqwest::Client,
}

impl LlmHttpClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    pub fn stream_chat(
        &self,
        preset: &PresetConfig,
        messages: &[ChatMessage],
        api_key: &str,
    ) -> ChunkStream {
        let provider = preset.provider.clone().unwrap_or_else(|| "openai".to_string());
        match provider.as_str() {
            "anthropic" => self.stream_anthropic(preset, messages, api_key),
            _ => self.stream_openai_compatible(preset, messages, api_key),
        }
    }

    fn stream_openai_compatible(
        &self,
        preset: &PresetConfig,
        messages: &[ChatMessage],
        api_key: &str,
    ) -> ChunkStream {
        let client = self.client.clone();
        let model = preset.model.clone().unwrap_or_else(|| "gpt-4o".to_string());
        let temperature = preset.temperature.unwrap_or(0.7);
        let max_tokens = preset.max_tokens.unwrap_or(4096);
        let url = preset
            .provider_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1/chat/completions".to_string());

        let msgs: Vec<Value> = messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content,
                })
            })
            .collect();

        let body = serde_json::json!({
            "model": model,
            "messages": msgs,
            "temperature": temperature,
            "max_tokens": max_tokens,
            "stream": true,
        });

        let (mut tx, rx) = mpsc::channel::<Result<String, String>>(64);
        let api_key = api_key.to_string();

        tokio::spawn(async move {
            let result = client
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
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
                tx.send(Err(format!("HTTP {}: {}", status, text))).await.ok();
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

                            // Only decode complete lines as UTF-8
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
                                    if let Some(content) = parsed["choices"][0]["delta"]["content"]
                                        .as_str()
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

            // Flush any remaining bytes in buffer as a last line
            if !buffer.is_empty() {
                let line = String::from_utf8_lossy(&buffer).trim().to_string();
                if !line.is_empty() {
                    if let Some(data) = line.strip_prefix("data: ") {
                        if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                            if let Some(content) = parsed["choices"][0]["delta"]["content"].as_str()
                            {
                                tx.send(Ok(content.to_string())).await.ok();
                            }
                        }
                    }
                }
            }

            tx.close_channel();
        });

        Box::pin(rx)
    }

    fn stream_anthropic(
        &self,
        preset: &PresetConfig,
        messages: &[ChatMessage],
        api_key: &str,
    ) -> ChunkStream {
        let client = self.client.clone();
        let model = preset.model.clone().unwrap_or_else(|| "claude-sonnet-4-6".to_string());
        let temperature = preset.temperature.unwrap_or(0.7);
        let max_tokens = preset.max_tokens.unwrap_or(4096);
        let url = preset
            .provider_url
            .clone()
            .unwrap_or_else(|| "https://api.anthropic.com/v1/messages".to_string());

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
            "stream": true,
        });

        if !system_prompt.is_empty() {
            body["system"] = serde_json::json!(system_prompt);
        }

        let (mut tx, rx) = mpsc::channel::<Result<String, String>>(64);
        let api_key = api_key.to_string();

        tokio::spawn(async move {
            let result = client
                .post(&url)
                .header("x-api-key", api_key)
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
                tx.send(Err(format!("HTTP {}: {}", status, text))).await.ok();
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
}
