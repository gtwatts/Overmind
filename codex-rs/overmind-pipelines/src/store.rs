//! The runs directory of a workspace: creating, listing and opening runs.

use std::path::Path;
use std::path::PathBuf;

use chrono::Local;
use indexmap::IndexMap;
use serde_json::Value;

use crate::Pipeline;
use crate::PipelineError;
use crate::run::NextAction;
use crate::run::RUNS_DIR;
use crate::run::Run;
use crate::run::RunState;
use crate::run::SNAPSHOT_FILE;
use crate::run::STATE_FILE;
use crate::run::STATE_SCHEMA;
use crate::run::StageRecord;
use crate::run::inputs_sha;
use crate::run::now;

/// The runs directory of one workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunStore {
    root: PathBuf,
}

impl RunStore {
    pub fn for_workspace(workspace: &Path) -> Self {
        Self {
            root: RUNS_DIR
                .iter()
                .fold(workspace.to_path_buf(), |dir, part| dir.join(part)),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Create a new run of `pipeline` with bound `inputs`, resolving outputs in `workspace`.
    pub fn create(
        &self,
        pipeline: &Pipeline,
        inputs: IndexMap<String, Value>,
        workspace: &Path,
    ) -> Result<Run, PipelineError> {
        let bytes = std::fs::read(pipeline.definition_path())
            .map_err(|err| PipelineError::io(pipeline.definition_path(), err))?;
        let snapshot = Pipeline::from_bytes(&bytes, &pipeline.dir, &pipeline.definition_path())?;
        self.ensure_root()?;
        let dir = self.new_run_dir(snapshot.name())?;
        write_private(&dir.join(SNAPSHOT_FILE), &bytes)?;
        let created = now();
        let stages = snapshot
            .steps()
            .iter()
            .map(|step| (step.id.clone(), StageRecord::default()))
            .collect();
        let run_id = dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut run = Run {
            state: RunState {
                schema_version: STATE_SCHEMA.to_string(),
                run_id,
                pipeline: snapshot.name().to_string(),
                pipeline_dir: snapshot.dir.clone(),
                definition_sha256: snapshot.sha256.clone(),
                inputs_sha256: inputs_sha(&inputs),
                inputs,
                workspace: workspace.to_path_buf(),
                created_at: created.clone(),
                updated_at: created,
                paused: None,
                stages,
                history: Vec::new(),
            },
            dir,
            pipeline: snapshot,
        };
        run.log(None, "created");
        run.save()?;
        Ok(run)
    }

    /// Every loadable run, newest first, plus the directories that failed to load.
    pub fn list(&self) -> (Vec<Run>, Vec<(PathBuf, String)>) {
        let mut runs = Vec::new();
        let mut problems = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return (runs, problems);
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if !path.join(STATE_FILE).is_file() {
                continue;
            }
            match Run::load(&path) {
                Ok(run) => runs.push(run),
                Err(err) => problems.push((path, err.to_string())),
            }
        }
        runs.sort_by(|left, right| {
            right
                .state
                .created_at
                .cmp(&left.state.created_at)
                .then_with(|| right.state.run_id.cmp(&left.state.run_id))
        });
        (runs, problems)
    }

    /// Open a run by exact id or unique prefix.
    pub fn open(&self, id: &str) -> Result<Run, PipelineError> {
        let exact = self.root.join(id);
        if !id.contains(['/', '\\']) && exact.join(STATE_FILE).is_file() {
            return Run::load(&exact);
        }
        let (runs, _) = self.list();
        let mut matches = runs
            .into_iter()
            .filter(|run| run.id().starts_with(id) || run.state.pipeline == id);
        match (matches.next(), matches.next()) {
            (Some(run), None) => Ok(run),
            (Some(_), Some(_)) => Err(PipelineError::Run(format!(
                "`{id}` matches several runs; use more of the run id"
            ))),
            (None, _) => Err(PipelineError::Run(format!("no run matches `{id}`"))),
        }
    }

    /// The newest run that has not completed.
    pub fn latest_unfinished(&self) -> Option<Run> {
        self.list()
            .0
            .into_iter()
            .find(|run| run.next_action() != NextAction::Complete)
    }

    pub fn latest(&self) -> Option<Run> {
        self.list().0.into_iter().next()
    }

    fn ensure_root(&self) -> Result<(), PipelineError> {
        if self.root.is_dir() {
            return Ok(());
        }
        std::fs::create_dir_all(&self.root).map_err(|err| PipelineError::io(&self.root, err))?;
        // Run state can hold briefs and other inputs; keep it out of version control.
        let ignore = self.root.join(".gitignore");
        std::fs::write(&ignore, "*\n").map_err(|err| PipelineError::io(&ignore, err))
    }

    fn new_run_dir(&self, pipeline: &str) -> Result<PathBuf, PipelineError> {
        let stamp = Local::now().format("%Y%m%d-%H%M%S");
        let slug: String = pipeline
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '-' {
                    ch
                } else {
                    '-'
                }
            })
            .take(40)
            .collect();
        for attempt in 1..100 {
            let name = if attempt == 1 {
                format!("{stamp}-{slug}")
            } else {
                format!("{stamp}-{slug}-{attempt}")
            };
            let dir = self.root.join(name);
            match create_private_dir(&dir) {
                Ok(()) => return Ok(dir),
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(PipelineError::io(&dir, err)),
            }
        }
        Err(PipelineError::Run(
            "could not allocate a run directory".to_string(),
        ))
    }
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
}

pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<(), PipelineError> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|err| PipelineError::io(path, err))?;
    file.write_all(bytes)
        .map_err(|err| PipelineError::io(path, err))
}
