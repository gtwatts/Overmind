use pretty_assertions::assert_eq;

use super::*;

#[test]
fn bundled_catalog_lists_exactly_the_supported_cursor_models() {
    let catalog = model_catalog().expect("catalog parses");
    let slugs: Vec<&str> = catalog
        .models
        .iter()
        .map(|model| model.slug.as_str())
        .collect();
    assert_eq!(slugs, CURSOR_MODEL_SLUGS.to_vec());
}

#[test]
fn bundled_catalog_uses_codex_base_instructions_and_real_metadata() {
    let catalog = model_catalog().expect("catalog parses");
    for model in &catalog.models {
        assert_eq!(
            (
                model.slug.as_str(),
                model.used_fallback_model_metadata,
                model
                    .model_messages
                    .as_ref()
                    .and_then(|messages| messages.instructions_template.as_deref()),
            ),
            (model.slug.as_str(), false, Some(BASE_INSTRUCTIONS)),
        );
    }
}
