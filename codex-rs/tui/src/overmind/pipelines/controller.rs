//! Drives pipeline runs: slash commands and turn lifecycle events in, effects out.

use std::path::Path;
use std::path::PathBuf;

use codex_overmind_pipelines::NextAction;
use codex_overmind_pipelines::PipelineError;
use codex_overmind_pipelines::Run;
use codex_overmind_pipelines::RunStore;
use codex_overmind_pipelines::StageOutcome;
use codex_overmind_pipelines::discover;
use codex_overmind_pipelines::inputs::bind_inputs;
use codex_overmind_pipelines::inputs::parse_input_args;
use codex_overmind_pipelines::prompt::PromptOptions;
use codex_overmind_pipelines::prompt::stage_prompt;
use ratatui::text::Line;

use super::command::PIPELINE_USAGE;
use super::command::PipelineCommand;
use super::inspect;
use super::progress::progress;
use super::progress::recoverable_from_outputs;
use super::render;
use crate::overmind::hud::PipelineProgress;
use crate::overmind::hud::meter::Glyphs;
use crate::overmind::hud::meter::Palette;
use crate::overmind::skill_refs::AvailableSkill;
use crate::overmind::skill_refs::SkillRef;
use crate::overmind::skill_refs::resolve_skill_refs;

/// What the chat widget knows that the controller needs.
pub(crate) struct PipelineContext<'a> {
    /// Working directory; runs live in `<workspace>/.overmind/runs`.
    pub(crate) workspace: &'a Path,
    /// Pipeline search directories, highest precedence first.
    pub(crate) pipeline_dirs: &'a [PathBuf],
    /// Loaded skills, or `None` before the skills list arrives.
    pub(crate) available_skills: Option<&'a [AvailableSkill]>,
    /// A turn is running or user input is queued, so a stage turn cannot start now.
    pub(crate) busy: bool,
    pub(crate) glyphs: Glyphs,
    pub(crate) palette: Palette,
}

/// A stage turn to submit as a user message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StageSubmission {
    pub(crate) text: String,
    pub(crate) skill: Option<AvailableSkill>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PipelineEffect {
    Lines(Vec<Line<'static>>),
    Error(String),
    Submit(StageSubmission),
    /// New HUD progress (`None` clears it).
    Progress(Option<PipelineProgress>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ActiveStage {
    run_dir: PathBuf,
    index: usize,
    /// The submitted turn has started.
    started: bool,
}

/// Per-session pipeline state: which run this session drives and the stage turn in flight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PipelineSession {
    auto_advance: bool,
    stop_requested: bool,
    run_dir: Option<PathBuf>,
    stage: Option<ActiveStage>,
}

impl Default for PipelineSession {
    fn default() -> Self {
        Self {
            auto_advance: true,
            stop_requested: false,
            run_dir: None,
            stage: None,
        }
    }
}

type Effects = Vec<PipelineEffect>;

impl PipelineSession {
    pub(crate) fn set_auto_advance(&mut self, auto_advance: bool) {
        self.auto_advance = auto_advance;
    }

    /// Whether a stage turn is submitted or running.
    pub(crate) fn stage_in_flight(&self) -> bool {
        self.stage.is_some()
    }

    /// A turn started; if it is the submitted stage turn, track it.
    pub(crate) fn on_turn_started(&mut self) {
        if let Some(stage) = self.stage.as_mut() {
            stage.started = true;
        }
    }

    pub(crate) fn handle_command(
        &mut self,
        ctx: &PipelineContext<'_>,
        command: PipelineCommand,
    ) -> Effects {
        let mut effects = Vec::new();
        let result = match command {
            PipelineCommand::Overview => {
                if self.run_dir.is_some() {
                    self.status(ctx, None, &mut effects)
                } else {
                    self.list(ctx, &mut effects)
                }
            }
            PipelineCommand::List => self.list(ctx, &mut effects),
            PipelineCommand::Run { name, args } => self.start(ctx, &name, &args, &mut effects),
            PipelineCommand::Resume => self.resume(ctx, None, false, &mut effects),
            PipelineCommand::Approve { run } => {
                self.resume(ctx, run.as_deref(), true, &mut effects)
            }
            PipelineCommand::Status { run } => self.status(ctx, run.as_deref(), &mut effects),
            PipelineCommand::Inspect { target } => self.inspect(ctx, &target, &mut effects),
            PipelineCommand::Rerun { first, second } => {
                self.rerun(ctx, first.as_deref(), second.as_deref(), &mut effects)
            }
            PipelineCommand::Stop => {
                self.stop(ctx, &mut effects);
                Ok(())
            }
            PipelineCommand::Help => {
                effects.push(PipelineEffect::Lines(render::usage_lines(ctx)));
                Ok(())
            }
            PipelineCommand::Unknown(sub) => Err(format!(
                "Unknown /pipeline subcommand `{sub}`. {PIPELINE_USAGE}"
            )),
        };
        if let Err(message) = result {
            effects.push(PipelineEffect::Error(message));
        }
        effects
    }

    /// The tracked stage turn finished normally: record it and maybe start the next stage.
    pub(crate) fn on_turn_finished(&mut self, ctx: &PipelineContext<'_>) -> Effects {
        let mut effects = Vec::new();
        let Some(stage) = self.stage.take_if(|stage| stage.started) else {
            return effects;
        };
        let mut run = match Run::load(&stage.run_dir) {
            Ok(run) => run,
            Err(err) => {
                effects.push(PipelineEffect::Error(format!(
                    "Pipeline run unavailable: {err}"
                )));
                return effects;
            }
        };
        let outcome = run.complete_stage(stage.index);
        effects.push(PipelineEffect::Lines(render::stage_result_lines(
            &run,
            stage.index,
            &outcome,
            ctx,
        )));
        let result = match outcome {
            StageOutcome::Done { .. } => {
                // Gates and completion are reported even when auto-advance would stop.
                let next_executes = matches!(run.next_action(), NextAction::Execute { .. });
                let reason = if self.stop_requested {
                    Some("stopped with /pipeline stop")
                } else if !self.auto_advance {
                    Some("auto_advance is off")
                } else if ctx.busy {
                    Some("queued messages are waiting")
                } else {
                    None
                };
                self.stop_requested = false;
                match reason {
                    Some(reason) if next_executes => {
                        self.pause(ctx, &mut run, reason, &mut effects)
                    }
                    _ => self.advance(ctx, &mut run, &mut effects),
                }
            }
            StageOutcome::MissingOutputs(_) => {
                self.stop_requested = false;
                self.pause(ctx, &mut run, "outputs missing", &mut effects)
            }
        };
        if let Err(message) = result {
            effects.push(PipelineEffect::Error(message));
        }
        effects
    }

    /// The tracked stage turn was interrupted: mark the stage failed and pause.
    pub(crate) fn on_turn_interrupted(&mut self, ctx: &PipelineContext<'_>) -> Effects {
        let mut effects = Vec::new();
        let Some(stage) = self.stage.take() else {
            return effects;
        };
        self.stop_requested = false;
        let Ok(mut run) = Run::load(&stage.run_dir) else {
            return effects;
        };
        run.fail_stage(stage.index, "interrupted");
        let id = run.pipeline.steps()[stage.index].id.clone();
        if let Err(message) = self.pause(ctx, &mut run, &format!("{id} interrupted"), &mut effects)
        {
            effects.push(PipelineEffect::Error(message));
        }
        effects
    }

    fn list(&mut self, ctx: &PipelineContext<'_>, effects: &mut Effects) -> Result<(), String> {
        let discovery = discover(ctx.pipeline_dirs);
        let (runs, _) = RunStore::for_workspace(ctx.workspace).list();
        effects.push(PipelineEffect::Lines(render::list_lines(
            &discovery, &runs, ctx,
        )));
        Ok(())
    }

    fn start(
        &mut self,
        ctx: &PipelineContext<'_>,
        name: &str,
        args: &str,
        effects: &mut Effects,
    ) -> Result<(), String> {
        self.ensure_idle(ctx)?;
        let discovery = discover(ctx.pipeline_dirs);
        let Some(pipeline) = discovery.find(name) else {
            return Err(format!(
                "No pipeline named `{name}`. /pipeline list shows what is installed."
            ));
        };
        let raw = parse_input_args(&pipeline.definition.inputs, args);
        let inputs = bind_inputs(&pipeline.definition.inputs, raw).map_err(|err| {
            format!(
                "{}: {err}. {}",
                pipeline.name(),
                render::input_hint(pipeline)
            )
        })?;
        let mut run = RunStore::for_workspace(ctx.workspace)
            .create(pipeline, inputs, ctx.workspace)
            .map_err(|err| err.to_string())?;
        effects.push(PipelineEffect::Lines(render::run_created_lines(&run, ctx)));
        self.run_dir = Some(run.dir.clone());
        self.advance(ctx, &mut run, effects)
    }

    fn resume(
        &mut self,
        ctx: &PipelineContext<'_>,
        run_id: Option<&str>,
        approve: bool,
        effects: &mut Effects,
    ) -> Result<(), String> {
        self.ensure_idle(ctx)?;
        let mut run = self.target_run(ctx, run_id, /*unfinished*/ true)?;
        self.run_dir = Some(run.dir.clone());
        if run.source_changed() {
            effects.push(PipelineEffect::Lines(render::note_lines(
                "The pipeline definition changed since this run started; the run continues on its pinned snapshot. Start a new run to pick up the changes.",
            )));
        }
        match run.next_action() {
            NextAction::AwaitApproval(index) if approve => {
                run.approve(index);
                save(&mut run)?;
            }
            NextAction::AwaitApproval(_) => {}
            _ if approve => {
                effects.push(PipelineEffect::Lines(render::note_lines(
                    "Nothing is waiting for approval; continuing the run.",
                )));
            }
            _ => {}
        }
        self.advance(ctx, &mut run, effects)
    }

    fn status(
        &mut self,
        ctx: &PipelineContext<'_>,
        run_id: Option<&str>,
        effects: &mut Effects,
    ) -> Result<(), String> {
        let run = self.target_run(ctx, run_id, /*unfinished*/ false)?;
        let views = run.views();
        effects.push(PipelineEffect::Lines(render::run_card(&run, &views, ctx)));
        if self
            .run_dir
            .as_deref()
            .is_none_or(|dir| dir == run.dir.as_path())
        {
            effects.push(PipelineEffect::Progress(progress(&run, &views)));
        }
        Ok(())
    }

    fn inspect(
        &mut self,
        ctx: &PipelineContext<'_>,
        target: &str,
        effects: &mut Effects,
    ) -> Result<(), String> {
        let discovery = discover(ctx.pipeline_dirs);
        if let Some(pipeline) = discovery.find(target) {
            effects.push(PipelineEffect::Lines(inspect::pipeline_detail(
                pipeline, ctx,
            )));
            return Ok(());
        }
        match RunStore::for_workspace(ctx.workspace).open(target) {
            Ok(run) => {
                let views = run.views();
                effects.push(PipelineEffect::Lines(inspect::run_detail(
                    &run, &views, ctx,
                )));
                Ok(())
            }
            Err(PipelineError::Run(message)) if message.starts_with("no run matches") => Err(
                format!("No pipeline or run named `{target}`. /pipeline list shows both."),
            ),
            Err(err) => Err(err.to_string()),
        }
    }

    fn rerun(
        &mut self,
        ctx: &PipelineContext<'_>,
        first: Option<&str>,
        second: Option<&str>,
        effects: &mut Effects,
    ) -> Result<(), String> {
        self.ensure_idle(ctx)?;
        let store = RunStore::for_workspace(ctx.workspace);
        // `rerun <run> <stage>`, `rerun <stage>` (session or latest run), or `rerun [<run>]`.
        let (mut run, stage) = match (first, second) {
            (Some(run_id), Some(stage)) => {
                (store.open(run_id).map_err(|e| e.to_string())?, Some(stage))
            }
            (Some(word), None) => {
                let current = self.target_run(ctx, None, /*unfinished*/ false).ok();
                match current {
                    Some(run) if run.pipeline.step_index(word).is_some() => (run, Some(word)),
                    _ => (store.open(word).map_err(|e| e.to_string())?, None),
                }
            }
            (None, _) => (self.target_run(ctx, None, /*unfinished*/ false)?, None),
        };
        let Some(stage) = stage else {
            // Repeat the whole run as a new run with the same inputs (the old one is kept).
            let source = codex_overmind_pipelines::Pipeline::load(&run.state.pipeline_dir)
                .map_err(|err| format!("Cannot rerun {}: {err}", run.state.pipeline))?;
            let mut fresh = store
                .create(&source, run.state.inputs.clone(), ctx.workspace)
                .map_err(|err| err.to_string())?;
            effects.push(PipelineEffect::Lines(render::run_created_lines(
                &fresh, ctx,
            )));
            self.run_dir = Some(fresh.dir.clone());
            return self.advance(ctx, &mut fresh, effects);
        };
        let Some(index) = run.pipeline.step_index(stage) else {
            return Err(format!("{} has no stage `{stage}`.", run.state.pipeline));
        };
        let reset = run.rerun_from(index);
        save(&mut run)?;
        effects.push(PipelineEffect::Lines(render::note_lines(&format!(
            "Reset {} for a rerun: {}.",
            run.id(),
            reset.join(", ")
        ))));
        self.run_dir = Some(run.dir.clone());
        self.advance(ctx, &mut run, effects)
    }

    fn stop(&mut self, ctx: &PipelineContext<'_>, effects: &mut Effects) {
        if self.stage.is_some() {
            self.stop_requested = true;
            effects.push(PipelineEffect::Lines(render::note_lines(
                "The pipeline will pause after the current stage. /pipeline run resumes it.",
            )));
            return;
        }
        let paused = self
            .run_dir
            .as_deref()
            .and_then(|dir| Run::load(dir).ok())
            .map(|mut run| self.pause(ctx, &mut run, "stopped with /pipeline stop", effects));
        if paused.is_none() {
            effects.push(PipelineEffect::Lines(render::note_lines(
                "No pipeline is running in this session.",
            )));
        }
    }

    /// Run the next stage, or report the gate, completion or blockage.
    fn advance(
        &mut self,
        ctx: &PipelineContext<'_>,
        run: &mut Run,
        effects: &mut Effects,
    ) -> Result<(), String> {
        loop {
            let views = run.views();
            match run.next_action_from(&views) {
                NextAction::Complete => {
                    run.state.paused = None;
                    save(run)?;
                    effects.push(PipelineEffect::Lines(render::complete_lines(run, ctx)));
                    effects.push(PipelineEffect::Progress(progress(run, &views)));
                    self.run_dir = None;
                    return Ok(());
                }
                NextAction::Blocked => {
                    effects.push(PipelineEffect::Lines(render::run_card(run, &views, ctx)));
                    return Err("The run is blocked: no stage can start. /pipeline rerun <stage> resets one.".to_string());
                }
                NextAction::AwaitApproval(index) => {
                    let id = run.pipeline.steps()[index].id.clone();
                    run.pause(&format!("awaiting approval for {id}"));
                    save(run)?;
                    effects.push(PipelineEffect::Lines(render::gate_lines(run, index, ctx)));
                    effects.push(PipelineEffect::Progress(progress(run, &run.views())));
                    return Ok(());
                }
                NextAction::Execute { index, retry } => {
                    if retry && recoverable_from_outputs(run, index) {
                        // An interrupted session or a turn that wrote its outputs late.
                        let outcome = run.complete_stage(index);
                        save(run)?;
                        effects.push(PipelineEffect::Lines(render::stage_result_lines(
                            run, index, &outcome, ctx,
                        )));
                        continue;
                    }
                    if ctx.busy {
                        return Err("Wait for the current turn to finish (or interrupt it) before running a pipeline stage.".to_string());
                    }
                    run.begin_stage(index);
                    save(run)?;
                    let submission = self.submission(ctx, run, index);
                    effects.push(PipelineEffect::Lines(render::stage_started_lines(
                        run, index, ctx,
                    )));
                    effects.push(PipelineEffect::Submit(submission));
                    effects.push(PipelineEffect::Progress(progress(run, &run.views())));
                    self.stage = Some(ActiveStage {
                        run_dir: run.dir.clone(),
                        index,
                        started: false,
                    });
                    self.run_dir = Some(run.dir.clone());
                    return Ok(());
                }
            }
        }
    }

    fn pause(
        &mut self,
        ctx: &PipelineContext<'_>,
        run: &mut Run,
        reason: &str,
        effects: &mut Effects,
    ) -> Result<(), String> {
        run.pause(reason);
        save(run)?;
        let views = run.views();
        effects.push(PipelineEffect::Lines(render::paused_lines(
            run, reason, ctx,
        )));
        effects.push(PipelineEffect::Progress(progress(run, &views)));
        Ok(())
    }

    fn submission(&self, ctx: &PipelineContext<'_>, run: &Run, index: usize) -> StageSubmission {
        let skill = run.pipeline.steps()[index]
            .skill
            .as_ref()
            .and_then(|skill| {
                resolve_skill_refs(std::slice::from_ref(skill), ctx.available_skills)
                    .into_iter()
                    .next()
            })
            .and_then(|resolved| match resolved {
                SkillRef::Resolved { skill, .. } => Some(skill),
                SkillRef::Ambiguous { .. }
                | SkillRef::Missing { .. }
                | SkillRef::Unverified { .. } => None,
            });
        let text = stage_prompt(
            run,
            index,
            PromptOptions {
                skill_attached: skill.is_some(),
            },
        );
        StageSubmission { text, skill }
    }

    /// Reject commands that would start a stage while a turn is busy. A stage marked in
    /// flight while nothing runs is left over from a turn that ended without a completion
    /// event; forget it so the run can resume.
    fn ensure_idle(&mut self, ctx: &PipelineContext<'_>) -> Result<(), String> {
        if ctx.busy {
            return Err(
                "Wait for the current turn to finish (or interrupt it) before running a pipeline stage."
                    .to_string(),
            );
        }
        self.stage = None;
        self.stop_requested = false;
        Ok(())
    }

    fn target_run(
        &self,
        ctx: &PipelineContext<'_>,
        run_id: Option<&str>,
        unfinished: bool,
    ) -> Result<Run, String> {
        let store = RunStore::for_workspace(ctx.workspace);
        if let Some(id) = run_id {
            return store.open(id).map_err(|err| err.to_string());
        }
        if let Some(dir) = &self.run_dir
            && let Ok(run) = Run::load(dir)
        {
            return Ok(run);
        }
        let found = if unfinished {
            store.latest_unfinished()
        } else {
            store.latest()
        };
        found.ok_or_else(|| {
            "No pipeline runs in this directory yet. Start one with /pipeline run <name>."
                .to_string()
        })
    }
}

fn save(run: &mut Run) -> Result<(), String> {
    run.save()
        .map_err(|err| format!("Cannot save pipeline run state: {err}"))
}
