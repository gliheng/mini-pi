use crate::auth::state::{SupabaseSession, SupabaseUser};
use reqwest::blocking::Client;
use serde::Deserialize;

const SUPABASE_URL: &str = "https://xgazvyjwnjwablelrrsc.supabase.co";

const SUPABASE_ANON_KEY: &str = "sb_publishable_wbwXXEx1TFLEz7zKTFHkOQ_HQaHIwAF";

fn anon_key() -> &'static str {
    SUPABASE_ANON_KEY
}

fn client() -> Client {
    Client::new()
}

#[derive(Debug)]
pub enum SupabaseAuthError {
    Http(reqwest::Error),
    Api { msg: String, status: u16 },
    Json(serde_json::Error),
    Io(std::io::Error),
}

impl std::fmt::Display for SupabaseAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SupabaseAuthError::Http(e) => write!(f, "HTTP error: {}", e),
            SupabaseAuthError::Api { msg, status } => {
                write!(f, "API error ({}): {}", status, msg)
            }
            SupabaseAuthError::Json(e) => write!(f, "JSON error: {}", e),
            SupabaseAuthError::Io(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl From<reqwest::Error> for SupabaseAuthError {
    fn from(e: reqwest::Error) -> Self {
        SupabaseAuthError::Http(e)
    }
}

impl From<serde_json::Error> for SupabaseAuthError {
    fn from(e: serde_json::Error) -> Self {
        SupabaseAuthError::Json(e)
    }
}

impl From<std::io::Error> for SupabaseAuthError {
    fn from(e: std::io::Error) -> Self {
        SupabaseAuthError::Io(e)
    }
}

#[derive(Deserialize)]
struct AuthResponse {
    access_token: String,
    refresh_token: String,
    expires_in: Option<i64>,
    user: AuthUserResponse,
}

#[derive(Deserialize)]
struct AuthUserResponse {
    id: String,
    email: String,
    created_at: String,
}

pub fn signup(email: &str, password: &str) -> Result<SupabaseSession, SupabaseAuthError> {
    let resp = client()
        .post(format!("{}/auth/v1/signup", SUPABASE_URL))
        .header("apikey", anon_key())
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "email": email,
            "password": password,
        }))
        .send()?;

    let status = resp.status().as_u16();
    let body = resp.text().map_err(SupabaseAuthError::Http)?;

    if status >= 400 {
        let msg = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                v.get("msg")
                    .or(v.get("error_description"))
                    .or(v.get("error"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| v.as_str().map(|s| s.to_string()))
            })
            .unwrap_or_else(|| format!("Signup failed (status {})", status));
        return Err(SupabaseAuthError::Api { msg, status });
    }

    let auth_resp: AuthResponse = serde_json::from_str(&body)?;
    let expires_in = auth_resp.expires_in.unwrap_or(3600);
    let expires_at = chrono::Utc::now().timestamp() + expires_in;

    Ok(SupabaseSession {
        access_token: auth_resp.access_token,
        refresh_token: auth_resp.refresh_token,
        expires_at,
        user: SupabaseUser {
            id: auth_resp.user.id,
            email: auth_resp.user.email,
            created_at: auth_resp.user.created_at,
        },
    })
}

pub fn login(email: &str, password: &str) -> Result<SupabaseSession, SupabaseAuthError> {
    let resp = client()
        .post(format!(
            "{}/auth/v1/token?grant_type=password",
            SUPABASE_URL
        ))
        .header("apikey", anon_key())
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "email": email,
            "password": password,
        }))
        .send()?;

    let status = resp.status().as_u16();
    let body = resp.text().map_err(SupabaseAuthError::Http)?;

    if status >= 400 {
        let msg = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                v.get("error_description")
                    .or(v.get("msg"))
                    .or(v.get("error"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| v.as_str().map(|s| s.to_string()))
            })
            .unwrap_or_else(|| format!("Login failed (status {})", status));
        return Err(SupabaseAuthError::Api { msg, status });
    }

    let auth_resp: AuthResponse = serde_json::from_str(&body)?;
    let expires_in = auth_resp.expires_in.unwrap_or(3600);
    let expires_at = chrono::Utc::now().timestamp() + expires_in;

    Ok(SupabaseSession {
        access_token: auth_resp.access_token,
        refresh_token: auth_resp.refresh_token,
        expires_at,
        user: SupabaseUser {
            id: auth_resp.user.id,
            email: auth_resp.user.email,
            created_at: auth_resp.user.created_at,
        },
    })
}

pub fn refresh_session(
    refresh_token: &str,
) -> Result<SupabaseSession, SupabaseAuthError> {
    let resp = client()
        .post(format!(
            "{}/auth/v1/token?grant_type=refresh_token",
            SUPABASE_URL
        ))
        .header("apikey", anon_key())
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "refresh_token": refresh_token,
        }))
        .send()?;

    let status = resp.status().as_u16();
    let body = resp.text().map_err(SupabaseAuthError::Http)?;

    if status >= 400 {
        let msg = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                v.get("error_description")
                    .or(v.get("msg"))
                    .or(v.get("error"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| v.as_str().map(|s| s.to_string()))
            })
            .unwrap_or_else(|| format!("Token refresh failed (status {})", status));
        return Err(SupabaseAuthError::Api { msg, status });
    }

    let auth_resp: AuthResponse = serde_json::from_str(&body)?;
    let expires_in = auth_resp.expires_in.unwrap_or(3600);
    let expires_at = chrono::Utc::now().timestamp() + expires_in;

    Ok(SupabaseSession {
        access_token: auth_resp.access_token,
        refresh_token: auth_resp.refresh_token,
        expires_at,
        user: SupabaseUser {
            id: auth_resp.user.id,
            email: auth_resp.user.email,
            created_at: auth_resp.user.created_at,
        },
    })
}

pub fn get_user(access_token: &str) -> Result<SupabaseUser, SupabaseAuthError> {
    let resp = client()
        .get(format!("{}/auth/v1/user", SUPABASE_URL))
        .header("apikey", anon_key())
        .header("Authorization", format!("Bearer {}", access_token))
        .send()?;

    let status = resp.status().as_u16();
    let body = resp.text().map_err(SupabaseAuthError::Http)?;

    if status >= 400 {
        let msg = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()).or(v.get("msg").and_then(|v| v.as_str().map(|s| s.to_string()))))
            .unwrap_or_else(|| format!("Get user failed (status {})", status));
        return Err(SupabaseAuthError::Api { msg, status });
    }

    let user_resp: AuthUserResponse = serde_json::from_str(&body)?;

    Ok(SupabaseUser {
        id: user_resp.id,
        email: user_resp.email,
        created_at: user_resp.created_at,
    })
}

pub fn logout(access_token: &str) -> Result<(), SupabaseAuthError> {
    let _ = client()
        .post(format!("{}/auth/v1/logout", SUPABASE_URL))
        .header("apikey", anon_key())
        .header("Authorization", format!("Bearer {}", access_token))
        .send()?;
    Ok(())
}

pub fn upload_file(
    access_token: &str,
    user_id: &str,
    file_path: &str,
    content_type: &str,
    data: &[u8],
) -> Result<(), SupabaseAuthError> {
    let bucket_path = format!("{}/{}", user_id, file_path);
    let encoded = percent_encoding::percent_encode(
        bucket_path.as_bytes(),
        percent_encoding::NON_ALPHANUMERIC,
    );

    let body = data.to_vec();

    client()
        .post(format!(
            "{}/storage/v1/object/pi-sync/{}",
            SUPABASE_URL, encoded
        ))
        .header("apikey", anon_key())
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", content_type)
        .body(body)
        .send()?;

    Ok(())
}

pub fn list_files(
    access_token: &str,
    user_id: &str,
) -> Result<Vec<StorageFile>, SupabaseAuthError> {
    let resp = client()
        .post(format!(
            "{}/storage/v1/object/list/pi-sync",
            SUPABASE_URL
        ))
        .header("apikey", anon_key())
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "prefix": format!("{}/", user_id),
            "limit": 1000,
        }))
        .send()?;

    let body = resp.text().map_err(SupabaseAuthError::Http)?;
    let files: Vec<StorageFile> = serde_json::from_str(&body)?;
    Ok(files)
}

pub fn download_file(
    access_token: &str,
    user_id: &str,
    file_path: &str,
) -> Result<Vec<u8>, SupabaseAuthError> {
    let bucket_path = format!("{}/{}", user_id, file_path);
    let encoded = percent_encoding::percent_encode(
        bucket_path.as_bytes(),
        percent_encoding::NON_ALPHANUMERIC,
    );

    let resp = client()
        .get(format!(
            "{}/storage/v1/object/pi-sync/{}",
            SUPABASE_URL, encoded
        ))
        .header("apikey", anon_key())
        .header("Authorization", format!("Bearer {}", access_token))
        .send()?;

    let data = resp.bytes().map_err(SupabaseAuthError::Http)?;
    Ok(data.to_vec())
}

pub fn delete_file(
    access_token: &str,
    user_id: &str,
    file_path: &str,
) -> Result<(), SupabaseAuthError> {
    let bucket_path = format!("{}/{}", user_id, file_path);
    let encoded = percent_encoding::percent_encode(
        bucket_path.as_bytes(),
        percent_encoding::NON_ALPHANUMERIC,
    );

    let _ = client()
        .delete(format!(
            "{}/storage/v1/object/pi-sync/{}",
            SUPABASE_URL, encoded
        ))
        .header("apikey", anon_key())
        .header("Authorization", format!("Bearer {}", access_token))
        .send()?;

    Ok(())
}

#[derive(Clone, Debug, Deserialize)]
pub struct StorageFile {
    pub name: String,
    pub id: String,
    pub updated_at: String,
    pub created_at: String,
    pub last_accessed_at: Option<String>,
    pub metadata: Option<serde_json::Value>,
}