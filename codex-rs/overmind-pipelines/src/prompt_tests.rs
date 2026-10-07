use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;
use crate::Pipeline;
use crate::RunStore;
use crate::test_fixtures::WHITEBOARD_JSON;
use crate::test_fixtures::write_pipeline;

#[test]
fn stage_prompts_carry_the_contract() {
    let source = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let dir = write_pipeline(source.path(), "mini-whiteboard", WHITEBOARD_JSON);
    let pipeline = Pipeline::load(&dir).unwrap();
    let long_brief = "b".repeat(300);
    let inputs =
        serde_json::from_value(json!({"brief": long_brief, "mode": "stroke-reveal"})).unwrap();
    let mut run = RunStore::for_workspace(workspace.path())
        .create(&pipeline, inputs, workspace.path())
        .unwrap();

    let first = stage_prompt(&run, 0, PromptOptions::default());
    let first_lines: Vec<&str> = first.lines().collect();
    assert_eq!(
        first_lines[0],
        "[Overmind pipeline] mini-whiteboard · stage 1/5 · brief-and-mode (instruction)"
    );
    assert!(first.contains(&format!("- brief: {}\n", "b".repeat(300))));
    assert!(first.contains("Role: producer\n"));
    assert!(first.contains("Tool hints: write production/brief.md"));
    assert!(first.contains("- production/brief.md\n"));
    assert!(first.contains("Ask before: publish, spend."));
    assert!(first.ends_with("say what is blocking and stop."));

    std::fs::create_dir_all(workspace.path().join("production")).unwrap();
    std::fs::write(workspace.path().join("production/brief.md"), "brief").unwrap();
    run.begin_stage(0);
    run.complete_stage(0);
    let second = stage_prompt(
        &run,
        1,
        PromptOptions {
            skill_attached: true,
        },
    );
    assert!(second.contains("Role: director · skill: ai-filmmaking (attached)\n"));
    assert!(second.contains("… (full value in state.json)"));
    assert!(second.contains("Upstream artifacts:\n- production/brief.md (from brief-and-mode)\n"));
    assert!(
        second.contains("- beatMapNotes (named deliverable; cover it in a file or your summary)")
    );

    let gate = stage_prompt(&run, 2, PromptOptions::default());
    assert!(gate.contains("This stage needs the user's approval; stop and ask before acting."));
    run.approve(2);
    let approved = stage_prompt(&run, 2, PromptOptions::default());
    assert!(approved.contains("The user approved this stage with /pipeline approve at "));

    let script = stage_prompt(&run, 4, PromptOptions::default());
    let expected = format!(
        "Script: run `{}` (validators take the workspace as the run directory) and keep its report as evidence.",
        dir.join("validators/validate-run.mjs").display()
    );
    assert_eq!(
        script.lines().find(|line| line.starts_with("Script:")),
        Some(expected.as_str())
    );
}
