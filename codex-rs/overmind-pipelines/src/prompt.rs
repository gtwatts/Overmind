//! The agent turn for one stage.
//!
//! Each stage runs as one bounded user turn that carries everything the agent needs: the stage
//! task, role, skill and tool hints, bound inputs, upstream artifacts, the outputs to write,
//! the pipeline guide, the approval policy, and a stop condition so the agent does not run
//! ahead into the next stage. Overmind (not the agent) decides when a stage is done, from the
//! declared outputs.

use std::fmt::Write;
use std::path::Path;

use crate::Run;
use crate::StepKind;
use crate::inputs::value_text;

/// Long input values are shortened after the first stage; the full value stays in state.json.
const SHORT_INPUT_CHARS: usize = 240;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PromptOptions {
    /// The stage's skill is attached to the turn as a structured skill input.
    pub skill_attached: bool,
}

pub fn stage_prompt(run: &Run, index: usize, options: PromptOptions) -> String {
    let pipeline = &run.pipeline;
    let Some(step) = pipeline.steps().get(index) else {
        return String::new();
    };
    let workspace = &run.state.workspace;
    let position = pipeline
        .order()
        .iter()
        .position(|candidate| *candidate == index)
        .unwrap_or(index);
    let total = pipeline.steps().len();
    let mut out = String::new();
    let _ = writeln!(
        out,
        "[Overmind pipeline] {} · stage {}/{total} · {} ({})",
        pipeline.name(),
        position + 1,
        step.id,
        step.kind.as_str()
    );
    let _ = writeln!(
        out,
        "Run: {} (workspace {})",
        relative(&run.dir, workspace),
        workspace.display()
    );
    out.push('\n');
    if let Some(title) = &step.title {
        let _ = writeln!(out, "Stage: {title}");
    }
    let _ = writeln!(out, "Task: {}", step.description.trim());

    let mut roles = Vec::new();
    if let Some(agent) = &step.agent {
        roles.push(format!("role: {agent}"));
    }
    if let Some(skill) = &step.skill {
        let note = if options.skill_attached {
            "attached"
        } else {
            "load it if available"
        };
        roles.push(format!("skill: {skill} ({note})"));
    }
    if let Some(model) = &step.model_required {
        roles.push(format!("model: {model}"));
    }
    if !roles.is_empty() {
        let _ = writeln!(out, "{}", capitalize(&roles.join(" · ")));
    }
    match (&step.kind, step.action.as_deref()) {
        (StepKind::Script, Some(action)) => {
            let script = pipeline.dir.join(action);
            if script.is_file() {
                let _ = writeln!(
                    out,
                    "Script: run `{}` (validators take the workspace as the run directory) and keep its report as evidence.",
                    script.display()
                );
            } else {
                let _ = writeln!(out, "Action: {action}");
            }
        }
        (_, Some(action)) => {
            let _ = writeln!(out, "Action: {action}");
        }
        (_, None) => {}
    }
    if let Some(hint) = &step.action_hint {
        let _ = writeln!(
            out,
            "Tool hints: {hint} (resolve the actual tools; names are hints)"
        );
    }
    if let Some(hint) = &step.integration_hint {
        let _ = writeln!(out, "Integration: {hint}");
    }
    if let Some(parameters) = &step.parameters {
        let _ = writeln!(out, "Parameters: {parameters}");
    }
    if step.dry_run {
        let _ = writeln!(
            out,
            "Dry run: do not perform paid or external actions in this stage."
        );
    }

    if !run.state.inputs.is_empty() {
        let first = position == 0;
        let _ = writeln!(out, "Inputs:");
        for (key, value) in &run.state.inputs {
            let text = value_text(value);
            let text = if first || text.chars().count() <= SHORT_INPUT_CHARS {
                text
            } else {
                let short: String = text.chars().take(SHORT_INPUT_CHARS).collect();
                format!("{short}… (full value in state.json)")
            };
            let _ = writeln!(out, "- {key}: {text}");
        }
    }

    let upstream: Vec<String> = pipeline
        .dependencies(index)
        .iter()
        .filter_map(|dep| {
            let dep_step = pipeline.steps().get(*dep)?;
            let record = run.record(*dep);
            let paths: Vec<String> = match record {
                Some(record) if !record.evidence.is_empty() => record
                    .evidence
                    .iter()
                    .map(|item| item.path.clone())
                    .collect(),
                _ => dep_step.output_paths().map(str::to_string).collect(),
            };
            (!paths.is_empty()).then(|| format!("- {} (from {})", paths.join(", "), dep_step.id))
        })
        .collect();
    if !upstream.is_empty() {
        let _ = writeln!(out, "Upstream artifacts:");
        for line in upstream {
            let _ = writeln!(out, "{line}");
        }
    }

    if !step.outputs.is_empty() {
        let _ = writeln!(
            out,
            "Outputs to produce (paths are relative to the workspace):"
        );
        for output in &step.outputs {
            if crate::definition::is_output_path(output) {
                let _ = writeln!(out, "- {output}");
            } else {
                let _ = writeln!(
                    out,
                    "- {output} (named deliverable; cover it in a file or your summary)"
                );
            }
        }
    }

    let mut guide = format!("Pipeline folder: {}", pipeline.dir.display());
    if pipeline.guide_path().is_some() {
        guide.push_str(" (read PIPELINE.md for this stage's guidance; prompts/, templates/ and validators/ live alongside)");
    }
    let _ = writeln!(out, "{guide}");

    if let Some(safety) = &pipeline.definition.safety {
        let mut line = String::from("Approval policy:");
        if let Some(policy) = &safety.default_approval_policy {
            let _ = write!(line, " {policy}.");
        }
        if !safety.approval_required_for.is_empty() {
            let _ = write!(
                line,
                " Ask before: {}.",
                safety.approval_required_for.join(", ")
            );
        }
        let _ = writeln!(out, "{line}");
    }
    if step.needs_approval() {
        match run
            .record(index)
            .and_then(|record| record.approved_at.as_deref())
        {
            Some(at) => {
                let _ = writeln!(
                    out,
                    "The user approved this stage with /pipeline approve at {at}. That approval covers this stage only{}.",
                    step.approval_reason
                        .as_deref()
                        .map(|reason| format!(" ({reason})"))
                        .unwrap_or_default()
                );
            }
            None => {
                let _ = writeln!(
                    out,
                    "This stage needs the user's approval; stop and ask before acting."
                );
            }
        }
    }
    out.push('\n');
    out.push_str(
        "Do only this stage. When its outputs exist, stop and give a short summary; Overmind checks the outputs and starts the next stage. If you cannot finish (missing information, a failing tool, or an action that needs approval), say what is blocking and stop.",
    );
    out
}

fn relative(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
#[path = "prompt_tests.rs"]
mod tests;
