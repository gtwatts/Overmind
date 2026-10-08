//! Overmind: built-in Cursor model provider.
//!
//! Cursor models (Grok, Composer, Gemini, Kimi, GLM, Muse Spark) are only
//! reachable through Cursor's agent runtime (`@cursor/sdk`). This crate bundles
//! a small Node helper (see `helper/OVERMIND.md`) that exposes that runtime as a
//! loopback OpenAI Responses endpoint whose tools are all *client* tools, so
//! Codex keeps executing shell, apply_patch and MCP tools itself. Overmind
//! spawns and owns the helper; nothing has to be started by hand.
//!
//! Integration points are deliberately small so the fork stays easy to rebase:
//! [`with_cursor_provider`] adds the `cursor` provider to the built-in catalog,
//! [`provider_for_model`] routes `-m <cursor model>` to it, [`activate`] starts
//! the helper and wires the credential, and [`model_catalog`] supplies model
//! metadata for `/model`.

mod catalog;
mod credential;
mod helper;

use std::collections::HashMap;
use std::io;
use std::path::Path;

use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;

pub use catalog::is_cursor_model;
pub use catalog::model_catalog;

/// Provider id used in `model_provider = "cursor"` and `-c model_provider=cursor`.
pub const CURSOR_PROVIDER_ID: &str = "cursor";

/// Set to an existing Responses endpoint (for example a standalone bridge) to
/// use it instead of spawning the bundled helper.
const BASE_URL_OVERRIDE_ENV: &str = "OVERMIND_CURSOR_BASE_URL";

/// Adds the built-in `cursor` provider. Like the other built-ins it takes
/// precedence over a user-defined `[model_providers.cursor]` table.
pub fn with_cursor_provider(
    mut providers: HashMap<String, ModelProviderInfo>,
) -> HashMap<String, ModelProviderInfo> {
    providers.insert(CURSOR_PROVIDER_ID.to_string(), provider_info());
    providers
}

/// Returns the `cursor` provider id when `model` is a Cursor model, so
/// `-m grok-4.7` works without also passing a provider.
pub fn provider_for_model(model: Option<&str>) -> Option<String> {
    model
        .filter(|slug| is_cursor_model(slug))
        .map(|_| CURSOR_PROVIDER_ID.to_string())
}

/// Makes `provider` usable: loads the Cursor API key, starts the bundled helper
/// (once per process) and points the provider at it.
pub async fn activate(provider: &mut ModelProviderInfo, codex_home: &Path) -> io::Result<()> {
    activate_with(
        provider,
        codex_home,
        std::env::var(credential::CURSOR_API_KEY_ENV).ok(),
        std::env::var(BASE_URL_OVERRIDE_ENV).ok(),
    )
    .await
}

/// Whether a Cursor API key is available (environment or `$CODEX_HOME/secrets/cursor.env`),
/// so Cursor models can be offered alongside another provider's models. The key is read only
/// to check that it is present; it is never returned or logged.
pub fn is_configured(codex_home: &Path) -> bool {
    credential::load_cursor_api_key(
        codex_home,
        std::env::var(credential::CURSOR_API_KEY_ENV).ok(),
    )
    .is_ok()
}

async fn activate_with(
    provider: &mut ModelProviderInfo,
    codex_home: &Path,
    api_key_env: Option<String>,
    base_url_override: Option<String>,
) -> io::Result<()> {
    let api_key = credential::load_cursor_api_key(codex_home, api_key_env)?;
    let base_url = match base_url_override.filter(|value| !value.trim().is_empty()) {
        Some(base_url) => base_url,
        None => {
            let codex_home = codex_home.to_path_buf();
            let port = tokio::task::spawn_blocking(move || helper::ensure_running(&codex_home))
                .await
                .map_err(io::Error::other)??;
            format!("http://127.0.0.1:{port}/v1")
        }
    };
    provider.base_url = Some(base_url);
    provider.experimental_bearer_token = Some(api_key.into());
    Ok(())
}

fn provider_info() -> ModelProviderInfo {
    ModelProviderInfo {
        name: "Cursor".to_string(),
        wire_api: WireApi::Responses,
        request_max_retries: Some(2),
        stream_max_retries: Some(2),
        // Cursor can think for a long time before the first token.
        stream_idle_timeout_ms: Some(600_000),
        requires_openai_auth: false,
        ..ModelProviderInfo::default()
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
