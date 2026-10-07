use std::fs;
use std::path::Path;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::unbounded_channel;

use super::*;
use crate::bottom_pane::AppEventSender;
use crate::bottom_pane::chat_composer::ActivePopup;
use crate::bottom_pane::chat_composer::InputResult;

fn composer_with_env(root: &Path) -> (ChatComposer, UnboundedReceiver<AppEvent>) {
    let (tx, rx) = unbounded_channel::<AppEvent>();
    let mut composer = ChatComposer::new(
        /*has_input_focus*/ true,
        AppEventSender::new(tx),
        /*enhanced_keys_supported*/ false,
        "Ask Codex to do anything".to_string(),
        /*disable_paste_burst*/ false,
    );
    composer.set_custom_commands_env(Some(CustomCommandEnv {
        user_commands_dir: root.join("codex-home/commands"),
        project_commands_dirs: vec![root.join("repo/.codex/commands")],
        pipeline_dirs: Vec::new(),
        cwd: root.join("repo"),
        home: Some(root.join("home")),
    }));
    (composer, rx)
}

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    fs::write(path, contents).expect("write file");
}

fn set_draft(composer: &mut ChatComposer, text: &str) {
    composer.draft.textarea.set_text_clearing_elements(text);
    composer.draft.textarea.set_cursor(text.len());
    composer.sync_popups();
}

fn custom_names(composer: &ChatComposer) -> Vec<String> {
    composer
        .custom_commands()
        .iter()
        .map(|command| command.name.clone())
        .collect()
}

fn description_of(composer: &ChatComposer, name: &str) -> Option<String> {
    composer
        .custom_commands()
        .iter()
        .find(|command| command.name == name)
        .map(|command| command.description.clone())
}

fn submitted_text_after_enter(composer: &mut ChatComposer, draft: &str) -> String {
    set_draft(composer, draft);
    match composer
        .handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .0
    {
        InputResult::Submitted { text, .. } => text,
        other => panic!("expected submitted text, got {other:?}"),
    }
}

fn history_cell_count(rx: &mut UnboundedReceiver<AppEvent>) -> usize {
    let mut count = 0;
    while let Ok(event) = rx.try_recv() {
        if matches!(event, AppEvent::InsertHistoryCell(_)) {
            count += 1;
        }
    }
    count
}

#[test]
fn opening_the_popup_picks_up_new_and_edited_files() {
    let tmp = TempDir::new().expect("tempdir");
    let (mut composer, _rx) = composer_with_env(tmp.path());
    let user_dir = tmp.path().join("codex-home/commands");

    set_draft(&mut composer, "/");
    assert!(matches!(composer.popups.active, ActivePopup::Command(_)));
    assert_eq!(
        custom_names(&composer),
        vec!["examples", "photocraft", "video", "whiteboard"]
    );
    set_draft(&mut composer, "");

    write(
        &user_dir.join("standup.md"),
        "---\ndescription: write my standup\n---\nWrite my standup notes.",
    );
    set_draft(&mut composer, "/sta");
    assert!(matches!(composer.popups.active, ActivePopup::Command(_)));
    assert_eq!(
        description_of(&composer, "standup").as_deref(),
        Some("write my standup")
    );
    set_draft(&mut composer, "");

    write(
        &user_dir.join("standup.md"),
        "---\ndescription: standup, edited\n---\nWrite my standup notes.",
    );
    set_draft(&mut composer, "/");
    assert_eq!(
        description_of(&composer, "standup").as_deref(),
        Some("standup, edited")
    );
}

#[test]
fn an_open_popup_picks_up_a_file_created_while_typing() {
    let tmp = TempDir::new().expect("tempdir");
    let (mut composer, _rx) = composer_with_env(tmp.path());

    set_draft(&mut composer, "/s");
    assert!(matches!(composer.popups.active, ActivePopup::Command(_)));
    assert_eq!(description_of(&composer, "standup"), None);

    write(
        &tmp.path().join("codex-home/commands/standup.md"),
        "---\ndescription: write my standup\n---\nWrite my standup notes.",
    );
    set_draft(&mut composer, "/st");
    assert!(matches!(composer.popups.active, ActivePopup::Command(_)));
    assert_eq!(
        description_of(&composer, "standup").as_deref(),
        Some("write my standup")
    );
    assert_eq!(
        submitted_text_after_enter(&mut composer, "/standup"),
        "/standup"
    );
}

#[test]
fn submission_rescans_before_rejecting_an_unknown_command() {
    let tmp = TempDir::new().expect("tempdir");
    let (mut composer, _rx) = composer_with_env(tmp.path());

    // The popup opened before the file existed.
    set_draft(&mut composer, "/");
    write(
        &tmp.path().join("repo/.codex/commands/fresh.md"),
        "Fresh: $ARGUMENTS",
    );
    set_draft(&mut composer, "/fresh go");

    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match result {
        InputResult::Submitted { text, .. } => assert_eq!(text, "/fresh go"),
        other => panic!("expected the custom command to submit, got {other:?}"),
    }
}

#[test]
fn rescans_report_each_new_warning_once() {
    let tmp = TempDir::new().expect("tempdir");
    let (mut composer, mut rx) = composer_with_env(tmp.path());
    assert!(composer.refresh_custom_commands());
    assert_eq!(history_cell_count(&mut rx), 0);

    write(
        &tmp.path().join("codex-home/commands/broken.md"),
        "---\nnever closed",
    );
    assert!(composer.refresh_custom_commands());
    assert_eq!(history_cell_count(&mut rx), 1);
    assert!(!composer.refresh_custom_commands());
    assert_eq!(history_cell_count(&mut rx), 0);
    assert_eq!(composer.custom_command_discovery().warnings.len(), 1);
}
