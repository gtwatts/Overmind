use std::path::Path;

use pretty_assertions::assert_eq;

use super::*;
use crate::test_fixtures::PI_YAML;
use crate::test_fixtures::WHITEBOARD_JSON;

fn parse(text: &str) -> Pipeline {
    Pipeline::from_bytes(
        text.as_bytes(),
        Path::new("/p"),
        Path::new("/p/pipeline.yaml"),
    )
    .unwrap()
}

#[test]
fn parses_codex_json_dialect() {
    let pipeline = parse(WHITEBOARD_JSON);
    let definition = &pipeline.definition;
    assert_eq!(definition.name, "mini-whiteboard");
    assert!(definition.is_active());
    assert_eq!(
        definition.inputs.keys().collect::<Vec<_>>(),
        vec!["brief", "mode", "includeMusic"]
    );
    assert_eq!(definition.inputs["mode"].kind, InputType::Enum);
    assert_eq!(definition.source_name(), Some("pi-mini-whiteboard"));
    let steps = pipeline.steps();
    assert_eq!(steps.len(), 5);
    assert_eq!(steps[2].kind, StepKind::HumanReview);
    assert!(steps[2].needs_approval());
    assert!(steps[3].needs_approval());
    assert!(!steps[1].needs_approval());
    assert_eq!(steps[1].skill.as_deref(), Some("ai-filmmaking"));
    assert_eq!(
        steps[1].output_paths().collect::<Vec<_>>(),
        vec!["production/script.md"]
    );
    assert_eq!(
        steps[3].extra.get("finalPageMustBeGptImagePng"),
        Some(&serde_json::Value::Bool(true))
    );
    assert_eq!(pipeline.dependencies(0), &[] as &[usize]);
    assert_eq!(pipeline.dependencies(4), &[3]);
    assert_eq!(pipeline.order(), &[0, 1, 2, 3, 4]);
    assert_eq!(pipeline.sha256.len(), 64);
}

#[test]
fn parses_pi_yaml_dialect_with_implicit_dependencies() {
    let pipeline = parse(PI_YAML);
    assert_eq!(pipeline.name(), "template-pipeline");
    assert!(!pipeline.definition.is_active());
    assert_eq!(pipeline.steps()[2].kind, StepKind::Checkpoint);
    assert_eq!(
        pipeline.steps()[1].action.as_deref(),
        Some("openai_image_batch")
    );
    assert!(pipeline.steps()[1].extra.contains_key("suggested_tools"));
    assert_eq!(
        pipeline
            .definition
            .requires
            .as_ref()
            .map(|r| r.skills.clone()),
        Some(vec!["ai-filmmaking".to_string()])
    );
    // Without dependsOn each step follows its predecessor.
    assert_eq!(pipeline.dependencies(0), &[] as &[usize]);
    assert_eq!(pipeline.dependencies(1), &[0]);
    assert_eq!(pipeline.dependencies(2), &[1]);
}

#[test]
fn rejects_unknown_dependencies_duplicates_and_cycles() {
    let unknown = r#"{"name":"x","steps":[{"id":"a","kind":"instruction","dependsOn":["zz"]}]}"#;
    let duplicate = r#"{"name":"x","steps":[{"id":"a","kind":"tool"},{"id":"a","kind":"tool"}]}"#;
    let cycle = r#"{"name":"x","steps":[
        {"id":"a","kind":"tool","dependsOn":["b"]},
        {"id":"b","kind":"tool","dependsOn":["a"]}]}"#;
    let empty = r#"{"name":"x","steps":[]}"#;
    let message = |text: &str| {
        Pipeline::from_bytes(
            text.as_bytes(),
            Path::new("/p"),
            Path::new("/p/pipeline.yaml"),
        )
        .unwrap_err()
        .to_string()
    };
    assert_eq!(
        message(unknown),
        "invalid pipeline: step `a` depends on unknown step `zz`"
    );
    assert_eq!(
        message(duplicate),
        "invalid pipeline: duplicate step id `a`"
    );
    assert_eq!(
        message(cycle),
        "invalid pipeline: dependency cycle among a, b"
    );
    assert_eq!(message(empty), "invalid pipeline: no steps");
}

#[test]
fn parse_errors_name_the_file() {
    let err = Pipeline::from_bytes(
        b"{ not json",
        Path::new("/p"),
        Path::new("/p/pipeline.yaml"),
    )
    .unwrap_err()
    .to_string();
    assert!(err.starts_with("/p/pipeline.yaml: "), "{err}");
}

#[test]
fn output_path_detection() {
    assert!(is_output_path("production/brief.md"));
    assert!(is_output_path("production/boards/"));
    assert!(is_output_path("render.mp4"));
    assert!(!is_output_path("brandPreservationNotes"));
    assert!(!is_output_path("../escape.md"));
    assert!(!is_output_path("/abs/path.md"));
    assert!(!is_output_path("<vault>/R1 Recordings/"));
}

#[test]
fn downstream_follows_dependencies_in_order() {
    let text = r#"{"name":"x","steps":[
        {"id":"a","kind":"tool","dependsOn":[]},
        {"id":"b","kind":"tool","dependsOn":["a"]},
        {"id":"c","kind":"tool","dependsOn":[]},
        {"id":"d","kind":"tool","dependsOn":["b","c"]}]}"#;
    let pipeline = parse(text);
    assert_eq!(pipeline.downstream(0), vec![1, 3]);
    assert_eq!(pipeline.downstream(2), vec![3]);
    assert_eq!(pipeline.downstream(3), Vec::<usize>::new());
}

/// Parse every real definition on this machine (read only). Run with `--ignored`.
#[test]
#[ignore = "reads the installed gordon-workflows plugin and ~/.pi pipelines"]
fn parses_installed_definitions() {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap();
    let codex_home = std::env::var_os("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    let mut roots = vec![home.join(".pi/agent/pipelines")];
    if let Ok(marketplaces) = std::fs::read_dir(codex_home.join("local-marketplaces")) {
        for marketplace in marketplaces.filter_map(Result::ok) {
            if let Ok(plugins) = std::fs::read_dir(marketplace.path().join("plugins")) {
                for plugin in plugins.filter_map(Result::ok) {
                    roots.push(plugin.path().join("pipelines"));
                }
            }
        }
    }
    let mut parsed = 0;
    let mut failures = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            if !entry.path().join(DEFINITION_FILE).is_file() {
                continue;
            }
            match Pipeline::load(&entry.path()) {
                Ok(_) => parsed += 1,
                Err(err) => failures.push(err.to_string()),
            }
        }
    }
    eprintln!("parsed {parsed} installed pipeline definitions");
    assert_eq!(failures, Vec::<String>::new());
}
