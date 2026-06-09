use crate::auth::state;
use crate::auth::supabase;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct SyncMeta {
    pub files: HashMap<String, FileSyncInfo>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileSyncInfo {
    pub hash: String,
    pub last_synced: String,
}

fn sync_meta_path() -> PathBuf {
    let dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mini-pi");
    dir.join("sync_meta.json")
}

pub fn load_sync_meta() -> SyncMeta {
    let path = sync_meta_path();
    if !path.exists() {
        return SyncMeta::default();
    }
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn save_sync_meta(meta: &SyncMeta) -> Result<(), std::io::Error> {
    let path = sync_meta_path();
    let content = serde_json::to_string_pretty(meta)?;
    std::fs::write(&path, content)
}

fn file_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    format!("{:x}", result)
}

fn scan_agent_files() -> Vec<(String, PathBuf)> {
    let agent_dir = state::agent_dir();
    let mut files = Vec::new();
    scan_dir_recursive(&agent_dir, &agent_dir, &mut files);
    files
}

fn scan_dir_recursive(base: &PathBuf, current: &PathBuf, files: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir_recursive(base, &path, files);
        } else {
            let relative = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            files.push((relative, path));
        }
    }
}

fn content_type_for(path: &str) -> &str {
    if path.ends_with(".json") {
        "application/json"
    } else if path.ends_with(".yaml") || path.ends_with(".yml") {
        "application/yaml"
    } else if path.ends_with(".toml") {
        "application/toml"
    } else if path.ends_with(".md") {
        "text/markdown"
    } else if path.ends_with(".txt") {
        "text/plain"
    } else {
        "application/octet-stream"
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SyncStatus {
    Idle,
    Syncing,
    Synced,
    Error(String),
}

pub fn pull_from_remote(
    access_token: &str,
    user_id: &str,
) -> Result<SyncMeta, String> {
    let agent_dir = state::agent_dir();
    let remote_files = supabase::list_files(access_token, user_id).map_err(|e| e.to_string())?;

    let mut meta = SyncMeta::default();

    for file in &remote_files {
        if file.name.ends_with('/') {
            continue;
        }

        let relative_path = file
            .name
            .strip_prefix(&format!("{}/", user_id))
            .unwrap_or(&file.name)
            .to_string();

        let data = supabase::download_file(access_token, user_id, &relative_path)
            .map_err(|e| e.to_string())?;

        let local_path = agent_dir.join(&relative_path);
        if let Some(parent) = local_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&local_path, &data).map_err(|e| e.to_string())?;

        let hash = file_hash(&data);
        let now = chrono::Utc::now().to_rfc3339();
        meta.files.insert(
            relative_path,
            FileSyncInfo {
                hash,
                last_synced: now,
            },
        );
    }

    let _ = save_sync_meta(&meta);
    Ok(meta)
}

pub fn push_to_remote(
    access_token: &str,
    user_id: &str,
) -> Result<SyncMeta, String> {
    let local_files = scan_agent_files();
    let mut meta = SyncMeta::default();

    for (relative_path, full_path) in &local_files {
        let data = std::fs::read(full_path).map_err(|e| e.to_string())?;
        let content_type = content_type_for(relative_path);

        supabase::upload_file(access_token, user_id, relative_path, content_type, &data)
            .map_err(|e| e.to_string())?;

        let hash = file_hash(&data);
        let now = chrono::Utc::now().to_rfc3339();
        meta.files.insert(
            relative_path.clone(),
            FileSyncInfo {
                hash,
                last_synced: now,
            },
        );
    }

    let _ = save_sync_meta(&meta);
    Ok(meta)
}

pub fn sync_changes(
    access_token: &str,
    user_id: &str,
) -> Result<SyncMeta, String> {
    let mut meta = load_sync_meta();
    let local_files = scan_agent_files();

    let local_map: HashMap<String, PathBuf> = local_files.into_iter().collect();

    let mut changed = false;

    for (relative_path, full_path) in &local_map {
        let data = match std::fs::read(full_path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let hash = file_hash(&data);

        let needs_upload = match meta.files.get(relative_path) {
            Some(info) => info.hash != hash,
            None => true,
        };

        if needs_upload {
            let content_type = content_type_for(relative_path);
            supabase::upload_file(access_token, user_id, relative_path, content_type, &data)
                .map_err(|e| e.to_string())?;
            let now = chrono::Utc::now().to_rfc3339();
            meta.files.insert(
                relative_path.clone(),
                FileSyncInfo {
                    hash,
                    last_synced: now,
                },
            );
            changed = true;
        }
    }

    let remote_files = supabase::list_files(access_token, user_id).map_err(|e| e.to_string())?;
    let remote_set: std::collections::HashSet<String> = remote_files
        .iter()
        .map(|f| {
            f.name
                .strip_prefix(&format!("{}/", user_id))
                .unwrap_or(&f.name)
                .to_string()
        })
        .collect();

    let local_set: std::collections::HashSet<String> =
        local_map.keys().cloned().collect();

    for remote_file in &remote_set {
        if !local_set.contains(remote_file) {
            let data = supabase::download_file(access_token, user_id, remote_file)
                .map_err(|e| e.to_string())?;
            let local_path = state::agent_dir().join(remote_file);
            if let Some(parent) = local_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&local_path, &data).map_err(|e| e.to_string())?;
            let hash = file_hash(&data);
            let now = chrono::Utc::now().to_rfc3339();
            meta.files.insert(
                remote_file.clone(),
                FileSyncInfo {
                    hash,
                    last_synced: now,
                },
            );
            changed = true;
        }
    }

    for local_only in &local_set {
        if !remote_set.contains(local_only) {
            if let Some(full_path) = local_map.get(local_only) {
                let data = std::fs::read(full_path).map_err(|e| e.to_string())?;
                let content_type = content_type_for(local_only);
                supabase::upload_file(access_token, user_id, local_only, content_type, &data)
                    .map_err(|e| e.to_string())?;
                let hash = file_hash(&data);
                let now = chrono::Utc::now().to_rfc3339();
                meta.files.insert(
                    local_only.clone(),
                    FileSyncInfo {
                        hash,
                        last_synced: now,
                    },
                );
                changed = true;
            }
        }
    }

    if changed {
        let _ = save_sync_meta(&meta);
    }

    Ok(meta)
}