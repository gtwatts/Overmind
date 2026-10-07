//! Segment builders for the HUD row.

use std::time::Instant;

use ratatui::text::Line;
use ratatui::text::Span;

use super::ContextGauge;
use super::HudState;
use super::LimitGauge;
use super::ModelBadge;
use super::PipelineProgress;
use super::PlanProgress;
use super::activity::ToolCounts;
use super::activity::TurnActivity;
use super::activity::format_elapsed;
use super::meter::Glyphs;
use super::meter::Palette;
use super::meter::StageStatus;
use super::meter::Tone;
use super::meter::bar;
use super::meter::format_tokens;
use super::meter::progress;
use super::meter::stage_dots;
use super::meter::stage_track;
use super::meter::truncate;
use super::meter::usage_tone;
use super::segments::Align;
use super::segments::Segment;
use super::segments::fit_line;

const PRIORITY_ACTIVITY: u8 = 100;
const PRIORITY_PIPELINE: u8 = 95;
const PRIORITY_CONTEXT: u8 = 90;
const PRIORITY_PLAN: u8 = 80;
const PRIORITY_BADGE: u8 = 70;
const PRIORITY_TOOLS: u8 = 50;
const PRIORITY_LIMITS: u8 = 40;

pub(super) fn hud_line(
    state: &HudState,
    width: u16,
    task_running: bool,
    now: Instant,
) -> Option<Line<'static>> {
    let config = &state.config;
    let (g, p) = (state.glyphs, state.palette);
    let mut segments = Vec::new();
    if config.activity
        && task_running
        && let Some(turn) = &state.turn
    {
        segments.push(activity_segment(turn, now, g, p));
        if let Some(tools) = tools_segment(&turn.counts, PRIORITY_TOOLS, Align::Left, p) {
            segments.push(tools);
        }
    }
    if config.pipeline
        && let Some(pipeline) = &state.pipeline
    {
        segments.push(pipeline_segment(pipeline, g, p));
    }
    if config.plan_progress
        && let Some(plan) = &state.plan
    {
        segments.push(plan_segment(plan, PRIORITY_PLAN, g, p));
    }
    if config.context_gauge
        && let Some(context) = &state.context
    {
        segments.push(context_segment(context, PRIORITY_CONTEXT, g, p));
    }
    if config.rate_limits
        && let Some(limits) = limits_segment(&state.limits, g, p)
    {
        segments.push(limits);
    }
    if config.model_badge
        && let Some(badge) = &state.badge
    {
        segments.push(badge_segment(badge, g, p));
    }
    // Leave one blank column on each side so the row lines up with the composer text.
    fit_line(
        &segments,
        width.saturating_sub(1),
        " ",
        g.separator(),
        p.style(Tone::Muted),
    )
}

fn activity_segment(turn: &TurnActivity, now: Instant, g: Glyphs, p: Palette) -> Segment {
    let attention = turn.phase.needs_attention();
    let head = vec![
        p.span(
            g.activity(),
            if attention { Tone::Warn } else { Tone::Accent },
        ),
        Span::raw(format!(" {}", format_elapsed(turn.elapsed(now)))),
    ];
    let verb_tone = if attention { Tone::Warn } else { Tone::Muted };
    let with_verb = |detail_width: Option<usize>| {
        let mut spans = head.clone();
        spans.push(p.span(format!("  {}", turn.phase.verb()), verb_tone));
        if let (Some(max), Some(detail)) = (detail_width, turn.phase.detail()) {
            spans.push(Span::raw(format!(" {}", truncate(&detail, max, g))));
        }
        spans
    };
    let mut short = head.clone();
    short.push(p.span(format!(" {}", turn.phase.short_verb()), verb_tone));
    Segment::new(
        PRIORITY_ACTIVITY,
        Align::Left,
        vec![
            with_verb(Some(40)),
            with_verb(Some(24)),
            with_verb(None),
            short,
            head.clone(),
        ],
    )
}

pub(super) fn tools_segment(
    counts: &ToolCounts,
    priority: u8,
    align: Align,
    p: Palette,
) -> Option<Segment> {
    let total = counts.total();
    if total == 0 {
        return None;
    }
    let noun = if total == 1 { "tool" } else { "tools" };
    let breakdown = counts
        .breakdown()
        .into_iter()
        .map(|(count, label)| format!("{count} {label}"))
        .collect::<Vec<_>>()
        .join(", ");
    let short = vec![
        Span::raw(total.to_string()),
        p.span(format!(" {noun}"), Tone::Muted),
    ];
    let mut full = short.clone();
    full.push(p.span(format!(" ({breakdown})"), Tone::Muted));
    Some(Segment::new(priority, align, vec![full, short]))
}

pub(super) fn plan_segment(plan: &PlanProgress, priority: u8, g: Glyphs, p: Palette) -> Segment {
    let with_bar = |cells: u16| progress("plan", plan.completed, plan.total, Some(cells), g, p);
    let mut full = with_bar(8);
    if let Some(current) = plan.current.as_deref().filter(|_| !plan.is_complete()) {
        full.push(p.span(format!(" {} ", g.pointer()), Tone::Muted));
        full.push(Span::raw(truncate(current, 28, g)));
    }
    Segment::new(
        priority,
        Align::Left,
        vec![
            full,
            with_bar(8),
            with_bar(4),
            progress("plan", plan.completed, plan.total, None, g, p),
        ],
    )
}

/// Stage tracks with labels only fit short pipelines; longer ones use one glyph per stage.
const LABELED_TRACK_MAX_STAGES: usize = 5;

/// `● whiteboard ✓✓●○○ 2/5 ▸ script`, degrading to a bar, then to `● 2/5`.
fn pipeline_segment(pipeline: &PipelineProgress, g: Glyphs, p: Palette) -> Segment {
    let headline = pipeline.headline();
    let done = pipeline.done();
    let total = pipeline.stages.len();
    let head = |name_width: Option<usize>| {
        let mut spans = vec![p.span(headline.glyph(g), headline.tone())];
        if let Some(width) = name_width {
            spans.push(p.span(
                format!(" {}", truncate(&pipeline.name, width, g)),
                Tone::Accent,
            ));
        }
        spans
    };
    let count_tone = if done >= total {
        Tone::Good
    } else {
        Tone::Calm
    };
    let count = || p.span(format!(" {done}/{total}"), count_tone);
    let current = |width: usize| -> Vec<Span<'static>> {
        let Some(current) = pipeline.current.as_deref() else {
            return Vec::new();
        };
        let (verb, tone) = match headline {
            StageStatus::Done => return Vec::new(),
            StageStatus::Waiting => (" awaiting ".to_string(), Tone::Warn),
            StageStatus::Failed => (" failed ".to_string(), Tone::Critical),
            StageStatus::Stale => (" stale ".to_string(), Tone::Warn),
            _ => (format!(" {} ", g.pointer()), Tone::Muted),
        };
        vec![p.span(verb, tone), Span::raw(truncate(current, width, g))]
    };
    let statuses: Vec<StageStatus> = pipeline.stages.iter().map(|(_, status)| *status).collect();

    let mut variants = Vec::new();
    if total <= LABELED_TRACK_MAX_STAGES {
        let labeled: Vec<(&str, StageStatus)> = pipeline
            .stages
            .iter()
            .map(|(id, status)| (id.as_str(), *status))
            .collect();
        let mut spans = head(Some(28));
        spans.push(Span::raw("  "));
        spans.extend(stage_track(&labeled, g, p));
        variants.push(spans);
    }
    let mut dots = head(Some(28));
    dots.push(Span::raw(" "));
    dots.extend(stage_dots(&statuses, g, p));
    dots.push(count());
    dots.extend(current(32));
    variants.push(dots);

    let ratio = if total == 0 {
        0.0
    } else {
        done as f64 / total as f64
    };
    let mut medium = head(Some(18));
    medium.push(Span::raw(" "));
    medium.extend(bar(ratio, 6, count_tone, g, p));
    medium.push(count());
    medium.extend(current(20));
    variants.push(medium);

    let mut short = head(Some(12));
    short.push(count());
    short.extend(current(12));
    variants.push(short);

    let mut tiny = head(None);
    tiny.push(count());
    variants.push(tiny);
    Segment::new(PRIORITY_PIPELINE, Align::Left, variants)
}

pub(super) fn context_segment(
    context: &ContextGauge,
    priority: u8,
    g: Glyphs,
    p: Palette,
) -> Segment {
    let used = context.used_percent.clamp(0, 100);
    let ratio = used as f64 / 100.0;
    let tone = usage_tone(ratio);
    let gauge = |cells: Option<u16>| {
        let mut spans = vec![p.span("ctx ", Tone::Muted)];
        if let Some(cells) = cells {
            spans.extend(bar(ratio, cells, tone, g, p));
            spans.push(Span::raw(" "));
        }
        spans.push(p.span(format!("{used}%"), tone));
        spans
    };
    let mut full = gauge(Some(10));
    if let (Some(tokens), Some(window)) = (context.used_tokens, context.window) {
        full.push(p.span(
            format!(" {}/{}", format_tokens(tokens), format_tokens(window)),
            Tone::Muted,
        ));
    }
    Segment::new(
        priority,
        Align::Left,
        vec![full, gauge(Some(10)), gauge(Some(5)), gauge(None)],
    )
}

fn short_limit_label(label: &str) -> String {
    match label.to_ascii_lowercase().as_str() {
        "weekly" => "wk".to_string(),
        "daily" => "day".to_string(),
        "monthly" => "mo".to_string(),
        "annual" | "yearly" => "yr".to_string(),
        _ => label.to_string(),
    }
}

fn limits_segment(limits: &[LimitGauge], g: Glyphs, p: Palette) -> Option<Segment> {
    if limits.is_empty() {
        return None;
    }
    let render = |limits: &[LimitGauge], cells: Option<u16>| {
        let mut spans = Vec::new();
        for (index, limit) in limits.iter().enumerate() {
            if index > 0 {
                spans.push(Span::raw("  "));
            }
            let ratio = limit.used_percent / 100.0;
            let tone = usage_tone(ratio);
            spans.push(p.span(format!("{} ", short_limit_label(&limit.label)), Tone::Muted));
            if let Some(cells) = cells {
                spans.extend(bar(ratio, cells, tone, g, p));
                spans.push(Span::raw(" "));
            }
            spans.push(p.span(
                format!("{:.0}%", limit.used_percent.clamp(0.0, 100.0)),
                tone,
            ));
        }
        spans
    };
    Some(Segment::new(
        PRIORITY_LIMITS,
        Align::Right,
        vec![
            render(limits, Some(6)),
            render(limits, None),
            render(&limits[..1], None),
        ],
    ))
}

fn badge_segment(badge: &ModelBadge, g: Glyphs, p: Palette) -> Segment {
    let head = vec![
        p.span(g.badge(), Tone::Accent),
        Span::styled(format!(" {}", badge.provider), p.style(Tone::Accent).bold()),
    ];
    let mut full = head.clone();
    full.push(Span::raw(format!(" {}", badge.model)));
    Segment::new(PRIORITY_BADGE, Align::Right, vec![full, head])
}
