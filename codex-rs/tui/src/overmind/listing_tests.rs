use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use pretty_assertions::assert_eq;

use super::*;
use crate::overmind::custom_commands::parse_custom_command;

fn test_env(root: &Path) -> CustomCommandEnv {
    CustomCommandEnv {
        user_commands_dir: root.join("home/.codex/commands"),
        project_commands_dirs: vec![root.join("repo/.codex/commands")],
        pipeline_dirs: Vec::new(),
        cwd: root.join("repo"),
        home: Some(root.join("home")),
    }
}

fn plain(lines: &[Line<'static>]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn lists_commands_sources_skill_resolution_and_warnings() {
    let root = PathBuf::from("/r");
    let env = test_env(&root);
    let video = parse_custom_command(
        "video",
        "---\ndescription: start a video job\nargument-hint: <brief>\nskills: [photocraft, video, nope]\n---\nBrief: $ARGUMENTS",
        CustomCommandSource::User(root.join("home/.codex/commands/video.md")),
    )
    .expect("video");
    let deploy = parse_custom_command(
        "deploy",
        "Deploy it.",
        CustomCommandSource::Project(root.join("repo/.codex/commands/deploy.md")),
    )
    .expect("deploy");
    let examples = parse_custom_command(
        "examples",
        "---\ndescription: find references\n---\nFind $ARGUMENTS",
        CustomCommandSource::Bundled,
    )
    .expect("examples");
    let discovery = CustomCommandDiscovery {
        commands: vec![Arc::new(deploy), Arc::new(examples), Arc::new(video)],
        warnings: vec![
            "Skipped custom command /review: the name is used by a built-in command".to_string(),
        ],
    };
    let available = vec![
        AvailableSkill {
            name: "photocraft:photocraft".to_string(),
            path: "/p/photocraft/SKILL.md".to_string(),
        },
        AvailableSkill {
            name: "video".to_string(),
            path: "/u/video/SKILL.md".to_string(),
        },
    ];

    assert_eq!(
        plain(&render_command_listing(&discovery, &env, Some(&available))),
        vec![
            "/commands · Overmind custom commands",
            "  /deploy    Deploy it.",
            "             project: /r/repo/.codex/commands/deploy.md",
            "  /examples  find references",
            "             bundled with Overmind",
            "  /video     start a video job <brief>",
            "             ~/.codex/commands/video.md · skills: photocraft → photocraft:photocraft, video, nope (not installed)",
            "",
            "Skipped",
            "  ⚠ Skipped custom command /review: the name is used by a built-in command",
            "",
            "Add a command by saving <name>.md in ~/.codex/commands or /r/repo/.codex/commands. Built-in commands win name clashes; edits apply the next time the / popup opens.",
        ]
    );
}

#[test]
fn empty_listing_still_explains_where_commands_live() {
    let env = test_env(Path::new("/r"));
    assert_eq!(
        plain(&render_command_listing(
            &CustomCommandDiscovery::default(),
            &env,
            /*available_skills*/ None
        )),
        vec![
            "/commands · Overmind custom commands",
            "  (none)",
            "",
            "Add a command by saving <name>.md in ~/.codex/commands or /r/repo/.codex/commands. Built-in commands win name clashes; edits apply the next time the / popup opens.",
        ]
    );
}
