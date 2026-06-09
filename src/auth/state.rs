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

pub fn auth_file_path() -> PathBuf {
    let dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mini-pi");
    dir.join("auth.json")
}

pub fn save_session(session: &SupabaseSession) -> Result<(), std::io::Error> {
    let path = auth_file_path();
    let content = serde_json::to_string_pretty(session)?;
    std::fs::write(&path, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&path, perms)?;
    }
    Ok(())
}

pub fn load_session() -> Option<SupabaseSession> {
    let path = auth_file_path();
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn clear_session() -> Result<(), std::io::Error> {
    let path = auth_file_path();
    if path.exists() {
        std::fs::remove_file(path)?;
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