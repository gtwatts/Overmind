//! Overmind extensions to the Codex TUI.
//!
//! Overmind is a private fork of Codex. To keep rebasing onto upstream cheap, Overmind-specific
//! logic lives in this module tree and upstream files only receive small, clearly marked hooks
//! that call into it. See `OVERMIND.md` at the repository root for the roadmap.

pub(crate) mod custom_commands;
