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
