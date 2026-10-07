//! Native Overmind pipelines.
//!
//! A pipeline is a directory with a `pipeline.yaml` stage contract and an optional
//! `PIPELINE.md` guide, in the format of Todd's gordon-workflows plugin (and the older Pi
//! library it was migrated from). This crate parses and validates definitions, discovers them
//! in the usual search directories, orders stages by their dependencies, binds run inputs, and
//! keeps resumable run state under `<workspace>/.overmind/runs/<run-id>/`.
//!
//! Execution itself is agent-orchestrated: the TUI turns each stage into one bounded agent turn
//! (see [`prompt`]), then records the stage from the outputs it declared. Nothing here talks to
//! a model or performs a side effect beyond writing run state.

pub mod definition;
pub mod discovery;
mod error;
mod evidence;
pub mod graph;
mod hashing;
pub mod inputs;
pub mod prompt;
pub mod run;
mod store;
#[cfg(test)]
mod test_fixtures;

pub use definition::DEFINITION_FILE;
pub use definition::GUIDE_FILE;
pub use definition::InputSpec;
pub use definition::InputType;
pub use definition::Pipeline;
pub use definition::PipelineDefinition;
pub use definition::Step;
pub use definition::StepKind;
pub use discovery::Discovery;
pub use discovery::discover;
pub use error::PipelineError;
pub use evidence::Evidence;
pub use evidence::EvidenceKind;
pub use run::NextAction;
pub use run::Run;
pub use run::StageOutcome;
pub use run::StageState;
pub use run::StageView;
pub use store::RunStore;
