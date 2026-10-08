//! The Overmind HUD row inside the real bottom-pane layout.

use super::*;
use crate::overmind::hud::ContextGauge;
use crate::overmind::hud::HudEvent;
use crate::overmind::hud::HudState;
use crate::overmind::hud::ModelBadge;
use crate::overmind::hud::PipelineProgress;
use crate::overmind::hud::PipelineStageDetail;
use crate::overmind::hud::config::HudConfig;
use crate::overmind::hud::meter::Glyphs;
use crate::overmind::hud::meter::Palette;
use crate::overmind::hud::meter::StageStatus;
use crate::token_usage::TokenUsage;
use codex_protocol::plan_tool::PlanItemArg;
use codex_protocol::plan_tool::StepStatus;
use codex_protocol::plan_tool::UpdatePlanArgs;
use pretty_assertions::assert_eq;

fn hud_pane() -> BottomPane {
    let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
    let mut pane = test_pane(AppEventSender::new(tx_raw));
    let mut hud = HudState::new(HudConfig::default(), Glyphs::Unicode, Palette::MONO);
    hud.set_context(Some(ContextGauge {
        used_percent: 42,
        used_tokens: Some(107_000),
        window: Some(256_000),
    }));
    hud.set_plan(&UpdatePlanArgs {
        explanation: None,
        plan: vec![
            PlanItemArg {
                step: "Add HUD widgets".to_string(),
                status: StepStatus::Completed,
            },
            PlanItemArg {
                step: "Wire the bottom pane".to_string(),
                status: StepStatus::InProgress,
            },
            PlanItemArg {
                step: "Snapshot tests".to_string(),
                status: StepStatus::Pending,
            },
        ],
    });
    hud.set_badge(Some(ModelBadge {
        provider: "Cursor".to_string(),
        model: "composer-2.5".to_string(),
    }));
    pane.overmind_hud = hud;
    pane
}

fn render(pane: &BottomPane, width: u16) -> String {
    let height = pane.desired_height(width);
    render_snapshot(pane, Rect::new(0, 0, width, height))
}

#[test]
fn hud_row_sits_above_the_idle_composer() {
    let pane = hud_pane();
    assert_snapshot!(
        "overmind_hud_idle_composer",
        format!("{}\n\n{}", render(&pane, 100), render(&pane, 56))
    );
}

#[test]
fn hud_row_tracks_a_running_turn() {
    let mut pane = hud_pane();
    pane.set_task_running(/*running*/ true);
    // A start time in the future pins the elapsed time at "<1s" for a stable snapshot.
    pane.overmind_hud.begin_turn(
        Instant::now() + Duration::from_secs(3_600),
        TokenUsage::default(),
    );
    pane.overmind_hud
        .record(HudEvent::CommandStarted("bash -lc 'just fmt'"));
    pane.overmind_hud.record(HudEvent::ToolFinished);
    pane.overmind_hud
        .record(HudEvent::PatchStarted { files: 2 });
    assert_snapshot!("overmind_hud_running_turn", render(&pane, 100));
}

#[test]
fn disabled_hud_adds_no_rows() {
    let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
    let plain = test_pane(AppEventSender::new(tx_raw));
    let mut configured = hud_pane();
    configured.overmind_hud = HudState::default();
    assert_eq!(configured.desired_height(80), plain.desired_height(80));
    assert_eq!(hud_pane().desired_height(80), plain.desired_height(80) + 1);
}

#[test]
fn pipeline_panel_yields_height_to_keep_the_composer_visible() {
    let mut pane = hud_pane();
    let stages: Vec<_> = (0..6)
        .map(|index| (format!("stage-{index}"), StageStatus::Pending))
        .collect();
    let details = stages
        .iter()
        .map(|(id, status)| PipelineStageDetail {
            id: id.clone(),
            status: *status,
            started_at_ms: None,
            finished_at_ms: None,
            expected: vec![format!("production/{id}.mp4")],
            recorded: Vec::new(),
        })
        .collect();
    pane.overmind_hud.set_pipeline(Some(PipelineProgress {
        name: "video".into(),
        stages,
        details,
        current: Some("stage-2".into()),
        paused: None,
        paused_at_ms: None,
    }));
    for width in [24, 40, 80, 120] {
        let composer_height = pane.composer.desired_height(width);
        for height in composer_height..=composer_height + 7 {
            let area = Rect::new(0, 0, width, height);
            let text = render_snapshot(&pane, area);
            assert!(text.contains("Ask Codex"), "{width}x{height}: {text}");
            let (x, y) = pane
                .cursor_pos(area)
                .expect("composer cursor remains visible");
            assert!(x < width && y < height);
        }
    }
}
