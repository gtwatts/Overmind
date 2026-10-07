//! The `pipeline.yaml` format.
//!
//! Two dialects exist on Todd's machines and both parse here:
//! - Codex-native (gordon-workflows plugin): JSON-compatible YAML (the file is JSON), every
//!   step lists `dependsOn`, and tools are named by `actionHint`.
//! - Pi (the library gordon-workflows was migrated from): block YAML, `dependsOn` is usually
//!   omitted, `action` names a tool or command, and `requires` lists skills, extensions, tools
//!   and CLIs.
//!
//! A step without `dependsOn` follows its predecessor (the execution contract's ordered-stage
//! rule); an explicit empty list makes it a root. Unknown fields are kept in `extra` so
//! domain-specific keys (`finalPageMustBeGptImagePng`, `suggested_tools`, ...) survive.

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;

use indexmap::IndexMap;
use serde::Deserialize;
use serde::Deserializer;
use serde_json::Value;

use crate::PipelineError;
use crate::graph;
use crate::hashing::sha256_hex;

/// Stage, input and output contract.
pub const DEFINITION_FILE: &str = "pipeline.yaml";
/// Human guide that accompanies the definition.
pub const GUIDE_FILE: &str = "PIPELINE.md";

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PipelineDefinition {
    pub name: String,
    #[serde(default)]
    pub schema_version: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: Option<String>,
    /// `active`, `draft`, ... Drafts and templates run only when named explicitly.
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub inputs: IndexMap<String, InputSpec>,
    pub steps: Vec<Step>,
    #[serde(default)]
    pub safety: Option<Safety>,
    #[serde(default)]
    pub outputs: Vec<String>,
    #[serde(default)]
    pub runtime: Option<Runtime>,
    #[serde(default)]
    pub requires: Option<Requires>,
    #[serde(default)]
    pub migration: Option<Migration>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn default_status() -> String {
    "active".to_string()
}

impl PipelineDefinition {
    pub fn is_active(&self) -> bool {
        self.status == "active"
    }

    /// The pre-migration name (for example the Pi name), when it differs.
    pub fn source_name(&self) -> Option<&str> {
        self.migration
            .as_ref()
            .and_then(|migration| migration.source_name.as_deref())
            .filter(|source| *source != self.name)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct InputSpec {
    #[serde(rename = "type", default)]
    pub kind: InputType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub values: Vec<Value>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum InputType {
    #[default]
    String,
    Enum,
    Integer,
    Number,
    Boolean,
    Array,
    Object,
    Other(String),
}

impl InputType {
    pub fn as_str(&self) -> &str {
        match self {
            InputType::String => "string",
            InputType::Enum => "enum",
            InputType::Integer => "integer",
            InputType::Number => "number",
            InputType::Boolean => "boolean",
            InputType::Array => "array",
            InputType::Object => "object",
            InputType::Other(other) => other,
        }
    }
}

impl<'de> Deserialize<'de> for InputType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(match raw.as_str() {
            "string" => InputType::String,
            "enum" => InputType::Enum,
            "integer" => InputType::Integer,
            "number" => InputType::Number,
            "boolean" => InputType::Boolean,
            "array" => InputType::Array,
            "object" => InputType::Object,
            _ => InputType::Other(raw),
        })
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Safety {
    #[serde(default)]
    pub default_approval_policy: Option<String>,
    #[serde(default)]
    pub approval_required_for: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Runtime {
    #[serde(default)]
    pub node: Option<String>,
    #[serde(default)]
    pub cli: Vec<String>,
    #[serde(default)]
    pub tool_resolution: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Requires {
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub cli: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Migration {
    #[serde(default)]
    pub source_name: Option<String>,
    #[serde(default)]
    pub source_version: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepKind {
    Instruction,
    Script,
    HumanReview,
    Tool,
    Checkpoint,
    Other(String),
}

impl StepKind {
    pub fn as_str(&self) -> &str {
        match self {
            StepKind::Instruction => "instruction",
            StepKind::Script => "script",
            StepKind::HumanReview => "human-review",
            StepKind::Tool => "tool",
            StepKind::Checkpoint => "checkpoint",
            StepKind::Other(other) => other,
        }
    }
}

impl<'de> Deserialize<'de> for StepKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(match raw.as_str() {
            "instruction" => StepKind::Instruction,
            "script" => StepKind::Script,
            "human-review" => StepKind::HumanReview,
            "tool" => StepKind::Tool,
            "checkpoint" => StepKind::Checkpoint,
            _ => StepKind::Other(raw),
        })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub id: String,
    pub kind: StepKind,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub skill: Option<String>,
    /// `None` means "after the previous step"; `Some(vec![])` means "no dependencies".
    #[serde(default)]
    pub depends_on: Option<Vec<String>>,
    #[serde(default)]
    pub outputs: Vec<String>,
    /// Pi dialect: tool, command or validator script.
    #[serde(default)]
    pub action: Option<String>,
    /// Codex dialect: native tool names to look up.
    #[serde(default)]
    pub action_hint: Option<String>,
    #[serde(default)]
    pub approval_required: bool,
    #[serde(default)]
    pub approval_reason: Option<String>,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub parameters: Option<Value>,
    #[serde(default)]
    pub extension: Option<String>,
    #[serde(default)]
    pub integration_hint: Option<String>,
    #[serde(default)]
    pub model_required: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Step {
    /// Title when present, otherwise the id.
    pub fn label(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.id)
    }

    /// Whether the user must approve before this stage runs: explicit `approvalRequired`, or a
    /// human-review stage.
    pub fn needs_approval(&self) -> bool {
        self.approval_required || self.kind == StepKind::HumanReview
    }

    /// Declared outputs that name files or directories (relative paths). Other entries, such
    /// as `brandPreservationNotes`, are named deliverables that cannot be checked on disk.
    pub fn output_paths(&self) -> impl Iterator<Item = &str> {
        self.outputs
            .iter()
            .map(String::as_str)
            .filter(|output| is_output_path(output))
    }
}

/// An output is a checkable path when it looks like one (`dir/file`, `file.ext`, `dir/`) and
/// stays inside the workspace.
pub fn is_output_path(output: &str) -> bool {
    let looks_like_path = output.contains('/') || output.contains('.');
    let safe = !output.starts_with('/')
        && !output.starts_with('~')
        && !output.split('/').any(|part| part == "..")
        && !output.contains(['<', '>', '{', '}', '*', ' ']);
    looks_like_path && safe
}

/// A parsed, validated pipeline and where it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Pipeline {
    pub definition: PipelineDefinition,
    /// Directory holding `pipeline.yaml`.
    pub dir: PathBuf,
    /// SHA-256 of the raw definition bytes.
    pub sha256: String,
    /// Resolved dependencies, by step index.
    dependencies: Vec<Vec<usize>>,
    /// Stable topological order of step indices.
    order: Vec<usize>,
}

impl Pipeline {
    /// Load `<dir>/pipeline.yaml`.
    pub fn load(dir: &Path) -> Result<Self, PipelineError> {
        let path = dir.join(DEFINITION_FILE);
        let bytes = std::fs::read(&path).map_err(|err| PipelineError::io(&path, err))?;
        Self::from_bytes(&bytes, dir, &path)
    }

    /// Parse definition bytes. `origin` is used in error messages only.
    pub fn from_bytes(bytes: &[u8], dir: &Path, origin: &Path) -> Result<Self, PipelineError> {
        let text = std::str::from_utf8(bytes).map_err(|err| PipelineError::Parse {
            path: origin.to_path_buf(),
            message: err.to_string(),
        })?;
        let definition = parse_definition(text).map_err(|message| PipelineError::Parse {
            path: origin.to_path_buf(),
            message,
        })?;
        let dependencies = resolve_dependencies(&definition)?;
        let order = graph::topological_order(&dependencies).map_err(|cycle| {
            let ids: Vec<&str> = cycle
                .iter()
                .map(|index| definition.steps[*index].id.as_str())
                .collect();
            PipelineError::Invalid(format!("dependency cycle among {}", ids.join(", ")))
        })?;
        Ok(Self {
            definition,
            dir: dir.to_path_buf(),
            sha256: sha256_hex(bytes),
            dependencies,
            order,
        })
    }

    pub fn name(&self) -> &str {
        &self.definition.name
    }

    pub fn steps(&self) -> &[Step] {
        &self.definition.steps
    }

    pub fn step_index(&self, id: &str) -> Option<usize> {
        self.definition.steps.iter().position(|step| step.id == id)
    }

    /// Direct dependencies of step `index`.
    pub fn dependencies(&self, index: usize) -> &[usize] {
        self.dependencies.get(index).map_or(&[], Vec::as_slice)
    }

    /// Step indices in execution order.
    pub fn order(&self) -> &[usize] {
        &self.order
    }

    /// Every step that depends on `index`, directly or transitively, in execution order.
    pub fn downstream(&self, index: usize) -> Vec<usize> {
        let affected = graph::downstream(&self.dependencies, index);
        self.order
            .iter()
            .copied()
            .filter(|step| affected.contains(step))
            .collect()
    }

    pub fn guide_path(&self) -> Option<PathBuf> {
        let path = self.dir.join(GUIDE_FILE);
        path.is_file().then_some(path)
    }

    pub fn definition_path(&self) -> PathBuf {
        self.dir.join(DEFINITION_FILE)
    }
}

/// Parse definition text: JSON first (the Codex dialect is JSON), then YAML.
pub fn parse_definition(text: &str) -> Result<PipelineDefinition, String> {
    if text.trim_start().starts_with('{') {
        return serde_json::from_str(text).map_err(|err| err.to_string());
    }
    serde_yaml::from_str(text).map_err(|err| err.to_string())
}

fn resolve_dependencies(definition: &PipelineDefinition) -> Result<Vec<Vec<usize>>, PipelineError> {
    if definition.name.trim().is_empty() {
        return Err(PipelineError::Invalid("missing name".to_string()));
    }
    if definition.steps.is_empty() {
        return Err(PipelineError::Invalid("no steps".to_string()));
    }
    let mut index_by_id: BTreeMap<&str, usize> = BTreeMap::new();
    for (index, step) in definition.steps.iter().enumerate() {
        if step.id.trim().is_empty() {
            return Err(PipelineError::Invalid(format!(
                "step {} has no id",
                index + 1
            )));
        }
        if index_by_id.insert(step.id.as_str(), index).is_some() {
            return Err(PipelineError::Invalid(format!(
                "duplicate step id `{}`",
                step.id
            )));
        }
    }
    let mut dependencies = Vec::with_capacity(definition.steps.len());
    for (index, step) in definition.steps.iter().enumerate() {
        let deps = match &step.depends_on {
            None => index.checked_sub(1).into_iter().collect(),
            Some(ids) => {
                let mut deps = Vec::with_capacity(ids.len());
                for id in ids {
                    let Some(dep) = index_by_id.get(id.as_str()).copied() else {
                        return Err(PipelineError::Invalid(format!(
                            "step `{}` depends on unknown step `{id}`",
                            step.id
                        )));
                    };
                    if dep == index {
                        return Err(PipelineError::Invalid(format!(
                            "step `{}` depends on itself",
                            step.id
                        )));
                    }
                    if !deps.contains(&dep) {
                        deps.push(dep);
                    }
                }
                deps
            }
        };
        dependencies.push(deps);
    }
    Ok(dependencies)
}

#[cfg(test)]
#[path = "definition_tests.rs"]
mod tests;
