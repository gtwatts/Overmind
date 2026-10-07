use std::fs;
use std::path::Path;
use std::path::PathBuf;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::*;

fn test_env(root: &Path) -> CustomCommandEnv {
    CustomCommandEnv {
        user_commands_dir: root.join("codex-home/commands"),
        project_commands_dirs: vec![root.join("repo/.codex/commands")],
        pipeline_dirs: vec![root.join("codex-home/pipelines")],
        cwd: root.join("repo"),
        home: Some(root.join("home")),
    }
}

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    fs::write(path, contents).expect("write file");
}

fn command(name: &str, body: &str) -> CustomCommand {
    CustomCommand {
        name: name.to_string(),
        description: DEFAULT_DESCRIPTION.to_string(),
        argument_hint: None,
        skills: Vec::new(),
        files: Vec::new(),
        pipelines: Vec::new(),
        body: body.to_string(),
        source: CustomCommandSource::Bundled,
    }
}

fn bundled() -> Vec<(String, CustomCommandSource)> {
    ["examples", "photocraft", "video", "whiteboard"]
        .into_iter()
        .map(|name| (name.to_string(), CustomCommandSource::Bundled))
        .collect()
}

fn names(discovery: &CustomCommandDiscovery) -> Vec<(String, CustomCommandSource)> {
    discovery
        .commands
        .iter()
        .map(|command| (command.name.clone(), command.source.clone()))
        .collect()
}

#[test]
fn parses_full_frontmatter() {
    let contents = "---\ndescription: Ship a release\nargument-hint: \"<version>\"\nskills: [$release, changelog]\nfiles: ~/notes/release.md\npipelines:\n  - release-train\n---\n\nRelease $1 now.\n";
    let parsed = parse_custom_command(
        "release",
        contents,
        CustomCommandSource::User(PathBuf::from("/tmp/release.md")),
    )
    .expect("parse");

    assert_eq!(
        parsed,
        CustomCommand {
            name: "release".to_string(),
            description: "Ship a release".to_string(),
            argument_hint: Some("<version>".to_string()),
            skills: vec!["release".to_string(), "changelog".to_string()],
            files: vec!["~/notes/release.md".to_string()],
            pipelines: vec!["release-train".to_string()],
            body: "Release $1 now.".to_string(),
            source: CustomCommandSource::User(PathBuf::from("/tmp/release.md")),
        }
    );
    assert_eq!(parsed.popup_description(), "Ship a release <version>");
}

#[test]
fn parses_without_frontmatter_using_first_line_as_description() {
    let parsed = parse_custom_command(
        "fix",
        "\n# Fix the failing test\n\nFind and fix it.",
        CustomCommandSource::Bundled,
    )
    .expect("parse");

    assert_eq!(
        parsed,
        CustomCommand {
            description: "Fix the failing test".to_string(),
            ..command("fix", "# Fix the failing test\n\nFind and fix it.")
        }
    );
}

#[test]
fn rejects_malformed_command_files() {
    let cases = [
        (
            "ok",
            "---\ndescription: x\nno closing line",
            "frontmatter starts with `---` but has no closing `---` line",
        ),
        ("ok", "---\n---\n   \n", "command body is empty"),
        (
            "Bad Name",
            "body",
            "command names may only use lowercase letters, digits, `-` and `_`, and must start with a letter or digit",
        ),
    ];
    for (name, contents, expected) in cases {
        assert_eq!(
            parse_custom_command(name, contents, CustomCommandSource::Bundled),
            Err(expected.to_string()),
            "{name}: {contents}"
        );
    }

    let invalid_yaml = parse_custom_command(
        "ok",
        "---\ndescription: [unclosed\n---\nbody",
        CustomCommandSource::Bundled,
    )
    .expect_err("invalid yaml should fail");
    assert!(
        invalid_yaml.starts_with("invalid frontmatter: "),
        "{invalid_yaml}"
    );
}

#[test]
fn discovery_applies_precedence_and_skips_bad_files() {
    let tmp = TempDir::new().expect("tempdir");
    let env = test_env(tmp.path());
    let user_video = env.user_commands_dir.join("video.md");
    let user_deploy = env.user_commands_dir.join("deploy.md");
    let project_deploy = tmp.path().join("repo/.codex/commands/deploy.md");
    write(
        &user_video,
        "---\ndescription: my video\n---\nUser video $ARGUMENTS",
    );
    write(&user_deploy, "User deploy");
    write(&project_deploy, "Project deploy");
    write(
        &env.user_commands_dir.join("review.md"),
        "Shadow a built-in",
    );
    write(
        &env.user_commands_dir.join("broken.md"),
        "---\nnever closed",
    );
    write(&env.user_commands_dir.join("notes.txt"), "not a command");

    let discovery = discover_custom_commands(&env);

    assert_eq!(
        names(&discovery),
        vec![
            (
                "deploy".to_string(),
                CustomCommandSource::Project(project_deploy)
            ),
            ("examples".to_string(), CustomCommandSource::Bundled),
            ("photocraft".to_string(), CustomCommandSource::Bundled),
            ("video".to_string(), CustomCommandSource::User(user_video)),
            ("whiteboard".to_string(), CustomCommandSource::Bundled),
        ]
    );
    assert_eq!(
        discovery.warnings,
        vec![
            format!(
                "Skipped custom command {}: frontmatter starts with `---` but has no closing `---` line",
                env.user_commands_dir.join("broken.md").display()
            ),
            "Skipped custom command /review: the name is used by a built-in command".to_string(),
        ]
    );
}

#[test]
fn discovery_without_files_returns_bundled_commands() {
    let tmp = TempDir::new().expect("tempdir");
    let discovery = discover_custom_commands(&test_env(tmp.path()));

    assert_eq!(names(&discovery), bundled());
    assert_eq!(discovery.warnings, Vec::<String>::new());
}

#[test]
fn untrusted_projects_do_not_contribute_commands() {
    let tmp = TempDir::new().expect("tempdir");
    let codex_home = tmp.path().join("codex-home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(repo.join(".git")).expect("git dir");
    write(&repo.join(".codex/commands/deploy.md"), "Project deploy");

    let untrusted = CustomCommandEnv::resolve(&codex_home, &repo, ProjectTrust::NotTrusted);
    assert_eq!(untrusted.project_commands_dirs, Vec::<PathBuf>::new());
    assert_eq!(names(&discover_custom_commands(&untrusted)), bundled());

    let trusted = CustomCommandEnv::resolve(&codex_home, &repo, ProjectTrust::Trusted);
    assert_eq!(
        trusted.project_commands_dirs,
        vec![repo.join(".codex/commands")]
    );
    assert_eq!(
        trusted.pipeline_dirs,
        vec![repo.join(".codex/pipelines"), codex_home.join("pipelines")]
    );
}

#[test]
fn bundled_commands_declare_their_skills_and_pipelines() {
    let tmp = TempDir::new().expect("tempdir");
    let discovery = discover_custom_commands(&test_env(tmp.path()));
    let declared: Vec<(String, Vec<String>, Vec<String>, bool)> = discovery
        .commands
        .iter()
        .map(|command| {
            (
                command.name.clone(),
                command.skills.clone(),
                command.pipelines.clone(),
                command.argument_hint.is_some(),
            )
        })
        .collect();

    let strings = |values: &[&str]| -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    };
    assert_eq!(
        declared,
        vec![
            ("examples".to_string(), Vec::new(), Vec::new(), true),
            (
                "photocraft".to_string(),
                strings(&["photocraft"]),
                Vec::new(),
                true
            ),
            (
                "video".to_string(),
                strings(&[
                    "photocraft",
                    "blender",
                    "remotion-video-production",
                    "ffmpeg-skill"
                ]),
                strings(&[
                    "blender-motion-graphics",
                    "storyboard-pipeline-creator",
                    "high-end-whiteboard"
                ]),
                true
            ),
            (
                "whiteboard".to_string(),
                strings(&[
                    "high-end-whiteboard",
                    "photocraft",
                    "imagemagick",
                    "whiteboard-animator",
                    "remotion-video-production"
                ]),
                strings(&["high-end-whiteboard"]),
                true
            ),
        ]
    );
}

#[test]
fn fingerprint_changes_only_when_command_files_change() {
    let tmp = TempDir::new().expect("tempdir");
    let env = test_env(tmp.path());
    let empty = command_dirs_fingerprint(&env);
    assert_eq!(command_dirs_fingerprint(&env), empty);

    let file = env.user_commands_dir.join("standup.md");
    write(&file, "Write my standup notes.");
    write(&env.user_commands_dir.join("notes.txt"), "ignored");
    let one_file = command_dirs_fingerprint(&env);
    assert_ne!(one_file, empty);
    assert_eq!(command_dirs_fingerprint(&env), one_file);

    write(&file, "Write my standup notes, shorter please.");
    assert_ne!(command_dirs_fingerprint(&env), one_file);

    fs::remove_file(&file).expect("remove");
    assert_eq!(
        command_dirs_fingerprint(&env),
        command_dirs_fingerprint(&test_env(tmp.path()))
    );
    assert_eq!(command_dirs_fingerprint(&env), empty);
}
