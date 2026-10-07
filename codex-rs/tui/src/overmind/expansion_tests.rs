use std::fs;
use std::path::Path;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::*;
use crate::overmind::custom_commands::CustomCommandSource;
use crate::overmind::custom_commands::discover_custom_commands;

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
        description: "custom command".to_string(),
        argument_hint: None,
        skills: Vec::new(),
        files: Vec::new(),
        pipelines: Vec::new(),
        body: body.to_string(),
        source: CustomCommandSource::Bundled,
    }
}

fn skill(name: &str, path: &str) -> AvailableSkill {
    AvailableSkill {
        name: name.to_string(),
        path: path.to_string(),
    }
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
            /*available_skills*/ None,
        )
        .text,
        "All: alpha beta | first: alpha | second: beta | missing:  | cost: $5 | $HOME"
    );
}

#[test]
fn appends_request_when_body_has_no_placeholders() {
    let tmp = TempDir::new().expect("tempdir");
    let env = test_env(tmp.path());
    let cmd = command("x", "Summarize the repo.");

    assert_eq!(
        expand_custom_command(&cmd, "focus on tests", &env, /*available_skills*/ None).text,
        "Summarize the repo.\n\n## Request\n\nfocus on tests"
    );
    assert_eq!(
        expand_custom_command(&cmd, "   ", &env, /*available_skills*/ None).text,
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
        expand_custom_command(
            &cmd,
            "promo for a bakery",
            &env,
            /*available_skills*/ None
        ),
        ExpandedCommand {
            text: expected,
            skills: Vec::new(),
        }
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
    let expanded = expand_custom_command(
        video,
        "30 s social ad for a coffee brand",
        &env,
        /*available_skills*/ None,
    )
    .text;
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

#[test]
fn resolved_skills_are_attached_and_named_in_full() {
    let tmp = TempDir::new().expect("tempdir");
    let env = test_env(tmp.path());
    let available = vec![
        skill("video", "/u/skills/video/SKILL.md"),
        skill("photocraft:photocraft", "/p/photocraft/SKILL.md"),
        skill("blender-mcp:blender", "/p/blender/SKILL.md"),
        skill("a:shared", "/a/shared/SKILL.md"),
        skill("b:shared", "/b/shared/SKILL.md"),
    ];
    let cmd = CustomCommand {
        skills: vec![
            "photocraft".to_string(),
            "blender".to_string(),
            "video".to_string(),
            "shared".to_string(),
            "ffmpeg-skill".to_string(),
        ],
        ..command("video", "Brief: $ARGUMENTS")
    };

    assert_eq!(
        expand_custom_command(&cmd, "teaser", &env, Some(&available)),
        ExpandedCommand {
            text: "Brief: teaser\n\nContext for /video (loaded by Overmind):\n- Use these skills: $photocraft:photocraft $blender-mcp:blender $video\n- These skills are not installed or are disabled; skip them: ffmpeg-skill\n- These skill names are ambiguous and were not loaded: shared (a:shared, b:shared)".to_string(),
            skills: vec![
                available[1].clone(),
                available[2].clone(),
                available[0].clone(),
            ],
        }
    );
}

#[test]
fn bundled_whiteboard_attaches_plugin_skills_and_points_at_the_pipeline() {
    let tmp = TempDir::new().expect("tempdir");
    let env = test_env(tmp.path());
    let pipeline = tmp.path().join("codex-home/pipelines/high-end-whiteboard");
    write(&pipeline.join("pipeline.yaml"), "{}");
    write(&pipeline.join("PIPELINE.md"), "# high-end-whiteboard");
    let discovery = discover_custom_commands(&env);
    let whiteboard = discovery
        .commands
        .iter()
        .find(|command| command.name == "whiteboard")
        .expect("bundled whiteboard command");
    // Named the way the skills list reports Todd's installed plugin skills.
    let available = vec![
        skill(
            "gordon-workflows:high-end-whiteboard",
            "/c/gordon-workflows/skills/high-end-whiteboard/SKILL.md",
        ),
        skill(
            "photocraft:photocraft",
            "/c/photocraft/skills/photocraft/SKILL.md",
        ),
        skill(
            "gordon-skills:imagemagick",
            "/c/gordon-skills/skills/imagemagick/SKILL.md",
        ),
        skill(
            "gordon-skills:whiteboard-animator",
            "/c/gordon-skills/skills/whiteboard-animator/SKILL.md",
        ),
        skill(
            "gordon-skills:remotion-video-production",
            "/c/gordon-skills/skills/remotion-video-production/SKILL.md",
        ),
    ];

    let expanded = expand_custom_command(
        whiteboard,
        "how a heat pump works, for homeowners",
        &env,
        Some(&available),
    );

    assert_eq!(expanded.skills, available);
    assert!(
        expanded
            .text
            .contains("Brief: how a heat pump works, for homeowners"),
        "{}",
        expanded.text
    );
    assert!(
        expanded.text.contains("- Use these skills: $gordon-workflows:high-end-whiteboard $photocraft:photocraft $gordon-skills:imagemagick $gordon-skills:whiteboard-animator $gordon-skills:remotion-video-production"),
        "{}",
        expanded.text
    );
    assert!(
        expanded.text.contains(&format!(
            "  - high-end-whiteboard: {}, {}",
            pipeline.join("pipeline.yaml").display(),
            pipeline.join("PIPELINE.md").display()
        )),
        "{}",
        expanded.text
    );
}

#[test]
fn bundled_photocraft_keeps_literal_home_variable() {
    let tmp = TempDir::new().expect("tempdir");
    let env = test_env(tmp.path());
    let discovery = discover_custom_commands(&env);
    let photocraft = discovery
        .commands
        .iter()
        .find(|command| command.name == "photocraft")
        .expect("bundled photocraft command");

    let expanded = expand_custom_command(
        photocraft,
        "lower third for Jane Doe",
        &env,
        /*available_skills*/ None,
    );

    assert!(
        expanded.text.contains("Request: lower third for Jane Doe"),
        "{}",
        expanded.text
    );
    assert!(
        expanded
            .text
            .contains("MCP file access is limited to `$HOME` and `/tmp`."),
        "{}",
        expanded.text
    );
    assert!(
        expanded.text.contains("- Use these skills: $photocraft"),
        "{}",
        expanded.text
    );
}
