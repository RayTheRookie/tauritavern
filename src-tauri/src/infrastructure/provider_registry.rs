use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ApiFormat {
    OpenAiCompatible,
    Anthropic,
    Gemini,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuthType {
    Bearer,
    ApiKey,
    GoogleApiKey,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderInfo {
    pub id: String,
    pub display_name: String,
    pub default_url: Option<String>,
    pub default_model: String,
    pub api_format: ApiFormat,
    pub auth_type: AuthType,
    pub key_placeholder: String,
    pub requires_url: bool,
}

pub fn all_providers() -> Vec<ProviderInfo> {
    vec![
        ProviderInfo {
            id: "anthropic".into(),
            display_name: "Anthropic".into(),
            default_url: Some("https://api.anthropic.com/v1/messages".into()),
            default_model: "claude-sonnet-4-6".into(),
            api_format: ApiFormat::Anthropic,
            auth_type: AuthType::ApiKey,
            key_placeholder: "sk-ant-...".into(),
            requires_url: false,
        },
        ProviderInfo {
            id: "openai".into(),
            display_name: "OpenAI".into(),
            default_url: Some("https://api.openai.com/v1/chat/completions".into()),
            default_model: "gpt-4o".into(),
            api_format: ApiFormat::OpenAiCompatible,
            auth_type: AuthType::Bearer,
            key_placeholder: "sk-...".into(),
            requires_url: false,
        },
        ProviderInfo {
            id: "xai".into(),
            display_name: "xAI (Grok)".into(),
            default_url: Some("https://api.x.ai/v1/chat/completions".into()),
            default_model: "grok-2".into(),
            api_format: ApiFormat::OpenAiCompatible,
            auth_type: AuthType::Bearer,
            key_placeholder: "xai-...".into(),
            requires_url: false,
        },
        ProviderInfo {
            id: "google_gemini".into(),
            display_name: "Google Gemini".into(),
            default_url: Some("https://generativelanguage.googleapis.com/v1beta/models".into()),
            default_model: "gemini-2.0-flash".into(),
            api_format: ApiFormat::Gemini,
            auth_type: AuthType::GoogleApiKey,
            key_placeholder: "AIza...".into(),
            requires_url: false,
        },
        ProviderInfo {
            id: "z_ai".into(),
            display_name: "Z.AI (智谱 GLM)".into(),
            default_url: Some("https://open.bigmodel.cn/api/paas/v4/chat/completions".into()),
            default_model: "glm-4".into(),
            api_format: ApiFormat::OpenAiCompatible,
            auth_type: AuthType::Bearer,
            key_placeholder: "...".into(),
            requires_url: false,
        },
        ProviderInfo {
            id: "deepseek".into(),
            display_name: "DeepSeek".into(),
            default_url: Some("https://api.deepseek.com/v1/chat/completions".into()),
            default_model: "deepseek-chat".into(),
            api_format: ApiFormat::OpenAiCompatible,
            auth_type: AuthType::Bearer,
            key_placeholder: "sk-...".into(),
            requires_url: false,
        },
        ProviderInfo {
            id: "qwen".into(),
            display_name: "Qwen (通义千问)".into(),
            default_url: Some(
                "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions".into(),
            ),
            default_model: "qwen-turbo".into(),
            api_format: ApiFormat::OpenAiCompatible,
            auth_type: AuthType::Bearer,
            key_placeholder: "sk-...".into(),
            requires_url: false,
        },
        ProviderInfo {
            id: "mimo".into(),
            display_name: "MiMo".into(),
            default_url: Some("https://api.mimo.run/v1/chat/completions".into()),
            default_model: "mimo-v1".into(),
            api_format: ApiFormat::OpenAiCompatible,
            auth_type: AuthType::Bearer,
            key_placeholder: "sk-...".into(),
            requires_url: false,
        },
        ProviderInfo {
            id: "minimax".into(),
            display_name: "MiniMax".into(),
            default_url: Some("https://api.minimax.chat/v1/text/chatcompletion_v2".into()),
            default_model: "abab6.5s-chat".into(),
            api_format: ApiFormat::OpenAiCompatible,
            auth_type: AuthType::Bearer,
            key_placeholder: "eyJ...".into(),
            requires_url: false,
        },
        ProviderInfo {
            id: "openrouter".into(),
            display_name: "OpenRouter".into(),
            default_url: Some("https://openrouter.ai/api/v1/chat/completions".into()),
            default_model: "openai/gpt-4o".into(),
            api_format: ApiFormat::OpenAiCompatible,
            auth_type: AuthType::Bearer,
            key_placeholder: "sk-or-...".into(),
            requires_url: false,
        },
        ProviderInfo {
            id: "openai_compatible".into(),
            display_name: "OpenAI Compatible (Custom)".into(),
            default_url: None,
            default_model: "gpt-4o".into(),
            api_format: ApiFormat::OpenAiCompatible,
            auth_type: AuthType::Bearer,
            key_placeholder: "sk-...".into(),
            requires_url: true,
        },
    ]
}

pub fn get_provider(id: &str) -> Option<ProviderInfo> {
    all_providers().into_iter().find(|p| p.id == id)
}

pub fn resolve_url(
    provider_id: &str,
    preset_url: &Option<String>,
    global_url: &Option<String>,
) -> String {
    let info = get_provider(provider_id);
    preset_url
        .clone()
        .or_else(|| global_url.clone())
        .or_else(|| info.and_then(|i| i.default_url))
        .unwrap_or_default()
}
