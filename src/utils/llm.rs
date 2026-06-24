/// Generate a title for a chat thread using an LLM.
///
/// Cloudflare AI Gateway credentials are read from environment variables.
pub fn generate_title(content: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let cloudflare_api_key = std::env::var("CLOUDFLARE_API_KEY")
        .map_err(|_| "CLOUDFLARE_API_KEY environment variable is not set")?;
    let cloudflare_account_id = std::env::var("CLOUDFLARE_ACCOUNT_ID")
        .map_err(|_| "CLOUDFLARE_ACCOUNT_ID environment variable is not set")?;
    let cloudflare_gateway_id = std::env::var("CLOUDFLARE_GATEWAY_ID")
        .map_err(|_| "CLOUDFLARE_GATEWAY_ID environment variable is not set")?;

    let client = reqwest::blocking::Client::new();

    const SYSTEM_PROMPT: &str = include_str!("../../assets/prompts/title_generator.txt");

    let body = serde_json::json!({
        "model": "deepseek/deepseek-v4-flash",
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": content }
        ]
    });

    let url = format!(
        "https://gateway.ai.cloudflare.com/v1/{}/{}/compat/chat/completions",
        cloudflare_account_id, cloudflare_gateway_id
    );
    let response = client
        .post(&url)
        .header(
            "cf-aig-authorization",
            format!("Bearer {}", cloudflare_api_key),
        )
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
