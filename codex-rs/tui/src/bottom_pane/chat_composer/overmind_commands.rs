//! Overmind: keep custom slash commands in sync with the command files on disk.
//!
//! While the user edits a `/name` token the composer compares a cheap fingerprint of the command
//! directories (file names, sizes and modification times) and rediscovers only when it changed,
//! so new or edited files in `$CODEX_HOME/commands` or a trusted project's `.codex/commands` show
//! up without a restart. A slash submission and `/commands` always rescan fully.

use std::sync::Arc;

use crate::app_event::AppEvent;
use crate::history_cell;
use crate::overmind::custom_commands::CommandDirsFingerprint;
use crate::overmind::custom_commands::CustomCommand;
use crate::overmind::custom_commands::CustomCommandDiscovery;
use crate::overmind::custom_commands::CustomCommandEnv;
use crate::overmind::custom_commands::command_dirs_fingerprint;
use crate::overmind::custom_commands::discover_custom_commands;

use super::ChatComposer;

/// Where custom commands come from, plus the directory state of the last scan.
#[derive(Debug)]
pub(super) struct CustomCommandWatch {
    env: CustomCommandEnv,
    fingerprint: Option<CommandDirsFingerprint>,
}

impl ChatComposer {
    /// Set where custom commands are discovered. `None` disables rescanning.
    pub(crate) fn set_custom_commands_env(&mut self, env: Option<CustomCommandEnv>) {
        self.custom_commands_watch = env.map(|env| {
            Box::new(CustomCommandWatch {
                env,
                fingerprint: None,
            })
        });
    }

    /// Replace the custom commands and stop rescanning, so tests control the exact list.
    /// Returns whether anything changed.
    #[cfg(test)]
    pub(crate) fn set_custom_commands(&mut self, discovery: CustomCommandDiscovery) -> bool {
        self.custom_commands_watch = None;
        if self.custom_commands == discovery {
            return false;
        }
        self.custom_commands = discovery;
        self.sync_popups();
        true
    }

    pub(crate) fn custom_commands(&self) -> &[Arc<CustomCommand>] {
        &self.custom_commands.commands
    }

    pub(crate) fn custom_command_discovery(&self) -> &CustomCommandDiscovery {
        &self.custom_commands
    }

    /// Rediscover only when the command directories changed since the last scan.
    pub(crate) fn refresh_custom_commands_if_changed(&mut self) -> bool {
        let Some(watch) = &self.custom_commands_watch else {
            return false;
        };
        if watch.fingerprint.as_ref() == Some(&command_dirs_fingerprint(&watch.env)) {
            return false;
        }
        self.refresh_custom_commands()
    }

    /// Rescan the command directories. Warnings that were not reported by the previous scan are
    /// added to the transcript. Returns whether the discovered commands or warnings changed.
    ///
    /// This does not resync popups, so it is safe to call while a popup is being rebuilt.
    pub(crate) fn refresh_custom_commands(&mut self) -> bool {
        let Some(watch) = &mut self.custom_commands_watch else {
            return false;
        };
        watch.fingerprint = Some(command_dirs_fingerprint(&watch.env));
        let discovery = discover_custom_commands(&watch.env);
        if discovery == self.custom_commands {
            return false;
        }
        let new_warnings: Vec<String> = discovery
            .warnings
            .iter()
            .filter(|warning| !self.custom_commands.warnings.contains(warning))
            .cloned()
            .collect();
        self.custom_commands = discovery;
        for warning in new_warnings {
            self.app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
                history_cell::new_warning_event(warning),
            )));
        }
        true
    }
}

#[cfg(test)]
#[path = "overmind_commands_tests.rs"]
mod tests;
