//! Finding pipelines in the search directories.
//!
//! Each search directory holds one sub-directory per pipeline with a `pipeline.yaml`.
//! Directories are given highest precedence first (project `.codex/pipelines`, then
//! `$CODEX_HOME/pipelines`, then installed plugin `pipelines/` folders); the first pipeline with
//! a given name wins. Directories starting with `_` or `.` (`_shared`, `_template`) are support
//! folders, not pipelines.

use std::path::Path;
use std::path::PathBuf;

use crate::DEFINITION_FILE;
use crate::Pipeline;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Discovery {
    /// Loaded pipelines, sorted by name.
    pub pipelines: Vec<Pipeline>,
    /// Pipelines hidden by a higher-precedence one with the same name.
    pub shadowed: Vec<PathBuf>,
    /// Definitions that failed to load, with the reason.
    pub problems: Vec<(PathBuf, String)>,
}

impl Discovery {
    /// Find by name, directory name, or pre-migration (Pi) name.
    pub fn find(&self, name: &str) -> Option<&Pipeline> {
        self.pipelines
            .iter()
            .find(|pipeline| pipeline.name() == name)
            .or_else(|| {
                self.pipelines.iter().find(|pipeline| {
                    pipeline.dir.file_name().is_some_and(|dir| dir == name)
                        || pipeline.definition.source_name() == Some(name)
                })
            })
    }
}

pub fn discover(roots: &[PathBuf]) -> Discovery {
    let mut discovery = Discovery::default();
    for root in roots {
        for dir in pipeline_dirs(root) {
            match Pipeline::load(&dir) {
                Ok(pipeline) => {
                    if discovery
                        .pipelines
                        .iter()
                        .any(|existing| existing.name() == pipeline.name())
                    {
                        discovery.shadowed.push(dir);
                    } else {
                        discovery.pipelines.push(pipeline);
                    }
                }
                Err(err) => discovery.problems.push((dir, err.to_string())),
            }
        }
    }
    discovery
        .pipelines
        .sort_by(|left, right| left.name().cmp(right.name()));
    discovery
}

fn pipeline_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            !name.starts_with('_') && !name.starts_with('.')
        })
        .map(|entry| entry.path())
        .filter(|path| path.join(DEFINITION_FILE).is_file())
        .collect();
    dirs.sort();
    dirs
}

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod tests;
