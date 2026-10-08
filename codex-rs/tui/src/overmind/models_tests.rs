use super::*;
use pretty_assertions::assert_eq;

fn cursor_presets() -> Vec<ModelPreset> {
    codex_overmind_cursor::model_catalog()
        .expect("catalog")
        .models
        .into_iter()
        .map(ModelPreset::from)
        .collect()
}

#[test]
fn cursor_models_are_appended_once_and_never_become_the_default() {
    let mut models = vec![cursor_presets().remove(0)];
    models[0].model = "gpt-test".to_string();
    models[0].is_default = true;
    let mut extra = cursor_presets();
    extra[0].is_default = true;
    let expected: Vec<String> = std::iter::once("gpt-test".to_string())
        .chain(
            extra
                .iter()
                .filter(|preset| preset.show_in_picker)
                .map(|preset| preset.model.clone()),
        )
        .collect();

    append_presets(&mut models, extra);
    append_presets(&mut models, cursor_presets());

    assert_eq!(
        models.iter().map(|m| m.model.clone()).collect::<Vec<_>>(),
        expected
    );
    assert_eq!(models.iter().filter(|m| m.is_default).count(), 1);
    assert!(
        models
            .iter()
            .skip(1)
            .all(|m| m.display_name.contains("Cursor"))
    );
}

#[test]
fn only_crossing_the_cursor_boundary_needs_a_new_session() {
    assert!(needs_new_session("openai", "composer-2.5"));
    assert!(needs_new_session("cursor", "gpt-6-astra"));
    assert!(!needs_new_session("cursor", "grok-4.7"));
    assert!(!needs_new_session("openai", "gpt-6-astra"));
    assert!(!needs_new_session("ollama", "qwen3-coder:30b"));
}

#[test]
fn switch_message_names_the_provider() {
    assert_eq!(
        switched_message("grok-4.7").0,
        "Started a new session on grok-4.7 (Cursor)."
    );
    assert_eq!(
        switched_message("gpt-6-astra").0,
        "Started a new session on gpt-6-astra (the default provider)."
    );
}
