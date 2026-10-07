//! `/pipeline`: run gordon-workflows pipelines natively, one agent turn per stage.
//!
//! Parsing, ordering and run state live in the `codex-overmind-pipelines` crate. This module
//! turns slash commands and turn lifecycle events into effects (transcript lines, a stage turn
//! to submit, HUD progress) that `chatwidget/overmind_pipelines.rs` applies:
//!
//! - `run <name> [inputs]` binds inputs, creates `.overmind/runs/<run-id>/` in the working
//!   directory and submits the first stage. When a stage turn ends, its declared outputs decide
//!   whether it is done; then the next stage starts automatically, unless it needs approval
//!   (`/pipeline approve`), outputs are missing, the turn was interrupted, or `/pipeline stop`
//!   was used. `run` alone resumes; `rerun` resets stages or repeats a run.
//! - Progress feeds the HUD stage track; `status` and `inspect` render the full picture.

mod command;
pub(crate) mod config;
mod controller;
mod inspect;
mod progress;
mod render;

pub(crate) use command::parse_pipeline_command;
pub(crate) use controller::PipelineContext;
pub(crate) use controller::PipelineEffect;
pub(crate) use controller::PipelineSession;

#[cfg(test)]
#[path = "pipelines_tests.rs"]
mod tests;
