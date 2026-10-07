//! Live turn activity: what the agent is doing right now and which tools it has used.

use std::time::Duration;
use std::time::Instant;

use crate::token_usage::TokenUsage;

/// The current phase of a running turn, as shown by the HUD.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Starting,
    Thinking,
    Responding,
    Running(String),
    Editing(usize),
    Calling(String),
    SearchingWeb,
    GeneratingImage,
    AwaitingApproval,
}

impl Phase {
    pub(crate) fn verb(&self) -> &'static str {
        match self {
            Phase::Starting => "starting",
            Phase::Thinking => "thinking",
            Phase::Responding => "responding",
            Phase::Running(_) => "running",
            Phase::Editing(_) => "editing",
            Phase::Calling(_) => "calling",
            Phase::SearchingWeb => "searching the web",
            Phase::GeneratingImage => "generating an image",
            Phase::AwaitingApproval => "awaiting approval",
        }
    }

    /// A few columns for narrow terminals.
    pub(crate) fn short_verb(&self) -> &'static str {
        match self {
            Phase::Starting => "start",
            Phase::Thinking => "think",
            Phase::Responding => "write",
            Phase::Running(_) => "sh",
            Phase::Editing(_) => "edit",
            Phase::Calling(_) => "tool",
            Phase::SearchingWeb => "web",
            Phase::GeneratingImage => "image",
            Phase::AwaitingApproval => "approve?",
        }
    }

    pub(crate) fn detail(&self) -> Option<String> {
        match self {
            Phase::Running(command) => Some(command.clone()),
            Phase::Editing(1) => Some("1 file".to_string()),
            Phase::Editing(files) => Some(format!("{files} files")),
            Phase::Calling(tool) => Some(tool.clone()),
            Phase::Starting
            | Phase::Thinking
            | Phase::Responding
            | Phase::SearchingWeb
            | Phase::GeneratingImage
            | Phase::AwaitingApproval => None,
        }
    }

    pub(crate) fn needs_attention(&self) -> bool {
        matches!(self, Phase::AwaitingApproval)
    }
}

/// Tool calls made during a turn, by kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ToolCounts {
    pub(crate) commands: u32,
    pub(crate) edits: u32,
    pub(crate) mcp: u32,
    pub(crate) web: u32,
    pub(crate) other: u32,
}

impl ToolCounts {
    pub(crate) fn total(&self) -> u32 {
        self.commands + self.edits + self.mcp + self.web + self.other
    }

    /// Non-zero counts with short labels, in a stable order.
    pub(crate) fn breakdown(&self) -> Vec<(u32, &'static str)> {
        [
            (self.commands, "sh"),
            (self.edits, "edit"),
            (self.mcp, "mcp"),
            (self.web, "web"),
            (self.other, "other"),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .collect()
    }
}

/// Events the ChatWidget forwards to the HUD.
#[derive(Clone, Copy, Debug)]
pub(crate) enum HudEvent<'a> {
    Thinking,
    Responding,
    CommandStarted(&'a str),
    PatchStarted { files: usize },
    McpStarted { server: &'a str, tool: &'a str },
    DynamicToolStarted(&'a str),
    WebSearchStarted,
    ImageGenerationStarted,
    ToolFinished,
    ApprovalRequested,
}

/// Activity for the turn in flight.
#[derive(Clone, Debug)]
pub(crate) struct TurnActivity {
    pub(crate) started_at: Instant,
    pub(crate) phase: Phase,
    pub(crate) counts: ToolCounts,
    pub(crate) start_usage: TokenUsage,
}

impl TurnActivity {
    pub(crate) fn new(started_at: Instant, start_usage: TokenUsage) -> Self {
        Self {
            started_at,
            phase: Phase::Starting,
            counts: ToolCounts::default(),
            start_usage,
        }
    }

    pub(crate) fn elapsed(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.started_at)
    }

    pub(crate) fn apply(&mut self, event: HudEvent<'_>) {
        match event {
            HudEvent::Thinking => self.set_phase(Phase::Thinking),
            HudEvent::Responding => self.set_phase(Phase::Responding),
            HudEvent::CommandStarted(command) => {
                self.counts.commands += 1;
                self.phase = Phase::Running(display_command(command));
            }
            HudEvent::PatchStarted { files } => {
                self.counts.edits += 1;
                self.phase = Phase::Editing(files);
            }
            HudEvent::McpStarted { server, tool } => {
                self.counts.mcp += 1;
                self.phase = Phase::Calling(format!("{server}.{tool}"));
            }
            HudEvent::DynamicToolStarted(tool) => {
                self.counts.other += 1;
                self.phase = Phase::Calling(tool.to_string());
            }
            HudEvent::WebSearchStarted => {
                self.counts.web += 1;
                self.phase = Phase::SearchingWeb;
            }
            HudEvent::ImageGenerationStarted => {
                self.counts.other += 1;
                self.phase = Phase::GeneratingImage;
            }
            HudEvent::ToolFinished => self.set_phase(Phase::Thinking),
            HudEvent::ApprovalRequested => self.set_phase(Phase::AwaitingApproval),
        }
    }

    fn set_phase(&mut self, phase: Phase) {
        if self.phase != phase {
            self.phase = phase;
        }
    }
}

/// One-line command for display: strips a `bash -lc '…'` wrapper and collapses whitespace.
pub(crate) fn display_command(command: &str) -> String {
    let mut text = command.trim();
    for prefix in [
        "/bin/bash -lc ",
        "bash -lc ",
        "/bin/zsh -lc ",
        "zsh -lc ",
        "sh -c ",
    ] {
        if let Some(rest) = text.strip_prefix(prefix) {
            text = rest.trim();
            break;
        }
    }
    let unquoted = text
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        .or_else(|| {
            text.strip_prefix('"')
                .and_then(|rest| rest.strip_suffix('"'))
        })
        .unwrap_or(text);
    unquoted.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Compact elapsed time: `<1s`, `42s`, `3m 07s`, `1h 02m`.
pub(crate) fn format_elapsed(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    let (hours, minutes, seconds) = (secs / 3_600, (secs % 3_600) / 60, secs % 60);
    if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else if seconds == 0 {
        "<1s".to_string()
    } else {
        format!("{seconds}s")
    }
}
