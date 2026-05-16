use crate::infrastructure::credentials::CredentialService;

#[tauri::command]
pub async fn get_api_key(provider: String) -> Result<Option<String>, String> {
    CredentialService::get_masked(&provider)
}

#[tauri::command]
pub async fn set_api_key(provider: String, key: String) -> Result<(), String> {
    CredentialService::store(&provider, &key)
}

#[tauri::command]
pub async fn delete_api_key(provider: String) -> Result<(), String> {
    CredentialService::delete(&provider)
}

#[derive(serde::Serialize)]
pub struct SettingEntry {
    pub key: String,
    pub value: Option<String>,
    pub has_key: bool,
}

#[tauri::command]
pub async fn get_all_api_keys() -> Result<Vec<SettingEntry>, String> {
    let providers = vec!["openai", "anthropic"];
    let mut entries = Vec::new();
    for provider in providers {
        let masked = CredentialService::get_masked(provider)?;
        let has_key = masked.is_some();
        entries.push(SettingEntry {
            key: format!("api_key_{}", provider),
            value: masked,
            has_key,
        });
    }
    Ok(entries)
}
