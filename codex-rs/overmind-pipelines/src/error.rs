use std::path::PathBuf;

/// Everything that can go wrong loading a pipeline or touching run state.
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error("cannot read {}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{}: {message}", path.display())]
    Parse { path: PathBuf, message: String },
    #[error("invalid pipeline: {0}")]
    Invalid(String),
    #[error("{}", .0.join("; "))]
    Inputs(Vec<String>),
    #[error("{0}")]
    Run(String),
}

impl PipelineError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
