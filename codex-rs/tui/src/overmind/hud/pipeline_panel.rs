//! A bounded, live view of the stages surrounding the current pipeline stage.

use std::time::Duration;

use ratatui::text::Line;
use ratatui::text::Span;

use super::PipelineProgress;
use super::PipelineStageDetail;
use super::activity::format_elapsed;
use super::meter::Glyphs;
use super::meter::Palette;
use super::meter::StageStatus;
use super::meter::Tone;
use super::meter::truncate;

/// Bound provider/path text before sanitizing and collapse it to one display line.
pub(super) fn inline(text: &str) -> String {
    let prefix: String = text.chars().take(2_048).collect();
    crate::history_cell::sanitize_user_text(prefix.into())
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|ch| !matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
        .collect()
}

pub(super) fn lines(
    pipeline: &PipelineProgress,
    width: u16,
    height: u16,
    now_ms: i64,
    glyphs: Glyphs,
    palette: Palette,
) -> Vec<Line<'static>> {
    if width < 18 || height == 0 || pipeline.details.is_empty() {
        return Vec::new();
    }
    let count = pipeline.details.len();
    let current = pipeline
        .current
        .as_ref()
        .and_then(|id| pipeline.details.iter().position(|stage| &stage.id == id))
        .unwrap_or_else(|| {
            if pipeline.done() == count {
                count - 1
            } else {
                pipeline.done().min(count - 1)
            }
        });
    let stage_limit = if width < 40 {
        1
    } else if width < 72 {
        2
    } else {
        4
    };
    let show_header = height > 1;
    let visible = stage_limit
        .min(usize::from(height) - usize::from(show_header))
        .min(count);
    let start = current
        .saturating_sub(usize::from(visible > 1))
        .min(count - visible);
    let mut lines = Vec::new();
    if show_header {
        let headline = pipeline.headline();
        let status = if headline == StageStatus::Done {
            "complete".to_string()
        } else if let Some(reason) = &pipeline.paused {
            let failure = if headline == StageStatus::Failed {
                "failed; "
            } else {
                ""
            };
            format!("{failure}paused: {}", inline(reason))
        } else {
            match headline {
                StageStatus::Done => "complete".to_string(),
                StageStatus::Failed => "failed".to_string(),
                StageStatus::Waiting => "awaiting approval".to_string(),
                StageStatus::Stale => "stale outputs".to_string(),
                StageStatus::Running => "running".to_string(),
                StageStatus::Pending | StageStatus::Skipped => "ready".to_string(),
            }
        };
        let heading = format!(
            "Pipeline {}{}{}  {}/{}  stages {}-{}",
            inline(&pipeline.name),
            glyphs.separator(),
            status,
            pipeline.done(),
            count,
            start + 1,
            start + visible
        );
        lines.push(Line::from(vec![
            Span::raw(" "),
            palette.span(
                truncate(&heading, usize::from(width - 1), glyphs),
                pipeline.headline().tone(),
            ),
        ]));
    }
    for stage in &pipeline.details[start..start + visible] {
        lines.push(stage_line(
            stage,
            width,
            now_ms,
            pipeline.paused_at_ms,
            glyphs,
            palette,
        ));
    }
    lines
}

fn stage_line(
    stage: &PipelineStageDetail,
    width: u16,
    now_ms: i64,
    paused_at_ms: Option<i64>,
    glyphs: Glyphs,
    palette: Palette,
) -> Line<'static> {
    let end = stage.finished_at_ms.or_else(|| {
        (stage.status == StageStatus::Running).then_some(paused_at_ms.unwrap_or(now_ms))
    });
    let elapsed = stage
        .started_at_ms
        .zip(end)
        .map(|(start, end)| {
            format_elapsed(Duration::from_millis(
                end.saturating_sub(start).max(0) as u64
            ))
        })
        .unwrap_or_else(|| "-".to_string());
    let budget = usize::from(width.saturating_sub(4));
    let id_budget = budget
        .saturating_sub(elapsed.len() + 1)
        .min(if width >= 72 { 30 } else { 22 });
    let id = truncate(&inline(&stage.id), id_budget, glyphs);
    let mut text = format!("{id} {elapsed}");
    let (label, paths) = if !stage.recorded.is_empty() {
        let label = if stage.status == StageStatus::Stale {
            "stale"
        } else if stage.status == StageStatus::Done {
            "verified"
        } else {
            "recorded"
        };
        (label, &stage.recorded)
    } else {
        ("expected", &stage.expected)
    };
    if !paths.is_empty() && width >= 40 {
        let suffix = if paths.len() > 1 {
            format!(" (+{})", paths.len() - 1)
        } else {
            String::new()
        };
        text.push_str(&format!(
            "{}{} {}{suffix}",
            glyphs.separator(),
            label,
            inline(&paths[0])
        ));
    }
    Line::from(vec![
        Span::raw("  "),
        palette.span(stage.status.glyph(glyphs), stage.status.tone()),
        Span::raw(" "),
        palette.span(
            truncate(&text, budget, glyphs),
            if stage.status == StageStatus::Running {
                Tone::Plain
            } else {
                stage.status.tone()
            },
        ),
    ])
}
