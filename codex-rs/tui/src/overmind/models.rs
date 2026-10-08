//! Cursor models in the `/model` picker.
//!
//! When another provider is active and a Cursor API key is configured, the Cursor models are
//! listed after that provider's own models. A thread cannot change providers, so choosing a model
//! from the other side (a Cursor model on an OpenAI thread, or the reverse) starts a fresh session
//! on that model instead of updating the current one. Nothing is written to the shared
//! `config.toml`: the stock Codex install reads it too and has no Cursor provider.

use codex_overmind_cursor::CURSOR_PROVIDER_ID;
use codex_overmind_cursor::is_cursor_model;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ReasoningEffort;

use crate::legacy_core::config::Config;

/// Appends the Cursor models to `models` when they are not already the active catalog.
pub(crate) fn extend_with_cursor_models(models: &mut Vec<ModelPreset>, config: &Config) {
    if config.model_provider_id == CURSOR_PROVIDER_ID || !cursor_configured(config) {
        return;
    }
    match codex_overmind_cursor::model_catalog() {
        Ok(catalog) => append_presets(
            models,
            catalog.models.into_iter().map(ModelPreset::from).collect(),
        ),
        Err(err) => tracing::warn!(%err, "could not load the Cursor model catalog"),
    }
}

#[cfg(not(test))]
fn cursor_configured(config: &Config) -> bool {
    codex_overmind_cursor::is_configured(&config.codex_home)
}

/// Unit tests must not depend on a developer's Cursor key.
#[cfg(test)]
fn cursor_configured(_config: &Config) -> bool {
    false
}

fn append_presets(models: &mut Vec<ModelPreset>, extra: Vec<ModelPreset>) {
    for mut preset in extra {
        if !preset.show_in_picker || models.iter().any(|model| model.model == preset.model) {
            continue;
        }
        preset.is_default = false;
        models.push(preset);
    }
}

/// Whether picking `model` on a thread served by `current_provider` needs a fresh session,
/// because only one side of the choice is a Cursor model.
pub(crate) fn needs_new_session(current_provider: &str, model: &str) -> bool {
    let on_cursor = current_provider == CURSOR_PROVIDER_ID;
    if on_cursor {
        !is_cursor_model(model)
    } else {
        is_cursor_model(model)
    }
}

/// Model and effort to apply to the next fresh session (set by the picker).
pub(crate) type NextSessionModel = Option<(String, Option<ReasoningEffort>)>;

/// Applies a pending picker choice to the config of a session that is about to start.
pub(crate) fn apply_next_session_model(config: &mut Config, next: NextSessionModel) {
    if let Some((model, effort)) = next {
        config.model = Some(model);
        config.model_reasoning_effort = effort;
    }
}

/// Info shown after the switch.
pub(crate) fn switched_message(model: &str) -> (String, String) {
    let provider = if is_cursor_model(model) {
        "Cursor"
    } else {
        "the default provider"
    };
    (
        format!("Started a new session on {model} ({provider})."),
        "Sessions cannot change provider mid-thread; the previous session is in /resume."
            .to_string(),
    )
}

#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
