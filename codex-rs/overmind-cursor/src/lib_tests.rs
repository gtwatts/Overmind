use std::collections::HashMap;

use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;
use pretty_assertions::assert_eq;

use super::*;

#[test]
fn cursor_provider_overrides_a_user_defined_cursor_table() {
    let user_defined = ModelProviderInfo {
        name: "my sidecar".to_string(),
        base_url: Some("http://127.0.0.1:18791/v1".to_string()),
        ..ModelProviderInfo::default()
    };
    let providers = with_cursor_provider(HashMap::from([(
        CURSOR_PROVIDER_ID.to_string(),
        user_defined,
    )]));
    assert_eq!(
        providers.get(CURSOR_PROVIDER_ID),
        Some(&ModelProviderInfo {
            name: "Cursor".to_string(),
            wire_api: WireApi::Responses,
            request_max_retries: Some(2),
            stream_max_retries: Some(2),
            stream_idle_timeout_ms: Some(600_000),
            requires_openai_auth: false,
            ..ModelProviderInfo::default()
        })
    );
}

#[test]
fn cursor_models_route_to_the_cursor_provider() {
    assert_eq!(
        [
            Some("grok-4.7"),
            Some("composer-2.5"),
            Some("gpt-6-astra"),
            Some("claude-opus-5-5"),
            None,
        ]
        .map(provider_for_model),
        [
            Some(CURSOR_PROVIDER_ID.to_string()),
            Some(CURSOR_PROVIDER_ID.to_string()),
            None,
            None,
            None,
        ]
    );
}

#[tokio::test]
async fn activate_points_the_provider_at_the_endpoint_with_the_cursor_key() {
    let codex_home = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(codex_home.path().join("secrets")).expect("secrets dir");
    std::fs::write(
        codex_home.path().join("secrets/cursor.env"),
        "CURSOR_API_KEY=key_from_file\n",
    )
    .expect("write secrets");
    let mut provider = provider_info();

    activate_with(
        &mut provider,
        codex_home.path(),
        /*api_key_env*/ None,
        Some("http://127.0.0.1:9/v1".to_string()),
    )
    .await
    .expect("activate");

    assert_eq!(
        provider,
        ModelProviderInfo {
            base_url: Some("http://127.0.0.1:9/v1".to_string()),
            experimental_bearer_token: Some("key_from_file".into()),
            ..provider_info()
        }
    );
}

#[tokio::test]
async fn activate_fails_without_a_cursor_key() {
    let codex_home = tempfile::tempdir().expect("tempdir");
    let mut provider = provider_info();

    let err = activate_with(
        &mut provider,
        codex_home.path(),
        /*api_key_env*/ None,
        Some("http://127.0.0.1:9/v1".to_string()),
    )
    .await
    .expect_err("missing key must fail");

    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    assert_eq!(provider, provider_info());
}
