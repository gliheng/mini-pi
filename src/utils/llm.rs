/// Generate a title for a chat thread using an LLM.
///
/// Cloudflare AI Gateway credentials are embedded below.
pub fn generate_title(content: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    const CLOUDFLARE_API_KEY: &str = "<REDACTED>";
    const CLOUDFLARE_ACCOUNT_ID: &str = "c963aaaebd80b17d39cc4789854876f8";
    const CLOUDFLARE_GATEWAY_ID: &str = "pub";

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
        CLOUDFLARE_ACCOUNT_ID, CLOUDFLARE_GATEWAY_ID
    );
    let response = client
        .post(&url)
        .header(
            "cf-aig-authorization",
            format!("Bearer {}", CLOUDFLARE_API_KEY),
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
