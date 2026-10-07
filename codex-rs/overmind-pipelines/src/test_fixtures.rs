//! Fixtures modeled on the real gordon-workflows (JSON) and Pi (YAML) definitions.

use std::path::Path;
use std::path::PathBuf;

/// Codex dialect: JSON text, explicit `dependsOn`, `actionHint`, a gate and a directory output.
pub(crate) const WHITEBOARD_JSON: &str = r#"{
  "name": "mini-whiteboard",
  "schemaVersion": "1.0.0",
  "description": "Produce a short whiteboard video.",
  "version": "1.0.0",
  "status": "active",
  "inputs": {
    "brief": {"type": "string", "required": true, "description": "Topic and audience."},
    "mode": {"type": "enum", "required": false, "default": "stroke-reveal",
             "values": ["stroke-reveal", "organic-drawon"]},
    "includeMusic": {"type": "enum", "default": "yes", "values": ["yes", "no"]}
  },
  "steps": [
    {"id": "brief-and-mode", "kind": "instruction", "description": "Restate the brief and lock one mode.",
     "agent": "producer", "outputs": ["production/brief.md"], "dependsOn": [],
     "actionHint": "write production/brief.md"},
    {"id": "script", "kind": "instruction", "description": "Write the script and beats.",
     "skill": "ai-filmmaking", "agent": "director",
     "outputs": ["production/script.md", "beatMapNotes"], "dependsOn": ["brief-and-mode"]},
    {"id": "approve-paid-generation", "kind": "human-review", "description": "Approve the paid image plan.",
     "approvalRequired": true, "outputs": ["production/generation-approval.md"], "dependsOn": ["script"]},
    {"id": "generate-art", "kind": "instruction", "description": "Generate the boards.",
     "approvalRequired": true, "outputs": ["production/boards/"], "dependsOn": ["approve-paid-generation"],
     "finalPageMustBeGptImagePng": true},
    {"id": "validate-run-evidence", "kind": "script", "description": "Validate the run evidence.",
     "action": "validators/validate-run.mjs", "dependsOn": ["generate-art"]}
  ],
  "safety": {"defaultApprovalPolicy": "require explicit approval for paid generation",
             "approvalRequiredFor": ["publish", "spend"]},
  "outputs": ["production/brief.md", "production/script.md"],
  "runtime": {"node": ">=22", "cli": ["ffmpeg"], "toolResolution": "Discover native tools."},
  "migration": {"sourceName": "pi-mini-whiteboard", "sourceVersion": "0.1.0"}
}
"#;

/// Pi dialect: block YAML, no `dependsOn`, `action` and `requires`.
pub(crate) const PI_YAML: &str = r#"name: "template-pipeline"
schemaVersion: "1.0.0"
description: "Reusable Pi pipeline template"
version: "0.1.0"
status: "draft"

inputs:
  input:
    type: string
    required: true

requires:
  skills: [ai-filmmaking]
  extensions: []
  tools: []
  cli: []

steps:
  - id: "plan"
    kind: "instruction"
    description: "Clarify inputs and produce an execution plan"
  - id: "execute"
    kind: "instruction"
    description: "Run the core routine"
    action: "openai_image_batch"
    suggested_tools: [photocraft]
  - id: "verify"
    kind: "checkpoint"
    description: "Check outputs against acceptance criteria"

safety:
  defaultApprovalPolicy: require explicit approval for irreversible/external side effects
  approvalRequiredFor:
    - publish

outputs:
  - "artifactPath"
"#;

/// Write `<root>/<dir>/pipeline.yaml` (plus a guide) and return the pipeline directory.
pub(crate) fn write_pipeline(root: &Path, dir: &str, definition: &str) -> PathBuf {
    let path = root.join(dir);
    std::fs::create_dir_all(path.join("validators")).unwrap();
    std::fs::write(path.join("pipeline.yaml"), definition).unwrap();
    std::fs::write(path.join("PIPELINE.md"), "# guide\n").unwrap();
    std::fs::write(path.join("validators/validate-run.mjs"), "// validator\n").unwrap();
    path
}
