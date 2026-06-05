use gpui::SharedString;

#[derive(Clone, Debug)]
pub struct ModelInfo {
    pub name: &'static str,
    pub id: &'static str,
}

const MODELS: &[ModelInfo] = &[
    ModelInfo {
        name: "Claude Sonnet 4",
        id: "claude-sonnet-4",
    },
    ModelInfo {
        name: "Claude Opus 4",
        id: "claude-opus-4",
    },
    ModelInfo {
        name: "Claude Opus 4.5",
        id: "claude-opus-4-5",
    },
    ModelInfo {
        name: "GPT-4o",
        id: "gpt-4o",
    },
    ModelInfo {
        name: "GPT-4o Mini",
        id: "gpt-4o-mini",
    },
    ModelInfo {
        name: "GPT-5.1",
        id: "gpt-5.1",
    },
    ModelInfo {
        name: "o4-mini",
        id: "o4-mini",
    },
    ModelInfo {
        name: "Gemini 2.5 Pro",
        id: "gemini-2.5-pro",
    },
    ModelInfo {
        name: "DeepSeek V4",
        id: "deepseek-v4",
    },
    ModelInfo {
        name: "DeepSeek V4 Flash",
        id: "deepseek-v4-flash",
    },
    ModelInfo {
        name: "Grok 3",
        id: "grok-3",
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

pub fn model_display_name(id: Option<&str>) -> SharedString {
    match id {
        Some(id) => get_model_name(id)
            .map(|name| SharedString::from(name))
            .unwrap_or_else(|| SharedString::from(id.to_string())),
        None => SharedString::from("Select Model"),
    }
}
