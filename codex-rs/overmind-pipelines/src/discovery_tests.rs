use pretty_assertions::assert_eq;

use super::*;
use crate::Pipeline;
use crate::test_fixtures::PI_YAML;
use crate::test_fixtures::WHITEBOARD_JSON;
use crate::test_fixtures::write_pipeline;

#[test]
fn first_root_wins_and_support_dirs_are_skipped() {
    let project = tempfile::tempdir().unwrap();
    let plugin = tempfile::tempdir().unwrap();
    write_pipeline(project.path(), "mini-whiteboard", WHITEBOARD_JSON);
    let shadowed = write_pipeline(plugin.path(), "mini-whiteboard", WHITEBOARD_JSON);
    write_pipeline(plugin.path(), "template-pipeline", PI_YAML);
    write_pipeline(plugin.path(), "_template", PI_YAML);
    write_pipeline(plugin.path(), "broken", "{ nope");
    let discovery = discover(&[project.path().to_path_buf(), plugin.path().to_path_buf()]);
    assert_eq!(
        discovery
            .pipelines
            .iter()
            .map(Pipeline::name)
            .collect::<Vec<_>>(),
        vec!["mini-whiteboard", "template-pipeline"]
    );
    assert_eq!(
        discovery.pipelines[0].dir,
        project.path().join("mini-whiteboard")
    );
    assert_eq!(discovery.shadowed, vec![shadowed]);
    assert_eq!(discovery.problems.len(), 1);
    assert!(discovery.problems[0].0.ends_with("broken"));
}

#[test]
fn find_matches_name_directory_and_source_name() {
    let root = tempfile::tempdir().unwrap();
    write_pipeline(root.path(), "whiteboard-dir", WHITEBOARD_JSON);
    let discovery = discover(&[root.path().to_path_buf()]);
    for name in ["mini-whiteboard", "whiteboard-dir", "pi-mini-whiteboard"] {
        assert!(discovery.find(name).is_some(), "{name}");
    }
    assert!(discovery.find("missing").is_none());
}

#[test]
fn missing_roots_are_fine() {
    let discovery = discover(&[std::path::PathBuf::from("/definitely/not/here")]);
    assert_eq!(discovery, Discovery::default());
}
