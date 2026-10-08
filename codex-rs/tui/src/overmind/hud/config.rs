//! `[tui]` settings for the Overmind HUD, read from `$CODEX_HOME/overmind.toml`.
//!
//! Overmind settings live in their own file rather than in `config.toml`: stock Codex shares
//! `config.toml` and rejects (or warns about) unknown tables, so an `[overmind]` table there
//! would break the stock binary. Semantically this file's `[tui]` table is `[overmind.tui]`.
//!
//! ```toml
//! [tui]
//! hud = true            # master switch for the HUD row above the composer
//! context_gauge = true  # context-window gauge
//! activity = true       # live phase, elapsed time and tool counts while a turn runs
//! plan_progress = true  # update_plan progress bar
//! pipeline = true       # stage track of the /pipeline run in progress
//! pipeline_panel = true # stage clocks and expected/verified artifacts above the HUD
//! rate_limits = true    # usage-limit bars when the provider reports them
//! model_badge = true    # provider badge for non-OpenAI providers such as Cursor
//! turn_summary = true   # per-turn token/tool/cost line in the transcript
//! ascii = false         # force ASCII bars (TERM=dumb always uses ASCII)
//!
//! # Optional USD prices per million tokens, keyed by model slug, for cost estimates.
//! [tui.prices."composer-2.5"]
//! input = 0.5
//! cached_input = 0.05
//! output = 2.5
//! ```

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

/// File name of the Overmind settings file inside `$CODEX_HOME`.
pub(crate) const OVERMIND_CONFIG_FILE: &str = "overmind.toml";

/// Prices in USD per million tokens.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelPrice {
    pub(crate) input: f64,
    #[serde(default)]
    pub(crate) cached_input: Option<f64>,
    pub(crate) output: f64,
}

impl ModelPrice {
    /// Estimated cost in USD; cached input falls back to the input price.
    pub(crate) fn cost_usd(&self, input: i64, cached_input: i64, output: i64) -> f64 {
        let cached = cached_input.clamp(0, input.max(0));
        let uncached = input.max(0) - cached;
        let cached_price = self.cached_input.unwrap_or(self.input);
        (uncached as f64 * self.input
            + cached as f64 * cached_price
            + output.max(0) as f64 * self.output)
            / 1_000_000.0
    }
}

/// Which HUD pieces are shown. Every piece can be turned off on its own.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HudConfig {
    pub(crate) hud: bool,
    pub(crate) context_gauge: bool,
    pub(crate) activity: bool,
    pub(crate) plan_progress: bool,
    pub(crate) pipeline: bool,
    pub(crate) pipeline_panel: bool,
    pub(crate) rate_limits: bool,
    pub(crate) model_badge: bool,
    pub(crate) turn_summary: bool,
    pub(crate) ascii: bool,
    pub(crate) prices: BTreeMap<String, ModelPrice>,
}

impl Default for HudConfig {
    fn default() -> Self {
        Self {
            hud: true,
            context_gauge: true,
            activity: true,
            plan_progress: true,
            pipeline: true,
            pipeline_panel: true,
            rate_limits: true,
            model_badge: true,
            turn_summary: true,
            ascii: false,
            prices: BTreeMap::new(),
        }
    }
}

impl HudConfig {
    /// Everything off; the state used until a config is loaded (and in upstream unit tests).
    pub(crate) fn off() -> Self {
        Self {
            hud: false,
            context_gauge: false,
            activity: false,
            plan_progress: false,
            pipeline: false,
            pipeline_panel: false,
            rate_limits: false,
            model_badge: false,
            turn_summary: false,
            ascii: false,
            prices: BTreeMap::new(),
        }
    }

    /// Whether the HUD row can show anything at all.
    pub(crate) fn row_enabled(&self) -> bool {
        self.hud
            && (self.context_gauge
                || self.activity
                || self.plan_progress
                || self.pipeline
                || self.rate_limits
                || self.model_badge)
    }

    pub(crate) fn price_for(&self, model: &str) -> Option<&ModelPrice> {
        self.prices.get(model)
    }
}

#[derive(Debug, Default, Deserialize)]
struct OvermindToml {
    #[serde(default)]
    tui: Option<HudConfig>,
}

/// Parse the contents of `overmind.toml`. Other top-level tables are reserved for future
/// Overmind features and ignored here; unknown keys inside `[tui]` are errors so typos surface.
pub(crate) fn parse_hud_config(contents: &str) -> Result<HudConfig, String> {
    toml::from_str::<OvermindToml>(contents)
        .map(|parsed| parsed.tui.unwrap_or_default())
        .map_err(|err| err.to_string())
}

/// Load the HUD config. A missing file means defaults; an invalid one means defaults plus a
/// warning for the transcript.
pub(crate) fn load_hud_config(codex_home: &Path) -> (HudConfig, Option<String>) {
    let path = codex_home.join(OVERMIND_CONFIG_FILE);
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return (HudConfig::default(), None);
        }
        Err(err) => {
            return (
                HudConfig::default(),
                Some(format!(
                    "Overmind: could not read {}: {err}",
                    path.display()
                )),
            );
        }
    };
    match parse_hud_config(&contents) {
        Ok(config) => (config, None),
        Err(err) => (
            HudConfig::default(),
            Some(format!(
                "Overmind: ignoring invalid {} (using HUD defaults): {}",
                path.display(),
                err.trim()
            )),
        ),
    }
}
