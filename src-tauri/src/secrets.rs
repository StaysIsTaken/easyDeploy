//! Secrets (SSH passwords, GitHub token) live in the OS keychain
//! (macOS Keychain, Windows Credential Manager, Linux Secret Service).
//! The frontend can store and check secrets, but never read them back.

const SERVICE: &str = "easyDeploy";

fn entry(key: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, key).map_err(|e| e.to_string())
}

pub fn get(key: &str) -> Option<String> {
    entry(key).ok()?.get_password().ok()
}

pub fn set(key: &str, value: &str) -> Result<(), String> {
    entry(key)?.set_password(value).map_err(|e| e.to_string())
}

pub fn delete(key: &str) -> Result<(), String> {
    match entry(key)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub fn secret_set(key: String, value: String) -> Result<(), String> {
    set(&key, &value)
}

#[tauri::command]
pub fn secret_has(key: String) -> bool {
    get(&key).is_some()
}

#[tauri::command]
pub fn secret_delete(key: String) -> Result<(), String> {
    delete(&key)
}
