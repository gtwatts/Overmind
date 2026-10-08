//! Overmind HUD hooks: forward ChatWidget events to `crate::overmind::hud`.
//!
//! Upstream handlers call these one-line hooks; the HUD state lives in the bottom pane, which
//! renders it above the composer. The HUD stays off in unit tests unless a test configures it,
//! so upstream snapshots are unaffected.

use super::*;
use crate::overmind::hud::ContextGauge;
use crate::overmind::hud::HudEvent;
use crate::overmind::hud::LimitGauge;
use crate::overmind::hud::ModelBadge;
use crate::overmind::hud::config::HudConfig;
use crate::overmind::hud::config::load_hud_config;

/// Provider ids that the stock status line already describes; no badge for these.
const UNBADGED_PROVIDERS: &[&str] = &["openai"];

impl ChatWidget {
    pub(super) fn overmind_hud_mcp_progress(
        &mut self,
        notification: codex_app_server_protocol::McpToolCallProgressNotification,
    ) {
        if self
            .thread_id()
            .is_none_or(|id| id.to_string() != notification.thread_id)
            || self.turn_lifecycle.last_turn_id.as_deref() != Some(notification.turn_id.as_str())
        {
            return;
        }
        self.overmind_hud_event(HudEvent::McpProgress {
            id: &notification.item_id,
            progress: notification.progress,
            total: notification.total,
            message: &notification.message,
        });
    }

    /// Load `$CODEX_HOME/overmind.toml` and enable the HUD (skipped in unit tests).
    pub(super) fn overmind_hud_init(&mut self) {
        if cfg!(test) {
            return;
        }
        let (config, warning) = load_hud_config(self.config.codex_home.as_path());
        self.overmind_hud_configure(config);
        if let Some(warning) = warning {
            self.on_warning(warning);
        }
    }

    pub(super) fn overmind_hud_configure(&mut self, config: HudConfig) {
        self.bottom_pane.overmind_hud_mut().configure(config);
        self.overmind_hud_sync_context();
        self.overmind_hud_sync_limits();
        self.overmind_hud_sync_badge();
    }

    pub(super) fn overmind_hud_event(&mut self, event: HudEvent<'_>) {
        if self.bottom_pane.overmind_hud_mut().record(event) {
            self.bottom_pane.request_redraw();
        }
    }

    /// Redraw after a HUD state change, but only when the HUD is visible at all.
    fn overmind_hud_changed(&self) {
        if self.bottom_pane.overmind_hud().row_enabled() {
            self.bottom_pane.request_redraw();
        }
    }

    pub(super) fn overmind_hud_turn_started(&mut self) {
        let start = self
            .token_info
            .as_ref()
            .map(|info| info.total_token_usage.clone())
            .unwrap_or_default();
        self.bottom_pane
            .overmind_hud_mut()
            .begin_turn(Instant::now(), start);
        self.overmind_hud_changed();
    }

    pub(super) fn overmind_hud_turn_finished(&mut self, from_replay: bool) {
        let end = self
            .token_info
            .as_ref()
            .map(|info| info.total_token_usage.clone())
            .unwrap_or_default();
        let model = self.current_model().to_string();
        let hud = self.bottom_pane.overmind_hud_mut();
        let summary = hud.finish_turn(&end, &model);
        if from_replay {
            return;
        }
        if let Some(summary) = summary {
            let cell = self.bottom_pane.overmind_hud_mut().summary_cell(summary);
            self.add_to_history(cell);
        }
    }

    pub(super) fn overmind_hud_plan(&mut self, update: &UpdatePlanArgs) {
        self.bottom_pane.overmind_hud_mut().set_plan(update);
        self.overmind_hud_changed();
    }

    pub(super) fn overmind_hud_sync_context(&mut self) {
        let context = self
            .status_line_context_used_percent()
            .filter(|_| self.token_info.is_some())
            .map(|used_percent| ContextGauge {
                used_percent,
                used_tokens: self
                    .token_info
                    .as_ref()
                    .map(|info| info.last_token_usage.tokens_in_context_window()),
                window: self.status_line_context_window_size(),
            });
        self.bottom_pane.overmind_hud_mut().set_context(context);
        self.overmind_hud_changed();
    }

    pub(super) fn overmind_hud_sync_limits(&mut self) {
        // Account limits describe the OpenAI account, not other providers such as Cursor.
        let openai = self.config.model_provider_id == "openai";
        let limits = self
            .rate_limit_snapshots_by_limit_id
            .get("codex")
            .filter(|_| openai)
            .map(|snapshot| {
                [(&snapshot.primary, false), (&snapshot.secondary, true)]
                    .into_iter()
                    .filter_map(|(window, is_secondary)| {
                        window.as_ref().map(|window| LimitGauge {
                            label: limit_label_for_window(window.window_minutes, is_secondary),
                            used_percent: window.used_percent,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.bottom_pane.overmind_hud_mut().set_limits(limits);
        self.overmind_hud_changed();
    }

    pub(super) fn overmind_hud_sync_badge(&mut self) {
        let provider_id = self.config.model_provider_id.as_str();
        let badge = (!UNBADGED_PROVIDERS.contains(&provider_id)).then(|| ModelBadge {
            provider: self.config.model_provider.name.clone(),
            model: self.current_model().to_string(),
        });
        self.bottom_pane.overmind_hud_mut().set_badge(badge);
        self.overmind_hud_changed();
    }
}
