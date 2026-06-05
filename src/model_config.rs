use gpui::SharedString;

#[derive(Clone, Debug)]
pub struct ModelInfo {
    pub provider: &'static str,
    pub name: &'static str,
    pub id: &'static str,
}

const MODELS: &[ModelInfo] = &[
    ModelInfo {
        provider: "anthropic",
        name: "Claude Sonnet 4",
        id: "anthropic/claude-sonnet-4",
    },
    ModelInfo {
        provider: "anthropic",
        name: "Claude Opus 4",
        id: "anthropic/claude-opus-4",
    },
    ModelInfo {
        provider: "anthropic",
        name: "Claude Opus 4.5",
        id: "anthropic/claude-opus-4-5",
    },
    ModelInfo {
        provider: "openai",
        name: "GPT-4o",
        id: "openai/gpt-4o",
    },
    ModelInfo {
        provider: "openai",
        name: "GPT-4o Mini",
        id: "openai/gpt-4o-mini",
    },
    ModelInfo {
        provider: "openai",
        name: "GPT-5.1",
        id: "openai/gpt-5.1",
    },
    ModelInfo {
        provider: "openai",
        name: "o4-mini",
        id: "openai/o4-mini",
    },
    ModelInfo {
        provider: "google",
        name: "Gemini 2.5 Pro",
        id: "google/gemini-2.5-pro",
    },
    ModelInfo {
        provider: "deepseek",
        name: "DeepSeek V4",
        id: "deepseek/deepseek-v4",
    },
    ModelInfo {
        provider: "deepseek",
        name: "DeepSeek V4 Flash",
        id: "deepseek/deepseek-v4-flash",
    },
    ModelInfo {
        provider: "xai",
        name: "Grok 3",
        id: "xai/grok-3",
    },
];

pub fn list_models() -> Vec<(&'static str, Vec<&'static ModelInfo>)> {
    let mut providers: Vec<&str> = MODELS.iter().map(|m| m.provider).collect();
    providers.sort();
    providers.dedup();

    providers
        .into_iter()
        .map(|provider| {
            let models: Vec<&ModelInfo> = MODELS
                .iter()
                .filter(|m| m.provider == provider)
                .collect();
            (provider, models)
        })
        .collect()
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

pub fn model_display_name(id: Option<&str>) -> SharedString {
    match id {
        Some(id) => get_model_name(id)
            .map(|name| SharedString::from(name))
            .unwrap_or_else(|| SharedString::from(id.to_string())),
        None => SharedString::from("Select Model"),
    }
}
