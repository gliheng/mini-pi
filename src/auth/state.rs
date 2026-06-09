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