use std::io;

use codex_models_manager::model_info::BASE_INSTRUCTIONS;
use codex_protocol::openai_models::ModelsResponse;

/// Cursor models Overmind offers. Claude and Cursor's GPT models are left out on
/// purpose (Codex already serves OpenAI models natively).
const CURSOR_MODELS_JSON: &str = include_str!("cursor_models.json");

const CURSOR_MODEL_SLUGS: [&str; 7] = [
    "grok-4.7",
    "grok-4.6",
    "composer-2.5",
    "gemini-3.8-flash",
    "kimi-k3",
    "glm-5p3",
    "muse-spark-1.3",
];

pub fn is_cursor_model(slug: &str) -> bool {
    CURSOR_MODEL_SLUGS.contains(&slug)
}

/// Model metadata for the Cursor models (context windows, reasoning levels,
/// tool shapes), using Codex's standard base instructions. Used as the model
/// catalog when the `cursor` provider is active and no `model_catalog_json` is
/// configured, so `/model` lists Cursor models and no fallback metadata is used.
pub fn model_catalog() -> io::Result<ModelsResponse> {
    let mut catalog: serde_json::Value =
        serde_json::from_str(CURSOR_MODELS_JSON).map_err(io::Error::other)?;
    if let Some(models) = catalog
        .get_mut("models")
        .and_then(serde_json::Value::as_array_mut)
    {
        for model in models
            .iter_mut()
            .filter_map(serde_json::Value::as_object_mut)
        {
            model.insert(
                "base_instructions".to_string(),
                serde_json::Value::String(BASE_INSTRUCTIONS.to_string()),
            );
        }
    }
    serde_json::from_value(catalog).map_err(io::Error::other)
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
