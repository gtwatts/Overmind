//! Live turn activity: what the agent is doing right now and which tools it has used.

use std::collections::HashMap;
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
    PatchStarted {
        files: usize,
    },
    McpStarted {
        id: &'a str,
        server: &'a str,
        tool: &'a str,
    },
    McpFinished(&'a str),
    McpProgress {
        id: &'a str,
        progress: Option<f64>,
        total: Option<f64>,
        message: &'a str,
    },
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
    mcp_calls: HashMap<String, String>,
    mcp_phase_call: Option<String>,
    mcp_reports: HashMap<String, ToolProgress>,
    mcp_report_order: Vec<String>,
    pub(crate) progress: Option<ToolProgress>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ToolProgress {
    pub(crate) id: String,
    pub(crate) tool: String,
    pub(crate) progress: Option<f64>,
    pub(crate) total: Option<f64>,
    pub(crate) message: String,
}

impl TurnActivity {
    pub(crate) fn new(started_at: Instant, start_usage: TokenUsage) -> Self {
        Self {
            started_at,
            phase: Phase::Starting,
            counts: ToolCounts::default(),
            start_usage,
            mcp_calls: HashMap::new(),
            mcp_phase_call: None,
            mcp_reports: HashMap::new(),
            mcp_report_order: Vec::new(),
            progress: None,
        }
    }

    pub(crate) fn elapsed(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.started_at)
    }

    /// Apply an event; returns whether anything visible changed.
    pub(crate) fn apply(&mut self, event: HudEvent<'_>) -> bool {
        let before = (self.phase.clone(), self.counts, self.progress.clone());
        if !matches!(
            event,
            HudEvent::McpStarted { .. } | HudEvent::McpFinished(_) | HudEvent::McpProgress { .. }
        ) {
            self.mcp_phase_call = None;
        }
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
            HudEvent::McpStarted { id, server, tool } => {
                let label = super::pipeline_panel::inline(&format!("{server}.{tool}"));
                if self
                    .mcp_calls
                    .insert(id.to_string(), label.clone())
                    .is_none()
                {
                    self.counts.mcp += 1;
                }
                self.phase = Phase::Calling(label);
                self.mcp_phase_call = Some(id.to_string());
            }
            HudEvent::McpFinished(id) => {
                if self.mcp_calls.remove(id).is_some() {
                    self.mcp_reports.remove(id);
                    self.mcp_report_order.retain(|call| call != id);
                    if self
                        .progress
                        .as_ref()
                        .is_some_and(|progress| progress.id == id)
                    {
                        self.progress = self
                            .mcp_report_order
                            .last()
                            .and_then(|id| self.mcp_reports.get(id))
                            .cloned();
                    }
                    if self.mcp_phase_call.as_deref() == Some(id) {
                        let remaining = self
                            .progress
                            .as_ref()
                            .map(|report| (report.id.clone(), report.tool.clone()))
                            .or_else(|| {
                                self.mcp_calls
                                    .iter()
                                    .min_by_key(|(id, _)| *id)
                                    .map(|(id, tool)| (id.clone(), tool.clone()))
                            });
                        self.mcp_phase_call = remaining.as_ref().map(|(id, _)| id.clone());
                        self.phase = remaining
                            .map(|(_, tool)| Phase::Calling(tool))
                            .unwrap_or(Phase::Thinking);
                    }
                }
            }
            HudEvent::McpProgress {
                id,
                progress,
                total,
                message,
            } => {
                let Some(tool) = self.mcp_calls.get(id) else {
                    return false;
                };
                if progress.is_some_and(|value| !value.is_finite() || value < 0.0) {
                    return false;
                }
                let total = total.filter(|value| value.is_finite() && *value > 0.0);
                if let Some(previous) = self.mcp_reports.get(id)
                    && previous.total == total
                    && previous
                        .progress
                        .zip(progress)
                        .is_some_and(|(before, after)| after < before)
                {
                    return false;
                }
                let reported = ToolProgress {
                    id: id.to_string(),
                    tool: tool.clone(),
                    progress,
                    total,
                    message: super::pipeline_panel::inline(message),
                };
                self.mcp_reports.insert(id.to_string(), reported.clone());
                self.mcp_report_order.retain(|call| call != id);
                self.mcp_report_order.push(id.to_string());
                self.progress = Some(reported);
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
        before != (self.phase.clone(), self.counts, self.progress.clone())
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
