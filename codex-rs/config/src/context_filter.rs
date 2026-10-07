//! Opt-in external context filter (`[context_filter]` in `config.toml`).
//!
//! When enabled, Codex runs `command` before every model request, sends it a JSON
//! description of the request (user prompt, visible skill catalog, older tool outputs) on
//! stdin, and applies the returned decision to that one request only. Stored history is
//! never changed, and any failure sends the request unfiltered.

use std::time::Duration;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// Default time budget for one filter invocation.
pub const DEFAULT_CONTEXT_FILTER_TIMEOUT_MS: u64 = 2_500;
/// Tool outputs smaller than this are never offered to the filter.
pub const DEFAULT_CONTEXT_FILTER_MIN_TOOL_OUTPUT_BYTES: usize = 2_000;

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ContextFilterToml {
    /// Turns the filter on. Defaults to `false`; `command` alone does nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    /// Program and arguments to run, e.g. `["node", "/path/filter.mjs"]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,

    /// Milliseconds to wait for the command before sending the request unfiltered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,

    /// Let the filter hide entries of the `<skills_instructions>` catalog. Defaults to `true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<bool>,

    /// Let the filter elide older tool outputs. Defaults to `true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_outputs: Option<bool>,

    /// Only tool outputs at least this many bytes long are offered for elision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_tool_output_bytes: Option<usize>,
}

/// Resolved `[context_filter]` settings. Present only when the filter is enabled and has a
/// non-empty command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextFilterConfig {
    pub command: Vec<String>,
    pub timeout: Duration,
    pub filter_skills: bool,
    pub filter_tool_outputs: bool,
    pub min_tool_output_bytes: usize,
}

impl ContextFilterToml {
    pub fn resolve(&self) -> Option<ContextFilterConfig> {
        if self.enabled != Some(true) {
            return None;
        }
        let command = self.command.clone().unwrap_or_default();
        if command
            .first()
            .is_none_or(|program| program.trim().is_empty())
        {
            tracing::warn!(
                "context_filter.enabled is true but context_filter.command is empty; filter disabled"
            );
            return None;
        }
        Some(ContextFilterConfig {
            command,
            timeout: Duration::from_millis(
                self.timeout_ms
                    .unwrap_or(DEFAULT_CONTEXT_FILTER_TIMEOUT_MS)
                    .max(1),
            ),
            filter_skills: self.skills.unwrap_or(true),
            filter_tool_outputs: self.tool_outputs.unwrap_or(true),
            min_tool_output_bytes: self
                .min_tool_output_bytes
                .unwrap_or(DEFAULT_CONTEXT_FILTER_MIN_TOOL_OUTPUT_BYTES),
        })
    }
}

#[cfg(test)]
#[path = "context_filter_tests.rs"]
mod tests;
