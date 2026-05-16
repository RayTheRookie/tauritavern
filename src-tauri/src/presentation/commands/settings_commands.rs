use crate::AppState;

#[tauri::command]
pub async fn get_api_key(
    state: tauri::State<'_, AppState>,
    provider: String,
) -> Result<String, String> {
    state
        .repo
        .get_setting(&format!("api_key_{}", provider))
        .await
        .map_err(|e| e.to_string())
        .map(|v| v.unwrap_or_default())
}

#[tauri::command]
pub async fn set_api_key(
    state: tauri::State<'_, AppState>,
    provider: String,
    key: String,
) -> Result<(), String> {
    state
        .repo
        .set_setting(&format!("api_key_{}", provider), &key)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_api_key(
    state: tauri::State<'_, AppState>,
    provider: String,
) -> Result<(), String> {
    state
        .repo
        .delete_setting(&format!("api_key_{}", provider))
        .await
        .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
pub struct SettingEntry {
    pub key: String,
    pub value: String,
}

#[tauri::command]
pub async fn get_all_api_keys(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<SettingEntry>, String> {
    let rows = state
        .repo
        .get_all_api_keys()
        .await
        .map_err(|e| e.to_string())?;

    Ok(rows
        .into_iter()
        .map(|(key, value)| SettingEntry { key, value })
        .collect())
}
