use crate::app_state::AppState;
use crate::infrastructure::apis::llm_http_client::LlmHttpClient;
use crate::infrastructure::credentials::CredentialService;
use crate::infrastructure::database::ProfileRow;
use crate::infrastructure::provider_registry::{all_providers, ProviderInfo};
use chrono::Utc;
use uuid::Uuid;

#[tauri::command]
pub async fn get_api_key(provider: String) -> Result<Option<String>, String> {
    CredentialService::get_masked(&provider)
}

#[tauri::command]
pub async fn get_raw_api_key(
    state: tauri::State<'_, AppState>,
    provider: String,
) -> Result<Option<String>, String> {
    CredentialService::get(&state.repo, &provider).await
}

#[tauri::command]
pub async fn set_api_key(
    state: tauri::State<'_, AppState>,
    provider: String,
    key: String,
) -> Result<(), String> {
    CredentialService::store(&state.repo, &provider, &key).await
}

#[tauri::command]
pub async fn delete_api_key(
    state: tauri::State<'_, AppState>,
    provider: String,
) -> Result<(), String> {
    CredentialService::delete(&state.repo, &provider).await
}

#[derive(serde::Serialize)]
pub struct SettingEntry {
    pub key: String,
    pub value: Option<String>,
    pub has_key: bool,
}

#[tauri::command]
pub async fn get_all_api_keys() -> Result<Vec<SettingEntry>, String> {
    let providers = all_providers();
    let mut entries = Vec::new();
    for info in &providers {
        let masked = CredentialService::get_masked(&info.id)?;
        let has_key = masked.is_some();
        entries.push(SettingEntry {
            key: format!("api_key_{}", info.id),
            value: masked,
            has_key,
        });
    }
    Ok(entries)
}

#[tauri::command]
pub async fn get_providers() -> Result<Vec<ProviderInfo>, String> {
    Ok(all_providers())
}

#[tauri::command]
pub async fn get_provider_url(
    state: tauri::State<'_, AppState>,
    provider: String,
) -> Result<Option<String>, String> {
    CredentialService::get_url(&state.repo, &provider).await
}

#[tauri::command]
pub async fn set_provider_url(
    state: tauri::State<'_, AppState>,
    provider: String,
    url: String,
) -> Result<(), String> {
    if url.trim().is_empty() {
        CredentialService::delete_url(&state.repo, &provider).await
    } else {
        CredentialService::store_url(&state.repo, &provider, url.trim()).await
    }
}

// ── Model fetching & connection testing ───────────────────

#[tauri::command]
pub async fn fetch_models(
    provider: String,
    api_key: String,
    api_url: String,
) -> Result<Vec<String>, String> {
    let client = LlmHttpClient::new();
    client.fetch_models(&provider, &api_key, &api_url).await
}

#[tauri::command]
pub async fn test_connection(
    provider: String,
    api_key: String,
    api_url: String,
    model: String,
) -> Result<(), String> {
    let client = LlmHttpClient::new();
    client
        .test_connection(&provider, &api_key, &api_url, &model)
        .await
}

// ── Profile CRUD ──────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ProfileDto {
    pub id: String,
    pub name: String,
    pub provider_id: String,
    pub model: String,
    pub api_url: Option<String>,
    pub has_api_key: bool,
    pub created_at: String,
}

impl From<ProfileRow> for ProfileDto {
    fn from(r: ProfileRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            provider_id: r.provider_id,
            model: r.model,
            api_url: r.api_url,
            has_api_key: false,
            created_at: r.created_at,
        }
    }
}

#[tauri::command]
pub async fn save_profile(
    state: tauri::State<'_, AppState>,
    name: String,
    provider_id: String,
    model: String,
    api_url: Option<String>,
) -> Result<ProfileDto, String> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let row = ProfileRow {
        id,
        name,
        provider_id,
        model,
        api_url,
        created_at: now,
    };
    state
        .repo
        .insert_profile(&row)
        .await
        .map_err(|e| format!("Failed to save profile: {}", e))?;
    Ok(row.into())
}

#[tauri::command]
pub async fn list_profiles(state: tauri::State<'_, AppState>) -> Result<Vec<ProfileDto>, String> {
    let rows = state
        .repo
        .list_profiles()
        .await
        .map_err(|e| format!("Failed to list profiles: {}", e))?;
    let mut result = Vec::new();
    for r in rows {
        let has_key = CredentialService::get(&state.repo, &r.provider_id)
            .await
            .ok()
            .flatten()
            .is_some();
        result.push(ProfileDto {
            has_api_key: has_key,
            ..r.into()
        });
    }
    Ok(result)
}

#[tauri::command]
pub async fn delete_profile(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .repo
        .delete_profile(&id)
        .await
        .map_err(|e| format!("Failed to delete profile: {}", e))
}

#[tauri::command]
pub async fn view_profile(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<ProfileDto, String> {
    let row = state
        .repo
        .get_profile(&id)
        .await
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or_else(|| format!("Profile not found: {}", id))?;
    let has_key = CredentialService::get(&state.repo, &row.provider_id)
        .await
        .ok()
        .flatten()
        .is_some();
    Ok(ProfileDto {
        has_api_key: has_key,
        ..row.into()
    })
}

// ── Active profile ────────────────────────────────────────

const ACTIVE_PROFILE_KEY: &str = "active_profile_id";

#[tauri::command]
pub async fn set_active_profile(
    state: tauri::State<'_, AppState>,
    profile_id: String,
) -> Result<(), String> {
    let profile = state
        .repo
        .get_profile(&profile_id)
        .await
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or_else(|| format!("Profile not found: {}", profile_id))?;
    state
        .repo
        .set_setting(ACTIVE_PROFILE_KEY, &profile_id)
        .await
        .map_err(|e| format!("Failed to save setting: {}", e))?;
    *state.active_profile_id.lock().unwrap() = Some(profile_id.clone());
    log::info!(
        "Active profile set: {} ({} / {})",
        profile.name,
        profile.provider_id,
        profile.model
    );
    Ok(())
}

#[derive(serde::Serialize)]
pub struct DiagnoseResult {
    pub active_profile_id: Option<String>,
    pub active_profile: Option<ProfileDto>,
    pub profiles_count: usize,
    pub profiles: Vec<DiagnoseProfileEntry>,
}

#[derive(serde::Serialize)]
pub struct DiagnoseProfileEntry {
    pub id: String,
    pub name: String,
    pub provider_id: String,
    pub model: String,
    pub has_api_key: bool,
}

#[tauri::command]
pub async fn check_keyring(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let test_value = "keyring_test_value_123";
    let test_provider = "__keyring_test__";
    CredentialService::store(&state.repo, test_provider, test_value).await?;
    match CredentialService::get(&state.repo, test_provider).await {
        Ok(Some(v)) if v == test_value => {
            let _ = CredentialService::delete(&state.repo, test_provider).await;
            Ok("OK: keyring works correctly (write + read + delete succeeded)".to_string())
        }
        Ok(Some(v)) => {
            let _ = CredentialService::delete(&state.repo, test_provider).await;
            Err(format!("FAIL: wrote '{}' but read back '{}'", test_value, v))
        }
        Ok(None) => {
            Err("FAIL: write succeeded but read returned Nothing (likely Windows Credential Manager permission issue)".to_string())
        }
        Err(e) => {
            Err(format!("FAIL: read error after write — {}", e))
        }
    }
}

#[tauri::command]
pub async fn diagnose(state: tauri::State<'_, AppState>) -> Result<DiagnoseResult, String> {
    let active_id = state.active_profile_id.lock().unwrap().clone();
    let mut active_profile = None;

    if let Some(ref pid) = active_id {
        if let Ok(Some(row)) = state.repo.get_profile(pid).await {
            let has_key = CredentialService::get(&state.repo, &row.provider_id)
                .await
                .ok()
                .flatten()
                .is_some();
            active_profile = Some(ProfileDto {
                has_api_key: has_key,
                ..row.into()
            });
        }
    }

    let all_profiles = state
        .repo
        .list_profiles()
        .await
        .map_err(|e| format!("DB error: {}", e))?;

    let profiles_count = all_profiles.len();

    let mut profiles: Vec<DiagnoseProfileEntry> = Vec::new();
    for p in all_profiles {
        let has_key = CredentialService::get(&state.repo, &p.provider_id)
            .await
            .ok()
            .flatten()
            .is_some();
        profiles.push(DiagnoseProfileEntry {
            id: p.id,
            name: p.name,
            provider_id: p.provider_id,
            model: p.model,
            has_api_key: has_key,
        });
    }

    Ok(DiagnoseResult {
        active_profile_id: active_id,
        active_profile,
        profiles_count,
        profiles,
    })
}

#[tauri::command]
pub async fn get_active_profile(
    state: tauri::State<'_, AppState>,
) -> Result<Option<ProfileDto>, String> {
    let id = state.active_profile_id.lock().unwrap().clone();
    match id {
        Some(pid) => {
            let row = state
                .repo
                .get_profile(&pid)
                .await
                .map_err(|e| format!("Database error: {}", e))?;
            if let Some(r) = row {
                let has_key = CredentialService::get(&state.repo, &r.provider_id)
                    .await
                    .ok()
                    .flatten()
                    .is_some();
                Ok(Some(ProfileDto {
                    has_api_key: has_key,
                    ..r.into()
                }))
            } else {
                Ok(None)
            }
        }
        None => Ok(None),
    }
}
