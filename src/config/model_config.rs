use gpui::SharedString;

#[derive(Clone, Debug)]
pub struct ModelInfo {
    pub name: &'static str,
    pub id: &'static str,
}

const MODELS: &[ModelInfo] = &[
    ModelInfo {
        name: "OpenAI GPT-4o Mini",
        id: "cloudflare-ai-gateway:gpt-4o-mini",
    },
    ModelInfo {
        name: "OpenAI GPT-5.5",
        id: "cloudflare-ai-gateway:gpt-5.5",
    },
    // ModelInfo {
    //     name: "Google Gemini 3.1 Pro",
    //     id: "cloudflare-ai-gateway:google/gemini-3.1-pro",
    // },
    // ModelInfo {
    //     name: "Google Gemini 3 Flash",
    //     id: "cloudflare-ai-gateway:google/gemini-3-flash",
    // },
    ModelInfo {
        name: "Anthropic Claude Sonnet 4.6",
        id: "cloudflare-ai-gateway:claude-sonnet-4-6",
    },
    ModelInfo {
        name: "Anthropic Claude Opus 4.8",
        id: "cloudflare-ai-gateway:claude-opus-4-8",
    },
    ModelInfo {
        name: "Moonshot Kimi K2.6",
        id: "cloudflare-ai-gateway:@cf/moonshotai/kimi-k2.6",
        // id: "cloudflare-ai-gateway:workers-ai/@cf/moonshotai/kimi-k2.6",
    },
    ModelInfo {
        name: "DeepSeek V4 Flash",
        id: "deepseek:deepseek-v4-flash",
    },
    ModelInfo {
        name: "DeepSeek V4 Pro",
        id: "deepseek:deepseek-v4-pro",
    },
];

pub fn list_models() -> &'static [ModelInfo] {
    MODELS
}

pub fn get_model_name(id: &str) -> Option<&'static str> {
    MODELS.iter().find(|m| m.id == id).map(|m| m.name)
}

pub fn get_model_id(name: &str) -> Option<&'static str> {
    MODELS.iter().find(|m| m.name == name).map(|m| m.id)
}

pub fn all_models() -> &'static [ModelInfo] {
    MODELS
}

pub fn parse_model_id(id: &str) -> Option<(&str, &str)> {
    id.split_once(':')
}

pub fn model_display_name(id: Option<&str>) -> SharedString {
    match id {
        Some(id) => get_model_name(id)
            .map(SharedString::from)
            .unwrap_or_else(|| SharedString::from(id.to_string())),
        None => SharedString::from("Select Model"),
    }
}
