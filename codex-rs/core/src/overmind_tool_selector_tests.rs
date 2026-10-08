use super::*;
use crate::session::tests::make_session_and_context;
use crate::tools::handlers::McpHandler;
use crate::tools::handlers::PlanHandler;
use crate::tools::handlers::ToolSearchHandlerCache;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::router::ToolRouter;
use crate::tools::spec_plan::finalize_tool_router;
use codex_extension_api::ToolPolicy;
use codex_mcp::ToolInfo;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ToolSpec;
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;
use std::sync::Arc;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;

fn user_text(text: &str) -> UserInput {
    UserInput::Text {
        text: text.to_string(),
        text_elements: Vec::new(),
    }
}

fn runtime(name: &str) -> Arc<dyn CoreToolRuntime> {
    Arc::new(McpHandler::new(ToolInfo {
        server_name: "fixture".to_string(),
        supports_parallel_tool_calls: true,
        server_origin: None,
        callable_name: name.to_string(),
        callable_namespace: "mcp__fixture".to_string(),
        namespace_description: Some("Fixture plugin tools".to_string()),
        tool: rmcp::model::Tool::new(
            name.to_string(),
            format!("Read-only {name} fixture"),
            Arc::new(rmcp::model::object(json!({
                "type": "object",
                "properties": {"city": {"type": "string", "description": "Exact city to look up"}},
                "required": ["city"],
                "additionalProperties": false
            }))),
        ),
        openai_file_input_optional_fields: Default::default(),
        connector_id: None,
        connector_name: None,
        plugin_display_names: Vec::new(),
    }).expect("fixture tool spec"))
}

fn tool_name(name: &str) -> ToolName {
    ToolName::namespaced("mcp__fixture", name)
}

fn fixture_registry() -> (ToolRegistry, HashSet<ToolName>) {
    let mut registry = ToolRegistry::default();
    registry.add(PlanHandler);
    for name in ["weather", "calendar"] {
        assert!(registry.register_external_with_exposure(runtime(name), ToolExposure::Deferred));
    }
    (
        registry,
        HashSet::from([tool_name("weather"), tool_name("calendar")]),
    )
}

fn exposures(registry: &ToolRegistry) -> BTreeMap<ToolName, ToolExposure> {
    registry
        .entries()
        .map(|tool| {
            (
                tool.runtime.tool_name().with_default_namespace(),
                tool.exposure,
            )
        })
        .collect()
}

fn answers(probabilities: &[f64]) -> Value {
    json!({
        "answers": probabilities.iter().enumerate().map(|(index, probability)| {
            json!({"type": "predicate", "name": format!("t{index}"), "probability": probability})
        }).collect::<Vec<_>>(),
        "usage": {"input_tokens": 123, "output_tokens": 2, "total_tokens": 125}
    })
}

async fn configured_turn(server: &MockServer) -> (tempfile::TempDir, TurnContext, ModelInfo) {
    let home = tempfile::tempdir().expect("selector test home");
    let key_path = home.path().join("test-key.env");
    std::fs::write(&key_path, "OPENMIND_TEST_SELECTOR_KEY='fixture-key'\n")
        .expect("fixture credential");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))
            .expect("private fixture credential");
    }
    let config = SelectorConfig {
        enabled: true,
        endpoint: format!("{}/v1/decisions", server.uri()),
        api_key_env: "OPENMIND_TEST_SELECTOR_KEY".to_string(),
        key_file: Some(key_path),
        ..Default::default()
    };
    write_config(home.path(), &config);
    let (_, mut turn) = make_session_and_context().await;
    Arc::make_mut(&mut turn.config).codex_home =
        codex_utils_absolute_path::AbsolutePathBuf::try_from(home.path())
            .expect("absolute selector home");
    turn.developer_instructions = Some("PRIVATE_DEVELOPER_INSTRUCTION_MUST_NOT_LEAVE".to_string());
    let mut model = (*turn.capture_current_model_info()).clone();
    model.supports_search_tool = true;
    model.tool_mode = Some(ToolMode::Direct);
    record_user_input(&turn, &[user_text("Get the weather for Paris")]);
    (home, turn, model)
}

fn write_config(home: &Path, config: &SelectorConfig) {
    let contents = format!(
        "[tool_selector]\n{}",
        toml::to_string(config).expect("fixture selector config")
    );
    std::fs::write(home.join(CONFIG_FILE), contents).expect("write selector config");
}

fn native_openai_model(slug: &str) -> ModelInfo {
    let model = codex_models_manager::bundled_models_response()
        .expect("bundled production model catalog")
        .models
        .into_iter()
        .find(|model| model.slug == slug)
        .expect("production OpenAI model");
    assert_eq!(model.tool_mode, Some(ToolMode::CodeModeOnly));
    assert!(model.supports_search_tool);
    model
}

fn exec_description(router: &ToolRouter) -> String {
    router
        .model_visible_specs()
        .iter()
        .find_map(|spec| match spec {
            ToolSpec::Freeform(tool) if tool.name == codex_code_mode::PUBLIC_TOOL_NAME => {
                Some(tool.description.clone())
            }
            _ => None,
        })
        .expect("native code-mode executor")
}

#[tokio::test]
async fn configuration_is_opt_in_and_uses_only_overmind_file() {
    let home = tempfile::tempdir().expect("home");
    assert!(
        !load_config(home.path())
            .await
            .expect("missing defaults")
            .enabled
    );
    std::fs::write(
        home.path().join("config.toml"),
        "[tool_selector]\nenabled=true\n",
    )
    .expect("stock config fixture");
    assert!(
        !load_config(home.path())
            .await
            .expect("ignores stock config")
            .enabled
    );
    std::fs::write(
        home.path().join(CONFIG_FILE),
        "[tui]\nhud=true\n[pipelines]\nauto_advance=false\n[tool_selector]\nenabled=true\n",
    )
    .expect("Overmind config");
    let config = load_config(home.path())
        .await
        .expect("valid selector config");
    assert!(config.enabled);
    assert_eq!(config.timeout_ms, 2000);
    assert_eq!(config.model, MODEL);
    std::fs::write(
        home.path().join(CONFIG_FILE),
        "[tool_selector]\nenabeld=true\n",
    )
    .expect("typo fixture");
    assert!(matches!(
        load_config(home.path()).await,
        Err(SelectorError::Config)
    ));
}

#[test]
fn invalid_config_rejects_unsupported_model_plaintext_remote_and_bad_limits() {
    for config in [
        SelectorConfig {
            model: "other-model".to_string(),
            ..Default::default()
        },
        SelectorConfig {
            endpoint: "http://example.com/v1/decisions".to_string(),
            ..Default::default()
        },
        SelectorConfig {
            endpoint: "https://user:password@api.openai.com/v1/decisions".to_string(),
            ..Default::default()
        },
        SelectorConfig {
            timeout_ms: 0,
            ..Default::default()
        },
        SelectorConfig {
            max_tools: 0,
            ..Default::default()
        },
        SelectorConfig {
            min_probability: f64::NAN,
            ..Default::default()
        },
    ] {
        assert_eq!(validate_config(&config), Err(SelectorError::Config));
    }
}

#[test]
fn key_parser_accepts_literal_assignments_and_rejects_shell_evaluation() {
    assert_eq!(
        parse_key_assignment(
            "# comment\nexport OPENAI_API_KEY = 'fixture-key'\n",
            "OPENAI_API_KEY"
        ),
        Some("fixture-key".to_string())
    );
    assert_eq!(
        parse_key_assignment("OPENAI_API_KEY=fixture-key # comment\n", "OPENAI_API_KEY"),
        Some("fixture-key".to_string())
    );
    for contents in [
        "OPENAI_API_KEY=$(cat secret)",
        "OPENAI_API_KEY=`cat secret`",
        "OPENAI_API_KEY=fixture;curl example.com",
        "OPENAI_API_KEY=\"$OTHER_KEY\"",
        "OPENAI_API_KEY=\"fixture-key\" extra",
        "OTHER_KEY=fixture-key",
        "OPENAI_API_KEY=",
    ] {
        assert_eq!(parse_key_assignment(contents, "OPENAI_API_KEY"), None);
    }
}

#[tokio::test]
async fn query_capture_reads_only_user_text_and_resets_cache_for_steering() {
    let (_, turn) = make_session_and_context().await;
    record_user_input(
        &turn,
        &[
            user_text("Original task"),
            UserInput::LocalImage {
                path: PathBuf::from("/private/image.png"),
                detail: None,
            },
            UserInput::Skill {
                name: "private-skill".to_string(),
                path: PathBuf::from("/private/SKILL.md"),
            },
        ],
    );
    let first = turn
        .extension_data
        .get::<TurnSelectorState>()
        .expect("query state");
    assert_eq!(first.query, "Original task");
    *first.cache.lock().await = Some(CachedSelection {
        key: vec![1],
        recorded_at: Instant::now(),
        result: Err(SelectorError::Timeout),
        usage: None,
    });
    record_user_input(&turn, &[user_text("Use the calendar too")]);
    let steered = turn
        .extension_data
        .get::<TurnSelectorState>()
        .expect("steered state");
    assert_eq!(steered.query, "Original task\n\nUse the calendar too");
    assert!(steered.cache.lock().await.is_none());
}

#[test]
fn catalog_keeps_full_specs_and_excludes_hidden_denied_and_native_tools() {
    let mut registry = ToolRegistry::with_tool_policy(Arc::new(ToolPolicy {
        allowed_tools: Some(vec![
            tool_name("weather"),
            tool_name("hidden"),
            ToolName::plain("update_plan"),
        ]),
        ..Default::default()
    }));
    registry.add(PlanHandler);
    registry.register_external_with_exposure(runtime("weather"), ToolExposure::Deferred);
    registry.register_external_with_exposure(runtime("hidden"), ToolExposure::Hidden);
    assert!(!registry.register_external_with_exposure(runtime("denied"), ToolExposure::Deferred));
    let eligible = HashSet::from([
        tool_name("weather"),
        tool_name("hidden"),
        tool_name("denied"),
        ToolName::plain("update_plan").with_default_namespace(),
    ]);
    let candidates = collect_candidates(&registry, &eligible).expect("eligible catalog");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].name, tool_name("weather"));
    let encoded = serde_json::to_string(&candidates[0].definition).expect("encoded full spec");
    assert!(encoded.contains("Exact city to look up"));
    assert!(encoded.contains("required"));
    assert!(encoded.contains("Read-only weather fixture"));
}

#[test]
fn promotion_preserves_unselected_tools_and_code_mode_availability() {
    let (mut registry, mut eligible) = fixture_registry();
    registry
        .register_external_with_exposure(runtime("model_only"), ToolExposure::DeferredModelOnly);
    registry.register_external_with_exposure(runtime("hidden"), ToolExposure::Hidden);
    eligible.extend([tool_name("model_only"), tool_name("hidden")]);
    apply_selection(
        &mut registry,
        &eligible,
        &[
            tool_name("weather"),
            tool_name("model_only"),
            tool_name("hidden"),
        ],
        ToolMode::Direct,
    );
    let actual = exposures(&registry);
    assert_eq!(actual[&tool_name("weather")], ToolExposure::Direct);
    assert_eq!(actual[&tool_name("calendar")], ToolExposure::Deferred);
    assert_eq!(
        actual[&tool_name("model_only")],
        ToolExposure::DirectModelOnly
    );
    assert_eq!(actual[&tool_name("hidden")], ToolExposure::Hidden);
    assert_eq!(
        actual[&ToolName::plain("update_plan").with_default_namespace()],
        ToolExposure::Direct
    );
}

#[test]
fn code_mode_preselection_cannot_expose_hidden_model_only_or_native_tools() {
    let (mut registry, mut eligible) = fixture_registry();
    registry
        .register_external_with_exposure(runtime("model_only"), ToolExposure::DeferredModelOnly);
    registry.register_external_with_exposure(runtime("hidden"), ToolExposure::Hidden);
    let native = ToolName::plain("update_plan").with_default_namespace();
    eligible.extend([tool_name("model_only"), tool_name("hidden"), native.clone()]);
    let before = exposures(&registry);
    apply_selection(
        &mut registry,
        &eligible,
        &[
            tool_name("weather"),
            tool_name("model_only"),
            tool_name("hidden"),
            native.clone(),
        ],
        ToolMode::CodeModeOnly,
    );
    assert_eq!(exposures(&registry), before);
    assert!(registry.is_code_mode_preselected(&tool_name("weather")));
    for name in [
        tool_name("calendar"),
        tool_name("model_only"),
        tool_name("hidden"),
        native,
    ] {
        assert!(!registry.is_code_mode_preselected(&name));
    }
}

#[test]
fn question_names_are_exact_and_refusal_or_malformed_answer_fails_completely() {
    let valid = answers(&[0.9, 0.1]);
    let (scores, usage) = parse_answers(&valid, 2, 0).expect("exact response");
    assert_eq!(scores, vec![(0, 0.9), (1, 0.1)]);
    assert_eq!(usage.input_tokens, Some(123));
    assert_eq!(usage.total_tokens, Some(125));
    let mut duplicate = valid.clone();
    duplicate["answers"][1]["name"] = json!("t0");
    let mut out_of_range = valid.clone();
    out_of_range["answers"][0]["probability"] = json!(1.1);
    for payload in [
        duplicate,
        out_of_range,
        json!({"answers": []}),
        json!({"answers": [{"type":"choice","name":"t0","choice":"yes"}]}),
        json!({"answers": [{"type":"predicate","name":"t0","probability":true}]}),
    ] {
        assert!(matches!(
            parse_answers(&payload, 2, 0),
            Err(SelectorError::InvalidResponse)
        ));
    }
    assert!(matches!(
        parse_answers(&json!({"answers": [{"type":"refusal","name":"t0"}]}), 2, 0),
        Err(SelectorError::Refusal)
    ));
}

#[test]
fn selector_usage_preserves_unknown_fields_and_only_derives_complete_totals() {
    for payload in [
        json!({}),
        json!({"usage": {}}),
        json!({"usage": {"input_tokens": -1, "output_tokens": "2"}}),
    ] {
        let metrics = AttemptMetrics::default();
        metrics.request_count.store(1, Ordering::Relaxed);
        metrics.record_usage(&payload);
        assert!(metrics.usage().is_none());
    }
    assert_eq!(
        serde_json::to_value(usage_from_payload(&json!({"usage": {"input_tokens": 7}})))
            .expect("partial usage JSON"),
        json!({"input_tokens": 7, "output_tokens": null, "total_tokens": null})
    );
    assert_eq!(
        usage_from_payload(&json!({"usage": {"input_tokens": 7, "output_tokens": 2}})).total_tokens,
        Some(9)
    );
    let metrics = AttemptMetrics::default();
    metrics.request_count.store(1, Ordering::Relaxed);
    metrics.record_usage(&json!({"usage": {"input_tokens": 0, "output_tokens": 0}}));
    assert_eq!(
        metrics
            .usage()
            .expect("reported zero is exact")
            .total_tokens,
        Some(0)
    );
}

#[test]
fn batch_usage_stays_unknown_after_missing_fields_or_unfinished_requests() {
    let metrics = AttemptMetrics::default();
    metrics.request_count.store(2, Ordering::Relaxed);
    metrics.record_usage(&json!({"usage": {"input_tokens": 7, "output_tokens": 3}}));
    assert!(
        metrics.usage().is_none(),
        "an unfinished batch has unknown cost"
    );
    metrics.record_usage(&json!({"usage": {"input_tokens": 5}}));
    let usage = metrics
        .usage()
        .expect("complete input counts remain useful");
    assert_eq!(usage.input_tokens, Some(12));
    assert_eq!(usage.output_tokens, None);
    assert_eq!(usage.total_tokens, None);
    metrics.request_count.store(3, Ordering::Relaxed);
    metrics.record_usage(&json!({"answers": []}));
    assert!(
        metrics.usage().is_none(),
        "missing batch counters make the sum unknown"
    );
}

#[tokio::test]
async fn successful_selection_promotes_tools_and_reuses_turn_catalog_cache() {
    let server = MockServer::start().await;
    let (_home, turn, model) = configured_turn(&server).await;
    // Catalog is sorted: calendar=t0, weather=t1.
    Mock::given(method("POST"))
        .and(path("/v1/decisions"))
        .and(header("authorization", "Bearer fixture-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(answers(&[0.1, 0.95])))
        .expect(1)
        .mount(&server)
        .await;
    let (mut registry, eligible) = fixture_registry();
    select_for_turn(&turn, &model, &mut registry, &eligible).await;
    assert_eq!(
        exposures(&registry)[&tool_name("weather")],
        ToolExposure::Direct
    );
    assert_eq!(
        exposures(&registry)[&tool_name("calendar")],
        ToolExposure::Deferred
    );
    let router = finalize_tool_router(
        &turn,
        &model,
        registry,
        Vec::new(),
        &ToolSearchHandlerCache::default(),
    )
    .expect("selected model tools");
    assert!(
        router
            .model_visible_specs()
            .iter()
            .any(|spec| matches!(spec, ToolSpec::ToolSearch { .. }))
    );
    let visible_specs = router.model_visible_specs();
    let namespace = visible_specs
        .iter()
        .find_map(|spec| match spec {
            ToolSpec::Namespace(namespace) if namespace.name == "mcp__fixture" => Some(namespace),
            _ => None,
        })
        .expect("selected plugin namespace");
    assert!(namespace.tools.iter().any(
        |tool| matches!(tool, ResponsesApiNamespaceTool::Function(tool) if tool.name == "weather")
    ));
    assert!(!namespace.tools.iter().any(
        |tool| matches!(tool, ResponsesApiNamespaceTool::Function(tool) if tool.name == "calendar")
    ));
    let (mut next_registry, eligible) = fixture_registry();
    select_for_turn(&turn, &model, &mut next_registry, &eligible).await;
    assert_eq!(
        exposures(&next_registry)[&tool_name("weather")],
        ToolExposure::Direct
    );
    let requests = server.received_requests().await.expect("selector requests");
    assert_eq!(requests.len(), 1);
    let body: Value = serde_json::from_slice(&requests[0].body).expect("request JSON");
    assert_eq!(body["model"], MODEL);
    let input: Value = serde_json::from_str(body["input"].as_str().expect("shared string input"))
        .expect("catalog input");
    assert_eq!(input["user_query"], "Get the weather for Paris");
    assert_eq!(input["tools"].as_array().expect("full catalog").len(), 2);
    let wire = String::from_utf8_lossy(&requests[0].body);
    assert!(wire.contains("Exact city to look up"));
    assert!(!wire.contains("PRIVATE_DEVELOPER_INSTRUCTION_MUST_NOT_LEAVE"));
    assert!(!wire.contains("fixture-key"));
}

#[tokio::test]
async fn native_openai_code_mode_preloads_bounded_definitions_without_direct_promotion() {
    // Use the bundled production modes: this must work without a Direct override.
    for (slug, strict) in [
        ("gpt-6.1-sol", false),
        ("gpt-6-astra", false),
        ("gpt-6.1-sol", true),
    ] {
        let server = MockServer::start().await;
        let (home, mut turn, _) = configured_turn(&server).await;
        if strict {
            Arc::make_mut(&mut turn.config)
                .features
                .enable(codex_features::Feature::CodeModeOnlyStrictThirdPartyTools)
                .expect("strict third-party test feature");
        }
        let model = native_openai_model(slug);
        let mut config = load_config(home.path()).await.expect("fixture config");
        config.max_tools = 1;
        write_config(home.path(), &config);
        Mock::given(method("POST"))
            .and(path("/v1/decisions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(answers(&[0.8, 0.95])))
            .expect(1)
            .mount(&server)
            .await;
        let (mut registry, eligible) = fixture_registry();
        let before = exposures(&registry);
        select_for_turn(&turn, &model, &mut registry, &eligible).await;
        assert_eq!(exposures(&registry), before);
        assert!(registry.is_code_mode_preselected(&tool_name("weather")));
        assert!(!registry.is_code_mode_preselected(&tool_name("calendar")));
        let router = finalize_tool_router(
            &turn,
            &model,
            registry,
            Vec::new(),
            &ToolSearchHandlerCache::default(),
        )
        .expect("native OpenAI selected tools");
        assert_eq!(router.tool_mode(), ToolMode::CodeModeOnly);
        assert!(router.requires_code_mode_worker());
        assert!(!router.model_visible_specs().iter().any(
            |spec| matches!(spec, ToolSpec::Namespace(namespace) if namespace.name == "mcp__fixture")
        ));
        let description = exec_description(&router);
        assert!(description.contains("mcp__fixture__weather"));
        assert!(description.contains("Exact city to look up"));
        assert!(!description.contains("mcp__fixture__calendar"));
        assert!(description.contains("ALL_TOOLS"));
        assert!(description.contains("update_plan"));
        for name in ["weather", "calendar"] {
            assert_eq!(
                router
                    .code_mode_tool_names()
                    .get(&codex_tools::code_mode_name_for_tool_name(&tool_name(name))),
                Some(&tool_name(name)),
                "unselected tools retain their executor route"
            );
        }
    }
}

#[tokio::test]
async fn native_code_mode_failure_timeout_and_empty_selection_preserve_plain_lazy_plan() {
    for (response, timeout_ms) in [
        (ResponseTemplate::new(503), 2000),
        (
            ResponseTemplate::new(200).set_body_json(answers(&[0.0, 0.1])),
            2000,
        ),
        (
            ResponseTemplate::new(200)
                .set_body_json(answers(&[0.0, 0.95]))
                .set_delay(Duration::from_millis(800)),
            200,
        ),
    ] {
        let server = MockServer::start().await;
        let (home, turn, _) = configured_turn(&server).await;
        let model = native_openai_model("gpt-6.1-sol");
        let mut config = load_config(home.path()).await.expect("fixture config");
        config.timeout_ms = timeout_ms;
        write_config(home.path(), &config);
        Mock::given(method("POST"))
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;
        let (baseline_registry, _) = fixture_registry();
        let baseline = finalize_tool_router(
            &turn,
            &model,
            baseline_registry,
            Vec::new(),
            &ToolSearchHandlerCache::default(),
        )
        .expect("plain lazy native plan");
        let (mut registry, eligible) = fixture_registry();
        let before = exposures(&registry);
        select_for_turn(&turn, &model, &mut registry, &eligible).await;
        assert_eq!(exposures(&registry), before);
        assert!(!registry.is_code_mode_preselected(&tool_name("weather")));
        assert!(!registry.is_code_mode_preselected(&tool_name("calendar")));
        let router = finalize_tool_router(
            &turn,
            &model,
            registry,
            Vec::new(),
            &ToolSearchHandlerCache::default(),
        )
        .expect("fallback native plan");
        assert_eq!(router.tool_mode(), ToolMode::CodeModeOnly);
        assert_eq!(router.model_visible_specs(), baseline.model_visible_specs());
        assert_eq!(
            router.code_mode_tool_names(),
            baseline.code_mode_tool_names()
        );
        assert!(exec_description(&router).contains("ALL_TOOLS"));
    }
}

#[tokio::test]
async fn valid_empty_selection_preserves_plain_lazy_discovery() {
    let server = MockServer::start().await;
    let (_home, turn, model) = configured_turn(&server).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(answers(&[0.0, 0.1])))
        .expect(1)
        .mount(&server)
        .await;
    let (mut registry, eligible) = fixture_registry();
    let before = exposures(&registry);
    select_for_turn(&turn, &model, &mut registry, &eligible).await;
    assert_eq!(exposures(&registry), before);
    let state = turn
        .extension_data
        .get::<TurnSelectorState>()
        .expect("cache state");
    assert!(
        state
            .cache
            .lock()
            .await
            .as_ref()
            .expect("cached empty result")
            .result
            .as_ref()
            .expect("empty is success")
            .selected
            .is_empty()
    );
}

#[tokio::test]
async fn http_refusal_and_invalid_answers_leave_entire_registry_unchanged_and_cache_failure() {
    for response in [
        ResponseTemplate::new(503),
        ResponseTemplate::new(200)
            .set_body_json(json!({"answers":[{"type":"refusal","name":"t0"}]})),
        ResponseTemplate::new(200).set_body_json(
            json!({"answers":[{"type":"predicate","name":"t0","probability":0.99}]}),
        ),
        ResponseTemplate::new(200).set_body_string("not JSON"),
    ] {
        let server = MockServer::start().await;
        let (_home, turn, model) = configured_turn(&server).await;
        Mock::given(method("POST"))
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;
        for _ in 0..2 {
            let (mut registry, eligible) = fixture_registry();
            let before = exposures(&registry);
            select_for_turn(&turn, &model, &mut registry, &eligible).await;
            assert_eq!(exposures(&registry), before);
        }
    }
}

#[tokio::test]
async fn refusal_keeps_reported_selector_tokens_for_fallback_accounting() {
    let server = MockServer::start().await;
    let (_home, turn, model) = configured_turn(&server).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "answers": [{"type":"refusal", "name":"t0"}],
            "usage": {"input_tokens": 321, "output_tokens": 4, "total_tokens": 325}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (mut registry, eligible) = fixture_registry();
    let before = exposures(&registry);
    select_for_turn(&turn, &model, &mut registry, &eligible).await;
    assert_eq!(exposures(&registry), before);
    let state = turn
        .extension_data
        .get::<TurnSelectorState>()
        .expect("cache state");
    let cache = state.cache.lock().await;
    let cached = cache.as_ref().expect("cached fallback");
    assert!(matches!(cached.result, Err(SelectorError::Refusal)));
    let usage = cached.usage.as_ref().expect("refusal usage is retained");
    assert_eq!(usage.input_tokens, Some(321));
    assert_eq!(usage.total_tokens, Some(325));
}

#[tokio::test]
async fn successful_selection_does_not_invent_missing_or_partial_usage_counters() {
    for usage in [None, Some(json!({})), Some(json!({"input_tokens": 17}))] {
        let server = MockServer::start().await;
        let (_home, turn, model) = configured_turn(&server).await;
        let mut payload = answers(&[0.1, 0.95]);
        payload
            .as_object_mut()
            .expect("fixture response")
            .remove("usage");
        if let Some(usage) = &usage {
            payload["usage"] = usage.clone();
        }
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(payload))
            .expect(1)
            .mount(&server)
            .await;
        let (mut registry, eligible) = fixture_registry();
        select_for_turn(&turn, &model, &mut registry, &eligible).await;
        assert_eq!(
            exposures(&registry)[&tool_name("weather")],
            ToolExposure::Direct
        );
        let state = turn
            .extension_data
            .get::<TurnSelectorState>()
            .expect("cache state");
        let cache = state.cache.lock().await;
        let cached_usage = cache.as_ref().expect("cached selection").usage.as_ref();
        if usage
            .as_ref()
            .and_then(|usage| usage["input_tokens"].as_u64())
            .is_some()
        {
            let cached_usage = cached_usage.expect("reported input count");
            assert_eq!(cached_usage.input_tokens, Some(17));
            assert_eq!(cached_usage.output_tokens, None);
            assert_eq!(cached_usage.total_tokens, None);
        } else {
            assert!(cached_usage.is_none());
        }
    }
}

#[tokio::test]
async fn timeout_has_one_total_budget_and_falls_back_without_retrying_next_step() {
    let server = MockServer::start().await;
    let (home, turn, model) = configured_turn(&server).await;
    let mut config = load_config(home.path()).await.expect("fixture config");
    // Leave room for first-use HTTP client setup under parallel test load; the
    // response remains well beyond the single whole-attempt deadline.
    config.timeout_ms = 200;
    write_config(home.path(), &config);
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(answers(&[0.1, 0.99]))
                .set_delay(Duration::from_millis(800)),
        )
        .expect(1)
        .mount(&server)
        .await;
    let started = Instant::now();
    for _ in 0..2 {
        let (mut registry, eligible) = fixture_registry();
        let before = exposures(&registry);
        select_for_turn(&turn, &model, &mut registry, &eligible).await;
        assert_eq!(exposures(&registry), before);
    }
    assert!(started.elapsed() < Duration::from_millis(600));
    assert_eq!(server.received_requests().await.expect("requests").len(), 1);
    let state = turn
        .extension_data
        .get::<TurnSelectorState>()
        .expect("cache state");
    assert!(matches!(
        state
            .cache
            .lock()
            .await
            .as_ref()
            .expect("cached timeout")
            .result,
        Err(SelectorError::Timeout)
    ));
}

#[tokio::test]
async fn catalog_or_query_change_invalidates_selection_cache() {
    let server = MockServer::start().await;
    let (_home, turn, model) = configured_turn(&server).await;
    Mock::given(method("POST")).respond_with(|request: &wiremock::Request| {
        let body: Value = serde_json::from_slice(&request.body).expect("request");
        ResponseTemplate::new(200).set_body_json(json!({"answers": body["questions"].as_array().expect("questions").iter().map(|question| json!({"name": question["name"], "type":"predicate", "probability":0.9})).collect::<Vec<_>>() }))
    }).expect(3).mount(&server).await;
    let (mut registry, eligible) = fixture_registry();
    select_for_turn(&turn, &model, &mut registry, &eligible).await;
    let (mut registry, mut eligible) = fixture_registry();
    registry.register_external_with_exposure(runtime("maps"), ToolExposure::Deferred);
    eligible.insert(tool_name("maps"));
    select_for_turn(&turn, &model, &mut registry, &eligible).await;
    record_user_input(&turn, &[user_text("Also check tomorrow")]);
    let (mut registry, eligible) = fixture_registry();
    select_for_turn(&turn, &model, &mut registry, &eligible).await;
}

#[tokio::test]
async fn disabled_missing_credentials_and_oversized_input_never_send_or_mutate_tools() {
    for mode in ["disabled", "credentials", "oversized"] {
        let server = MockServer::start().await;
        let (home, turn, model) = configured_turn(&server).await;
        let mut config = load_config(home.path()).await.expect("fixture config");
        match mode {
            "disabled" => config.enabled = false,
            "credentials" => config.key_file = Some(home.path().join("missing.env")),
            "oversized" => record_user_input(&turn, &[user_text(&"x".repeat(MAX_INPUT_BYTES + 1))]),
            _ => unreachable!(),
        }
        write_config(home.path(), &config);
        let (mut registry, eligible) = fixture_registry();
        let before = exposures(&registry);
        select_for_turn(&turn, &model, &mut registry, &eligible).await;
        assert_eq!(exposures(&registry), before);
        assert!(
            server
                .received_requests()
                .await
                .expect("requests")
                .is_empty()
        );
    }
}

#[tokio::test]
async fn batching_considers_every_definition_and_applies_global_max_tools() {
    let server = MockServer::start().await;
    let (home, turn, model) = configured_turn(&server).await;
    let mut config = load_config(home.path()).await.expect("fixture config");
    config.max_tools = 2;
    write_config(home.path(), &config);
    Mock::given(method("POST")).respond_with(|request: &wiremock::Request| {
        let body: Value = serde_json::from_slice(&request.body).expect("request");
        let questions = body["questions"].as_array().expect("questions");
        assert!(questions.len() <= QUESTIONS_PER_BATCH);
        ResponseTemplate::new(200).set_body_json(json!({"answers": questions.iter().map(|question| {
            let index = question["name"].as_str().expect("name").trim_start_matches('t').parse::<usize>().expect("index");
            json!({"name":question["name"], "type":"predicate", "probability":if index >=128 {0.99} else {0.75}})
        }).collect::<Vec<_>>(), "usage":{"input_tokens":10}}))
    }).expect(3).mount(&server).await;
    let mut registry = ToolRegistry::default();
    let mut eligible = HashSet::new();
    for index in 0..130 {
        let name = format!("tool_{index:03}");
        registry.register_external_with_exposure(runtime(&name), ToolExposure::Deferred);
        eligible.insert(tool_name(&name));
    }
    select_for_turn(&turn, &model, &mut registry, &eligible).await;
    let selected = exposures(&registry)
        .into_iter()
        .filter_map(|(name, exposure)| exposure.is_direct().then_some(name))
        .collect::<Vec<_>>();
    assert_eq!(selected, vec![tool_name("tool_128"), tool_name("tool_129")]);
    let requests = server.received_requests().await.expect("batched requests");
    let count: usize = requests
        .iter()
        .map(|request| {
            let body: Value = serde_json::from_slice(&request.body).expect("request");
            let input: Value =
                serde_json::from_str(body["input"].as_str().expect("input")).expect("catalog");
            input["tools"].as_array().expect("full batch").len()
        })
        .sum();
    assert_eq!(count, 130);
    let state = turn
        .extension_data
        .get::<TurnSelectorState>()
        .expect("cache state");
    let cache = state.cache.lock().await;
    let usage = cache
        .as_ref()
        .expect("cached selection")
        .usage
        .as_ref()
        .expect("all batches reported input tokens");
    assert_eq!(usage.input_tokens, Some(30));
    assert_eq!(usage.output_tokens, None);
    assert_eq!(usage.total_tokens, None);
}
