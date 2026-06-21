use std::collections::HashMap;
use std::sync::Arc;

use gpui::SharedString;

use crate::rpc::pi_rpc::{BridgeModel, PiBridge, PiRpcError};

#[derive(Clone, Debug)]
pub struct ModelInfo {
    pub name: String,
    pub id: String,
    pub thinking_level_map: Option<HashMap<String, Option<String>>>,
}

pub fn load_models(bridge: &Arc<PiBridge>) -> Result<Vec<ModelInfo>, PiRpcError> {
    let bridge_models = bridge.get_models()?;
    Ok(bridge_models
        .into_iter()
        .map(|m: BridgeModel| ModelInfo {
            id: format!("{}:{}", m.provider, m.id),
            name: m.name,
            thinking_level_map: m.thinking_level_map,
        })
        .collect())
}

pub fn list_models(models: &[ModelInfo]) -> &[ModelInfo] {
    models
}

pub fn get_model_name<'a>(models: &'a [ModelInfo], id: &str) -> Option<&'a str> {
    models.iter().find(|m| m.id == id).map(|m| m.name.as_str())
}

pub fn get_model_id<'a>(models: &'a [ModelInfo], name: &str) -> Option<&'a str> {
    models.iter().find(|m| m.name == name).map(|m| m.id.as_str())
}

pub fn all_models(models: &[ModelInfo]) -> &[ModelInfo] {
    models
}

pub fn parse_model_id(id: &str) -> Option<(&str, &str)> {
    id.split_once(':')
}

pub fn model_display_name(models: &[ModelInfo], id: Option<&str>) -> SharedString {
    match id {
        Some(id) => get_model_name(models, id)
            .map(|name| SharedString::from(name.to_string()))
            .unwrap_or_else(|| SharedString::from(id.to_string())),
        None => SharedString::from("Select Model"),
    }
}
