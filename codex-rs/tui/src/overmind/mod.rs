//! Overmind extensions to the Codex TUI.
//!
//! Overmind is a private fork of Codex. To keep rebasing onto upstream cheap, Overmind-specific
//! logic lives in this module tree and upstream files only receive small, clearly marked hooks
//! that call into it. See `OVERMIND.md` at the repository root for the roadmap.

pub(crate) mod custom_commands;
pub(crate) mod expansion;
pub(crate) mod listing;
pub(crate) mod skill_refs;

/// Whether this provider lives only in Overmind's core and so must be served by the embedded
/// app server. The shared background daemon is the stock Codex binary, which lacks it.
pub(crate) fn requires_embedded_server(model_provider_id: &str) -> bool {
    model_provider_id == codex_overmind_cursor::CURSOR_PROVIDER_ID
}
