//! Mapping run state to HUD progress and stage glyphs.

use codex_overmind_pipelines::Run;
use codex_overmind_pipelines::StageState;
use codex_overmind_pipelines::StageView;

use crate::overmind::hud::PipelineProgress;
use crate::overmind::hud::meter::StageStatus;

/// A failed-for-missing-outputs or interrupted-by-exit stage whose outputs now all exist.
pub(super) fn recoverable_from_outputs(run: &Run, index: usize) -> bool {
    let Some(record) = run.record(index) else {
        return false;
    };
    let eligible = match record.status {
        StageState::Running => true,
        StageState::Failed => record.note.as_deref() == Some("missing outputs"),
        StageState::Pending | StageState::Done | StageState::Skipped => false,
    };
    eligible
        && run.pipeline.steps()[index].output_paths().next().is_some()
        && run.missing_outputs(index).is_empty()
}

pub(super) fn stage_status(view: StageView) -> StageStatus {
    match view {
        StageView::Pending | StageView::Ready => StageStatus::Pending,
        StageView::AwaitingApproval => StageStatus::Waiting,
        StageView::Running => StageStatus::Running,
        StageView::Done => StageStatus::Done,
        StageView::Stale => StageStatus::Stale,
        StageView::Failed => StageStatus::Failed,
        StageView::Skipped => StageStatus::Skipped,
    }
}

/// HUD progress for `run`.
pub(super) fn progress(run: &Run, views: &[StageView]) -> Option<PipelineProgress> {
    let steps = run.pipeline.steps();
    let stages: Vec<(String, StageStatus)> = run
        .pipeline
        .order()
        .iter()
        .map(|index| (steps[*index].id.clone(), stage_status(views[*index])))
        .collect();
    let current = run
        .pipeline
        .order()
        .iter()
        .find(|index| {
            !matches!(
                views[**index],
                StageView::Done | StageView::Skipped | StageView::Pending
            )
        })
        .map(|index| steps[*index].id.clone());
    Some(PipelineProgress {
        name: run.state.pipeline.clone(),
        stages,
        current,
    })
}
