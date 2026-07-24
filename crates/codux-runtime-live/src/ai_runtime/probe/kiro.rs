use crate::ai_runtime::{
    probe::{
        common::{is_awaiting_user_decision, now_seconds, parse_iso8601_seconds},
        paths::kiro_session_paths,
    },
    snapshot::{AIRuntimeContextSnapshot, AIRuntimeProbeRequest, AIUsageAmountSnapshot},
    state::normalized_string,
};
use serde_json::Value;
use std::{fs, io::BufRead, path::Path};

/// Kiro CLI 2.x stores current CLI chat sessions under
/// `~/.kiro/sessions/cli/<session-id>.json[l]`.
pub(crate) fn probe_kiro_runtime(
    request: &AIRuntimeProbeRequest,
) -> Option<AIRuntimeContextSnapshot> {
    let project_path = normalized_string(request.project_path.as_deref())?;
    let (json_path, jsonl_path) = kiro_session_paths(
        &project_path,
        request.external_session_id.as_deref(),
        request.started_at,
    )?;
    let parsed = parse_kiro_session_files(&json_path, &jsonl_path, request.started_at)?;
    let transcript_path = if jsonl_path.exists() {
        &jsonl_path
    } else {
        &json_path
    };
    let session_id = parsed
        .session_id
        .clone()
        .or_else(|| request.external_session_id.clone());
    Some(kiro_snapshot_from_parsed(
        request,
        session_id.as_deref(),
        transcript_path,
        parsed,
    ))
}

fn kiro_snapshot_from_parsed(
    request: &AIRuntimeProbeRequest,
    session_id: Option<&str>,
    transcript_path: &Path,
    parsed: KiroParsed,
) -> AIRuntimeContextSnapshot {
    let mut response_state = parsed.response_state();
    if is_awaiting_user_decision(
        response_state.as_deref() == Some("responding"),
        true,
        parsed.pending_tool,
        parsed.last_activity_at,
        now_seconds(),
    ) {
        response_state = Some("needsInput".to_string());
    }
    let has_completed_turn = response_state.as_deref() == Some("idle");
    AIRuntimeContextSnapshot {
        tool: "kiro".to_string(),
        external_session_id: normalized_string(session_id),
        transcript_path: Some(transcript_path.display().to_string()),
        model: parsed.model,
        assistant_preview: parsed.assistant_preview,
        input_tokens: parsed.input_tokens,
        output_tokens: parsed.output_tokens,
        cached_input_tokens: 0,
        total_tokens: parsed.total_tokens,
        usage_amounts: parsed.usage_amounts,
        baseline_usage_amounts: parsed.baseline_usage_amounts,
        updated_at: parsed.last_activity_at.max(request.updated_at),
        runtime_activity_at: (parsed.last_activity_at > 0.0).then_some(parsed.last_activity_at),
        last_user_input_at: (parsed.last_user_at > 0.0).then_some(parsed.last_user_at),
        started_at: (parsed.last_user_at > 0.0).then_some(parsed.last_user_at),
        completed_at: has_completed_turn.then_some(parsed.last_activity_at),
        response_state,
        was_interrupted: false,
        has_completed_turn,
        session_origin: "unknown".to_string(),
        source: "probe".to_string(),
        plan: None,
    }
}

#[derive(Default)]
struct KiroParsed {
    session_id: Option<String>,
    model: Option<String>,
    assistant_preview: Option<String>,
    pending_tool: bool,
    turn_open: bool,
    turn_completed: bool,
    last_user_at: f64,
    last_activity_at: f64,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    usage_amounts: Vec<AIUsageAmountSnapshot>,
    baseline_usage_amounts: Vec<AIUsageAmountSnapshot>,
}

impl KiroParsed {
    fn response_state(&self) -> Option<String> {
        if self.pending_tool || self.turn_open {
            Some("responding".to_string())
        } else if self.turn_completed || self.assistant_preview.is_some() {
            Some("idle".to_string())
        } else {
            None
        }
    }
}

fn parse_kiro_session_files(
    json_path: &Path,
    jsonl_path: &Path,
    started_at: Option<f64>,
) -> Option<KiroParsed> {
    let mut parsed = fs::read_to_string(json_path)
        .ok()
        .and_then(|value| parse_kiro_session_summary(&value, started_at))
        .unwrap_or_default();
    if let Some(from_events) = parse_kiro_session_jsonl(jsonl_path) {
        parsed.last_user_at = parsed.last_user_at.max(from_events.last_user_at);
        parsed.last_activity_at = parsed.last_activity_at.max(from_events.last_activity_at);
        parsed.pending_tool = from_events.pending_tool;
        parsed.turn_open = from_events.turn_open;
        parsed.turn_completed = from_events.turn_completed || parsed.turn_completed;
        if from_events.assistant_preview.is_some() {
            parsed.assistant_preview = from_events.assistant_preview;
        }
    }
    (parsed.last_activity_at > 0.0 || parsed.last_user_at > 0.0).then_some(parsed)
}

fn parse_kiro_session_summary(value: &str, started_at: Option<f64>) -> Option<KiroParsed> {
    let root = serde_json::from_str::<Value>(value).ok()?;
    let mut parsed = KiroParsed {
        session_id: normalized_string(root.get("session_id").and_then(|value| value.as_str())),
        model: root
            .get("session_state")
            .and_then(|value| value.get("rts_model_state"))
            .and_then(|value| value.get("model_info"))
            .and_then(|value| value.get("model_id"))
            .and_then(|value| value.as_str())
            .and_then(|value| normalized_string(Some(value))),
        ..Default::default()
    };
    if let Some(created_at) = root
        .get("created_at")
        .and_then(|value| value.as_str())
        .and_then(parse_iso8601_seconds)
    {
        parsed.last_activity_at = parsed.last_activity_at.max(created_at);
    }
    if let Some(updated_at) = root
        .get("updated_at")
        .and_then(|value| value.as_str())
        .and_then(parse_iso8601_seconds)
    {
        parsed.last_activity_at = parsed.last_activity_at.max(updated_at);
    }
    if let Some(turns) = root
        .get("session_state")
        .and_then(|value| value.get("conversation_metadata"))
        .and_then(|value| value.get("user_turn_metadatas"))
        .and_then(|value| value.as_array())
    {
        for turn in turns {
            let has_completed_result = turn
                .get("result")
                .and_then(|value| value.get("Ok"))
                .is_some()
                || turn.get("end_timestamp").is_some()
                || turn
                    .get("output_token_count")
                    .and_then(|value| value.as_i64())
                    .unwrap_or(0)
                    > 0;
            parsed.turn_completed = parsed.turn_completed || has_completed_result;
            parsed.input_tokens += turn
                .get("input_token_count")
                .and_then(|value| value.as_i64())
                .unwrap_or(0);
            parsed.output_tokens += turn
                .get("output_token_count")
                .and_then(|value| value.as_i64())
                .unwrap_or(0);
            let usage_amounts = kiro_metering_usage(turn);
            for amount in usage_amounts.iter().cloned() {
                merge_usage_amount(&mut parsed.usage_amounts, amount);
            }
            if let Some(timestamp) = turn
                .get("end_timestamp")
                .and_then(|value| value.as_str())
                .and_then(parse_iso8601_seconds)
            {
                parsed.last_activity_at = parsed.last_activity_at.max(timestamp);
                if started_at.is_some_and(|started_at| timestamp + 1.0 < started_at) {
                    for amount in usage_amounts {
                        merge_usage_amount(&mut parsed.baseline_usage_amounts, amount);
                    }
                }
            }
            if let Some(preview) = turn
                .get("result")
                .and_then(|value| value.get("Ok"))
                .and_then(|value| {
                    kiro_content_preview(value.get("content").unwrap_or(&Value::Null))
                })
            {
                parsed.assistant_preview = Some(preview);
            }
        }
    }
    parsed.total_tokens = parsed.input_tokens + parsed.output_tokens;
    Some(parsed)
}

fn kiro_metering_usage(turn: &Value) -> Vec<AIUsageAmountSnapshot> {
    turn.get("metering_usage")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let unit = item
                .get("unit")
                .and_then(|value| value.as_str())
                .and_then(|value| normalized_string(Some(value)))?;
            let value = item.get("value").and_then(|value| value.as_f64())?;
            (value > 0.0).then_some(AIUsageAmountSnapshot { unit, value })
        })
        .collect()
}

fn merge_usage_amount(amounts: &mut Vec<AIUsageAmountSnapshot>, next: AIUsageAmountSnapshot) {
    if let Some(existing) = amounts.iter_mut().find(|item| item.unit == next.unit) {
        existing.value += next.value;
    } else {
        amounts.push(next);
    }
}

fn parse_kiro_session_jsonl(path: &Path) -> Option<KiroParsed> {
    let file = fs::File::open(path).ok()?;
    let mut reader = std::io::BufReader::new(file);
    let mut parsed = KiroParsed::default();
    let mut line = String::new();
    loop {
        line.clear();
        let Ok(bytes) = reader.read_line(&mut line) else {
            break;
        };
        if bytes == 0 {
            break;
        }
        let Ok(row) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let kind = row
            .get("kind")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let data = row.get("data").unwrap_or(&Value::Null);
        let timestamp = data
            .get("meta")
            .and_then(|value| value.get("timestamp"))
            .and_then(kiro_timestamp_seconds)
            .unwrap_or(0.0);
        parsed.last_activity_at = parsed.last_activity_at.max(timestamp);
        match kind {
            "Prompt" => {
                parsed.last_user_at = parsed.last_user_at.max(timestamp);
                parsed.turn_open = true;
            }
            "AssistantMessage" => {
                parsed.pending_tool = false;
                parsed.turn_open = false;
                parsed.turn_completed = true;
                if let Some(preview) =
                    kiro_content_preview(data.get("content").unwrap_or(&Value::Null))
                {
                    parsed.assistant_preview = Some(preview);
                }
            }
            "ToolUse" | "ToolCall" | "ToolRequest" => {
                parsed.pending_tool = true;
                parsed.turn_open = true;
            }
            "ToolResult" | "ToolResponse" => {
                parsed.pending_tool = false;
                parsed.turn_open = true;
            }
            _ => {}
        }
    }
    (parsed.last_activity_at > 0.0 || parsed.last_user_at > 0.0).then_some(parsed)
}

fn kiro_timestamp_seconds(value: &Value) -> Option<f64> {
    value
        .as_i64()
        .map(|value| value as f64)
        .or_else(|| value.as_f64())
        .map(|value| {
            if value >= 10_000_000_000.0 {
                value / 1000.0
            } else {
                value
            }
        })
}

fn kiro_content_preview(content: &Value) -> Option<String> {
    let items = content.as_array()?;
    for item in items {
        let kind = item.get("kind").and_then(|value| value.as_str());
        if kind != Some("text") {
            continue;
        }
        if let Some(text) = item
            .get("data")
            .and_then(|value| value.as_str())
            .and_then(|value| normalized_string(Some(value)))
        {
            return Some(text);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn current_session_files_with_assistant_response_are_idle() {
        let dir = std::env::temp_dir().join(format!("codux-kiro-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let json_path = dir.join("81db0bb0-c9aa-4e5f-9db4-6c88d5bf4817.json");
        let jsonl_path = dir.join("81db0bb0-c9aa-4e5f-9db4-6c88d5bf4817.jsonl");
        std::fs::write(
            &json_path,
            serde_json::json!({
                "session_id": "81db0bb0-c9aa-4e5f-9db4-6c88d5bf4817",
                "cwd": "/tmp/project",
                "created_at": "2026-06-28T01:00:00Z",
                "updated_at": "2026-06-28T01:02:00Z",
                "session_state": {
                    "rts_model_state": {
                        "model_info": {"model_id": "auto"}
                    },
                    "conversation_metadata": {
                        "user_turn_metadatas": [{
                            "input_token_count": 120,
                            "output_token_count": 80,
                            "end_timestamp": "2026-06-28T01:02:00Z",
                            "result": {
                                "Ok": {
                                    "content": [{"kind": "text", "data": "summary preview"}]
                                }
                            }
                        }]
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            &jsonl_path,
            concat!(
                r#"{"version":"v1","kind":"Prompt","data":{"message_id":"u1","content":[{"kind":"text","data":"你好"}],"meta":{"timestamp":1782629883}}}"#,
                "\n",
                r#"{"version":"v1","kind":"AssistantMessage","data":{"message_id":"a1","content":[{"kind":"text","data":"你好！有什么我可以帮你的吗？"}]}}"#,
                "\n",
            ),
        )
        .unwrap();

        let parsed = parse_kiro_session_files(&json_path, &jsonl_path, None).expect("parsed");
        assert_eq!(
            parsed.session_id.as_deref(),
            Some("81db0bb0-c9aa-4e5f-9db4-6c88d5bf4817")
        );
        assert_eq!(parsed.response_state().as_deref(), Some("idle"));
        assert_eq!(parsed.model.as_deref(), Some("auto"));
        assert_eq!(
            parsed.assistant_preview.as_deref(),
            Some("你好！有什么我可以帮你的吗？")
        );
        assert_eq!(parsed.input_tokens, 120);
        assert_eq!(parsed.output_tokens, 80);
        assert_eq!(parsed.total_tokens, 200);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn prompt_only_session_is_responding() {
        let dir = std::env::temp_dir().join(format!("codux-kiro-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let json_path = dir.join("session.json");
        let jsonl_path = dir.join("session.jsonl");
        std::fs::write(
            &json_path,
            serde_json::json!({
                "session_id": "session",
                "cwd": "/tmp/project",
                "updated_at": "2026-06-28T01:00:00Z"
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            &jsonl_path,
            concat!(
                r#"{"version":"v1","kind":"Prompt","data":{"message_id":"u1","content":[{"kind":"text","data":"继续"}],"meta":{"timestamp":1782629883}}}"#,
                "\n",
            ),
        )
        .unwrap();

        let parsed = parse_kiro_session_files(&json_path, &jsonl_path, None).expect("parsed");
        assert_eq!(parsed.response_state().as_deref(), Some("responding"));
        assert!(parsed.assistant_preview.is_none());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn snapshot_from_prompt_only_session_keeps_model_and_loading_state() {
        let dir = std::env::temp_dir().join(format!("codux-kiro-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let json_path = dir.join("session.json");
        let jsonl_path = dir.join("session.jsonl");
        std::fs::write(
            &json_path,
            serde_json::json!({
                "session_id": "session",
                "cwd": "/tmp/project",
                "updated_at": "2026-06-28T01:00:00Z",
                "session_state": {
                    "rts_model_state": {
                        "model_info": {"model_id": "auto"}
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            &jsonl_path,
            concat!(
                r#"{"version":"v1","kind":"Prompt","data":{"message_id":"u1","content":[{"kind":"text","data":"继续"}],"meta":{"timestamp":1782629883}}}"#,
                "\n",
            ),
        )
        .unwrap();

        let parsed = parse_kiro_session_files(&json_path, &jsonl_path, None).expect("parsed");
        let snapshot = kiro_snapshot_from_parsed(
            &AIRuntimeProbeRequest {
                terminal_id: "terminal-1".to_string(),
                terminal_instance_id: Some("instance-1".to_string()),
                project_id: "project-1".to_string(),
                project_path: Some("/tmp/project".to_string()),
                tool: "kiro".to_string(),
                external_session_id: Some("session".to_string()),
                transcript_path: None,
                started_at: Some(1782629880.0),
                updated_at: 1782629880.0,
                occupied_external_session_ids: Default::default(),
            },
            Some("session"),
            &jsonl_path,
            parsed,
        );
        assert_eq!(snapshot.model.as_deref(), Some("auto"));
        assert_eq!(snapshot.response_state.as_deref(), Some("responding"));
        assert_eq!(snapshot.total_tokens, 0);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn summary_only_completed_turn_is_idle() {
        let dir = std::env::temp_dir().join(format!("codux-kiro-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let json_path = dir.join("session-1.json");
        let jsonl_path = dir.join("session-1.jsonl");
        std::fs::write(
            &json_path,
            serde_json::json!({
                "session_id": "session-1",
                "cwd": "/tmp/project",
                "updated_at": "2026-06-28T01:00:00Z",
                "session_state": {
                    "conversation_metadata": {
                        "user_turn_metadatas": [{
                            "input_token_count": 1,
                            "output_token_count": 2,
                            "end_timestamp": "2026-06-28T01:00:00Z",
                            "metering_usage": [{ "value": 0.25, "unit": "credit" }],
                            "result": {"Ok": {"content": [{"kind": "text", "data": "done"}]}}
                        }]
                    }
                }
            })
            .to_string(),
        )
        .unwrap();

        let parsed = parse_kiro_session_files(&json_path, &jsonl_path, None).expect("parsed");
        assert_eq!(parsed.response_state().as_deref(), Some("idle"));
        assert_eq!(parsed.assistant_preview.as_deref(), Some("done"));
        assert_eq!(parsed.total_tokens, 3);
        assert_eq!(parsed.usage_amounts[0].unit, "credit");
        assert_eq!(parsed.usage_amounts[0].value, 0.25);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn restored_session_splits_prelaunch_credit_baseline() {
        let dir = std::env::temp_dir().join(format!("codux-kiro-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let json_path = dir.join("session-1.json");
        let jsonl_path = dir.join("session-1.jsonl");
        std::fs::write(
            &json_path,
            serde_json::json!({
                "session_id": "session-1",
                "cwd": "/tmp/project",
                "created_at": "2026-06-29T06:45:59Z",
                "updated_at": "2026-06-29T07:27:18Z",
                "session_state": {
                    "conversation_metadata": {
                        "user_turn_metadatas": [
                            {
                                "end_timestamp": "2026-06-29T06:46:03Z",
                                "metering_usage": [{ "value": 0.041447917081260374, "unit": "credit" }],
                                "result": {"Ok": {"content": [{"kind": "text", "data": "old"}]}}
                            },
                            {
                                "end_timestamp": "2026-06-29T07:27:18Z",
                                "metering_usage": [{ "value": 0.026586924046434493, "unit": "credit" }],
                                "result": {"Ok": {"content": [{"kind": "text", "data": "new"}]}}
                            }
                        ]
                    }
                }
            })
            .to_string(),
        )
        .unwrap();

        let parsed = parse_kiro_session_files(&json_path, &jsonl_path, Some(1_782_718_000.0))
            .expect("parsed");

        assert_eq!(parsed.usage_amounts[0].unit, "credit");
        assert!((parsed.usage_amounts[0].value - 0.06803484112769487).abs() < 0.000_000_001);
        assert_eq!(parsed.baseline_usage_amounts[0].unit, "credit");
        assert!(
            (parsed.baseline_usage_amounts[0].value - 0.041447917081260374).abs() < 0.000_000_001
        );

        let _ = std::fs::remove_dir_all(dir);
    }
}
