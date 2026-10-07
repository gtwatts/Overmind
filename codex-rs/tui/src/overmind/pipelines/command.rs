//! `/pipeline` argument parsing.

pub(crate) const PIPELINE_USAGE: &str = "Usage: /pipeline [list | run <name> [inputs] | run | approve | status [run] | inspect <name|run> | rerun [stage | run] | stop]";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PipelineCommand {
    /// `/pipeline` alone: the active run's status, or the list.
    Overview,
    List,
    /// Start a new run. `args` holds `key=value` inputs and free text.
    Run {
        name: String,
        args: String,
    },
    /// `/pipeline run` with no name: resume the session's (or latest unfinished) run.
    Resume,
    /// Approve the gate the run is waiting on, then continue.
    Approve {
        run: Option<String>,
    },
    Status {
        run: Option<String>,
    },
    Inspect {
        target: String,
    },
    /// `rerun` alone repeats the latest run with the same inputs as a new run; `rerun <stage>`
    /// resets a stage (and its dependents) of the latest run; `rerun <run> <stage>` names both.
    Rerun {
        first: Option<String>,
        second: Option<String>,
    },
    /// Pause auto-advance after the current stage.
    Stop,
    Help,
    Unknown(String),
}

pub(crate) fn parse_pipeline_command(args: &str) -> PipelineCommand {
    let args = args.trim();
    let (sub, rest) = split_first(args);
    let mut words = rest.split_whitespace().map(str::to_string);
    match sub.to_ascii_lowercase().as_str() {
        "" => PipelineCommand::Overview,
        "list" | "ls" => PipelineCommand::List,
        "run" | "start" => {
            let (name, args) = split_first(rest);
            if name.is_empty() {
                PipelineCommand::Resume
            } else {
                PipelineCommand::Run {
                    name: name.to_string(),
                    args: args.to_string(),
                }
            }
        }
        "resume" | "continue" => PipelineCommand::Resume,
        "approve" => PipelineCommand::Approve { run: words.next() },
        "status" => PipelineCommand::Status { run: words.next() },
        "inspect" | "show" => match words.next() {
            Some(target) => PipelineCommand::Inspect { target },
            None => PipelineCommand::Help,
        },
        "rerun" => PipelineCommand::Rerun {
            first: words.next(),
            second: words.next(),
        },
        "stop" | "pause" => PipelineCommand::Stop,
        "help" | "-h" | "--help" => PipelineCommand::Help,
        _ => PipelineCommand::Unknown(sub.to_string()),
    }
}

fn split_first(text: &str) -> (&str, &str) {
    let text = text.trim_start();
    match text.find(char::is_whitespace) {
        Some(index) => (&text[..index], text[index..].trim()),
        None => (text, ""),
    }
}
