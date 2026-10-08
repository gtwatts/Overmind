use super::*;

#[tokio::test]
async fn mcp_progress_requires_the_live_thread_turn_and_active_call() {
    use crate::overmind::hud::config::HudConfig;
    use codex_app_server_protocol::McpToolCallProgressNotification;

    let (mut chat, _events, _operations) = make_chatwidget_manual(None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.overmind_hud_configure(HudConfig {
        ascii: true,
        ..HudConfig::default()
    });
    handle_turn_started(&mut chat, "turn-1");
    let item: AppServerThreadItem = serde_json::from_value(json!({
        "type": "mcpToolCall", "id": "render", "server": "photocraft", "tool": "render",
        "status": "inProgress", "arguments": {}
    }))
    .expect("MCP tool call");
    chat.on_mcp_tool_call_started(item.clone());
    let base = McpToolCallProgressNotification {
        thread_id: chat.thread_id().unwrap().to_string(),
        turn_id: "turn-1".into(),
        item_id: "render".into(),
        message: "Frame ready".into(),
        progress: Some(0.5),
        total: Some(2.0),
    };
    let hud = |chat: &ChatWidget| {
        chat.bottom_pane
            .overmind_hud()
            .line(140, true, Instant::now())
            .unwrap()
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    };
    for invalid in [
        McpToolCallProgressNotification {
            thread_id: ThreadId::new().to_string(),
            ..base.clone()
        },
        McpToolCallProgressNotification {
            turn_id: "old-turn".into(),
            ..base.clone()
        },
        McpToolCallProgressNotification {
            item_id: "unknown".into(),
            ..base.clone()
        },
    ] {
        chat.handle_server_notification(ServerNotification::McpToolCallProgress(invalid), None);
        assert!(!hud(&chat).contains('%'));
    }
    chat.handle_server_notification(
        ServerNotification::McpToolCallProgress(base.clone()),
        Some(ReplayKind::ThreadSnapshot),
    );
    assert!(!hud(&chat).contains('%'));
    chat.handle_server_notification(ServerNotification::McpToolCallProgress(base.clone()), None);
    assert!(hud(&chat).contains("25%"));
    let mut completed = item;
    if let AppServerThreadItem::McpToolCall { status, .. } = &mut completed {
        *status = codex_app_server_protocol::McpToolCallStatus::Completed;
    }
    chat.on_mcp_tool_call_completed(completed);
    assert!(!hud(&chat).contains('%'));
    chat.handle_server_notification(ServerNotification::McpToolCallProgress(base), None);
    assert!(!hud(&chat).contains('%'));
}

#[tokio::test]
async fn image_result_live_and_replay_render_in_the_owning_call() {
    let item: AppServerThreadItem = serde_json::from_value(json!({
        "type": "mcpToolCall", "id": "image", "server": "node_repl", "tool": "js",
        "status": "completed", "arguments": {"title": "Inspect screenshot", "code": "showImage()"},
        "result": {"content": [
            {"type": "text", "text": "Screenshot captured"},
            {"type": "image", "mimeType": "image/png", "data": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg=="}
        ]},
        "durationMs": 5
    })).expect("MCP image result");
    let mut outputs = Vec::new();
    for replay in [false, true] {
        let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
        if replay {
            chat.replay_thread_item(item.clone(), "turn-1".into(), ReplayKind::ThreadSnapshot);
        } else {
            let mut started = item.clone();
            if let AppServerThreadItem::McpToolCall { status, result, .. } = &mut started {
                *status = codex_app_server_protocol::McpToolCallStatus::InProgress;
                *result = None;
            }
            chat.on_mcp_tool_call_started(started);
            chat.on_mcp_tool_call_completed(item.clone());
        }
        let cells = drain_insert_history(&mut rx);
        assert_eq!(cells.len(), 1);
        outputs.push(lines_to_single_string(&cells[0]));
    }
    assert_eq!(outputs[0], outputs[1]);
    insta::assert_snapshot!(outputs[0]);
}
