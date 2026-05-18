use crate::infrastructure::database::SqliteRepo;

const SERVICE_NAME: &str = "TauriTavern";

// ── CredentialService ────────────────────────────────────

pub struct CredentialService;

impl CredentialService {
    // ── Keyring (sync) ──────────────────────

    pub fn store_keyring(provider: &str, key: &str) -> Result<(), String> {
        let entry =
            keyring::Entry::new(SERVICE_NAME, &format!("api_key_{}", provider))
                .map_err(|e| e.to_string())?;
        entry.set_password(key).map_err(|e| e.to_string())
    }

    pub fn get_keyring(provider: &str) -> Result<Option<String>, String> {
        let entry =
            keyring::Entry::new(SERVICE_NAME, &format!("api_key_{}", provider))
                .map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(key) => Ok(Some(key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn delete_keyring(provider: &str) -> Result<(), String> {
        let entry =
            keyring::Entry::new(SERVICE_NAME, &format!("api_key_{}", provider))
                .map_err(|e| e.to_string())?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn store_url_keyring(provider: &str, url: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(SERVICE_NAME, &format!("provider_url_{}", provider))
            .map_err(|e| e.to_string())?;
        entry.set_password(url).map_err(|e| e.to_string())
    }

    pub fn get_url_keyring(provider: &str) -> Result<Option<String>, String> {
        let entry = keyring::Entry::new(SERVICE_NAME, &format!("provider_url_{}", provider))
            .map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(url) => Ok(Some(url)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn delete_url_keyring(provider: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(SERVICE_NAME, &format!("provider_url_{}", provider))
            .map_err(|e| e.to_string())?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    // ── Fallback: settings table (async) ────

    fn fallback_key(provider: &str, kind: &str) -> String {
        format!("kv_{}_{}", kind, provider)
    }

    pub async fn fallback_store(repo: &SqliteRepo, provider: &str, value: &str, kind: &str) -> Result<(), String> {
        repo.set_setting(&Self::fallback_key(provider, kind), value)
            .await
            .map_err(|e| format!("Fallback store failed: {}", e))
    }

    pub async fn fallback_get(repo: &SqliteRepo, provider: &str, kind: &str) -> Result<Option<String>, String> {
        repo.get_setting(&Self::fallback_key(provider, kind))
            .await
            .map_err(|e| format!("Fallback read failed: {}", e))
    }

    // ── Public API: keyring → fallback ──────

    /// Store an API key.
    pub async fn store(repo: &SqliteRepo, provider: &str, key: &str) -> Result<(), String> {
        if Self::store_keyring(provider, key).is_ok() {
            if let Ok(Some(rb)) = Self::get_keyring(provider) {
                if rb == key {
                    return Ok(());
                }
            }
        }
        Self::fallback_store(repo, provider, key, "api_key").await
    }

    /// Read an API key.
    pub async fn get(repo: &SqliteRepo, provider: &str) -> Result<Option<String>, String> {
        if let Ok(Some(key)) = Self::get_keyring(provider) {
            return Ok(Some(key));
        }
        Self::fallback_get(repo, provider, "api_key").await
    }

    /// Delete an API key.
    pub async fn delete(repo: &SqliteRepo, provider: &str) -> Result<(), String> {
        let _ = Self::delete_keyring(provider);
        let _ = Self::fallback_store(repo, provider, "", "api_key").await;
        Ok(())
    }

    /// Store a provider URL.
    pub async fn store_url(repo: &SqliteRepo, provider: &str, url: &str) -> Result<(), String> {
        if Self::store_url_keyring(provider, url).is_ok() {
            return Ok(());
        }
        Self::fallback_store(repo, provider, url, "provider_url").await
    }

    /// Read a provider URL.
    pub async fn get_url(repo: &SqliteRepo, provider: &str) -> Result<Option<String>, String> {
        if let Ok(Some(url)) = Self::get_url_keyring(provider) {
            return Ok(Some(url));
        }
        Self::fallback_get(repo, provider, "provider_url").await
    }

    /// Delete a provider URL.
    pub async fn delete_url(repo: &SqliteRepo, provider: &str) -> Result<(), String> {
        let _ = Self::delete_url_keyring(provider);
        let _ = Self::fallback_store(repo, provider, "", "provider_url").await;
        Ok(())
    }

    // ── Masked key (display only) ───────────

    pub fn get_masked(provider: &str) -> Result<Option<String>, String> {
        Self::get_keyring(provider).map(|opt| opt.map(mask_key))
    }
}

fn mask_key(key: String) -> String {
    if key.len() <= 8 {
        return "****".to_string();
    }
    let prefix = &key[..4];
    let suffix = &key[key.len() - 4..];
    format!("{}...{}", prefix, suffix)
}
