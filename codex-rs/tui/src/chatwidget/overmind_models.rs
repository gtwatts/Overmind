//! Overmind: cross-provider model choices from the `/model` picker.

use codex_protocol::openai_models::ReasoningEffort;

use super::ChatWidget;

impl ChatWidget {
    /// Remembers a picker choice that needs a fresh session on another provider.
    pub(crate) fn set_overmind_next_session_model(
        &mut self,
        model: String,
        effort: Option<ReasoningEffort>,
    ) {
        self.overmind_next_session_model = Some((model, effort));
    }

    /// Takes the pending choice so it applies to exactly one new session.
    pub(crate) fn take_overmind_next_session_model(
        &mut self,
    ) -> crate::overmind::models::NextSessionModel {
        self.overmind_next_session_model.take()
    }
}
