use super::*;
use crate::overmind::custom_commands::CustomCommandDiscovery;
use crate::overmind::custom_commands::CustomCommandSource;
use crate::overmind::custom_commands::parse_custom_command;
use pretty_assertions::assert_eq;

fn install_custom_command(chat: &mut ChatWidget, name: &str, contents: &str) {
    let command = parse_custom_command(name, contents, CustomCommandSource::Bundled)
        .expect("valid custom command");
    chat.bottom_pane
        .set_custom_commands(CustomCommandDiscovery {
            commands: vec![std::sync::Arc::new(command)],
            warnings: Vec::new(),
        });
}

#[tokio::test]
async fn custom_command_submission_sends_expanded_turn() {
    let (mut chat, _events, mut operations) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    install_custom_command(&mut chat, "greet", "Say hello to $ARGUMENTS.");

    chat.apply_external_edit("/greet the whole team".to_string());
    chat.handle_key_event(KeyEvent::from(KeyCode::Enter));

    let Op::UserTurn { items, .. } = next_submit_op(&mut operations) else {
        panic!("expected submitted user turn");
    };
    assert_eq!(
        items,
        vec![UserInput::Text {
            text: "Say hello to the whole team.".into(),
            text_elements: Vec::new(),
        }]
    );
}

#[tokio::test]
async fn unknown_slash_text_is_not_expanded() {
    let (mut chat, _events, mut operations) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    install_custom_command(&mut chat, "greet", "Say hello to $ARGUMENTS.");

    let message = UserMessage {
        text: "/greeting everyone".to_string(),
        local_images: Vec::new(),
        remote_image_urls: Vec::new(),
        text_elements: Vec::new(),
        mention_bindings: Vec::new(),
    };
    assert_eq!(
        chat.expand_custom_command_submission(message).text,
        "/greeting everyone"
    );
    assert_no_submit_op(&mut operations);
}

fn skill_metadata(name: &str, path: &std::path::Path) -> SkillMetadata {
    SkillMetadata {
        name: name.to_string(),
        description: format!("{name} skill"),
        short_description: None,
        interface: None,
        dependencies: None,
        path: path.to_path_buf().abs().into(),
        scope: crate::test_support::skill_scope_user(),
        enabled: true,
        plugin_id: None,
    }
}

#[tokio::test]
async fn custom_command_attaches_plugin_and_user_skills_by_path() {
    let (mut chat, _events, mut operations) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    let plugin_skill = test_path_buf("/tmp/plugins/photocraft/skills/photocraft/SKILL.md");
    let user_skill = test_path_buf("/tmp/user/skills/video/SKILL.md");
    chat.set_skills(Some(vec![
        skill_metadata("photocraft:photocraft", &plugin_skill),
        skill_metadata("video", &user_skill),
    ]));
    install_custom_command(
        &mut chat,
        "thumb",
        "---\nskills: [photocraft, video, missing-skill]\n---\nMake a thumbnail for $ARGUMENTS.",
    );

    chat.apply_external_edit("/thumb the launch".to_string());
    chat.handle_key_event(KeyEvent::from(KeyCode::Enter));

    let Op::UserTurn { items, .. } = next_submit_op(&mut operations) else {
        panic!("expected submitted user turn");
    };
    assert_eq!(
        items,
        vec![
            UserInput::Text {
                text: "Make a thumbnail for the launch.\n\nContext for /thumb (loaded by Overmind):\n- Use these skills: $photocraft:photocraft $video\n- These skills are not installed or are disabled; skip them: missing-skill".into(),
                text_elements: Vec::new(),
            },
            UserInput::Skill {
                name: "photocraft:photocraft".to_string(),
                path: plugin_skill,
            },
            UserInput::Skill {
                name: "video".to_string(),
                path: user_skill,
            },
        ]
    );
}

#[tokio::test]
async fn commands_listing_shows_custom_commands_and_warnings() {
    let (mut chat, mut events, _operations) = make_chatwidget_manual(/*model_override*/ None).await;
    let command = parse_custom_command(
        "standup",
        "---\ndescription: write my standup\n---\nWrite my standup notes.",
        CustomCommandSource::Bundled,
    )
    .expect("valid custom command");
    chat.bottom_pane
        .set_custom_commands(CustomCommandDiscovery {
            commands: vec![std::sync::Arc::new(command)],
            warnings: vec![
                "Skipped custom command /review: the name is used by a built-in command"
                    .to_string(),
            ],
        });
    drain_insert_history(&mut events);

    chat.show_custom_commands();

    let rendered = drain_insert_history(&mut events)
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect::<String>();
    assert!(
        rendered.contains("/commands · Overmind custom commands"),
        "{rendered}"
    );
    assert!(rendered.contains("write my standup"), "{rendered}");
    assert!(
        rendered.contains("Skipped custom command /review: the name is used by a built-in command"),
        "{rendered}"
    );
}
