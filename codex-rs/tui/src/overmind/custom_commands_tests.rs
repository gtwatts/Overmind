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
            ("video".to_string(), CustomCommandSource::User(user_video)),
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
fn discovery_without_files_returns_bundled_video() {
    let tmp = TempDir::new().expect("tempdir");
    let discovery = discover_custom_commands(&test_env(tmp.path()));

    assert_eq!(
        names(&discovery),
        vec![("video".to_string(), CustomCommandSource::Bundled)]
    );
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
    assert_eq!(
        names(&discover_custom_commands(&untrusted)),
        vec![("video".to_string(), CustomCommandSource::Bundled)]
    );

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
fn substitutes_argument_placeholders() {
    let tmp = TempDir::new().expect("tempdir");
    let env = test_env(tmp.path());

    assert_eq!(
        expand_custom_command(
            &command(
                "x",
                "All: $ARGUMENTS | first: $1 | second: $2 | missing: $3 | cost: $$5 | $HOME"
            ),
            "  alpha beta  ",
            &env,
        ),
        "All: alpha beta | first: alpha | second: beta | missing:  | cost: $5 | $HOME"
    );
}

#[test]
fn appends_request_when_body_has_no_placeholders() {
    let tmp = TempDir::new().expect("tempdir");
    let env = test_env(tmp.path());
    let cmd = command("x", "Summarize the repo.");

    assert_eq!(
        expand_custom_command(&cmd, "focus on tests", &env),
        "Summarize the repo.\n\n## Request\n\nfocus on tests"
    );
    assert_eq!(
        expand_custom_command(&cmd, "   ", &env),
        "Summarize the repo."
    );
}

#[test]
fn renders_declared_context() {
    let tmp = TempDir::new().expect("tempdir");
    let env = test_env(tmp.path());
    let home = tmp.path().join("home");
    let pipeline = tmp.path().join("codex-home/pipelines/whiteboard");
    write(&home.join("notes/style.md"), "style");
    write(&pipeline.join("pipeline.yaml"), "{}");
    write(&pipeline.join("PIPELINE.md"), "# whiteboard");
    let cmd = CustomCommand {
        skills: vec!["photocraft".to_string(), "blender".to_string()],
        files: vec!["~/notes/style.md".to_string(), "missing.md".to_string()],
        pipelines: vec!["whiteboard".to_string(), "nope".to_string()],
        ..command("video", "Brief: $ARGUMENTS")
    };

    let expected = format!(
        "Brief: promo for a bakery\n\nContext for /video (loaded by Overmind):\n- Use these skills: $photocraft $blender\n- Read these files before starting:\n  - {}\n  - {} (not found; skip it)\n- Use these pipelines as guidance (pipeline.yaml holds the stages and dependencies, PIPELINE.md the guide):\n  - whiteboard: {}, {}\n  - nope (pipeline not found; skip it)",
        home.join("notes/style.md").display(),
        tmp.path().join("repo/missing.md").display(),
        pipeline.join("pipeline.yaml").display(),
        pipeline.join("PIPELINE.md").display(),
    );
    assert_eq!(
        expand_custom_command(&cmd, "promo for a bakery", &env),
        expected
    );
}

#[test]
fn bundled_video_command_expands_brief_and_context() {
    let tmp = TempDir::new().expect("tempdir");
    let env = test_env(tmp.path());
    let discovery = discover_custom_commands(&env);
    let video = discovery
        .commands
        .iter()
        .find(|command| command.name == "video")
        .expect("bundled video command");

    assert_eq!(
        video.argument_hint.as_deref(),
        Some("<who it's for / kind of video / style>")
    );
    let expanded = expand_custom_command(video, "30 s social ad for a coffee brand", &env);
    assert!(
        expanded.contains("Brief: 30 s social ad for a coffee brand"),
        "{expanded}"
    );
    assert!(
        expanded.contains(
            "- Use these skills: $photocraft $blender $remotion-video-production $ffmpeg-skill"
        ),
        "{expanded}"
    );
    assert!(
        expanded.contains("  - blender-motion-graphics (pipeline not found; skip it)"),
        "{expanded}"
    );
}
