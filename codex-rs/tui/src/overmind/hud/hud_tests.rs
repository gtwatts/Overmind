use std::time::Duration;
use std::time::Instant;

use codex_protocol::plan_tool::PlanItemArg;
use codex_protocol::plan_tool::StepStatus;
use codex_protocol::plan_tool::UpdatePlanArgs;
use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use ratatui::text::Line;

use super::*;
use crate::history_cell::HistoryCell;
use crate::overmind::hud::activity::ToolCounts;
use crate::overmind::hud::config::ModelPrice;
use crate::overmind::hud::meter::StageStatus;
use crate::overmind::hud::meter::stage_track;

const WIDTHS: &[u16] = &[140, 100, 80, 60, 40, 24];

fn pipeline_state(glyphs: Glyphs) -> HudState {
    let mut state = HudState::new(HudConfig::default(), glyphs, Palette::MONO);
    state.set_pipeline(Some(PipelineProgress {
        name: "demo".to_string(),
        stages: vec![
            ("script".into(), StageStatus::Done),
            ("render".into(), StageStatus::Running),
            ("review".into(), StageStatus::Pending),
        ],
        current: Some("render".into()),
        details: vec![
            PipelineStageDetail {
                id: "script".into(),
                status: StageStatus::Done,
                started_at_ms: Some(0),
                finished_at_ms: Some(2_000),
                expected: vec!["production/script.md".into()],
                recorded: vec!["production/script.md".into()],
            },
            PipelineStageDetail {
                id: "render".into(),
                status: StageStatus::Running,
                started_at_ms: Some(4_000),
                finished_at_ms: None,
                expected: vec!["production/render.mp4".into()],
                recorded: Vec::new(),
            },
            PipelineStageDetail {
                id: "review".into(),
                status: StageStatus::Pending,
                started_at_ms: None,
                finished_at_ms: None,
                expected: Vec::new(),
                recorded: Vec::new(),
            },
        ],
        paused: None,
        paused_at_ms: None,
    }));
    state
}

#[test]
fn pipeline_panel_uses_stage_clocks_and_labels_artifact_evidence() {
    let state = pipeline_state(Glyphs::Unicode);
    let text = state
        .lines(120, false, Instant::now(), 10_000, 6)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("script 2s · verified production/script.md"),
        "{text}"
    );
    assert!(
        text.contains("render 6s · expected production/render.mp4"),
        "{text}"
    );
    let later = state
        .lines(120, false, Instant::now(), 20_000, 6)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(later.contains("script 2s"));
    assert!(later.contains("render 16s"));
}

#[test]
fn pipeline_panel_is_bounded_and_respects_ascii_monochrome_and_toggle() {
    for glyphs in [Glyphs::Ascii, Glyphs::Unicode] {
        let mut state = pipeline_state(glyphs);
        for width in [1, 8, 17, 18, 24, 40, 60, 80, 140] {
            for height in [0, 1, 2, 3, 6, 20] {
                let lines = state.lines(width, false, Instant::now(), 10_000, height);
                assert!(lines.len() <= usize::from(height.min(6)));
                for line in lines {
                    assert!(
                        line.width() <= usize::from(width),
                        "width {width}: {}",
                        line_text(&line)
                    );
                    assert!(
                        line.spans
                            .iter()
                            .all(|span| span.style.fg.is_none() && span.style.bg.is_none())
                    );
                    if glyphs == Glyphs::Ascii {
                        assert!(line_text(&line).is_ascii());
                    }
                }
            }
        }
        state.config.pipeline_panel = false;
        assert_eq!(state.row(false, Instant::now()).desired_height(120), 1);
        state.config.hud = false;
        assert_eq!(state.row(false, Instant::now()).desired_height(120), 0);
    }
}

#[test]
fn paused_pipeline_clock_stops_and_failure_and_stale_artifacts_are_explicit() {
    let mut state = pipeline_state(Glyphs::Unicode);
    let pipeline = state.pipeline.as_mut().unwrap();
    pipeline.paused = Some("stopped by user".into());
    pipeline.paused_at_ms = Some(8_000);
    pipeline.details[0].status = StageStatus::Stale;
    let text = state
        .lines(120, false, Instant::now(), 60_000, 6)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("paused: stopped by user"));
    assert!(text.contains("script 2s · stale production/script.md"));
    assert!(text.contains("render 4s"));
    let pipeline = state.pipeline.as_mut().unwrap();
    pipeline.paused = None;
    pipeline.paused_at_ms = None;
    pipeline.stages[1].1 = StageStatus::Failed;
    pipeline.details[1].status = StageStatus::Failed;
    pipeline.details[1].finished_at_ms = Some(9_000);
    let text = state
        .lines(120, false, Instant::now(), 60_000, 6)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("failed"));
    assert!(text.contains("render 5s"));
}

#[test]
fn completed_pipeline_panel_has_fixed_clocks_even_if_a_pause_reason_remains() {
    let mut state = pipeline_state(Glyphs::Ascii);
    let pipeline = state.pipeline.as_mut().unwrap();
    pipeline.current = None;
    pipeline.paused = Some("old reason".into());
    for (index, stage) in pipeline.details.iter_mut().enumerate() {
        stage.status = StageStatus::Done;
        stage.started_at_ms = Some(1_000);
        stage.finished_at_ms = Some(4_000);
        pipeline.stages[index].1 = StageStatus::Done;
    }
    let now = Instant::now();
    let first = state.lines(120, false, now, 10_000, 6);
    let later = state.lines(120, false, now, 80_000, 6);
    assert_eq!(first, later);
    assert!(line_text(&first[0]).contains("complete"));
    assert!(!line_text(&first[0]).contains("paused"));
}

#[test]
fn reported_tool_progress_is_determinate_only_with_a_valid_total() {
    let mut state = HudState::new(HudConfig::default(), Glyphs::Ascii, Palette::MONO);
    let now = Instant::now();
    state.begin_turn(now, TokenUsage::default());
    state.record(HudEvent::McpStarted {
        id: "render",
        server: "photocraft",
        tool: "render",
    });
    state.record(HudEvent::McpProgress {
        id: "render",
        progress: Some(0.5),
        total: Some(1.5),
        message: "\x1b[31mFrame\nready\x07\u{202e}",
    });
    for width in [140, 80, 40, 14, 5] {
        let text = line_text(&state.line(width, true, now).unwrap());
        assert!(text.contains("33%"), "{text}");
        assert!(text.is_ascii());
        assert!(!text.contains('\x1b') && !text.contains('\n'));
    }
    state.record(HudEvent::McpProgress {
        id: "render",
        progress: Some(0.75),
        total: None,
        message: "Finishing",
    });
    let text = line_text(&state.line(140, true, now).unwrap());
    assert!(!text.contains('%'));
    assert!(text.contains("Finishing"));
    state.record(HudEvent::McpFinished("render"));
    let text = line_text(&state.line(140, true, now).unwrap());
    assert!(!text.contains("Finishing"));
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

fn framed(line: Option<Line<'_>>, width: u16) -> String {
    use unicode_width::UnicodeWidthStr;
    let text = line.as_ref().map(line_text).unwrap_or_default();
    let pad = usize::from(width).saturating_sub(text.width());
    format!("{width:>3} |{text}{}|", " ".repeat(pad))
}

fn plan(steps: &[(&str, StepStatus)]) -> UpdatePlanArgs {
    UpdatePlanArgs {
        explanation: None,
        plan: steps
            .iter()
            .map(|(step, status)| PlanItemArg {
                step: (*step).to_string(),
                status: status.clone(),
            })
            .collect(),
    }
}

fn five_step_plan() -> UpdatePlanArgs {
    plan(&[
        ("Study the bottom pane", StepStatus::Completed),
        ("Build the meter widgets", StepStatus::Completed),
        ("Wire HUD hooks", StepStatus::Completed),
        ("Run the codex-tui test suite", StepStatus::InProgress),
        ("Update OVERMIND.md", StepStatus::Pending),
    ])
}

fn usage(input: i64, cached: i64, output: i64, reasoning: i64) -> TokenUsage {
    TokenUsage {
        input_tokens: input,
        cached_input_tokens: cached,
        output_tokens: output,
        reasoning_output_tokens: reasoning,
        total_tokens: input + output,
    }
}

fn session_state(glyphs: Glyphs) -> HudState {
    let mut state = HudState::new(HudConfig::default(), glyphs, Palette::MONO);
    state.set_context(Some(ContextGauge {
        used_percent: 58,
        used_tokens: Some(148_000),
        window: Some(256_000),
    }));
    state.set_limits(vec![
        LimitGauge {
            label: "5h".to_string(),
            used_percent: 38.0,
        },
        LimitGauge {
            label: "weekly".to_string(),
            used_percent: 12.0,
        },
    ]);
    state.set_badge(Some(ModelBadge {
        provider: "Cursor".to_string(),
        model: "grok-4.7".to_string(),
    }));
    state
}

fn running_state(glyphs: Glyphs, t0: Instant) -> HudState {
    let mut state = session_state(glyphs);
    state.set_plan(&five_step_plan());
    state.begin_turn(t0, TokenUsage::default());
    for _ in 0..3 {
        state.record(HudEvent::CommandStarted("rg -n HudState"));
        state.record(HudEvent::ToolFinished);
    }
    state.record(HudEvent::PatchStarted { files: 2 });
    state.record(HudEvent::PatchStarted { files: 1 });
    state.record(HudEvent::McpStarted {
        id: "github-1",
        server: "github",
        tool: "get_file_contents",
    });
    state.record(HudEvent::CommandStarted(
        "bash -lc 'cargo nextest run -p codex-tui --no-fail-fast'",
    ));
    state
}

fn render_widths(state: &HudState, task_running: bool, now: Instant) -> String {
    WIDTHS
        .iter()
        .map(|&width| framed(state.line(width, task_running, now), width))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn hud_running_turn_snapshot() {
    let t0 = Instant::now();
    let state = running_state(Glyphs::Unicode, t0);
    assert_snapshot!(
        "hud_running_turn",
        render_widths(
            &state,
            /*task_running*/ true,
            t0 + Duration::from_secs(72)
        )
    );
}

#[test]
fn hud_running_turn_ascii_snapshot() {
    let t0 = Instant::now();
    let state = running_state(Glyphs::Ascii, t0);
    assert_snapshot!(
        "hud_running_turn_ascii",
        render_widths(
            &state,
            /*task_running*/ true,
            t0 + Duration::from_secs(72)
        )
    );
}

#[test]
fn hud_idle_near_context_limit_snapshot() {
    let t0 = Instant::now();
    let mut state = session_state(Glyphs::Unicode);
    state.set_context(Some(ContextGauge {
        used_percent: 91,
        used_tokens: Some(233_000),
        window: Some(256_000),
    }));
    assert_snapshot!(
        "hud_idle_near_context_limit",
        render_widths(&state, /*task_running*/ false, t0)
    );
}

#[test]
fn hud_awaiting_approval_snapshot() {
    let t0 = Instant::now();
    let mut state = session_state(Glyphs::Unicode);
    state.set_badge(None);
    state.begin_turn(t0, TokenUsage::default());
    state.record(HudEvent::Thinking);
    state.record(HudEvent::ApprovalRequested);
    assert_snapshot!(
        "hud_awaiting_approval",
        render_widths(
            &state,
            /*task_running*/ true,
            t0 + Duration::from_secs(9)
        )
    );
}

fn sample_summary() -> TurnSummary {
    TurnSummary {
        tokens: usage(18_240, 12_032, 2_130, 640),
        tools: ToolCounts {
            commands: 4,
            edits: 2,
            mcp: 1,
            web: 0,
            other: 0,
        },
        plan: PlanProgress::from_update(&five_step_plan()),
        context: Some(ContextGauge {
            used_percent: 58,
            used_tokens: Some(148_000),
            window: Some(256_000),
        }),
        cost_usd: Some(0.0412),
    }
}

#[test]
fn turn_summary_cell_snapshot() {
    let cell = TurnSummaryCell::new(sample_summary(), Glyphs::Unicode, Palette::MONO);
    let rendered = [140u16, 100, 80, 50, 30]
        .iter()
        .map(|&width| {
            let line = cell.display_lines(width).into_iter().next();
            framed(line, width)
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_snapshot!("turn_summary_cell", rendered);
}

#[test]
fn turn_summary_raw_lines_are_ascii() {
    let cell = TurnSummaryCell::new(sample_summary(), Glyphs::Unicode, Palette::MONO);
    let raw = cell.raw_lines();
    assert_eq!(raw.len(), 1);
    assert!(line_text(&raw[0]).is_ascii(), "{:?}", line_text(&raw[0]));
    assert!(line_text(&raw[0]).contains("18.2K in"));
}

#[test]
fn stage_track_snapshot() {
    let stages = [
        ("plan", StageStatus::Done),
        ("build", StageStatus::Done),
        ("test", StageStatus::Running),
        ("review", StageStatus::Pending),
        ("ship", StageStatus::Skipped),
    ];
    let failed = [
        ("fetch", StageStatus::Done),
        ("render", StageStatus::Failed),
    ];
    let rendered = [
        line_text(&Line::from(stage_track(
            &stages,
            Glyphs::Unicode,
            Palette::MONO,
        ))),
        line_text(&Line::from(stage_track(
            &stages,
            Glyphs::Ascii,
            Palette::MONO,
        ))),
        line_text(&Line::from(stage_track(
            &failed,
            Glyphs::Unicode,
            Palette::MONO,
        ))),
    ]
    .join("\n");
    assert_snapshot!("stage_track", rendered);
}

#[test]
fn hud_hidden_when_disabled_or_empty() {
    let now = Instant::now();
    let mut state = HudState::default();
    state.set_context(Some(ContextGauge {
        used_percent: 10,
        used_tokens: None,
        window: None,
    }));
    assert_eq!(state.line(80, /*task_running*/ false, now), None);
    assert_eq!(state.row(false, now).desired_height(80), 0);

    state.begin_turn(now, TokenUsage::default());
    assert!(
        !state.record(HudEvent::Thinking),
        "a disabled HUD never asks for redraws"
    );

    let empty = HudState::new(HudConfig::default(), Glyphs::Unicode, Palette::MONO);
    assert_eq!(empty.line(80, /*task_running*/ false, now), None);

    let config = HudConfig {
        hud: false,
        ..HudConfig::default()
    };
    let mut off = session_state(Glyphs::Unicode);
    off.configure(config);
    assert_eq!(off.line(80, /*task_running*/ false, now), None);
}

#[test]
fn hud_individual_pieces_can_be_turned_off() {
    let t0 = Instant::now();
    let mut state = running_state(Glyphs::Unicode, t0);
    state.config = HudConfig {
        context_gauge: false,
        rate_limits: false,
        model_badge: false,
        activity: false,
        ..HudConfig::default()
    };
    let text = line_text(
        &state
            .line(140, /*task_running*/ true, t0)
            .expect("plan visible"),
    );
    assert!(text.contains("plan"), "{text}");
    for hidden in ["ctx", "5h", "Cursor", "running", "tools"] {
        assert!(!text.contains(hidden), "{hidden} should be hidden: {text}");
    }
    assert_eq!(state.row(true, t0).desired_height(140), 1);
}

#[test]
fn activity_only_shows_while_task_runs() {
    let t0 = Instant::now();
    let state = running_state(Glyphs::Unicode, t0);
    let idle = line_text(&state.line(140, /*task_running*/ false, t0).expect("line"));
    assert!(!idle.contains("running"), "{idle}");
    assert!(!idle.contains("tools"), "{idle}");
}

#[test]
fn finish_turn_reports_deltas_and_cost() {
    let t0 = Instant::now();
    let mut config = HudConfig::default();
    config.prices.insert(
        "composer-2.5".to_string(),
        ModelPrice {
            input: 1.0,
            cached_input: Some(0.1),
            output: 10.0,
        },
    );
    let mut state = HudState::new(config, Glyphs::Unicode, Palette::MONO);
    state.begin_turn(t0, usage(1_000, 0, 100, 0));
    state.record(HudEvent::WebSearchStarted);
    let summary = state
        .finish_turn(&usage(1_001_000, 500_000, 200_100, 50), "composer-2.5")
        .expect("summary");
    assert_eq!(summary.tokens, usage(1_000_000, 500_000, 200_000, 50));
    assert_eq!(summary.tools.web, 1);
    let cost = summary.cost_usd.expect("priced model");
    assert!((cost - (0.5 + 0.05 + 2.0)).abs() < 1e-9, "{cost}");

    state.begin_turn(t0, usage(5, 0, 5, 0));
    let unpriced = state
        .finish_turn(&usage(10, 0, 10, 0), "grok-4.7")
        .expect("summary");
    assert_eq!(unpriced.cost_usd, None);
}

#[test]
fn finish_turn_skips_empty_or_disabled_summaries() {
    let t0 = Instant::now();
    let mut state = HudState::new(HudConfig::default(), Glyphs::Unicode, Palette::MONO);
    assert_eq!(state.finish_turn(&TokenUsage::default(), "m"), None);
    state.begin_turn(t0, TokenUsage::default());
    assert_eq!(state.finish_turn(&TokenUsage::default(), "m"), None);

    let config = HudConfig {
        turn_summary: false,
        ..HudConfig::default()
    };
    let mut off = HudState::new(config, Glyphs::Unicode, Palette::MONO);
    off.begin_turn(t0, TokenUsage::default());
    assert_eq!(off.finish_turn(&usage(10, 0, 10, 0), "m"), None);
}

#[test]
fn completed_plan_clears_at_next_turn() {
    let t0 = Instant::now();
    let mut state = HudState::new(HudConfig::default(), Glyphs::Unicode, Palette::MONO);
    state.set_plan(&plan(&[("only", StepStatus::Completed)]));
    assert!(state.plan.is_some());
    state.begin_turn(t0, TokenUsage::default());
    assert_eq!(state.plan, None);

    state.set_plan(&plan(&[
        ("a", StepStatus::Completed),
        ("b", StepStatus::Pending),
    ]));
    state.begin_turn(t0, TokenUsage::default());
    assert_eq!(
        state.plan,
        Some(PlanProgress {
            completed: 1,
            total: 2,
            current: None,
        })
    );
    state.set_plan(&plan(&[]));
    assert_eq!(state.plan, None);
}
