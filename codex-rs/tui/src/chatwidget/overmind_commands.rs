//! Overmind custom slash commands: discovery and submission-time expansion.
//!
//! Custom commands reach `ChatWidget` as ordinary submitted text (`/name args`). This module
//! loads the command definitions into the composer and rewrites matching submissions into the
//! expanded user turn before they are sent or queued. Built-in and service-tier commands always
//! take precedence because resolution goes through `find_slash_command`.

use super::*;
use crate::bottom_pane::prompt_args::parse_slash_name;
use crate::bottom_pane::slash_commands::SlashCommandItem;
use crate::bottom_pane::slash_commands::find_slash_command;
use crate::overmind::custom_commands::CustomCommandEnv;
use crate::overmind::custom_commands::ProjectTrust;
use crate::overmind::custom_commands::discover_custom_commands;
use crate::overmind::custom_commands::expand_custom_command;

impl ChatWidget {
    /// Reload custom commands for the current cwd and surface new discovery warnings.
    pub(super) fn sync_custom_commands(&mut self) {
        let discovery = discover_custom_commands(&self.custom_command_env());
        let warnings = discovery.warnings.clone();
        if self.bottom_pane.set_custom_commands(discovery) {
            for warning in warnings {
                self.add_warning_message(warning);
            }
        }
    }

    fn custom_command_env(&self) -> CustomCommandEnv {
        let trust = if self.config.active_project.is_trusted() {
            ProjectTrust::Trusted
        } else {
            ProjectTrust::NotTrusted
        };
        CustomCommandEnv::resolve(
            self.config.codex_home.as_path(),
            self.config.cwd.as_path(),
            trust,
        )
    }

    /// Expand `/name args` when `name` resolves to a custom command; otherwise return the
    /// message unchanged.
    pub(super) fn expand_custom_command_submission(
        &self,
        user_message: UserMessage,
    ) -> UserMessage {
        let Some((name, args, _)) = parse_slash_name(&user_message.text) else {
            return user_message;
        };
        if name.contains('/') {
            return user_message;
        }
        let Some(SlashCommandItem::Custom(command)) = find_slash_command(
            name,
            self.builtin_command_flags(),
            &self.current_model_service_tier_commands(),
            self.bottom_pane.custom_commands(),
        ) else {
            return user_message;
        };
        let text = expand_custom_command(&command, args, &self.custom_command_env());
        // Element ranges refer to the typed text, so they cannot survive the rewrite.
        UserMessage {
            text,
            text_elements: Vec::new(),
            ..user_message
        }
    }
}
