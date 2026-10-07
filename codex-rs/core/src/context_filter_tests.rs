use super::*;
use crate::context_manager::ContextManager;
use codex_protocol::openai_models::InputModality;
use codex_utils_output_truncation::TruncationPolicy;
use pretty_assertions::assert_eq;

const BIG: usize = 3_000;

fn message(role: &str, text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: role.to_string(),
        content: vec![ContentItem::InputText {
            text: text.to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

fn call(call_id: &str, arguments: &str) -> ResponseItem {
    ResponseItem::FunctionCall {
        id: None,
        name: "shell".to_string(),
        namespace: None,
        arguments: arguments.to_string(),
        encrypted_function_args: None,
        call_id: call_id.to_string(),
        internal_chat_message_metadata_passthrough: None,
    }
}

fn output(call_id: &str, text: &str) -> ResponseItem {
    ResponseItem::FunctionCallOutput {
        id: None,
        call_id: Some(call_id.to_string()),
        name: None,
        namespace: None,
        output: FunctionCallOutputPayload {
            body: FunctionCallOutputBody::Text(text.to_string()),
            success: Some(true),
        },
        internal_chat_message_metadata_passthrough: None,
    }
}

fn catalog() -> String {
    [
        "<skills_instructions>",
        "## Skills",
        "A skill is a set of local instructions.",
        "### Skill roots",
        "- `r0` = `/home/u/.codex/skills`",
        "### Available skills",
        "- alpha: Alpha helper (for x). (file: r0/alpha/SKILL.md)",
        "- beta: Beta helper. (file: /abs/beta/SKILL.md)",
        "- plug:gamma: Gamma helper. (file: r0/gamma/SKILL.md)",
        "### How to use skills",
        "- Discovery: The list above is the skills available in this session (name + description + short path).",
        "</skills_instructions>",
    ]
    .join("\n")
}

/// developer catalog, turn 1 (big output), turn 2 (small + big latest output).
fn items() -> Vec<ResponseItem> {
    vec![
        message("developer", &catalog()),
        message("user", "first question"),
        call("call-1", "{\"cmd\":\"cat big.log\"}"),
        output("call-1", &"a".repeat(BIG)),
        message("assistant", "first answer"),
        message("user", "please fix it with $beta"),
        call("call-2", "{\"cmd\":\"ls\"}"),
        output("call-2", "small"),
        call("call-3", "{\"cmd\":\"cat other.log\"}"),
        output("call-3", &"c".repeat(BIG)),
    ]
}

fn config(command: &[&str]) -> ContextFilterConfig {
    ContextFilterConfig {
        command: command.iter().map(|part| part.to_string()).collect(),
        timeout: Duration::from_millis(2_000),
        filter_skills: true,
        filter_tool_outputs: true,
        min_tool_output_bytes: 1_000,
    }
}

fn meta() -> ContextFilterRequestMeta<'static> {
    ContextFilterRequestMeta {
        turn_id: "turn-1",
        model: "test-model",
        cwd: "/work",
    }
}

fn text_of(item: &ResponseItem) -> String {
    match item {
        ResponseItem::Message { content, .. } => match &content[0] {
            ContentItem::InputText { text } | ContentItem::OutputText { text } => text.clone(),
            _ => String::new(),
        },
        ResponseItem::FunctionCallOutput { output, .. } => {
            output.body.to_text().unwrap_or_default()
        }
        _ => String::new(),
    }
}

#[test]
fn builds_request_with_skills_and_older_tool_outputs() {
    let (request, plan) = build_request(&config(&["unused"]), &items(), &meta());
    assert_eq!(request.version, CONTEXT_FILTER_PROTOCOL_VERSION);
    assert_eq!(request.prompt, "please fix it with $beta");
    assert_eq!(
        request.skills,
        vec![
            FilterSkill {
                name: "alpha".to_string(),
                description: "Alpha helper (for x).".to_string(),
                locator: "r0/alpha/SKILL.md".to_string(),
                path: Some("/home/u/.codex/skills/alpha/SKILL.md".to_string()),
            },
            FilterSkill {
                name: "beta".to_string(),
                description: "Beta helper.".to_string(),
                locator: "/abs/beta/SKILL.md".to_string(),
                path: Some("/abs/beta/SKILL.md".to_string()),
            },
            FilterSkill {
                name: "plug:gamma".to_string(),
                description: "Gamma helper.".to_string(),
                locator: "r0/gamma/SKILL.md".to_string(),
                path: Some("/home/u/.codex/skills/gamma/SKILL.md".to_string()),
            },
        ]
    );
    // call-2 is too small and call-3 is the latest output: only call-1 is offered.
    assert_eq!(request.tool_outputs.len(), 1);
    let offered = &request.tool_outputs[0];
    assert_eq!(offered.id, "call-1");
    assert_eq!(offered.tool.as_deref(), Some("shell"));
    assert_eq!(offered.call.as_deref(), Some("{\"cmd\":\"cat big.log\"}"));
    assert_eq!(offered.bytes, BIG);
    assert_eq!(offered.approx_tokens, BIG / 4);
    assert!(!offered.current_turn);
    assert!(offered.preview.len() < BIG);
    assert_eq!(plan.mentioned_skills, HashSet::from(["beta".to_string()]));
}

#[test]
fn applies_skill_and_tool_output_decisions() {
    let original = items();
    let mut prompt = original.clone();
    let (_, plan) = build_request(&config(&["unused"]), &prompt, &meta());
    let response = ContextFilterResponse {
        // beta is not listed but is kept because the prompt mentions `$beta`.
        keep_skills: Some(vec!["alpha".to_string(), "unknown".to_string()]),
        // call-2 and call-3 were never offered and must survive; ghost is ignored.
        elide_tool_outputs: vec![
            "call-1".to_string(),
            "call-2".to_string(),
            "call-3".to_string(),
            "ghost".to_string(),
        ],
    };
    let outcome = apply_response(&mut prompt, &plan, &response);

    assert_eq!(outcome.skills_listed, 3);
    assert_eq!(outcome.skills_hidden, 1);
    assert_eq!(outcome.tool_outputs_offered, 1);
    assert_eq!(outcome.tool_outputs_elided, 1);
    assert_eq!(
        text_of(&prompt[0]),
        [
            "<skills_instructions>",
            "## Skills",
            "A skill is a set of local instructions.",
            "### Skill roots",
            "- `r0` = `/home/u/.codex/skills`",
            "### Available skills",
            "- alpha: Alpha helper (for x). (file: r0/alpha/SKILL.md)",
            "- beta: Beta helper. (file: /abs/beta/SKILL.md)",
            "- (1 more skills are hidden from this request by the context filter. Mention one as `$name` to load it.)",
            "### How to use skills",
            "- Discovery: The list above is the skills available in this session (name + description + short path).",
            "</skills_instructions>",
        ]
        .join("\n")
    );
    assert_eq!(
        text_of(&prompt[3]),
        "[elided by context filter: 750 tokens]"
    );
    // Everything else, including both user messages and the latest output, is unchanged.
    for index in [1, 2, 4, 5, 6, 7, 8, 9] {
        assert_eq!(prompt[index], original[index], "item {index} changed");
    }
}

#[test]
fn content_item_outputs_are_offered_and_elided_but_images_are_not() {
    let text_items = |text: &str| FunctionCallOutputPayload {
        body: FunctionCallOutputBody::ContentItems(vec![
            FunctionCallOutputContentItem::InputText {
                text: "Script completed\n".to_string(),
            },
            FunctionCallOutputContentItem::InputText {
                text: text.to_string(),
            },
        ]),
        success: None,
    };
    let custom_output =
        |call_id: &str, output: FunctionCallOutputPayload| ResponseItem::CustomToolCallOutput {
            id: None,
            call_id: call_id.to_string(),
            name: None,
            output,
            internal_chat_message_metadata_passthrough: None,
        };
    let mut with_image = text_items(&"i".repeat(BIG));
    if let FunctionCallOutputBody::ContentItems(items) = &mut with_image.body {
        items.push(FunctionCallOutputContentItem::InputImage {
            image: codex_protocol::models::ImageReference::Inline {
                image_url: "data:image/png;base64,AAAA".to_string(),
            },
            detail: None,
        });
    }
    let mut prompt = vec![
        message("user", "look at the logs"),
        custom_output("code-1", text_items(&"x".repeat(BIG))),
        custom_output("code-2", with_image),
        custom_output("code-3", text_items(&"y".repeat(BIG))),
    ];
    let (request, plan) = build_request(&config(&["unused"]), &prompt, &meta());
    let ids: Vec<&str> = request
        .tool_outputs
        .iter()
        .map(|output| output.id.as_str())
        .collect();
    assert_eq!(ids, vec!["code-1"]);
    assert_eq!(
        request.tool_outputs[0].bytes,
        BIG + "Script completed\n".len()
    );

    let response = ContextFilterResponse {
        keep_skills: None,
        elide_tool_outputs: vec!["code-1".to_string(), "code-2".to_string()],
    };
    let outcome = apply_response(&mut prompt, &plan, &response);
    assert_eq!(outcome.tool_outputs_elided, 1);
    let ResponseItem::CustomToolCallOutput { output, .. } = &prompt[1] else {
        panic!("expected custom tool output");
    };
    assert_eq!(
        output.body,
        FunctionCallOutputBody::Text("[elided by context filter: 755 tokens]".to_string())
    );
}

#[test]
fn null_keep_skills_keeps_catalog() {
    let original = items();
    let mut prompt = original.clone();
    let (_, plan) = build_request(&config(&["unused"]), &prompt, &meta());
    let outcome = apply_response(&mut prompt, &plan, &ContextFilterResponse::default());
    assert_eq!(outcome.skills_hidden, 0);
    assert_eq!(outcome.tool_outputs_elided, 0);
    assert_eq!(prompt, original);
}

#[test]
fn disabled_parts_are_not_offered() {
    let mut cfg = config(&["unused"]);
    cfg.filter_skills = false;
    cfg.filter_tool_outputs = false;
    let (request, _) = build_request(&cfg, &items(), &meta());
    assert!(request.skills.is_empty());
    assert!(request.tool_outputs.is_empty());
}

#[cfg(unix)]
mod command {
    use super::*;
    use pretty_assertions::assert_eq;
    use pretty_assertions::assert_ne;

    async fn run(cfg: &ContextFilterConfig) -> (Vec<ResponseItem>, Option<ContextFilterOutcome>) {
        let mut prompt = items();
        let outcome = filter_prompt_input(cfg, &mut prompt, meta()).await;
        (prompt, outcome)
    }

    #[tokio::test]
    async fn filter_command_is_applied_and_receives_request() {
        let dir = tempfile::tempdir().unwrap();
        let stdin_path = dir.path().join("stdin.json");
        let script = format!(
            "cat > '{}'; printf '%s' '{{\"keep_skills\":[\"alpha\"],\"elide_tool_outputs\":[\"call-1\"]}}'",
            stdin_path.display()
        );
        let (prompt, outcome) = run(&config(&["sh", "-c", &script])).await;
        let outcome = outcome.expect("filter applied");
        assert_eq!(outcome.skills_hidden, 1);
        assert_eq!(outcome.tool_outputs_elided, 1);
        assert_eq!(
            text_of(&prompt[3]),
            "[elided by context filter: 750 tokens]"
        );
        assert!(!text_of(&prompt[0]).contains("plug:gamma"));

        let sent: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&stdin_path).unwrap()).unwrap();
        assert_eq!(sent["version"], 1);
        assert_eq!(sent["turn_id"], "turn-1");
        assert_eq!(sent["prompt"], "please fix it with $beta");
        assert_eq!(sent["skills"].as_array().unwrap().len(), 3);
        assert_eq!(sent["tool_outputs"][0]["id"], "call-1");
    }

    #[tokio::test]
    async fn fails_open_on_timeout() {
        let mut cfg = config(&["sh", "-c", "sleep 10"]);
        cfg.timeout = Duration::from_millis(200);
        let started = Instant::now();
        let (prompt, outcome) = run(&cfg).await;
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(outcome, None);
        assert_eq!(prompt, items());
    }

    #[tokio::test]
    async fn fails_open_on_error_exit() {
        let script = "cat > /dev/null; printf '%s' '{\"keep_skills\":[]}'; echo boom >&2; exit 3";
        let (prompt, outcome) = run(&config(&["sh", "-c", script])).await;
        assert_eq!(outcome, None);
        assert_eq!(prompt, items());
    }

    #[tokio::test]
    async fn fails_open_on_bad_json() {
        for script in [
            "cat > /dev/null; echo 'not json'",
            "cat > /dev/null",
            "cat > /dev/null; echo '{\"keep_skills\": \"alpha\"}'",
        ] {
            let (prompt, outcome) = run(&config(&["sh", "-c", script])).await;
            assert_eq!(outcome, None, "script: {script}");
            assert_eq!(prompt, items(), "script: {script}");
        }
    }

    #[tokio::test]
    async fn fails_open_when_command_is_missing() {
        let (prompt, outcome) = run(&config(&["/nonexistent/context-filter"])).await;
        assert_eq!(outcome, None);
        assert_eq!(prompt, items());
    }

    #[tokio::test]
    async fn stored_history_is_not_modified() {
        let mut history = ContextManager::new();
        history.record_items(items().iter(), TruncationPolicy::Tokens(10_000));
        let before = history.clone().for_prompt(&[InputModality::Text]);

        let script = "cat > /dev/null; printf '%s' '{\"keep_skills\":[],\"elide_tool_outputs\":[\"call-1\"]}'";
        let mut prompt = history.clone().for_prompt(&[InputModality::Text]);
        let outcome = filter_prompt_input(&config(&["sh", "-c", script]), &mut prompt, meta())
            .await
            .expect("filter applied");
        assert_eq!(outcome.tool_outputs_elided, 1);
        assert_ne!(prompt, before);

        let after = history.clone().for_prompt(&[InputModality::Text]);
        assert_eq!(after, before);
        assert!(after.iter().any(|item| text_of(item) == "a".repeat(BIG)));
    }
}
