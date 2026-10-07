//! Resumable run state under `<workspace>/.overmind/runs/<run-id>/`.
//!
//! A run directory holds `state.json` (inputs, per-stage records, history) and
//! `pipeline.snapshot.yaml`, the exact definition bytes the run started with. Runs are pinned
//! to their snapshot: editing the source pipeline does not change a run in flight (start a new
//! run to pick up changes), the same rule gordon-workflows' ledger applies.
//!
//! Stage states follow the ledger semantics: a finished stage is bound to the evidence of its
//! declared outputs; missing or changed evidence makes it stale, a stale stage invalidates
//! everything downstream, and re-running a stage resets its dependents. Approvals are recorded
//! per stage and cleared whenever the stage or an upstream stage is re-run.

use std::path::Path;
use std::path::PathBuf;

use chrono::Local;
use chrono::SecondsFormat;
use indexmap::IndexMap;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::Pipeline;
use crate::PipelineError;
use crate::evidence;
use crate::evidence::Evidence;
use crate::hashing::sha256_hex;
use crate::store::write_private;

/// Run state lives under this directory, relative to the workspace.
pub const RUNS_DIR: [&str; 2] = [".overmind", "runs"];
pub const STATE_FILE: &str = "state.json";
pub const SNAPSHOT_FILE: &str = "pipeline.snapshot.yaml";
pub const STATE_SCHEMA: &str = "overmind.pipeline-run/1";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StageState {
    #[default]
    Pending,
    Running,
    Done,
    Failed,
    Skipped,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageRecord {
    pub status: StageState,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<Evidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    pub event: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunState {
    pub schema_version: String,
    pub run_id: String,
    pub pipeline: String,
    pub pipeline_dir: PathBuf,
    pub definition_sha256: String,
    pub inputs: IndexMap<String, Value>,
    pub inputs_sha256: String,
    /// Where stage outputs are resolved (the project directory the run started in).
    pub workspace: PathBuf,
    pub created_at: String,
    pub updated_at: String,
    /// Why auto-advance stopped, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused: Option<String>,
    pub stages: IndexMap<String, StageRecord>,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
}

/// Effective state of a stage, combining its record with evidence and dependency checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageView {
    /// Waiting on unfinished dependencies.
    Pending,
    /// Dependencies are done; can run now.
    Ready,
    /// Dependencies are done; needs the user's approval first.
    AwaitingApproval,
    Running,
    Done,
    /// Was done, but its evidence changed or an upstream stage did.
    Stale,
    Failed,
    Skipped,
}

/// What resuming the run should do next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NextAction {
    /// Run stage `index`. `retry` is set for failed, stale or interrupted stages.
    Execute {
        index: usize,
        retry: bool,
    },
    AwaitApproval(usize),
    Complete,
    /// Nothing can run, for example because a dependency was skipped incorrectly.
    Blocked,
}

/// Result of recording a finished stage turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StageOutcome {
    /// Every declared output path exists. `unchecked` lists named deliverables that are not
    /// paths and so could not be verified on disk.
    Done {
        files: usize,
        unchecked: Vec<String>,
    },
    /// These declared outputs are missing or empty.
    MissingOutputs(Vec<String>),
}

pub(crate) fn now() -> String {
    Local::now().to_rfc3339_opts(SecondsFormat::Secs, false)
}

pub(crate) fn inputs_sha(inputs: &IndexMap<String, Value>) -> String {
    sha256_hex(serde_json::to_string(inputs).unwrap_or_default().as_bytes())
}

/// A loaded run: its directory, state and pinned pipeline snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub dir: PathBuf,
    pub state: RunState,
    pub pipeline: Pipeline,
}

impl Run {
    pub fn id(&self) -> &str {
        &self.state.run_id
    }

    pub fn record(&self, index: usize) -> Option<&StageRecord> {
        let step = self.pipeline.steps().get(index)?;
        self.state.stages.get(&step.id)
    }

    fn record_mut(&mut self, index: usize) -> Option<&mut StageRecord> {
        let id = self.pipeline.steps().get(index)?.id.clone();
        Some(self.state.stages.entry(id).or_default())
    }

    /// Effective view of every stage, by step index. Checks evidence on disk.
    pub fn views(&self) -> Vec<StageView> {
        let steps = self.pipeline.steps();
        let mut views = vec![StageView::Pending; steps.len()];
        for &index in self.pipeline.order() {
            let deps = self.pipeline.dependencies(index);
            let deps_done = deps
                .iter()
                .all(|dep| matches!(views[*dep], StageView::Done | StageView::Skipped));
            let record = self.record(index).cloned().unwrap_or_default();
            views[index] = match record.status {
                StageState::Done => {
                    let intact = record
                        .evidence
                        .iter()
                        .all(|item| evidence::still_valid(&self.state.workspace, item));
                    if deps_done && intact {
                        StageView::Done
                    } else {
                        StageView::Stale
                    }
                }
                StageState::Skipped => StageView::Skipped,
                StageState::Running => StageView::Running,
                StageState::Failed => StageView::Failed,
                StageState::Pending if !deps_done => StageView::Pending,
                StageState::Pending
                    if steps[index].needs_approval() && record.approved_at.is_none() =>
                {
                    StageView::AwaitingApproval
                }
                StageState::Pending => StageView::Ready,
            };
        }
        views
    }

    pub fn next_action(&self) -> NextAction {
        self.next_action_from(&self.views())
    }

    pub fn next_action_from(&self, views: &[StageView]) -> NextAction {
        for &index in self.pipeline.order() {
            match views[index] {
                StageView::Done | StageView::Skipped | StageView::Pending => {}
                StageView::Ready => {
                    return NextAction::Execute {
                        index,
                        retry: false,
                    };
                }
                StageView::AwaitingApproval => return NextAction::AwaitApproval(index),
                StageView::Running | StageView::Failed | StageView::Stale => {
                    return NextAction::Execute { index, retry: true };
                }
            }
        }
        if views
            .iter()
            .all(|view| matches!(view, StageView::Done | StageView::Skipped))
        {
            NextAction::Complete
        } else {
            NextAction::Blocked
        }
    }

    /// Recorded progress (no disk checks): finished stages and the total.
    pub fn progress(&self) -> (usize, usize) {
        let done = self
            .state
            .stages
            .values()
            .filter(|record| matches!(record.status, StageState::Done | StageState::Skipped))
            .count();
        (done, self.pipeline.steps().len())
    }

    /// Declared output paths of stage `index` that are missing or empty right now.
    pub fn missing_outputs(&self, index: usize) -> Vec<String> {
        let Some(step) = self.pipeline.steps().get(index) else {
            return Vec::new();
        };
        step.output_paths()
            .filter(|output| evidence::collect(&self.state.workspace, output).is_none())
            .map(str::to_string)
            .collect()
    }

    /// Mark stage `index` running. Re-running a finished stage resets its dependents.
    pub fn begin_stage(&mut self, index: usize) {
        let rerun = self
            .record(index)
            .is_some_and(|record| record.status != StageState::Pending);
        if rerun {
            self.reset_stages(&self.pipeline.downstream(index), "reset by upstream re-run");
        }
        let at = now();
        if let Some(record) = self.record_mut(index) {
            record.status = StageState::Running;
            record.attempts += 1;
            record.started_at = Some(at);
            record.finished_at = None;
            record.note = None;
            record.missing.clear();
            record.evidence.clear();
        }
        self.state.paused = None;
        self.log(Some(index), "started");
    }

    /// Record the end of a stage turn from its declared outputs.
    pub fn complete_stage(&mut self, index: usize) -> StageOutcome {
        let Some(step) = self.pipeline.steps().get(index).cloned() else {
            return StageOutcome::MissingOutputs(Vec::new());
        };
        let mut found = Vec::new();
        let mut missing = Vec::new();
        for output in step.output_paths() {
            match evidence::collect(&self.state.workspace, output) {
                Some(item) => found.push(item),
                None => missing.push(output.to_string()),
            }
        }
        let unchecked: Vec<String> = step
            .outputs
            .iter()
            .filter(|output| !crate::definition::is_output_path(output))
            .cloned()
            .collect();
        let at = now();
        let files = found.len();
        let outcome = if missing.is_empty() {
            StageOutcome::Done { files, unchecked }
        } else {
            StageOutcome::MissingOutputs(missing.clone())
        };
        if let Some(record) = self.record_mut(index) {
            record.finished_at = Some(at);
            if missing.is_empty() {
                record.status = StageState::Done;
                record.evidence = found;
                record.missing.clear();
                record.note = None;
            } else {
                record.status = StageState::Failed;
                record.evidence.clear();
                record.note = Some("missing outputs".to_string());
                record.missing = missing;
            }
        }
        let event = match &outcome {
            StageOutcome::Done { .. } => "done".to_string(),
            StageOutcome::MissingOutputs(missing) => format!("missing {}", missing.join(", ")),
        };
        self.log(Some(index), &event);
        outcome
    }

    pub fn fail_stage(&mut self, index: usize, note: &str) {
        let at = now();
        if let Some(record) = self.record_mut(index) {
            record.status = StageState::Failed;
            record.finished_at = Some(at);
            record.note = Some(note.to_string());
        }
        self.log(Some(index), &format!("failed: {note}"));
    }

    pub fn approve(&mut self, index: usize) {
        let at = now();
        if let Some(record) = self.record_mut(index) {
            record.approved_at = Some(at);
        }
        self.log(Some(index), "approved by the user");
    }

    /// Reset stage `index` and everything downstream so they run again. Returns the reset
    /// stage ids in execution order.
    pub fn rerun_from(&mut self, index: usize) -> Vec<String> {
        let mut affected = vec![index];
        affected.extend(self.pipeline.downstream(index));
        self.reset_stages(&affected, "rerun requested");
        self.state.paused = None;
        affected
            .iter()
            .filter_map(|index| self.pipeline.steps().get(*index))
            .map(|step| step.id.clone())
            .collect()
    }

    fn reset_stages(&mut self, indices: &[usize], reason: &str) {
        for index in indices {
            let changed = self
                .record(*index)
                .is_some_and(|record| *record != StageRecord::default());
            if !changed {
                continue;
            }
            if let Some(record) = self.record_mut(*index) {
                let attempts = record.attempts;
                *record = StageRecord {
                    attempts,
                    ..StageRecord::default()
                };
            }
            self.log(Some(*index), reason);
        }
    }

    pub fn pause(&mut self, reason: &str) {
        self.state.paused = Some(reason.to_string());
        self.log(None, &format!("paused: {reason}"));
    }

    /// Whether the source `pipeline.yaml` differs from this run's snapshot (or is gone).
    pub fn source_changed(&self) -> bool {
        Pipeline::load(&self.state.pipeline_dir).map_or(true, |current| {
            current.sha256 != self.state.definition_sha256
        })
    }

    pub(crate) fn log(&mut self, index: Option<usize>, event: &str) {
        let stage = index
            .and_then(|index| self.pipeline.steps().get(index))
            .map(|step| step.id.clone());
        self.state.history.push(HistoryEntry {
            at: now(),
            stage,
            event: event.to_string(),
        });
    }

    /// Write `state.json` atomically.
    pub fn save(&mut self) -> Result<(), PipelineError> {
        self.state.updated_at = now();
        let json = serde_json::to_string_pretty(&self.state)
            .map_err(|err| PipelineError::Run(format!("cannot serialize run state: {err}")))?;
        let path = self.dir.join(STATE_FILE);
        let temp = self
            .dir
            .join(format!(".{STATE_FILE}.{}.tmp", std::process::id()));
        write_private(&temp, format!("{json}\n").as_bytes())?;
        std::fs::rename(&temp, &path).map_err(|err| PipelineError::io(&path, err))
    }

    /// Load a run directory, checking the snapshot and inputs against their recorded hashes.
    pub fn load(dir: &Path) -> Result<Self, PipelineError> {
        let state_path = dir.join(STATE_FILE);
        let text = std::fs::read_to_string(&state_path)
            .map_err(|err| PipelineError::io(&state_path, err))?;
        let state: RunState = serde_json::from_str(&text).map_err(|err| PipelineError::Parse {
            path: state_path.clone(),
            message: err.to_string(),
        })?;
        if state.schema_version != STATE_SCHEMA {
            return Err(PipelineError::Run(format!(
                "{}: unsupported run state version {}",
                state_path.display(),
                state.schema_version
            )));
        }
        let snapshot_path = dir.join(SNAPSHOT_FILE);
        let bytes =
            std::fs::read(&snapshot_path).map_err(|err| PipelineError::io(&snapshot_path, err))?;
        let pipeline = Pipeline::from_bytes(&bytes, &state.pipeline_dir, &snapshot_path)?;
        if pipeline.sha256 != state.definition_sha256 {
            return Err(PipelineError::Run(format!(
                "{}: pipeline snapshot changed; start a new run",
                dir.display()
            )));
        }
        if inputs_sha(&state.inputs) != state.inputs_sha256 {
            return Err(PipelineError::Run(format!(
                "{}: run inputs changed; start a new run",
                dir.display()
            )));
        }
        Ok(Self {
            dir: dir.to_path_buf(),
            state,
            pipeline,
        })
    }
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
