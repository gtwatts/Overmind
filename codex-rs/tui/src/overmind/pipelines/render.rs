//! Transcript lines for `/pipeline`: the list, run cards, definitions and stage events.

use std::path::Path;

use codex_overmind_pipelines::Discovery;
use codex_overmind_pipelines::NextAction;
use codex_overmind_pipelines::Pipeline;
use codex_overmind_pipelines::Run;
use codex_overmind_pipelines::StageOutcome;
use codex_overmind_pipelines::StageView;
use codex_overmind_pipelines::inputs::value_text;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use super::controller::PipelineContext;
use super::progress::stage_status;
use crate::overmind::hud::meter::Palette;
use crate::overmind::hud::meter::StageStatus;
use crate::overmind::hud::meter::Tone;
use crate::overmind::hud::meter::bar;
use crate::overmind::hud::meter::truncate;

pub(super) const DESCRIPTION_WIDTH: usize = 64;
const BAR_CELLS: u16 = 20;
pub(super) const MAX_ID_WIDTH: usize = 30;
const MAX_EVIDENCE_SHOWN: usize = 3;

pub(super) fn header(ctx: &PipelineContext<'_>, title: &str, detail: &str) -> Line<'static> {
    let p = ctx.palette;
    let mut spans = vec![
        p.span(ctx.glyphs.badge(), Tone::Accent),
        p.span(format!(" {title}"), Tone::Accent).bold(),
    ];
    if !detail.is_empty() {
        spans.push(p.span(format!("  {detail}"), Tone::Muted));
    }
    Line::from(spans)
}

pub(super) fn indent(spans: Vec<Span<'static>>, depth: usize) -> Line<'static> {
    let mut all = vec![Span::raw("  ".repeat(depth))];
    all.extend(spans);
    Line::from(all)
}

pub(super) fn muted(p: Palette, text: impl Into<String>) -> Span<'static> {
    p.span(text, Tone::Muted)
}

pub(super) fn note_lines(text: &str) -> Vec<Line<'static>> {
    vec![Line::from(Span::raw(text.to_string()).dim())]
}

pub(super) fn usage_lines(ctx: &PipelineContext<'_>) -> Vec<Line<'static>> {
    let p = ctx.palette;
    let rows = [
        ("list", "installed pipelines and recent runs here"),
        (
            "run <name> [inputs]",
            "start a run (free text fills the main input; key=value sets others)",
        ),
        ("run", "resume the current or latest unfinished run"),
        (
            "approve",
            "approve the stage the run is waiting on, then continue",
        ),
        ("status [run]", "stage track of a run"),
        (
            "inspect <name|run>",
            "a pipeline's inputs and stages, or a run's evidence",
        ),
        (
            "rerun [stage|run]",
            "reset a stage and its dependents, or repeat a run",
        ),
        ("stop", "pause after the current stage"),
    ];
    let mut lines = vec![header(
        ctx,
        "/pipeline",
        "native pipelines, one agent turn per stage",
    )];
    for (command, help) in rows {
        lines.push(indent(
            vec![
                p.span(format!("{command:<22}"), Tone::Plain),
                muted(p, help),
            ],
            1,
        ));
    }
    lines
}

pub(super) fn list_lines(
    discovery: &Discovery,
    runs: &[Run],
    ctx: &PipelineContext<'_>,
) -> Vec<Line<'static>> {
    let (g, p) = (ctx.glyphs, ctx.palette);
    let mut lines = vec![header(
        ctx,
        "/pipeline",
        &match discovery.pipelines.len() {
            1 => "1 pipeline".to_string(),
            count => format!("{count} pipelines"),
        },
    )];
    if discovery.pipelines.is_empty() {
        lines.push(indent(
            vec![muted(
                p,
                "none found in .codex/pipelines, $CODEX_HOME/pipelines or installed plugins",
            )],
            1,
        ));
    }
    let width = discovery
        .pipelines
        .iter()
        .map(|pipeline| pipeline.name().len())
        .max()
        .unwrap_or(0)
        .min(MAX_ID_WIDTH);
    for pipeline in &discovery.pipelines {
        let status_tone = if pipeline.definition.is_active() {
            Tone::Good
        } else {
            Tone::Warn
        };
        lines.push(indent(
            vec![
                p.span(
                    format!("{:<width$}", truncate(pipeline.name(), width, g)),
                    Tone::Plain,
                ),
                p.span(format!("  {:<6}", pipeline.definition.status), status_tone),
                muted(p, format!(" {:>2} stages  ", pipeline.steps().len())),
                Span::raw(truncate(
                    pipeline.definition.description.trim(),
                    DESCRIPTION_WIDTH,
                    g,
                )),
                muted(
                    p,
                    format!("  {}", source_label(&pipeline.dir, ctx.workspace)),
                ),
            ],
            1,
        ));
    }
    for (dir, problem) in &discovery.problems {
        lines.push(indent(
            vec![p.span(
                format!("skipped {}: {problem}", display_path(dir)),
                Tone::Warn,
            )],
            1,
        ));
    }
    if !runs.is_empty() {
        lines.push(indent(vec![muted(p, "Recent runs in this directory")], 1));
        for run in runs.iter().take(5) {
            let (done, total) = run.progress();
            lines.push(indent(
                vec![
                    Span::raw(run.id().to_string()),
                    muted(p, format!("  {done}/{total}  ")),
                    muted(p, run_state_text(run)),
                ],
                2,
            ));
        }
    }
    lines.push(indent(
        vec![muted(
            p,
            "/pipeline run <name> [inputs] · /pipeline inspect <name> · /pipeline help",
        )],
        1,
    ));
    lines
}

/// `plugin:gordon-workflows`, `project` or `user`.
fn source_label(dir: &Path, workspace: &Path) -> String {
    let parts: Vec<String> = dir
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    if let Some(index) = parts.iter().rposition(|part| part == "plugins")
        && let Some(plugin) = parts.get(index + 1)
    {
        return format!("plugin:{plugin}");
    }
    if workspace
        .ancestors()
        .any(|root| dir.starts_with(root.join(".codex")))
    {
        "project".to_string()
    } else {
        "user".to_string()
    }
}

pub(super) fn run_state_text(run: &Run) -> String {
    match (&run.state.paused, run.next_action()) {
        (_, NextAction::Complete) => "complete".to_string(),
        (Some(reason), _) => format!("paused: {reason}"),
        (None, _) => "in progress".to_string(),
    }
}

pub(super) fn input_hint(pipeline: &Pipeline) -> String {
    let inputs: Vec<String> = pipeline
        .definition
        .inputs
        .iter()
        .map(|(key, spec)| {
            let mut text = key.clone();
            if !spec.values.is_empty() {
                let values: Vec<String> = spec.values.iter().map(value_text).collect();
                text.push_str(&format!("={}", values.join("|")));
            }
            if spec.required {
                text
            } else {
                format!("[{text}]")
            }
        })
        .collect();
    format!(
        "Inputs: {} (free text fills the first required text input)",
        inputs.join(" ")
    )
}

pub(super) fn run_created_lines(run: &Run, ctx: &PipelineContext<'_>) -> Vec<Line<'static>> {
    let p = ctx.palette;
    let mut lines = vec![header(
        ctx,
        &run.state.pipeline,
        &format!("new run {}", run.id()),
    )];
    for (key, value) in &run.state.inputs {
        lines.push(indent(
            vec![
                muted(p, format!("{key}: ")),
                Span::raw(truncate(&value_text(value), DESCRIPTION_WIDTH, ctx.glyphs)),
            ],
            1,
        ));
    }
    lines.push(indent(
        vec![muted(p, format!("state: {}", display_path(&run.dir)))],
        1,
    ));
    lines
}

fn position(run: &Run, index: usize) -> usize {
    run.pipeline
        .order()
        .iter()
        .position(|candidate| *candidate == index)
        .unwrap_or(index)
        + 1
}

pub(super) fn stage_started_lines(
    run: &Run,
    index: usize,
    ctx: &PipelineContext<'_>,
) -> Vec<Line<'static>> {
    let p = ctx.palette;
    let step = &run.pipeline.steps()[index];
    let attempts = run.record(index).map_or(1, |record| record.attempts);
    let mut spans = vec![
        p.span(StageStatus::Running.glyph(ctx.glyphs), Tone::Calm),
        p.span(
            format!(
                " {} {}/{}",
                run.state.pipeline,
                position(run, index),
                run.pipeline.steps().len()
            ),
            Tone::Accent,
        ),
        Span::raw(format!("  {}", step.id)),
    ];
    if attempts > 1 {
        spans.push(muted(p, format!("  attempt {attempts}")));
    }
    vec![Line::from(spans)]
}

pub(super) fn stage_result_lines(
    run: &Run,
    index: usize,
    outcome: &StageOutcome,
    ctx: &PipelineContext<'_>,
) -> Vec<Line<'static>> {
    let (g, p) = (ctx.glyphs, ctx.palette);
    let step = &run.pipeline.steps()[index];
    match outcome {
        StageOutcome::Done { files, unchecked } => {
            let mut spans = vec![
                p.span(StageStatus::Done.glyph(g), Tone::Good),
                Span::raw(format!(" {}", step.id)),
            ];
            let mut detail = match files {
                0 => " done".to_string(),
                1 => " done · 1 output recorded".to_string(),
                n => format!(" done · {n} outputs recorded"),
            };
            if !unchecked.is_empty() {
                detail.push_str(&format!(" · not checkable: {}", unchecked.join(", ")));
            }
            spans.push(muted(p, detail));
            vec![Line::from(spans)]
        }
        StageOutcome::MissingOutputs(missing) => vec![
            Line::from(vec![
                p.span(StageStatus::Failed.glyph(g), Tone::Critical),
                Span::raw(format!(" {}", step.id)),
                p.span(" missing outputs", Tone::Critical),
            ]),
            indent(vec![muted(p, missing.join(", "))], 1),
        ],
    }
}

pub(super) fn gate_lines(run: &Run, index: usize, ctx: &PipelineContext<'_>) -> Vec<Line<'static>> {
    let (g, p) = (ctx.glyphs, ctx.palette);
    let step = &run.pipeline.steps()[index];
    let mut lines = vec![Line::from(vec![
        p.span(StageStatus::Waiting.glyph(g), Tone::Warn),
        Span::raw(format!(" {}", step.id)),
        p.span(" needs your approval", Tone::Warn),
    ])];
    lines.push(indent(
        vec![Span::raw(step.description.trim().to_string())],
        1,
    ));
    if let Some(reason) = &step.approval_reason {
        lines.push(indent(vec![muted(p, reason.clone())], 1));
    }
    let review: Vec<String> = run
        .pipeline
        .dependencies(index)
        .iter()
        .filter_map(|dep| run.record(*dep))
        .flat_map(|record| record.evidence.iter().map(|item| item.path.clone()))
        .collect();
    if !review.is_empty() {
        lines.push(indent(
            vec![muted(p, format!("review: {}", review.join(", ")))],
            1,
        ));
    }
    lines.push(indent(
        vec![muted(
            p,
            "/pipeline approve runs it · /pipeline rerun <stage> redoes earlier work",
        )],
        1,
    ));
    lines
}

pub(super) fn paused_lines(
    run: &Run,
    reason: &str,
    ctx: &PipelineContext<'_>,
) -> Vec<Line<'static>> {
    let p = ctx.palette;
    let hint = match run.next_action() {
        NextAction::AwaitApproval(_) => "/pipeline approve continues",
        NextAction::Complete => "/pipeline status shows the result",
        NextAction::Execute { .. } | NextAction::Blocked => {
            "/pipeline run retries · /pipeline rerun <stage> resets a stage"
        }
    };
    vec![indent(
        vec![
            p.span(
                format!("{} paused: {reason}", run.state.pipeline),
                Tone::Warn,
            ),
            muted(p, format!(" · {hint}")),
        ],
        0,
    )]
}

pub(super) fn complete_lines(run: &Run, ctx: &PipelineContext<'_>) -> Vec<Line<'static>> {
    let p = ctx.palette;
    let total = run.pipeline.steps().len();
    let mut lines = vec![Line::from(vec![
        p.span(StageStatus::Done.glyph(ctx.glyphs), Tone::Good),
        p.span(format!(" {} complete", run.state.pipeline), Tone::Good)
            .bold(),
        muted(p, format!("  {total}/{total} stages · run {}", run.id())),
    ])];
    let outputs: Vec<&str> = run
        .pipeline
        .definition
        .outputs
        .iter()
        .map(String::as_str)
        .filter(|output| codex_overmind_pipelines::definition::is_output_path(output))
        .collect();
    if !outputs.is_empty() {
        lines.push(indent(
            vec![muted(
                p,
                format!(
                    "deliverables: {}",
                    truncate(&outputs.join(", "), 160, ctx.glyphs)
                ),
            )],
            1,
        ));
    }
    lines
}

/// Header, progress bar and one line per stage.
pub(super) fn run_card(
    run: &Run,
    views: &[StageView],
    ctx: &PipelineContext<'_>,
) -> Vec<Line<'static>> {
    let (g, p) = (ctx.glyphs, ctx.palette);
    let steps = run.pipeline.steps();
    let done = views
        .iter()
        .filter(|view| matches!(view, StageView::Done | StageView::Skipped))
        .count();
    let total = steps.len();
    let mut lines = vec![header(
        ctx,
        &run.state.pipeline,
        &format!("run {}", run.id()),
    )];
    let ratio = if total == 0 {
        0.0
    } else {
        done as f64 / total as f64
    };
    let tone = if done == total {
        Tone::Good
    } else {
        Tone::Calm
    };
    let mut progress = bar(ratio, BAR_CELLS, tone, g, p);
    progress.push(p.span(format!(" {done}/{total}"), tone));
    progress.push(muted(p, format!(" · {}", run_state_text(run))));
    lines.push(indent(progress, 1));
    let width = steps
        .iter()
        .map(|step| step.id.len())
        .max()
        .unwrap_or(0)
        .min(MAX_ID_WIDTH);
    for &index in run.pipeline.order() {
        let step = &steps[index];
        let status = stage_status(views[index]);
        let id_tone = match status {
            StageStatus::Running => Tone::Plain,
            StageStatus::Failed => Tone::Critical,
            StageStatus::Waiting | StageStatus::Stale => Tone::Warn,
            StageStatus::Pending | StageStatus::Done | StageStatus::Skipped => Tone::Muted,
        };
        let mut spans = vec![
            p.span(status.glyph(g), status.tone()),
            p.span(
                format!(" {:<width$}", truncate(&step.id, width, g)),
                id_tone,
            ),
        ];
        let detail = stage_detail(run, index, views[index]);
        if !detail.is_empty() {
            spans.push(muted(p, format!("  {detail}")));
        }
        lines.push(indent(spans, 1));
    }
    lines
}

fn stage_detail(run: &Run, index: usize, view: StageView) -> String {
    let step = &run.pipeline.steps()[index];
    let record = run.record(index);
    match view {
        StageView::Done => {
            let paths: Vec<&str> = record
                .map(|record| {
                    record
                        .evidence
                        .iter()
                        .map(|item| item.path.as_str())
                        .collect()
                })
                .unwrap_or_default();
            let mut text = paths
                .iter()
                .take(MAX_EVIDENCE_SHOWN)
                .copied()
                .collect::<Vec<_>>()
                .join(", ");
            if paths.len() > MAX_EVIDENCE_SHOWN {
                text.push_str(&format!(" +{}", paths.len() - MAX_EVIDENCE_SHOWN));
            }
            text
        }
        StageView::Running => format!(
            "running · attempt {}",
            record.map_or(1, |record| record.attempts)
        ),
        StageView::Failed => {
            let note = record
                .and_then(|record| record.note.clone())
                .unwrap_or_else(|| "failed".to_string());
            match record.filter(|record| !record.missing.is_empty()) {
                Some(record) => format!("{note}: {}", record.missing.join(", ")),
                None => note,
            }
        }
        StageView::Stale => "stale: outputs or an upstream stage changed".to_string(),
        StageView::AwaitingApproval => "waiting for /pipeline approve".to_string(),
        StageView::Pending | StageView::Ready if step.needs_approval() => {
            "needs approval".to_string()
        }
        StageView::Pending | StageView::Ready | StageView::Skipped => String::new(),
    }
}

pub(super) fn display_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(rest) = path.strip_prefix(&home)
    {
        return format!("~/{}", rest.display());
    }
    path.display().to_string()
}
