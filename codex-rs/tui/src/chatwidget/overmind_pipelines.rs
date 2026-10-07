//! Overmind `/pipeline` hooks: build the controller context, apply its effects, and forward
//! turn lifecycle events so stage turns are recorded and the next stage starts.

use super::*;
use crate::overmind::pipelines::PipelineContext;
use crate::overmind::pipelines::PipelineEffect;
use crate::overmind::pipelines::config::load_pipelines_config;
use crate::overmind::pipelines::parse_pipeline_command;

/// How a controller call sees the widget.
#[derive(Clone, Copy)]
enum Busy {
    /// Commands: any running or pending turn, or queued input, blocks a new stage.
    TurnOrQueue,
    /// Turn completion: the finishing turn does not count, queued input does.
    QueueOnly,
}

impl ChatWidget {
    /// Read `[pipelines]` from `$CODEX_HOME/overmind.toml` (skipped in unit tests).
    pub(super) fn overmind_pipelines_init(&mut self) {
        if cfg!(test) {
            return;
        }
        let config = load_pipelines_config(self.config.codex_home.as_path());
        self.overmind_pipelines
            .set_auto_advance(config.auto_advance);
    }

    pub(super) fn overmind_pipeline_command(&mut self, args: &str) {
        let command = parse_pipeline_command(args);
        self.overmind_pipeline_run(Busy::TurnOrQueue, |session, ctx| {
            session.handle_command(ctx, command)
        });
    }

    pub(super) fn overmind_pipeline_turn_started(&mut self) {
        self.overmind_pipelines.on_turn_started();
    }

    pub(super) fn overmind_pipeline_turn_finished(&mut self, from_replay: bool) {
        if from_replay || !self.overmind_pipelines.stage_in_flight() {
            return;
        }
        self.overmind_pipeline_run(Busy::QueueOnly, |session, ctx| {
            session.on_turn_finished(ctx)
        });
    }

    pub(super) fn overmind_pipeline_turn_interrupted(&mut self) {
        if !self.overmind_pipelines.stage_in_flight() {
            return;
        }
        self.overmind_pipeline_run(Busy::QueueOnly, |session, ctx| {
            session.on_turn_interrupted(ctx)
        });
    }

    fn overmind_pipeline_run(
        &mut self,
        busy: Busy,
        call: impl FnOnce(
            &mut crate::overmind::pipelines::PipelineSession,
            &PipelineContext<'_>,
        ) -> Vec<PipelineEffect>,
    ) {
        let env = self.custom_command_env();
        let skills = self.available_skills_for_commands();
        let busy = match busy {
            Busy::TurnOrQueue => {
                self.is_user_turn_pending_or_running() || self.has_queued_follow_up_messages()
            }
            Busy::QueueOnly => self.has_queued_follow_up_messages(),
        };
        let (glyphs, palette) = self.bottom_pane.overmind_hud().render_style();
        let ctx = PipelineContext {
            workspace: self.config.cwd.as_path(),
            pipeline_dirs: &env.pipeline_dirs,
            available_skills: skills.as_deref(),
            busy,
            glyphs,
            palette,
        };
        let effects = call(&mut self.overmind_pipelines, &ctx);
        self.overmind_pipeline_apply(effects);
    }

    fn overmind_pipeline_apply(&mut self, effects: Vec<PipelineEffect>) {
        for effect in effects {
            match effect {
                PipelineEffect::Lines(lines) => self.add_plain_history_lines(lines),
                PipelineEffect::Error(message) => self.add_error_message(message),
                PipelineEffect::Progress(progress) => {
                    if self.bottom_pane.overmind_hud_mut().set_pipeline(progress) {
                        self.bottom_pane.request_redraw();
                    }
                }
                PipelineEffect::Submit(submission) => {
                    let mention_bindings = submission
                        .skill
                        .into_iter()
                        .map(|skill| MentionBinding {
                            sigil: '$',
                            mention: skill.name,
                            path: skill.path,
                        })
                        .collect();
                    self.queue_user_message(UserMessage {
                        mention_bindings,
                        ..UserMessage::from(submission.text)
                    });
                }
            }
        }
        self.request_redraw();
    }
}
