//! `/pipeline inspect`: a pipeline definition, or a run with its evidence and history.

use codex_overmind_pipelines::Pipeline;
use codex_overmind_pipelines::Run;
use codex_overmind_pipelines::StageState;
use codex_overmind_pipelines::StageView;
use codex_overmind_pipelines::inputs::value_text;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use super::controller::PipelineContext;
use super::render::DESCRIPTION_WIDTH;
use super::render::MAX_ID_WIDTH;
use super::render::display_path;
use super::render::header;
use super::render::indent;
use super::render::input_hint;
use super::render::muted;
use super::render::run_card;
use crate::overmind::hud::meter::Tone;
use crate::overmind::hud::meter::truncate;

/// `inspect <pipeline>`: inputs, stages, policy and how to run it.
pub(super) fn pipeline_detail(
    pipeline: &Pipeline,
    ctx: &PipelineContext<'_>,
) -> Vec<Line<'static>> {
    let (g, p) = (ctx.glyphs, ctx.palette);
    let definition = &pipeline.definition;
    let mut detail = vec![definition.status.clone()];
    if let Some(version) = &definition.version {
        detail.push(format!("v{version}"));
    }
    detail.push(format!("{} stages", pipeline.steps().len()));
    let mut lines = vec![header(ctx, pipeline.name(), &detail.join(" · "))];
    if !definition.description.trim().is_empty() {
        lines.push(indent(
            vec![Span::raw(definition.description.trim().to_string())],
            1,
        ));
    }
    let guide = if pipeline.guide_path().is_some() {
        " (PIPELINE.md)"
    } else {
        ""
    };
    lines.push(indent(
        vec![muted(p, format!("{}{guide}", display_path(&pipeline.dir)))],
        1,
    ));
    if !definition.inputs.is_empty() {
        lines.push(indent(vec![p.span("Inputs", Tone::Plain).bold()], 1));
        let width = definition.inputs.keys().map(String::len).max().unwrap_or(0);
        for (key, spec) in &definition.inputs {
            let mut kind = spec.kind.as_str().to_string();
            if spec.required {
                kind.push_str(", required");
            }
            if let Some(default) = &spec.default {
                kind.push_str(&format!(" = {}", value_text(default)));
            }
            if !spec.values.is_empty() {
                let values: Vec<String> = spec.values.iter().map(value_text).collect();
                kind.push_str(&format!(" ({})", values.join(" | ")));
            }
            let mut spans = vec![Span::raw(format!("{key:<width$}  ")), muted(p, kind)];
            if let Some(description) = &spec.description {
                spans.push(Span::raw(format!(
                    "  {}",
                    truncate(description, DESCRIPTION_WIDTH, g)
                )));
            }
            lines.push(indent(spans, 2));
        }
    }
    lines.push(indent(vec![p.span("Stages", Tone::Plain).bold()], 1));
    let steps = pipeline.steps();
    let width = steps
        .iter()
        .map(|step| step.id.len())
        .max()
        .unwrap_or(0)
        .min(MAX_ID_WIDTH);
    for (position, &index) in pipeline.order().iter().enumerate() {
        let step = &steps[index];
        let mut tags = vec![step.kind.as_str().to_string()];
        if let Some(agent) = &step.agent {
            tags.push(agent.clone());
        }
        if let Some(skill) = &step.skill {
            tags.push(format!("${skill}"));
        }
        if step.needs_approval() {
            tags.push("approval".to_string());
        }
        let deps = pipeline.dependencies(index);
        let follows_previous = position > 0 && deps == [pipeline.order()[position - 1]];
        if !follows_previous && !deps.is_empty() {
            let names: Vec<&str> = deps.iter().map(|dep| steps[*dep].id.as_str()).collect();
            tags.push(format!("after {}", names.join(", ")));
        }
        let mut spans = vec![
            muted(p, format!("{:>2} ", position + 1)),
            p.span(
                format!("{:<width$}", truncate(&step.id, width, g)),
                Tone::Plain,
            ),
            muted(p, format!("  {}", tags.join(" · "))),
        ];
        if !step.outputs.is_empty() {
            spans.push(muted(
                p,
                format!(
                    " {} {}",
                    g.pointer(),
                    truncate(&step.outputs.join(", "), 48, g)
                ),
            ));
        }
        lines.push(indent(spans, 2));
    }
    if let Some(safety) = &definition.safety
        && !safety.approval_required_for.is_empty()
    {
        lines.push(indent(
            vec![muted(
                p,
                format!("Ask before: {}", safety.approval_required_for.join(", ")),
            )],
            1,
        ));
    }
    lines.push(indent(
        vec![muted(
            p,
            format!(
                "/pipeline run {} <inputs> · {}",
                pipeline.name(),
                input_hint(pipeline)
            ),
        )],
        1,
    ));
    lines
}

/// `inspect <run>`: the card plus attempts, evidence hashes and recent history.
pub(super) fn run_detail(
    run: &Run,
    views: &[StageView],
    ctx: &PipelineContext<'_>,
) -> Vec<Line<'static>> {
    let p = ctx.palette;
    let mut lines = run_card(run, views, ctx);
    lines.push(indent(
        vec![muted(
            p,
            format!(
                "workspace {} · state {}",
                display_path(&run.state.workspace),
                display_path(&run.dir)
            ),
        )],
        1,
    ));
    for &index in run.pipeline.order() {
        let step = &run.pipeline.steps()[index];
        let Some(record) = run
            .record(index)
            .filter(|record| record.status != StageState::Pending || record.attempts > 0)
        else {
            continue;
        };
        let mut text = format!("{}: attempts {}", step.id, record.attempts);
        if let Some(at) = &record.approved_at {
            text.push_str(&format!(" · approved {at}"));
        }
        if let Some(at) = &record.finished_at {
            text.push_str(&format!(" · finished {at}"));
        }
        lines.push(indent(vec![muted(p, text)], 1));
        for item in &record.evidence {
            let short: String = item.sha256.chars().take(12).collect();
            lines.push(indent(
                vec![muted(
                    p,
                    format!("{}  sha256:{short}  {} bytes", item.path, item.bytes),
                )],
                2,
            ));
        }
    }
    let history = &run.state.history;
    if !history.is_empty() {
        lines.push(indent(vec![p.span("History", Tone::Plain).bold()], 1));
        for entry in history.iter().rev().take(8).rev() {
            let stage = entry
                .stage
                .as_deref()
                .map(|stage| format!("{stage}: "))
                .unwrap_or_default();
            lines.push(indent(
                vec![muted(p, format!("{}  {stage}{}", entry.at, entry.event))],
                2,
            ));
        }
    }
    lines
}
