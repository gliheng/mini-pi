use crate::data::store::Store;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum AuthState {
    LoggedOut,
    LoggingIn,
    LoggedIn(SupabaseUser),
    #[allow(dead_code)]
    Error(String),
}

impl AuthState {
    pub fn user(&self) -> Option<&SupabaseUser> {
        match self {
            AuthState::LoggedIn(user) => Some(user),
            _ => None,
        }
    }

    pub fn is_logged_in(&self) -> bool {
        matches!(self, AuthState::LoggedIn(_))
    }

    pub fn initials(&self) -> String {
        match self {
            AuthState::LoggedIn(user) => {
                let email = &user.email;
                if let Some(at) = email.find('@') {
                    email[..at].to_uppercase()
                } else {
                    email.to_uppercase()
                }
                .chars()
                .take(2)
                .collect()
            }
            _ => "?".to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupabaseUser {
    pub id: String,
    pub email: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupabaseSession {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub user: SupabaseUser,
}

impl SupabaseSession {
    pub fn is_expired(&self) -> bool {
        let now = chrono::Utc::now().timestamp();
        now >= self.expires_at
    }
}

fn auth_file_path() -> PathBuf {
    let dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mini-pi");
    dir.join("auth.json")
}

pub fn save_session(store: &Store, session: &SupabaseSession) -> Result<(), crate::data::store::StoreError> {
    let content = serde_json::to_string_pretty(session)
        .map_err(|e| crate::data::store::StoreError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;
    store.set_user_setting("supabase_session", &content)
}

pub fn load_session(store: &Store) -> Option<SupabaseSession> {
    // Try database first.
    if let Ok(Some(db_value)) = store.get_user_setting("supabase_session") {
        if let Ok(session) = serde_json::from_str::<SupabaseSession>(&db_value) {
            // Clean up legacy file if it still exists.
            let legacy = auth_file_path();
            if legacy.exists() {
                let _ = std::fs::remove_file(&legacy);
            }
            return Some(session);
        }
    }

    // One-time migration: fall back to legacy auth.json.
    let legacy = auth_file_path();
    if !legacy.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&legacy).ok()?;
    let session: SupabaseSession = serde_json::from_str(&content).ok()?;
    let _ = save_session(store, &session);
    let _ = std::fs::remove_file(&legacy);
    Some(session)
}

pub fn clear_session(store: &Store) -> Result<(), crate::data::store::StoreError> {
    store.delete_user_setting("supabase_session")?;
    let legacy = auth_file_path();
    if legacy.exists() {
        let _ = std::fs::remove_file(&legacy);
    }
    Ok(())
}

pub fn agent_dir() -> PathBuf {
    let dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mini-pi")
        .join("agent");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn is_first_run() -> bool {
    let agent = agent_dir();
    if !agent.exists() {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(&agent) else {
        return true;
    };
    entries.count() == 0
}

pub fn pi_agent_source_dir() -> Option<PathBuf> {
    let dir = dirs::home_dir()?.join(".pi").join("agent");
    if dir.exists() { Some(dir) } else { None }
}

pub fn list_pi_agent_json_files() -> Vec<(String, PathBuf)> {
    let Some(source) = pi_agent_source_dir() else {
        return Vec::new();
    };
    let mut files = Vec::new();
    collect_json_files(&source, &source, &mut files);
    files
}

fn collect_json_files(base: &PathBuf, current: &PathBuf, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(current) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_json_files(base, &path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("json") {
            let rel = path.strip_prefix(base).unwrap_or(&path).to_string_lossy().to_string();
            out.push((rel, path));
        }
    }
}

pub fn import_from_pi_agent() -> Result<usize, std::io::Error> {
    let files = list_pi_agent_json_files();
    if files.is_empty() {
        return Ok(0);
    }
    let target = agent_dir();
    let mut imported = 0;
    for (rel, src) in files {
        let dst = target.join(&rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&src, &dst)?;
        imported += 1;
    }
    Ok(imported)
}