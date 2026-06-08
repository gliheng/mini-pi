use std::io::{Error, ErrorKind};

/// Generate a title for a chat thread using an LLM.
///
/// Requires the following environment variables:
/// - `CLOUDFLARE_API_KEY` - API token for Cloudflare AI Gateway
/// - `CLOUDFLARE_ACCOUNT_ID` - Cloudflare account ID
/// - `CLOUDFLARE_GATEWAY_ID` - AI Gateway ID
pub fn generate_title(content: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let api_key = std::env::var("CLOUDFLARE_API_KEY").map_err(|_| {
        Box::new(Error::new(ErrorKind::NotFound, "CLOUDFLARE_API_KEY env var not set"))
            as Box<dyn std::error::Error + Send + Sync>
    })?;
    let account_id = std::env::var("CLOUDFLARE_ACCOUNT_ID").map_err(|_| {
        Box::new(Error::new(ErrorKind::NotFound, "CLOUDFLARE_ACCOUNT_ID env var not set"))
            as Box<dyn std::error::Error + Send + Sync>
    })?;
    let gateway_id = std::env::var("CLOUDFLARE_GATEWAY_ID").map_err(|_| {
        Box::new(Error::new(ErrorKind::NotFound, "CLOUDFLARE_GATEWAY_ID env var not set"))
            as Box<dyn std::error::Error + Send + Sync>
    })?;

    let client = reqwest::blocking::Client::new();

    const SYSTEM_PROMPT: &str = include_str!("../../prompts/title_generator.txt");

    let body = serde_json::json!({
        "model": "deepseek/deepseek-v4-flash",
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": content }
        ]
    });

    let url = format!(
        "https://gateway.ai.cloudflare.com/v1/{}/{}/compat/chat/completions",
        account_id, gateway_id
    );
    let response = client
        .post(&url)
        .header("cf-aig-authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()?;

    let response_json: serde_json::Value = response.json()?;

    let title = response_json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or("Invalid response format from title API")?
        .trim()
        .to_string();

    Ok(title)
}
