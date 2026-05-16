use keyring::Entry;

const SERVICE_NAME: &str = "TauriTavern";

pub struct CredentialService;

impl CredentialService {
    pub fn store(provider: &str, key: &str) -> Result<(), String> {
        let entry =
            Entry::new(SERVICE_NAME, &format!("api_key_{}", provider)).map_err(|e| e.to_string())?;
        entry.set_password(key).map_err(|e| e.to_string())
    }

    pub fn get(provider: &str) -> Result<Option<String>, String> {
        let entry =
            Entry::new(SERVICE_NAME, &format!("api_key_{}", provider)).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(key) => Ok(Some(key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn delete(provider: &str) -> Result<(), String> {
        let entry =
            Entry::new(SERVICE_NAME, &format!("api_key_{}", provider)).map_err(|e| e.to_string())?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn get_masked(provider: &str) -> Result<Option<String>, String> {
        Self::get(provider).map(|opt| opt.map(mask_key))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_key_short() {
        assert_eq!(mask_key("short".into()), "****");
    }

    #[test]
    fn test_mask_key_normal() {
        let masked = mask_key("sk-ant-api03-verylongkey".into());
        assert!(masked.starts_with("sk-a"));
        assert!(masked.contains("..."));
        assert!(masked.ends_with("key"));
    }
}
