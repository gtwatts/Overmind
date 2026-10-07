use std::path::Path;

use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;
use crate::RunStore;
use crate::test_fixtures::WHITEBOARD_JSON;
use crate::test_fixtures::write_pipeline;

struct Fixture {
    _dirs: (tempfile::TempDir, tempfile::TempDir),
    workspace: PathBuf,
    store: RunStore,
    pipeline: Pipeline,
}

fn fixture() -> Fixture {
    let source = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let dir = write_pipeline(source.path(), "mini-whiteboard", WHITEBOARD_JSON);
    let pipeline = Pipeline::load(&dir).unwrap();
    let store = RunStore::for_workspace(workspace.path());
    Fixture {
        workspace: workspace.path().to_path_buf(),
        _dirs: (source, workspace),
        store,
        pipeline,
    }
}

fn inputs() -> IndexMap<String, Value> {
    serde_json::from_value(json!({"brief": "explain", "mode": "stroke-reveal"})).unwrap()
}

fn write(workspace: &Path, rel: &str, contents: &str) {
    let path = workspace.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

#[test]
fn full_lifecycle_with_gates_and_evidence() {
    let fx = fixture();
    let mut run = fx
        .store
        .create(&fx.pipeline, inputs(), &fx.workspace)
        .unwrap();
    assert!(run.dir.join(STATE_FILE).is_file());
    assert!(run.dir.join(SNAPSHOT_FILE).is_file());
    assert!(fx.store.root().join(".gitignore").is_file());
    assert!(run.id().ends_with("-mini-whiteboard"), "{}", run.id());
    assert_eq!(
        run.next_action(),
        NextAction::Execute {
            index: 0,
            retry: false
        }
    );

    run.begin_stage(0);
    assert_eq!(run.views()[0], StageView::Running);
    assert_eq!(
        run.complete_stage(0),
        StageOutcome::MissingOutputs(vec!["production/brief.md".to_string()])
    );
    assert_eq!(
        run.next_action(),
        NextAction::Execute {
            index: 0,
            retry: true
        }
    );
    write(&fx.workspace, "production/brief.md", "brief");
    assert_eq!(run.missing_outputs(0), Vec::<String>::new());
    assert_eq!(
        run.complete_stage(0),
        StageOutcome::Done {
            files: 1,
            unchecked: vec![]
        }
    );

    run.begin_stage(1);
    write(&fx.workspace, "production/script.md", "script");
    assert_eq!(
        run.complete_stage(1),
        StageOutcome::Done {
            files: 1,
            unchecked: vec!["beatMapNotes".to_string()]
        }
    );
    assert_eq!(run.progress(), (2, 5));

    // Human review gate.
    assert_eq!(run.views()[2], StageView::AwaitingApproval);
    assert_eq!(run.next_action(), NextAction::AwaitApproval(2));
    run.approve(2);
    assert_eq!(
        run.next_action(),
        NextAction::Execute {
            index: 2,
            retry: false
        }
    );
    run.begin_stage(2);
    write(&fx.workspace, "production/generation-approval.md", "ok");
    run.complete_stage(2);

    // Paid generation gate with a directory output.
    run.approve(3);
    run.begin_stage(3);
    std::fs::create_dir_all(fx.workspace.join("production/boards")).unwrap();
    assert_eq!(
        run.complete_stage(3),
        StageOutcome::MissingOutputs(vec!["production/boards/".to_string()])
    );
    write(&fx.workspace, "production/boards/b1.png", "png");
    run.begin_stage(3);
    assert!(matches!(
        run.complete_stage(3),
        StageOutcome::Done { files: 1, .. }
    ));

    run.begin_stage(4);
    assert!(matches!(
        run.complete_stage(4),
        StageOutcome::Done { files: 0, .. }
    ));
    assert_eq!(run.next_action(), NextAction::Complete);
    run.save().unwrap();

    // Changing an upstream artifact makes it and everything after it stale.
    write(&fx.workspace, "production/script.md", "script v2");
    let views = run.views();
    assert_eq!(views[0], StageView::Done);
    assert_eq!(&views[1..], &[StageView::Stale; 4]);
    assert_eq!(
        run.next_action(),
        NextAction::Execute {
            index: 1,
            retry: true
        }
    );

    // Rerun resets the stage and its dependents, including approvals.
    let reset = run.rerun_from(1);
    assert_eq!(
        reset,
        vec![
            "script",
            "approve-paid-generation",
            "generate-art",
            "validate-run-evidence"
        ]
    );
    assert!(run.record(2).unwrap().approved_at.is_none());
    assert_eq!(run.record(3).unwrap().attempts, 2);
    run.begin_stage(1);
    run.complete_stage(1);
    assert_eq!(run.next_action(), NextAction::AwaitApproval(2));
}

#[test]
fn re_running_a_finished_stage_resets_dependents() {
    let fx = fixture();
    let mut run = fx
        .store
        .create(&fx.pipeline, inputs(), &fx.workspace)
        .unwrap();
    write(&fx.workspace, "production/brief.md", "brief");
    write(&fx.workspace, "production/script.md", "script");
    run.begin_stage(0);
    run.complete_stage(0);
    run.begin_stage(1);
    run.complete_stage(1);
    run.begin_stage(0);
    assert_eq!(run.record(1).unwrap().status, StageState::Pending);
    assert_eq!(run.views()[1], StageView::Pending);
}

#[test]
fn runs_round_trip_and_resume() {
    let fx = fixture();
    let mut run = fx
        .store
        .create(&fx.pipeline, inputs(), &fx.workspace)
        .unwrap();
    run.begin_stage(0);
    run.pause("interrupted");
    run.save().unwrap();
    // A second run with the same second stamp gets a suffix.
    let second = fx
        .store
        .create(&fx.pipeline, inputs(), &fx.workspace)
        .unwrap();
    assert_ne!(second.id(), run.id());

    let loaded = Run::load(&run.dir).unwrap();
    assert_eq!(loaded.state, run.state);
    assert_eq!(loaded.views()[0], StageView::Running);
    assert_eq!(
        loaded.next_action(),
        NextAction::Execute {
            index: 0,
            retry: true
        }
    );

    let (runs, problems) = fx.store.list();
    assert_eq!(runs.len(), 2);
    assert!(problems.is_empty());
    assert_eq!(fx.store.open(run.id()).unwrap().id(), run.id());
    assert!(
        fx.store.open("mini-whiteboard").is_err(),
        "ambiguous by name"
    );
    assert!(fx.store.open("nope").is_err());
    assert!(fx.store.latest_unfinished().is_some());
    assert!(!run.source_changed());
}

#[test]
fn edited_inputs_or_snapshots_are_rejected() {
    let fx = fixture();
    let run = fx
        .store
        .create(&fx.pipeline, inputs(), &fx.workspace)
        .unwrap();
    let state_path = run.dir.join(STATE_FILE);
    let original = std::fs::read_to_string(&state_path).unwrap();
    std::fs::write(&state_path, original.replace("\"explain\"", "\"edited\"")).unwrap();
    let err = Run::load(&run.dir).unwrap_err().to_string();
    assert!(
        err.ends_with("run inputs changed; start a new run"),
        "{err}"
    );

    std::fs::write(&state_path, original).unwrap();
    std::fs::write(
        run.dir.join(SNAPSHOT_FILE),
        WHITEBOARD_JSON.replace("short", "long"),
    )
    .unwrap();
    let err = Run::load(&run.dir).unwrap_err().to_string();
    assert!(
        err.ends_with("pipeline snapshot changed; start a new run"),
        "{err}"
    );
}

#[test]
fn source_edits_do_not_change_a_pinned_run() {
    let fx = fixture();
    let run = fx
        .store
        .create(&fx.pipeline, inputs(), &fx.workspace)
        .unwrap();
    std::fs::write(
        fx.pipeline.definition_path(),
        WHITEBOARD_JSON.replace("Produce a short", "Produce a long"),
    )
    .unwrap();
    assert!(run.source_changed());
    let loaded = Run::load(&run.dir).unwrap();
    assert_eq!(
        loaded.pipeline.definition.description,
        "Produce a short whiteboard video."
    );
}
