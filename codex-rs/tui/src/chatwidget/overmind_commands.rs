//! Overmind custom slash commands: discovery, `/commands`, and submission-time expansion.
//!
//! Custom commands reach `ChatWidget` as ordinary submitted text (`/name args`). This module
//! points the composer at the command directories (the composer rescans them when the slash
//! popup opens) and rewrites matching submissions into the expanded user turn before they are
//! sent or queued. Built-in and service-tier commands always take precedence because resolution
//! goes through `find_slash_command`.

use super::*;
use crate::bottom_pane::prompt_args::parse_slash_name;
use crate::bottom_pane::slash_commands::SlashCommandItem;
use crate::bottom_pane::slash_commands::find_slash_command;
use crate::overmind::custom_commands::CustomCommandEnv;
use crate::overmind::custom_commands::ProjectTrust;
use crate::overmind::expansion::expand_custom_command;
use crate::overmind::listing::render_command_listing;
use crate::overmind::skill_refs::AvailableSkill;

impl ChatWidget {
    /// Point custom command discovery at the current cwd and rescan.
    pub(super) fn sync_custom_commands(&mut self) {
        let env = self.custom_command_env();
        self.bottom_pane.set_custom_commands_env(env);
    }

    /// `/commands`: rescan, then list custom commands, their skills, and skipped files.
    pub(super) fn show_custom_commands(&mut self) {
        self.bottom_pane.refresh_custom_commands();
        let available = self.available_skills_for_commands();
        let lines = render_command_listing(
            self.bottom_pane.custom_command_discovery(),
            &self.custom_command_env(),
            available.as_deref(),
        );
        self.add_plain_history_lines(lines);
    }

    pub(super) fn custom_command_env(&self) -> CustomCommandEnv {
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

    /// Enabled skills from the loaded skills list, or `None` before it has loaded.
    pub(super) fn available_skills_for_commands(&self) -> Option<Vec<AvailableSkill>> {
        self.bottom_pane.skills().map(|skills| {
            skills
                .iter()
                .filter(|skill| skill.enabled)
                .map(|skill| AvailableSkill {
                    name: skill.name.clone(),
                    path: skill.path.as_str().to_string(),
                })
                .collect()
        })
    }

    /// Expand `/name args` when `name` resolves to a custom command; otherwise return the
    /// message unchanged. Skills named by the command are attached through mention bindings,
    /// which submission turns into structured skill inputs.
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
        let available = self.available_skills_for_commands();
        let expanded = expand_custom_command(
            &command,
            args,
            &self.custom_command_env(),
            available.as_deref(),
        );
        let mut mention_bindings = user_message.mention_bindings;
        mention_bindings.extend(expanded.skills.into_iter().map(|skill| MentionBinding {
            sigil: '$',
            mention: skill.name,
            path: skill.path,
        }));
        // Element ranges refer to the typed text, so they cannot survive the rewrite.
        UserMessage {
            text: expanded.text,
            text_elements: Vec::new(),
            mention_bindings,
            ..user_message
        }
    }
}
