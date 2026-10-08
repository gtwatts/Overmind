//! Overmind HUD: compact visual feedback about the session and the turn in flight.
//!
//! The HUD is one row above the composer that combines, as width allows: live turn activity
//! (phase, elapsed time, tool counts), `/pipeline` stage progress, update_plan progress, a
//! context-window gauge, usage-limit
//! bars and a provider badge (for example when a Cursor model is active). After each turn a
//! one-line summary (tokens, tools, plan, context, optional cost) goes into the transcript.
//!
//! State lives here and is fed by small hooks in `chatwidget/overmind_hud.rs`; `BottomPane`
//! renders [`HudRow`]. Pieces are toggled in `$CODEX_HOME/overmind.toml` (see [`config`]).

pub(crate) mod activity;
pub(crate) mod config;
pub(crate) mod meter;
mod pipeline_panel;
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

/// Progress of the `/pipeline` run this session is driving.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PipelineProgress {
    pub(crate) name: String,
    /// Stage ids and statuses in execution order.
    pub(crate) stages: Vec<(String, meter::StageStatus)>,
    /// Id of the stage running or waiting, if any.
    pub(crate) current: Option<String>,
    pub(crate) details: Vec<PipelineStageDetail>,
    pub(crate) paused: Option<String>,
    pub(crate) paused_at_ms: Option<i64>,
}

/// Persisted stage information, independent of the current chat turn's clock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PipelineStageDetail {
    pub(crate) id: String,
    pub(crate) status: meter::StageStatus,
    pub(crate) started_at_ms: Option<i64>,
    pub(crate) finished_at_ms: Option<i64>,
    pub(crate) expected: Vec<String>,
    pub(crate) recorded: Vec<String>,
}

impl PipelineProgress {
    pub(crate) fn done(&self) -> usize {
        self.stages
            .iter()
            .filter(|(_, status)| {
                matches!(
                    status,
                    meter::StageStatus::Done | meter::StageStatus::Skipped
                )
            })
            .count()
    }

    /// The status of the current stage, or `Done` when every stage finished.
    pub(crate) fn headline(&self) -> meter::StageStatus {
        use meter::StageStatus;
        for wanted in [
            StageStatus::Failed,
            StageStatus::Waiting,
            StageStatus::Running,
            StageStatus::Stale,
        ] {
            if self.stages.iter().any(|(_, status)| *status == wanted) {
                return wanted;
            }
        }
        if self.done() == self.stages.len() {
            StageStatus::Done
        } else {
            StageStatus::Pending
        }
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
    pipeline: Option<PipelineProgress>,
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
            pipeline: None,
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

    /// Glyph set and palette in effect, for other Overmind surfaces to match the HUD.
    pub(crate) fn render_style(&self) -> (Glyphs, Palette) {
        (self.glyphs, self.palette)
    }

    pub(crate) fn set_context(&mut self, context: Option<ContextGauge>) {
        self.context = context;
    }

    pub(crate) fn set_plan(&mut self, update: &UpdatePlanArgs) {
        self.plan = PlanProgress::from_update(update);
    }

    /// Returns whether the HUD row needs a redraw.
    pub(crate) fn set_pipeline(&mut self, pipeline: Option<PipelineProgress>) -> bool {
        let changed = self.pipeline != pipeline;
        self.pipeline = pipeline;
        changed && self.config.pipeline && self.row_enabled()
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

    /// Record a turn event; returns whether the HUD row needs a redraw.
    pub(crate) fn record(&mut self, event: HudEvent<'_>) -> bool {
        let changed = self.turn.as_mut().is_some_and(|turn| turn.apply(event));
        changed && self.row_enabled()
    }

    /// Whether the HUD row is configured to show anything.
    pub(crate) fn row_enabled(&self) -> bool {
        self.config.row_enabled()
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
            wall_time_ms: chrono::Utc::now().timestamp_millis(),
        }
    }

    fn lines(
        &self,
        width: u16,
        task_running: bool,
        now: Instant,
        wall_time_ms: i64,
        height: u16,
    ) -> Vec<Line<'static>> {
        if height == 0 {
            return Vec::new();
        }
        let hud = self.line(width, task_running, now);
        let panel_height = height.saturating_sub(u16::from(hud.is_some())).min(5);
        let mut lines = if self.config.hud && self.config.pipeline && self.config.pipeline_panel {
            self.pipeline
                .as_ref()
                .map(|pipeline| {
                    pipeline_panel::lines(
                        pipeline,
                        width,
                        panel_height,
                        wall_time_ms,
                        self.glyphs,
                        self.palette,
                    )
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        lines.extend(hud);
        lines
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

/// The HUD plus a bounded pipeline panel; constrained areas keep the compact HUD.
pub(crate) struct HudRow<'a> {
    state: &'a HudState,
    task_running: bool,
    now: Instant,
    wall_time_ms: i64,
}

impl Renderable for HudRow<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        for (index, line) in self
            .state
            .lines(
                area.width,
                self.task_running,
                self.now,
                self.wall_time_ms,
                area.height,
            )
            .into_iter()
            .enumerate()
        {
            line.render(Rect::new(area.x, area.y + index as u16, area.width, 1), buf);
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.state
            .lines(width, self.task_running, self.now, self.wall_time_ms, 6)
            .len() as u16
    }
}

#[cfg(test)]
#[path = "hud_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "unit_tests.rs"]
mod unit_tests;
