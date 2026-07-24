mod parse;
mod preview;
mod types;

use crate::ai_runtime::{
    constants::CODEX_STALE_PRELAUNCH_OPEN_TURN_SOURCE,
    probe::common::now_seconds,
    probe::paths::{
        codex_session_id_from_rollout, find_codex_rollout_by_cwd_since, find_codex_rollout_path,
    },
    snapshot::{AIRuntimeContextSnapshot, AIRuntimeProbeRequest},
    state::normalized_string,
};

use self::parse::parse_codex_runtime_state;
use std::path::Path;

pub(crate) fn probe_codex_runtime(
    request: &AIRuntimeProbeRequest,
) -> Option<AIRuntimeContextSnapshot> {
    let project_path = normalized_string(request.project_path.as_deref())?;
    let file_path = normalized_string(request.transcript_path.as_deref())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            // Known id → match by name; otherwise locate the live rollout by cwd.
            match normalized_string(request.external_session_id.as_deref()) {
                Some(external_id) => find_codex_rollout_path(&project_path, &external_id),
                None => find_codex_rollout_by_cwd_since(&project_path, request.started_at),
            }
        })?;
    let transcript_path = file_path.display().to_string();
    let parsed = parse_codex_runtime_state(
        &file_path,
        Some(&project_path),
        request.started_at,
        request.updated_at,
    )?;
    let stale_prelaunch_open_turn =
        stale_prelaunch_open_turn(&parsed, &file_path, request.started_at);
    let stale_completed_at = stale_prelaunch_open_turn.then(|| {
        request
            .updated_at
            .max(request.started_at.unwrap_or(request.updated_at))
    });
    let mut updated_at = parsed.updated_at.unwrap_or(request.updated_at);
    if let Some(completed_at) = stale_completed_at {
        updated_at = updated_at.max(completed_at);
    }

    // Pure-file approval wait: a command/patch call is written with no result
    // yet, the approval policy can still prompt, and it has sat idle past the
    // gap. (codex sessions re-probe on a quiet-session interval, so for
    // non-`never` policies this can surface a few seconds late; `never` -- the
    // common headless setup -- gates it off entirely.)
    let mut response_state = parsed.response_state.clone();
    if stale_prelaunch_open_turn {
        response_state = Some("idle".to_string());
    } else if parsed.needs_user_input(now_seconds()) {
        response_state = Some("needsInput".to_string());
    }
    let external_session_id = normalized_string(request.external_session_id.as_deref())
        .or_else(|| codex_session_id_from_rollout(&file_path));
    let mut plan = parsed.plan;
    if let (Some(plan), Some(session_id)) = (plan.as_mut(), external_session_id.as_ref()) {
        plan.session_id = session_id.clone();
    }
    Some(AIRuntimeContextSnapshot {
        tool: "codex".to_string(),
        external_session_id,
        transcript_path: Some(transcript_path),
        model: parsed.model,
        assistant_preview: parsed.assistant_preview,
        input_tokens: parsed.input_tokens.unwrap_or(0),
        output_tokens: parsed.output_tokens.unwrap_or(0),
        cached_input_tokens: parsed.cached_input_tokens.unwrap_or(0),
        total_tokens: parsed.total_tokens.unwrap_or(0),
        usage_amounts: Vec::new(),
        baseline_usage_amounts: Vec::new(),
        updated_at,
        runtime_activity_at: parsed.last_event_at,
        last_user_input_at: parsed.last_user_message_at,
        started_at: parsed.started_at,
        completed_at: stale_completed_at.or(parsed.completed_at),
        response_state,
        was_interrupted: !stale_prelaunch_open_turn && parsed.was_interrupted,
        has_completed_turn: parsed.has_completed_turn && !stale_prelaunch_open_turn,
        session_origin: parsed.origin,
        source: if stale_prelaunch_open_turn {
            CODEX_STALE_PRELAUNCH_OPEN_TURN_SOURCE
        } else {
            "probe"
        }
        .to_string(),
        plan,
    })
}

fn stale_prelaunch_open_turn(
    parsed: &parse::CodexParsedState,
    file_path: &Path,
    launch_started_at: Option<f64>,
) -> bool {
    let Some(launch_started_at) = launch_started_at else {
        return false;
    };
    if parsed.response_state.as_deref() != Some("responding") {
        return false;
    }
    let Some(last_event_at) = parsed
        .last_event_at
        .or_else(|| transcript_modified_seconds(file_path))
    else {
        return false;
    };
    last_event_at + 1.0 < launch_started_at
}

fn transcript_modified_seconds(file_path: &Path) -> Option<f64> {
    std::fs::metadata(file_path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    const PROJECT_PATH: &str = "/tmp/codex-project";
    const LAUNCH_AT: f64 = 1_767_225_610.0;
    const POST_LAUNCH_AT: f64 = 1_767_225_612.0;

    fn request_for(path: &Path) -> AIRuntimeProbeRequest {
        request_for_updated_at(path, LAUNCH_AT)
    }

    fn request_for_updated_at(path: &Path, updated_at: f64) -> AIRuntimeProbeRequest {
        AIRuntimeProbeRequest {
            terminal_id: "terminal-1".to_string(),
            terminal_instance_id: Some("instance-1".to_string()),
            project_id: "project-1".to_string(),
            project_path: Some(PROJECT_PATH.to_string()),
            tool: "codex".to_string(),
            external_session_id: Some("session-1".to_string()),
            transcript_path: Some(path.display().to_string()),
            started_at: Some(LAUNCH_AT),
            updated_at,
            occupied_external_session_ids: Default::default(),
        }
    }

    fn temp_transcript(name: &str, contents: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("codux-codex-probe-{name}-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rollout.jsonl");
        fs::write(&path, contents).unwrap();
        (dir, path)
    }

    fn json_line(value: serde_json::Value) -> String {
        value.to_string()
    }

    fn turn_context(timestamp: &str) -> String {
        json_line(serde_json::json!({
            "timestamp": timestamp,
            "type": "turn_context",
            "payload": {
                "cwd": PROJECT_PATH,
                "model": "gpt-5",
                "approval_policy": "on-request"
            }
        }))
    }

    fn task_started(timestamp: &str) -> String {
        json_line(serde_json::json!({
            "timestamp": timestamp,
            "type": "event_msg",
            "payload": {"type": "task_started"}
        }))
    }

    fn function_call(timestamp: &str) -> String {
        json_line(serde_json::json!({
            "timestamp": timestamp,
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "shell",
                "arguments": "{}"
            }
        }))
    }

    fn agent_message(timestamp: &str) -> String {
        json_line(serde_json::json!({
            "timestamp": timestamp,
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "assistant",
                "content": [{"type":"output_text","text":"working"}]
            }
        }))
    }

    #[test]
    fn prelaunch_unfinished_resume_is_interrupted_idle() {
        let transcript = [
            turn_context("2026-01-01T00:00:00Z"),
            task_started("2026-01-01T00:00:00Z"),
            function_call("2026-01-01T00:00:00Z"),
        ]
        .join("\n");
        let (dir, path) = temp_transcript("stale", &(transcript + "\n"));

        let snapshot = probe_codex_runtime(&request_for_updated_at(&path, LAUNCH_AT + 30.0))
            .expect("snapshot");

        assert_eq!(snapshot.response_state.as_deref(), Some("idle"));
        assert!(!snapshot.was_interrupted);
        assert!(!snapshot.has_completed_turn);
        assert_eq!(snapshot.completed_at, Some(LAUNCH_AT + 30.0));
        assert!(snapshot.updated_at >= LAUNCH_AT + 30.0);
        assert_eq!(snapshot.source, CODEX_STALE_PRELAUNCH_OPEN_TURN_SOURCE);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn postlaunch_append_restores_responding() {
        let transcript = [
            turn_context("2026-01-01T00:00:00Z"),
            task_started("2026-01-01T00:00:00Z"),
            agent_message("2026-01-01T00:00:12Z"),
        ]
        .join("\n");
        let (dir, path) = temp_transcript("postlaunch", &(transcript + "\n"));

        let snapshot = probe_codex_runtime(&request_for(&path)).expect("snapshot");

        assert_eq!(snapshot.response_state.as_deref(), Some("responding"));
        assert!(!snapshot.was_interrupted);
        assert!(snapshot.updated_at >= POST_LAUNCH_AT);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn fresh_session_after_launch_stays_responding() {
        let transcript = [
            turn_context("2026-01-01T00:00:12Z"),
            task_started("2026-01-01T00:00:12Z"),
        ]
        .join("\n");
        let (dir, path) = temp_transcript("fresh", &(transcript + "\n"));

        let snapshot = probe_codex_runtime(&request_for(&path)).expect("snapshot");

        assert_eq!(snapshot.response_state.as_deref(), Some("responding"));
        assert!(!snapshot.was_interrupted);
        assert!(snapshot.started_at.unwrap_or_default() >= POST_LAUNCH_AT);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn large_tail_without_task_started_cannot_revive_prelaunch_turn() {
        let filler =
            "x".repeat(crate::ai_runtime::constants::CODEX_LIVE_TRANSCRIPT_TAIL_BYTES as usize);
        let transcript = [
            turn_context("2026-01-01T00:00:00Z"),
            task_started("2026-01-01T00:00:00Z"),
            json_line(serde_json::json!({
                "timestamp": "2026-01-01T00:00:01Z",
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type":"output_text","text": filler}]
                }
            })),
            json_line(serde_json::json!({
                "timestamp": "2026-01-01T00:00:02Z",
                "type": "response_item",
                "payload": {"type": "function_call_output", "call_id": "c1"}
            })),
        ]
        .join("\n");
        let (dir, path) = temp_transcript("large-tail", &(transcript + "\n"));

        let snapshot = probe_codex_runtime(&request_for(&path)).expect("snapshot");

        assert_eq!(snapshot.response_state.as_deref(), Some("idle"));
        assert!(!snapshot.was_interrupted);
        assert_eq!(snapshot.completed_at, Some(LAUNCH_AT));
        assert_eq!(snapshot.source, CODEX_STALE_PRELAUNCH_OPEN_TURN_SOURCE);
        fs::remove_dir_all(dir).unwrap();
    }
}
