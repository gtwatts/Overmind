use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use ratatui::text::Line;

use super::command::PipelineCommand;
use super::command::parse_pipeline_command;
use super::config::parse_pipelines_config;
use super::*;
use crate::overmind::hud::HudState;
use crate::overmind::hud::PipelineProgress;
use crate::overmind::hud::config::HudConfig;
use crate::overmind::hud::meter::Glyphs;
use crate::overmind::hud::meter::Palette;
use crate::overmind::hud::meter::StageStatus;
use crate::overmind::skill_refs::AvailableSkill;

const PIPELINE: &str = r#"{
  "name": "mini-whiteboard",
  "schemaVersion": "1.0.0",
  "description": "Produce a short whiteboard video.",
  "version": "1.0.0",
  "status": "active",
  "inputs": {
    "brief": {"type": "string", "required": true, "description": "Topic and audience."},
    "mode": {"type": "enum", "default": "stroke-reveal", "values": ["stroke-reveal", "organic-drawon"]}
  },
  "steps": [
    {"id": "brief-and-mode", "kind": "instruction", "description": "Restate the brief.",
     "agent": "producer", "outputs": ["production/brief.md"], "dependsOn": []},
    {"id": "script", "kind": "instruction", "description": "Write the script.",
     "skill": "ai-filmmaking", "agent": "director", "outputs": ["production/script.md"],
     "dependsOn": ["brief-and-mode"]},
    {"id": "approve-paid-generation", "kind": "human-review", "description": "Approve the paid image plan.",
     "approvalRequired": true, "outputs": ["production/approval.md"], "dependsOn": ["script"]},
    {"id": "render", "kind": "instruction", "description": "Render the video.",
     "outputs": ["production/render.mp4"], "dependsOn": ["approve-paid-generation"]}
  ],
  "safety": {"approvalRequiredFor": ["publish", "spend"]},
  "outputs": ["production/render.mp4"]
}"#;

struct Fixture {
    _dirs: (tempfile::TempDir, tempfile::TempDir),
    workspace: PathBuf,
    roots: Vec<PathBuf>,
    skills: Vec<AvailableSkill>,
}

impl Fixture {
    fn new() -> Self {
        let pipelines = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let dir = pipelines.path().join("mini-whiteboard");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pipeline.yaml"), PIPELINE).unwrap();
        std::fs::write(dir.join("PIPELINE.md"), "# guide\n").unwrap();
        Self {
            workspace: workspace.path().to_path_buf(),
            roots: vec![pipelines.path().to_path_buf()],
            _dirs: (pipelines, workspace),
            skills: vec![AvailableSkill {
                name: "gordon-skills:ai-filmmaking".to_string(),
                path: "/skills/ai-filmmaking/SKILL.md".to_string(),
            }],
        }
    }

    fn ctx(&self, busy: bool) -> PipelineContext<'_> {
        PipelineContext {
            workspace: &self.workspace,
            pipeline_dirs: &self.roots,
            available_skills: Some(&self.skills),
            busy,
            glyphs: Glyphs::Unicode,
            palette: Palette::MONO,
        }
    }

    fn write(&self, rel: &str) {
        let path = self.workspace.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "content").unwrap();
    }

    /// Transcript text with temp paths and run ids replaced.
    fn text(&self, effects: &[PipelineEffect]) -> String {
        let mut out = Vec::new();
        for effect in effects {
            match effect {
                PipelineEffect::Lines(lines) => out.push(lines_text(lines)),
                PipelineEffect::Error(message) => out.push(format!("error: {message}")),
                PipelineEffect::Submit(submission) => {
                    let first = submission.text.lines().next().unwrap_or_default();
                    let skill = submission
                        .skill
                        .as_ref()
                        .map(|skill| format!(" +skill {}", skill.name))
                        .unwrap_or_default();
                    out.push(format!("submit: {first}{skill}"));
                }
                PipelineEffect::Progress(Some(progress)) => out.push(format!(
                    "hud: {} {}/{} current={:?}",
                    progress.name,
                    progress.done(),
                    progress.stages.len(),
                    progress.current
                )),
                PipelineEffect::Progress(None) => out.push("hud: cleared".to_string()),
            }
        }
        self.normalize(&out.join("\n"))
    }

    fn normalize(&self, text: &str) -> String {
        let runs = self.workspace.join(".overmind/runs");
        let mut text = text.to_string();
        if let Ok(entries) = std::fs::read_dir(&runs) {
            let mut ids: Vec<String> = entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| !name.starts_with('.'))
                .collect();
            ids.sort();
            let mut numbered: Vec<(usize, String)> = ids.into_iter().enumerate().collect();
            // Replace longer ids first: a same-second rerun's id extends the first one.
            numbered.sort_by_key(|(_, id)| std::cmp::Reverse(id.len()));
            for (index, id) in numbered {
                text = text.replace(&id, &format!("<run-{}>", index + 1));
            }
        }
        text = text.replace(&self.roots[0].display().to_string(), "<pipelines>");
        text.replace(&self.workspace.display().to_string(), "<workspace>")
    }
}

fn lines_text(lines: &[Line<'static>]) -> String {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn submissions(effects: &[PipelineEffect]) -> Vec<&StageSubmissionView> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            PipelineEffect::Submit(submission) => Some(submission),
            _ => None,
        })
        .collect()
}

type StageSubmissionView = super::controller::StageSubmission;

fn hud_progress(effects: &[PipelineEffect]) -> &PipelineProgress {
    effects
        .iter()
        .find_map(|effect| match effect {
            PipelineEffect::Progress(Some(progress)) => Some(progress),
            _ => None,
        })
        .expect("visible pipeline progress")
}

#[test]
fn completion_keeps_frozen_progress_and_status_refreshes_stale_evidence() {
    let fx = Fixture::new();
    let mut session = PipelineSession::default();
    session.handle_command(
        &fx.ctx(false),
        parse_pipeline_command("run mini-whiteboard topic"),
    );
    let mut effects = Vec::new();
    for (index, output) in [
        "production/brief.md",
        "production/script.md",
        "production/approval.md",
        "production/render.mp4",
    ]
    .into_iter()
    .enumerate()
    {
        if index == 2 {
            let approved =
                session.handle_command(&fx.ctx(false), parse_pipeline_command("approve"));
            assert_eq!(submissions(&approved).len(), 1);
        }
        assert!(session.stage_in_flight());
        session.on_turn_started();
        fx.write(output);
        effects = session.on_turn_finished(&fx.ctx(false));
    }

    // Completion releases the execution lifecycle but leaves verified final evidence visible.
    assert_eq!(session, PipelineSession::default());
    assert!(submissions(&effects).is_empty());
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, PipelineEffect::Progress(None)))
    );
    let completed = hud_progress(&effects).clone();
    assert_eq!(completed.done(), 4);
    assert_eq!(completed.current, None);
    assert_eq!(completed.paused, None);
    let clocks = completed
        .details
        .iter()
        .map(|stage| {
            assert_eq!(stage.status, StageStatus::Done);
            assert_eq!(stage.recorded, stage.expected);
            let started = stage.started_at_ms.expect("persisted stage start");
            let finished = stage.finished_at_ms.expect("fixed stage finish");
            assert!(finished >= started);
            (stage.started_at_ms, stage.finished_at_ms)
        })
        .collect::<Vec<_>>();
    let completed_run = codex_overmind_pipelines::RunStore::for_workspace(&fx.workspace)
        .latest()
        .expect("completed run");
    let status = session.handle_command(&fx.ctx(false), parse_pipeline_command("status"));
    assert_eq!(hud_progress(&status), &completed);

    // A real output change invalidates this stage and its dependents, without rewriting clocks.
    std::fs::write(fx.workspace.join("production/brief.md"), "changed evidence").unwrap();
    let status = session.handle_command(&fx.ctx(false), parse_pipeline_command("status"));
    let stale = hud_progress(&status);
    assert_eq!(stale.done(), 0);
    assert_eq!(stale.current.as_deref(), Some("brief-and-mode"));
    assert!(
        stale
            .details
            .iter()
            .all(|stage| stage.status == StageStatus::Stale && stage.recorded == stage.expected)
    );
    assert_eq!(
        stale
            .details
            .iter()
            .map(|stage| (stage.started_at_ms, stage.finished_at_ms))
            .collect::<Vec<_>>(),
        clocks
    );
    assert!(!session.stage_in_flight());

    // A new run replaces the final panel; inspecting an older run cannot replace an active one.
    let started = session.handle_command(
        &fx.ctx(false),
        parse_pipeline_command("run mini-whiteboard another topic"),
    );
    assert_eq!(
        hud_progress(&started).details[0].status,
        StageStatus::Running
    );
    let status = session.handle_command(
        &fx.ctx(true),
        parse_pipeline_command(&format!("status {}", completed_run.id())),
    );
    assert!(
        !status
            .iter()
            .any(|effect| matches!(effect, PipelineEffect::Progress(_)))
    );
    assert!(session.stage_in_flight());
}

#[test]
fn hud_details_preserve_stage_timestamps_and_verified_output_evidence() {
    let fx = Fixture::new();
    let mut session = PipelineSession::default();
    let effects = session.handle_command(
        &fx.ctx(false),
        parse_pipeline_command("run mini-whiteboard explain cost segregation"),
    );
    let current = effects
        .iter()
        .find_map(|effect| match effect {
            PipelineEffect::Progress(Some(progress)) => Some(progress),
            _ => None,
        })
        .expect("run progress");
    let first = &current.details[0];
    assert_eq!(first.id, "brief-and-mode");
    assert_eq!(first.status, StageStatus::Running);
    let started = first.started_at_ms.expect("stage start timestamp");
    assert_eq!(first.finished_at_ms, None);
    assert_eq!(first.expected, vec!["production/brief.md"]);
    assert!(first.recorded.is_empty());

    session.on_turn_started();
    fx.write("production/brief.md");
    let effects = session.on_turn_finished(&fx.ctx(false));
    let current = effects
        .iter()
        .find_map(|effect| match effect {
            PipelineEffect::Progress(Some(progress)) => Some(progress),
            _ => None,
        })
        .expect("advanced progress");
    let first = &current.details[0];
    assert_eq!(first.status, StageStatus::Done);
    assert_eq!(first.started_at_ms, Some(started));
    assert!(
        first
            .finished_at_ms
            .is_some_and(|finished| finished >= started)
    );
    assert_eq!(first.recorded, first.expected);
    assert_eq!(current.details[1].status, StageStatus::Running);
    assert!(current.details[1].started_at_ms.is_some());
    assert!(current.details[1].recorded.is_empty());
}

#[test]
fn parses_subcommands() {
    assert_eq!(parse_pipeline_command(""), PipelineCommand::Overview);
    assert_eq!(parse_pipeline_command(" list "), PipelineCommand::List);
    assert_eq!(parse_pipeline_command("run"), PipelineCommand::Resume);
    assert_eq!(
        parse_pipeline_command("run mini-whiteboard  explain \"cost seg\" mode=organic-drawon"),
        PipelineCommand::Run {
            name: "mini-whiteboard".to_string(),
            args: "explain \"cost seg\" mode=organic-drawon".to_string(),
        }
    );
    assert_eq!(
        parse_pipeline_command("approve"),
        PipelineCommand::Approve { run: None }
    );
    assert_eq!(
        parse_pipeline_command("status 2026"),
        PipelineCommand::Status {
            run: Some("2026".to_string())
        }
    );
    assert_eq!(parse_pipeline_command("inspect"), PipelineCommand::Help);
    assert_eq!(
        parse_pipeline_command("rerun run-1 script"),
        PipelineCommand::Rerun {
            first: Some("run-1".to_string()),
            second: Some("script".to_string()),
        }
    );
    assert_eq!(parse_pipeline_command("stop"), PipelineCommand::Stop);
    assert_eq!(
        parse_pipeline_command("frobnicate"),
        PipelineCommand::Unknown("frobnicate".to_string())
    );
}

#[test]
fn pipelines_config_defaults_and_parses() {
    assert!(parse_pipelines_config("").unwrap().auto_advance);
    assert!(
        parse_pipelines_config("[tui]\nhud = false\n")
            .unwrap()
            .auto_advance
    );
    assert!(
        !parse_pipelines_config("[pipelines]\nauto_advance = false\n")
            .unwrap()
            .auto_advance
    );
    assert!(parse_pipelines_config("[pipelines]\nbogus = 1\n").is_err());
}

#[test]
fn run_advances_through_stages_and_gates() {
    let fx = Fixture::new();
    let mut session = PipelineSession::default();

    let effects = session.handle_command(
        &fx.ctx(false),
        parse_pipeline_command("run mini-whiteboard explain cost segregation"),
    );
    assert_snapshot!("run_started", fx.text(&effects));
    let first = submissions(&effects);
    assert_eq!(first.len(), 1);
    assert!(first[0].text.contains("- brief: explain cost segregation"));
    assert!(session.stage_in_flight());

    // A completion before the stage turn starts belongs to some other turn.
    assert_eq!(session.on_turn_finished(&fx.ctx(false)), Vec::new());

    session.on_turn_started();
    fx.write("production/brief.md");
    let effects = session.on_turn_finished(&fx.ctx(false));
    assert_snapshot!("stage_done_next_started", fx.text(&effects));
    assert!(submissions(&effects)[0].text.contains("(attached)"));

    session.on_turn_started();
    fx.write("production/script.md");
    let effects = session.on_turn_finished(&fx.ctx(false));
    assert_snapshot!("gate_reached", fx.text(&effects));
    assert!(!session.stage_in_flight());

    let effects = session.handle_command(&fx.ctx(false), parse_pipeline_command("approve"));
    let gate_prompt = &submissions(&effects)[0].text;
    assert!(gate_prompt.contains("The user approved this stage with /pipeline approve at "));

    // Interrupting the stage pauses the run; `run` retries it.
    session.on_turn_started();
    let effects = session.on_turn_interrupted(&fx.ctx(false));
    assert_snapshot!("stage_interrupted", fx.text(&effects));
    let effects = session.handle_command(&fx.ctx(true), parse_pipeline_command("run"));
    assert_eq!(
        fx.text(&effects),
        "error: Wait for the current turn to finish (or interrupt it) before running a pipeline stage."
    );
    let effects = session.handle_command(&fx.ctx(false), parse_pipeline_command("run"));
    assert!(
        fx.text(&effects).contains("attempt 2"),
        "{}",
        fx.text(&effects)
    );

    // The turn ends without the declared output: the run pauses with the missing list.
    session.on_turn_started();
    let effects = session.on_turn_finished(&fx.ctx(false));
    assert_snapshot!("missing_outputs", fx.text(&effects));

    // The output shows up later; resuming records it instead of re-running the stage.
    fx.write("production/approval.md");
    let effects = session.handle_command(&fx.ctx(false), parse_pipeline_command("run"));
    assert_snapshot!("recovered_from_outputs", fx.text(&effects));

    session.on_turn_started();
    fx.write("production/render.mp4");
    let effects = session.on_turn_finished(&fx.ctx(false));
    assert_snapshot!("run_complete", fx.text(&effects));

    let effects = session.handle_command(&fx.ctx(false), parse_pipeline_command("status"));
    assert_snapshot!("status_card_complete", fx.text(&effects));
}

#[test]
fn stop_and_queued_input_pause_auto_advance() {
    let fx = Fixture::new();
    let mut session = PipelineSession::default();
    session.handle_command(
        &fx.ctx(false),
        parse_pipeline_command("run mini-whiteboard topic"),
    );
    let effects = session.handle_command(&fx.ctx(true), parse_pipeline_command("stop"));
    assert_eq!(
        fx.text(&effects),
        "The pipeline will pause after the current stage. /pipeline run resumes it."
    );
    session.on_turn_started();
    fx.write("production/brief.md");
    let effects = session.on_turn_finished(&fx.ctx(false));
    assert!(submissions(&effects).is_empty());
    assert!(
        fx.text(&effects)
            .contains("mini-whiteboard paused: stopped with /pipeline stop")
    );

    let effects = session.handle_command(&fx.ctx(false), parse_pipeline_command("run"));
    assert_eq!(submissions(&effects).len(), 1);
    session.on_turn_started();
    fx.write("production/script.md");
    let effects = session.on_turn_finished(&fx.ctx(true));
    assert!(
        fx.text(&effects).contains("needs your approval"),
        "{}",
        fx.text(&effects)
    );
}

#[test]
fn rerun_resets_a_stage_or_repeats_the_run() {
    let fx = Fixture::new();
    let mut session = PipelineSession::default();
    session.handle_command(
        &fx.ctx(false),
        parse_pipeline_command("run mini-whiteboard topic mode=organic-drawon"),
    );
    session.on_turn_started();
    fx.write("production/brief.md");
    session.on_turn_finished(&fx.ctx(false));
    session.on_turn_started();
    fx.write("production/script.md");
    session.on_turn_finished(&fx.ctx(false));

    let effects = session.handle_command(&fx.ctx(false), parse_pipeline_command("rerun script"));
    assert_snapshot!("rerun_stage", fx.text(&effects));
    session.on_turn_started();
    session.on_turn_interrupted(&fx.ctx(false));

    let effects = session.handle_command(&fx.ctx(false), parse_pipeline_command("rerun"));
    let text = fx.text(&effects);
    assert!(text.contains("new run <run-2>"), "{text}");
    assert!(text.contains("mode: organic-drawon"), "{text}");
    assert!(
        text.contains("submit: [Overmind pipeline] mini-whiteboard · stage 1/4"),
        "{text}"
    );
}

#[test]
fn input_errors_and_unknown_pipelines() {
    let fx = Fixture::new();
    let mut session = PipelineSession::default();
    let effects = session.handle_command(
        &fx.ctx(false),
        parse_pipeline_command("run mini-whiteboard mode=watercolor"),
    );
    assert_snapshot!("input_errors", fx.text(&effects));
    let effects = session.handle_command(&fx.ctx(false), parse_pipeline_command("run nope"));
    assert_eq!(
        fx.text(&effects),
        "error: No pipeline named `nope`. /pipeline list shows what is installed."
    );
    let effects = session.handle_command(&fx.ctx(false), parse_pipeline_command("status"));
    assert_eq!(
        fx.text(&effects),
        "error: No pipeline runs in this directory yet. Start one with /pipeline run <name>."
    );
    assert!(!fx.workspace.join(".overmind").exists());
}

#[test]
fn list_and_inspect_render() {
    let fx = Fixture::new();
    let mut session = PipelineSession::default();
    session.handle_command(
        &fx.ctx(false),
        parse_pipeline_command("run mini-whiteboard topic"),
    );
    let effects = session.handle_command(&fx.ctx(false), parse_pipeline_command("list"));
    assert_snapshot!("list", fx.text(&effects));
    let effects = session.handle_command(
        &fx.ctx(false),
        parse_pipeline_command("inspect mini-whiteboard"),
    );
    assert_snapshot!("inspect_pipeline", fx.text(&effects));
    let effects = session.handle_command(&fx.ctx(false), parse_pipeline_command("status"));
    assert_snapshot!("status_card_running", fx.text(&effects));
    let effects = session.handle_command(&fx.ctx(false), parse_pipeline_command("help"));
    assert_snapshot!("help", fx.text(&effects));
}

fn hud_with(progress: PipelineProgress) -> HudState {
    let mut hud = HudState::new(HudConfig::default(), Glyphs::Unicode, Palette::MONO);
    hud.set_pipeline(Some(progress));
    hud
}

fn progress(stages: &[(&str, StageStatus)], current: Option<&str>) -> PipelineProgress {
    PipelineProgress {
        name: "high-end-whiteboard".to_string(),
        stages: stages
            .iter()
            .map(|(id, status)| (id.to_string(), *status))
            .collect(),
        current: current.map(str::to_string),
        details: Vec::new(),
        paused: None,
        paused_at_ms: None,
    }
}

fn hud_renders(hud: &HudState, widths: &[u16]) -> String {
    widths
        .iter()
        .map(|width| {
            let line = hud
                .line(*width, /*task_running*/ false, Instant::now())
                .map(|line| lines_text(&[line]))
                .unwrap_or_default();
            format!("{width:>3} |{line}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn hud_stage_track_degrades_with_width() {
    use StageStatus::*;
    let long: Vec<(&str, StageStatus)> = [
        "brief-and-mode",
        "preflight",
        "script-and-beats",
        "shotlist",
        "plan-image-assets",
        "review-asset-cost",
        "approve-paid-generation",
        "generate-art",
        "board-craft",
        "reveal-pass",
        "blender-world",
        "audio",
        "build-and-verify",
        "render",
        "qc-and-handoff",
        "validate-run-evidence",
    ]
    .iter()
    .enumerate()
    .map(|(index, id)| {
        let status = match index {
            0..=2 => Done,
            3 => Running,
            _ => Pending,
        };
        (*id, status)
    })
    .collect();
    let running = hud_with(progress(&long, Some("shotlist")));
    assert_snapshot!(
        "hud_long_running",
        hud_renders(&running, &[120, 80, 50, 30, 14])
    );

    let mut gate = long.clone();
    gate[3].1 = Done;
    gate[4].1 = Done;
    gate[5].1 = Done;
    gate[6].1 = Waiting;
    let waiting = hud_with(progress(&gate, Some("approve-paid-generation")));
    assert_snapshot!("hud_long_gate", hud_renders(&waiting, &[120, 60]));

    let short = hud_with(progress(
        &[("plan", Done), ("execute", Failed), ("verify", Pending)],
        Some("execute"),
    ));
    assert_snapshot!("hud_short_failed", hud_renders(&short, &[100, 40]));
}

#[test]
fn hud_ascii_track() {
    let mut hud = HudState::new(HudConfig::default(), Glyphs::Ascii, Palette::MONO);
    hud.set_pipeline(Some(progress(
        &[
            ("a", StageStatus::Done),
            ("b", StageStatus::Stale),
            ("c", StageStatus::Running),
            ("d", StageStatus::Pending),
            ("e", StageStatus::Pending),
            ("f", StageStatus::Pending),
        ],
        Some("c"),
    )));
    assert_snapshot!("hud_ascii", hud_renders(&hud, &[80, 30]));
}

#[test]
fn set_pipeline_reports_visible_changes_only() {
    let mut hud = HudState::new(HudConfig::default(), Glyphs::Unicode, Palette::MONO);
    let value = progress(&[("a", StageStatus::Running)], Some("a"));
    assert!(hud.set_pipeline(Some(value.clone())));
    assert!(!hud.set_pipeline(Some(value)));
    let mut off = HudState::default();
    assert!(!off.set_pipeline(None));
    assert!(!Path::new("/nonexistent").exists());
}
