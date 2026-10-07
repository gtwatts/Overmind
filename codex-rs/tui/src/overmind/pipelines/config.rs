//! `[pipelines]` settings in `$CODEX_HOME/overmind.toml`.
//!
//! ```toml
//! [pipelines]
//! auto_advance = true   # start the next stage when one finishes (gates still stop)
//! ```

use std::path::Path;

use serde::Deserialize;

use crate::overmind::hud::config::OVERMIND_CONFIG_FILE;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PipelinesConfig {
    pub(crate) auto_advance: bool,
}

impl Default for PipelinesConfig {
    fn default() -> Self {
        Self { auto_advance: true }
    }
}

#[derive(Debug, Default, Deserialize)]
struct OvermindToml {
    #[serde(default)]
    pipelines: Option<PipelinesConfig>,
}

pub(crate) fn parse_pipelines_config(contents: &str) -> Result<PipelinesConfig, String> {
    toml::from_str::<OvermindToml>(contents)
        .map(|parsed| parsed.pipelines.unwrap_or_default())
        .map_err(|err| err.to_string())
}

/// Defaults when the file is missing or invalid (the HUD loader already reports parse errors).
pub(crate) fn load_pipelines_config(codex_home: &Path) -> PipelinesConfig {
    std::fs::read_to_string(codex_home.join(OVERMIND_CONFIG_FILE))
        .ok()
        .and_then(|contents| parse_pipelines_config(&contents).ok())
        .unwrap_or_default()
}
