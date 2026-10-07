//! Overmind HUD: compact visual feedback about the session and the turn in flight.
//!
//! The HUD is one row above the composer that combines, as width allows: live turn activity
//! (phase, elapsed time, tool counts), update_plan progress, a context-window gauge, usage-limit
//! bars and a provider badge (for example when a Cursor model is active). After each turn a
//! one-line summary (tokens, tools, plan, context, optional cost) goes into the transcript.
//!
//! State lives here and is fed by small hooks in `chatwidget/overmind_hud.rs`; `BottomPane`
//! renders [`HudRow`]. Pieces are toggled in `$CODEX_HOME/overmind.toml` (see [`config`]).

pub(crate) mod activity;
pub(crate) mod config;
pub(crate) mod meter;
mod render;
pub(crate) mod segments;
mod summary;

use std::time::Instant;

use codex_protocol::plan_tool::StepStatus;
use codex_protocol::plan_tool::UpdatePlanArgs;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Widget;

pub(crate) use activity::HudEvent;
use activity::TurnActivity;
use config::HudConfig;
use meter::Glyphs;
use meter::Palette;
pub(crate) use summary::TurnSummary;
pub(crate) use summary::TurnSummaryCell;

use crate::render::renderable::Renderable;
use crate::token_usage::TokenUsage;

/// Context-window usage for the gauge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ContextGauge {
    pub(crate) used_percent: i64,
    pub(crate) used_tokens: Option<i64>,
    pub(crate) window: Option<i64>,
}

/// update_plan progress.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlanProgress {
    pub(crate) completed: usize,
    pub(crate) total: usize,
    /// The in-progress step, if any.
    pub(crate) current: Option<String>,
}

impl PlanProgress {
    pub(crate) fn from_update(update: &UpdatePlanArgs) -> Option<Self> {
        let total = update.plan.len();
        if total == 0 {
            return None;
        }
        let completed = update
            .plan
            .iter()
            .filter(|item| matches!(item.status, StepStatus::Completed))
            .count();
        let current = update
            .plan
            .iter()
            .find(|item| matches!(item.status, StepStatus::InProgress))
            .map(|item| item.step.clone());
        Some(Self {
            completed,
            total,
            current,
        })
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.completed >= self.total
    }
}

/// One usage-limit window (for example the 5h or weekly window).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LimitGauge {
    pub(crate) label: String,
    pub(crate) used_percent: f64,
}

/// Provider badge, shown for non-OpenAI providers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ModelBadge {
    pub(crate) provider: String,
    pub(crate) model: String,
}

#[derive(Debug)]
pub(crate) struct HudState {
    config: HudConfig,
    glyphs: Glyphs,
    palette: Palette,
    context: Option<ContextGauge>,
    plan: Option<PlanProgress>,
    limits: Vec<LimitGauge>,
    badge: Option<ModelBadge>,
    turn: Option<TurnActivity>,
}

impl Default for HudState {
    fn default() -> Self {
        Self::new(HudConfig::off(), Glyphs::Unicode, Palette::MONO)
    }
}

impl HudState {
    pub(crate) fn new(config: HudConfig, glyphs: Glyphs, palette: Palette) -> Self {
        Self {
            config,
            glyphs,
            palette,
            context: None,
            plan: None,
            limits: Vec::new(),
            badge: None,
            turn: None,
        }
    }

    /// Apply a loaded config, detecting glyphs (TERM=dumb) and color support (NO_COLOR).
    pub(crate) fn configure(&mut self, config: HudConfig) {
        self.glyphs = Glyphs::detect(config.ascii);
        self.palette = Palette::detect();
        self.config = config;
    }

    pub(crate) fn set_context(&mut self, context: Option<ContextGauge>) {
        self.context = context;
    }

    pub(crate) fn set_plan(&mut self, update: &UpdatePlanArgs) {
        self.plan = PlanProgress::from_update(update);
    }

    pub(crate) fn set_limits(&mut self, limits: Vec<LimitGauge>) {
        self.limits = limits;
    }

    pub(crate) fn set_badge(&mut self, badge: Option<ModelBadge>) {
        self.badge = badge;
    }

    pub(crate) fn begin_turn(&mut self, now: Instant, start_usage: TokenUsage) {
        if self.plan.as_ref().is_some_and(PlanProgress::is_complete) {
            self.plan = None;
        }
        self.turn = Some(TurnActivity::new(now, start_usage));
    }

    pub(crate) fn record(&mut self, event: HudEvent<'_>) {
        if let Some(turn) = self.turn.as_mut() {
            turn.apply(event);
        }
    }

    /// End the turn and build its summary, if summaries are enabled and anything happened.
    pub(crate) fn finish_turn(
        &mut self,
        end_usage: &TokenUsage,
        model: &str,
    ) -> Option<TurnSummary> {
        let turn = self.turn.take()?;
        if !self.config.turn_summary {
            return None;
        }
        let tokens = usage_delta(&turn.start_usage, end_usage);
        if tokens.is_zero() && turn.counts.total() == 0 {
            return None;
        }
        let cost_usd = self.config.price_for(model).map(|price| {
            price.cost_usd(
                tokens.input_tokens,
                tokens.cached_input_tokens,
                tokens.output_tokens,
            )
        });
        Some(TurnSummary {
            tokens,
            tools: turn.counts,
            plan: self.plan.clone().filter(|_| self.config.plan_progress),
            context: self.context.clone().filter(|_| self.config.context_gauge),
            cost_usd,
        })
    }

    pub(crate) fn summary_cell(&self, summary: TurnSummary) -> TurnSummaryCell {
        TurnSummaryCell::new(summary, self.glyphs, self.palette)
    }

    /// The HUD line for `width` columns, or `None` when there is nothing to show.
    pub(crate) fn line(
        &self,
        width: u16,
        task_running: bool,
        now: Instant,
    ) -> Option<Line<'static>> {
        if !self.config.row_enabled() {
            return None;
        }
        render::hud_line(self, width, task_running, now)
    }

    pub(crate) fn row(&self, task_running: bool, now: Instant) -> HudRow<'_> {
        HudRow {
            state: self,
            task_running,
            now,
        }
    }
}

fn usage_delta(start: &TokenUsage, end: &TokenUsage) -> TokenUsage {
    let diff = |end: i64, start: i64| (end - start).max(0);
    TokenUsage {
        input_tokens: diff(end.input_tokens, start.input_tokens),
        cached_input_tokens: diff(end.cached_input_tokens, start.cached_input_tokens),
        output_tokens: diff(end.output_tokens, start.output_tokens),
        reasoning_output_tokens: diff(end.reasoning_output_tokens, start.reasoning_output_tokens),
        total_tokens: diff(end.total_tokens, start.total_tokens),
    }
}

/// The HUD row as a bottom-pane renderable: one row tall when there is something to show.
pub(crate) struct HudRow<'a> {
    state: &'a HudState,
    task_running: bool,
    now: Instant,
}

impl Renderable for HudRow<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        if let Some(line) = self.state.line(area.width, self.task_running, self.now) {
            line.render(area, buf);
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        u16::from(
            self.state
                .line(width, self.task_running, self.now)
                .is_some(),
        )
    }
}

#[cfg(test)]
#[path = "hud_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "unit_tests.rs"]
mod unit_tests;
