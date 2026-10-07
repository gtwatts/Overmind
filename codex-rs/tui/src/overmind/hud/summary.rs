//! Per-turn summary line inserted into the transcript after each turn.

use ratatui::text::Line;
use ratatui::text::Span;

use super::ContextGauge;
use super::PlanProgress;
use super::activity::ToolCounts;
use super::meter::Glyphs;
use super::meter::Palette;
use super::meter::Tone;
use super::meter::format_tokens;
use super::render::context_segment;
use super::render::plan_segment;
use super::render::tools_segment;
use super::segments::Align;
use super::segments::Segment;
use super::segments::fit_line;
use crate::history_cell::HistoryCell;
use crate::token_usage::TokenUsage;

const PRIORITY_TOKENS: u8 = 90;
const PRIORITY_COST: u8 = 80;
const PRIORITY_TOOLS: u8 = 70;
const PRIORITY_CONTEXT: u8 = 60;
const PRIORITY_PLAN: u8 = 50;

/// What happened during one turn.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TurnSummary {
    pub(crate) tokens: TokenUsage,
    pub(crate) tools: ToolCounts,
    pub(crate) plan: Option<PlanProgress>,
    pub(crate) context: Option<ContextGauge>,
    pub(crate) cost_usd: Option<f64>,
}

/// `~$0.42`, `~$0.031`, `~$0.0042`.
pub(crate) fn format_usd(cost: f64) -> String {
    let cost = cost.max(0.0);
    if cost >= 1.0 {
        format!("~${cost:.2}")
    } else if cost >= 0.01 {
        format!("~${cost:.3}")
    } else {
        format!("~${cost:.4}")
    }
}

impl TurnSummary {
    fn segments(&self, g: Glyphs, p: Palette) -> Vec<Segment> {
        let mut segments = Vec::new();
        let t = &self.tokens;
        if !t.is_zero() {
            let arrow = match g {
                Glyphs::Unicode => " → ",
                Glyphs::Ascii => " -> ",
            };
            let compact = vec![
                Span::raw(format_tokens(t.input_tokens)),
                p.span(" in", Tone::Muted),
                p.span(arrow, Tone::Muted),
                Span::raw(format_tokens(t.output_tokens)),
                p.span(" out", Tone::Muted),
            ];
            let mut full = compact.clone();
            if t.cached_input_tokens > 0 {
                full.insert(
                    2,
                    p.span(
                        format!(" ({} cached)", format_tokens(t.cached_input_tokens)),
                        Tone::Muted,
                    ),
                );
            }
            if t.reasoning_output_tokens > 0 {
                full.push(p.span(
                    format!(" ({} reasoning)", format_tokens(t.reasoning_output_tokens)),
                    Tone::Muted,
                ));
            }
            let mut labeled = vec![p.span("tokens ", Tone::Muted)];
            labeled.extend(full.clone());
            segments.push(Segment::new(
                PRIORITY_TOKENS,
                Align::Left,
                vec![labeled, full, compact],
            ));
        }
        if let Some(tools) = tools_segment(&self.tools, PRIORITY_TOOLS, Align::Left, p) {
            segments.push(tools);
        }
        if let Some(plan) = &self.plan {
            segments.push(plan_segment(plan, PRIORITY_PLAN, g, p));
        }
        if let Some(context) = &self.context {
            segments.push(context_segment(context, PRIORITY_CONTEXT, g, p));
        }
        if let Some(cost) = self.cost_usd {
            segments.push(Segment::new(
                PRIORITY_COST,
                Align::Left,
                vec![vec![p.span(format_usd(cost), Tone::Plain)]],
            ));
        }
        segments
    }
}

/// The transcript cell for a [`TurnSummary`]; reflows to the available width.
#[derive(Debug)]
pub(crate) struct TurnSummaryCell {
    summary: TurnSummary,
    glyphs: Glyphs,
    palette: Palette,
}

impl TurnSummaryCell {
    pub(crate) fn new(summary: TurnSummary, glyphs: Glyphs, palette: Palette) -> Self {
        Self {
            summary,
            glyphs,
            palette,
        }
    }

    fn lead(&self) -> &'static str {
        match self.glyphs {
            Glyphs::Unicode => "  ◇ ",
            Glyphs::Ascii => "  - ",
        }
    }
}

impl HistoryCell for TurnSummaryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let segments = self.summary.segments(self.glyphs, self.palette);
        fit_line(
            &segments,
            width,
            self.lead(),
            self.glyphs.separator(),
            self.palette.style(Tone::Muted),
        )
        .map(|line| vec![line])
        .unwrap_or_default()
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        let segments = self.summary.segments(Glyphs::Ascii, Palette::MONO);
        fit_line(&segments, u16::MAX, "", " | ", Default::default())
            .map(|line| vec![line])
            .unwrap_or_default()
    }
}
