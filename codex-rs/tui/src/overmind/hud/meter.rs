//! Generic meters, gauges and stage glyphs for Overmind surfaces.
//!
//! Nothing here knows about ChatWidget state, so M2 pipelines can reuse these widgets for stage
//! progress. Every renderer takes explicit [`Glyphs`] and [`Palette`] values: snapshot tests stay
//! deterministic, and `TERM=dumb` / `NO_COLOR` degrade predictably (ASCII bars, no color, with
//! bold kept for warnings so emphasis survives without color).

use ratatui::style::Style;
use ratatui::text::Span;

use crate::style::StatusTone;
use crate::style::status_style;
use crate::terminal_palette::StdoutColorLevel;
use crate::terminal_palette::effective_stdout_color_level;

/// Character set for bars and glyphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Glyphs {
    Unicode,
    Ascii,
}

impl Glyphs {
    /// ASCII when forced by config or when `TERM=dumb`.
    pub(crate) fn detect(force_ascii: bool) -> Self {
        let dumb = std::env::var("TERM").is_ok_and(|term| term == "dumb");
        if force_ascii || dumb {
            Glyphs::Ascii
        } else {
            Glyphs::Unicode
        }
    }

    pub(crate) fn separator(self) -> &'static str {
        match self {
            Glyphs::Unicode => " · ",
            Glyphs::Ascii => " | ",
        }
    }

    pub(crate) fn activity(self) -> &'static str {
        match self {
            Glyphs::Unicode => "●",
            Glyphs::Ascii => "*",
        }
    }

    pub(crate) fn badge(self) -> &'static str {
        match self {
            Glyphs::Unicode => "◆",
            Glyphs::Ascii => "@",
        }
    }

    pub(crate) fn pointer(self) -> &'static str {
        match self {
            Glyphs::Unicode => "▸",
            Glyphs::Ascii => ">",
        }
    }

    pub(crate) fn ellipsis(self) -> &'static str {
        match self {
            Glyphs::Unicode => "…",
            Glyphs::Ascii => "...",
        }
    }
}

/// Semantic tone of a meter or label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    Calm,
    Good,
    Warn,
    Critical,
    Accent,
    Muted,
    Plain,
}

/// Whether to emit colors. Modifiers (bold, dim) are kept either way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Palette {
    pub(crate) color: bool,
}

impl Palette {
    #[cfg(test)]
    pub(crate) const COLOR: Self = Self { color: true };
    pub(crate) const MONO: Self = Self { color: false };

    /// No color when the terminal reports none (this covers `NO_COLOR` and `TERM=dumb`).
    pub(crate) fn detect() -> Self {
        Self {
            color: effective_stdout_color_level() != StdoutColorLevel::Unknown,
        }
    }

    pub(crate) fn style(self, tone: Tone) -> Style {
        match (tone, self.color) {
            (Tone::Plain, _) => Style::default(),
            (Tone::Muted, _) => Style::default().dim(),
            (Tone::Warn | Tone::Critical, false) => Style::default().bold(),
            (Tone::Calm | Tone::Good | Tone::Accent, false) => Style::default(),
            (Tone::Calm, true) => Style::default().cyan(),
            (Tone::Good, true) => Style::default().green(),
            (Tone::Warn, true) => status_style(StatusTone::Attention),
            (Tone::Critical, true) => Style::default().red().bold(),
            (Tone::Accent, true) => Style::default().magenta(),
        }
    }

    pub(crate) fn span(self, text: impl Into<String>, tone: Tone) -> Span<'static> {
        Span::styled(text.into(), self.style(tone))
    }
}

/// Tone for a "how full is it" ratio: calm, then warn at 60%, critical at 85%.
pub(crate) fn usage_tone(ratio: f64) -> Tone {
    let ratio = sanitize(ratio);
    if ratio >= 0.85 {
        Tone::Critical
    } else if ratio >= 0.60 {
        Tone::Warn
    } else {
        Tone::Calm
    }
}

fn sanitize(ratio: f64) -> f64 {
    if ratio.is_nan() {
        0.0
    } else {
        ratio.clamp(0.0, 1.0)
    }
}

/// A horizontal bar `cells` wide (ASCII adds two bracket cells).
///
/// Unicode bars use half-cell resolution (`━━━╸──`); any non-zero ratio shows at least a half
/// cell and anything short of 100% leaves at least half a cell empty, so "almost" never looks
/// like "empty" or "done".
pub(crate) fn bar(
    ratio: f64,
    cells: u16,
    tone: Tone,
    glyphs: Glyphs,
    palette: Palette,
) -> Vec<Span<'static>> {
    let ratio = sanitize(ratio);
    let cells = usize::from(cells.max(1));
    let total_halves = cells * 2;
    let mut halves = (ratio * total_halves as f64).round() as usize;
    if ratio > 0.0 && halves == 0 {
        halves = 1;
    }
    if ratio < 1.0 && halves >= total_halves {
        halves = total_halves - 1;
    }
    let full = halves / 2;
    let half = halves % 2 == 1;
    let empty = cells - full - usize::from(half);
    let fill_style = palette.style(tone);
    let empty_style = palette.style(Tone::Muted);
    match glyphs {
        Glyphs::Unicode => {
            let mut filled = "━".repeat(full);
            if half {
                filled.push('╸');
            }
            let mut spans = Vec::with_capacity(2);
            if !filled.is_empty() {
                spans.push(Span::styled(filled, fill_style));
            }
            if empty > 0 {
                spans.push(Span::styled("─".repeat(empty), empty_style));
            }
            spans
        }
        Glyphs::Ascii => {
            let mut filled = "#".repeat(full);
            if half {
                filled.push('=');
            }
            let mut spans = vec![Span::styled("[", empty_style)];
            if !filled.is_empty() {
                spans.push(Span::styled(filled, fill_style));
            }
            if empty > 0 {
                spans.push(Span::styled("-".repeat(empty), empty_style));
            }
            spans.push(Span::styled("]", empty_style));
            spans
        }
    }
}

/// `label ━━━╸── 3/5`, the building block for plan and pipeline progress.
pub(crate) fn progress(
    label: &str,
    done: usize,
    total: usize,
    cells: Option<u16>,
    glyphs: Glyphs,
    palette: Palette,
) -> Vec<Span<'static>> {
    let ratio = if total == 0 {
        0.0
    } else {
        done as f64 / total as f64
    };
    let tone = if total > 0 && done >= total {
        Tone::Good
    } else {
        Tone::Calm
    };
    let mut spans = Vec::new();
    if !label.is_empty() {
        spans.push(palette.span(format!("{label} "), Tone::Muted));
    }
    if let Some(cells) = cells {
        spans.extend(bar(ratio, cells, tone, glyphs, palette));
        spans.push(Span::raw(" "));
    }
    spans.push(palette.span(format!("{done}/{total}"), tone));
    spans
}

/// Lifecycle state of a pipeline stage or plan step.
#[allow(
    dead_code,
    reason = "Stage widgets are shared with the upcoming M2 pipelines."
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StageStatus {
    Pending,
    Running,
    Done,
    Failed,
    Skipped,
}

#[allow(
    dead_code,
    reason = "Stage widgets are shared with the upcoming M2 pipelines."
)]
impl StageStatus {
    pub(crate) fn glyph(self, glyphs: Glyphs) -> &'static str {
        match (self, glyphs) {
            (StageStatus::Pending, Glyphs::Unicode) => "○",
            (StageStatus::Running, Glyphs::Unicode) => "●",
            (StageStatus::Done, Glyphs::Unicode) => "✓",
            (StageStatus::Failed, Glyphs::Unicode) => "✗",
            (StageStatus::Skipped, Glyphs::Unicode) => "–",
            (StageStatus::Pending, Glyphs::Ascii) => ".",
            (StageStatus::Running, Glyphs::Ascii) => ">",
            (StageStatus::Done, Glyphs::Ascii) => "x",
            (StageStatus::Failed, Glyphs::Ascii) => "!",
            (StageStatus::Skipped, Glyphs::Ascii) => "-",
        }
    }

    pub(crate) fn tone(self) -> Tone {
        match self {
            StageStatus::Pending | StageStatus::Skipped => Tone::Muted,
            StageStatus::Running => Tone::Calm,
            StageStatus::Done => Tone::Good,
            StageStatus::Failed => Tone::Critical,
        }
    }
}

/// `✓ plan  ● build  ○ test`: a compact stage track for pipelines.
#[allow(
    dead_code,
    reason = "Stage widgets are shared with the upcoming M2 pipelines."
)]
pub(crate) fn stage_track(
    stages: &[(&str, StageStatus)],
    glyphs: Glyphs,
    palette: Palette,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (index, (label, status)) in stages.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(palette.span(status.glyph(glyphs), status.tone()));
        let label_tone = match status {
            StageStatus::Running => Tone::Plain,
            StageStatus::Failed => Tone::Critical,
            StageStatus::Pending | StageStatus::Done | StageStatus::Skipped => Tone::Muted,
        };
        spans.push(palette.span(format!(" {label}"), label_tone));
    }
    spans
}

/// Truncate `text` to at most `max` display columns, ending with an ellipsis when cut.
pub(crate) fn truncate(text: &str, max: usize, glyphs: Glyphs) -> String {
    use unicode_width::UnicodeWidthChar;
    use unicode_width::UnicodeWidthStr;
    if text.width() <= max {
        return text.to_string();
    }
    let ellipsis = glyphs.ellipsis();
    let budget = max.saturating_sub(ellipsis.width());
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let width = ch.width().unwrap_or(0);
        if used + width > budget {
            break;
        }
        used += width;
        out.push(ch);
    }
    if max >= ellipsis.width() {
        out.push_str(ellipsis);
    }
    out
}

/// Compact token counts: `950`, `18.2k`, `1.25M`.
pub(crate) fn format_tokens(value: i64) -> String {
    crate::status::format_tokens_compact(value)
}
