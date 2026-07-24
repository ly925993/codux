use crate::ai_runtime::{
    probe::{
        common::parse_iso8601_seconds,
        paths::{directory_files, file_modified_millis, paths_equivalent},
        preview::joined_preview_from_values,
    },
    snapshot::{AIRuntimeContextSnapshot, AIRuntimeProbeRequest},
    state::normalized_string,
};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) fn probe_codewhale_runtime(
    request: &AIRuntimeProbeRequest,
) -> Option<AIRuntimeContextSnapshot> {
    let project_path = normalized_string(request.project_path.as_deref())?;
    let preferred_id = normalized_string(request.external_session_id.as_deref());
    let mut state = parse_current_codewhale_session(
        &project_path,
        preferred_id.as_deref(),
        normalized_string(request.transcript_path.as_deref()).map(PathBuf::from),
        request.started_at,
    )?;
    state.origin = codewhale_session_origin(&state, request.started_at);
    Some(codewhale_snapshot_from_state(request, state))
}

fn codewhale_session_origin(state: &CodeWhaleParsedState, started_at: Option<f64>) -> String {
    if started_at
        .map(|started| state.started_at + 1.0 >= started || state.updated_at + 1.0 >= started)
        .unwrap_or(false)
    {
        "fresh".to_string()
    } else {
        "restored".to_string()
    }
}

fn codewhale_snapshot_from_state(
    request: &AIRuntimeProbeRequest,
    state: CodeWhaleParsedState,
) -> AIRuntimeContextSnapshot {
    let response_state = state
        .response_state
        .clone()
        .or_else(|| (state.origin == "restored").then(|| "idle".to_string()));
    let has_completed_turn = state.has_completed_turn
        || (state.origin == "restored" && response_state.as_deref() == Some("idle"));
    let completed_at = has_completed_turn.then_some(state.completed_at.unwrap_or(state.updated_at));
    AIRuntimeContextSnapshot {
        tool: "codewhale".to_string(),
        external_session_id: Some(state.external_session_id),
        transcript_path: normalized_string(Some(&state.file_path)),
        model: state.model,
        assistant_preview: state.assistant_preview,
        input_tokens: 0,
        output_tokens: 0,
        cached_input_tokens: 0,
        total_tokens: state.total_tokens,
        usage_amounts: Vec::new(),
        baseline_usage_amounts: Vec::new(),
        updated_at: state.updated_at.max(request.updated_at),
        runtime_activity_at: Some(state.updated_at),
        last_user_input_at: state.last_user_input_at,
        started_at: Some(state.started_at),
        completed_at,
        response_state,
        was_interrupted: false,
        has_completed_turn,
        session_origin: state.origin,
        source: "probe".to_string(),
        plan: None,
    }
}

#[derive(Clone)]
struct CodeWhaleParsedState {
    external_session_id: String,
    file_path: String,
    model: Option<String>,
    assistant_preview: Option<String>,
    started_at: f64,
    updated_at: f64,
    last_user_input_at: Option<f64>,
    completed_at: Option<f64>,
    response_state: Option<String>,
    has_completed_turn: bool,
    origin: String,
    total_tokens: i64,
}

pub(crate) fn codewhale_runtime_resource_paths(session_id: Option<&str>) -> Vec<PathBuf> {
    if let Some(session_id) = session_id.and_then(codewhale_valid_record_id) {
        return vec![
            codewhale_home()
                .join("sessions")
                .join(format!("{session_id}.json")),
        ];
    }
    codewhale_session_paths().into_iter().take(32).collect()
}

fn codewhale_session_paths() -> Vec<PathBuf> {
    let mut paths = directory_files(&codewhale_home().join("sessions"), "json");
    paths.sort_by_key(|path| std::cmp::Reverse(file_modified_millis(path).unwrap_or(0)));
    paths
}

fn codewhale_home() -> PathBuf {
    codewhale_home_override().unwrap_or_else(|| crate::runtime_paths::home_dir().join(".codewhale"))
}

fn codewhale_home_override() -> Option<PathBuf> {
    std::env::var_os("CODEWHALE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn codewhale_valid_record_id(id: &str) -> Option<&str> {
    let trimmed = id.trim();
    if trimmed.is_empty() || trimmed != id {
        return None;
    }
    trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        .then_some(trimmed)
}

fn parse_current_codewhale_session(
    project_path: &str,
    preferred_id: Option<&str>,
    transcript_path: Option<PathBuf>,
    started_at: Option<f64>,
) -> Option<CodeWhaleParsedState> {
    let session_paths = transcript_path
        .map(|path| vec![path])
        .unwrap_or_else(codewhale_session_paths);
    session_paths
        .into_iter()
        .filter(|path| {
            preferred_id
                .and_then(codewhale_valid_record_id)
                .map(|id| path.file_stem().and_then(|value| value.to_str()) == Some(id))
                .unwrap_or(true)
        })
        .filter(|path| {
            preferred_id.is_some()
                || started_at
                    .map(|started_at| {
                        file_modified_millis(path)
                            .map(|modified| modified as f64 + 1_000.0 >= started_at * 1000.0)
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
        })
        .filter_map(|path| parse_current_codewhale_session_file(&path, project_path))
        .max_by(|left, right| left.updated_at.total_cmp(&right.updated_at))
}

fn parse_current_codewhale_session_file(
    path: &Path,
    project_path: &str,
) -> Option<CodeWhaleParsedState> {
    let root = serde_json::from_str::<Value>(&fs::read_to_string(path).ok()?).ok()?;
    let metadata = root.get("metadata").unwrap_or(&Value::Null);
    let workspace = metadata
        .get("workspace")
        .and_then(|value| value.as_str())
        .and_then(|value| normalized_string(Some(value)))?;
    if !paths_equivalent(Some(&workspace), project_path) {
        return None;
    }
    let external_session_id = metadata
        .get("id")
        .and_then(|value| value.as_str())
        .and_then(|value| normalized_string(Some(value)))
        .or_else(|| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .and_then(|value| normalized_string(Some(value)))
        })?;
    let created_at = metadata
        .get("created_at")
        .or_else(|| metadata.get("createdAt"))
        .and_then(|value| value.as_str())
        .and_then(parse_iso8601_seconds)
        .unwrap_or_else(|| {
            file_modified_millis(path)
                .map(|value| value as f64 / 1000.0)
                .unwrap_or(0.0)
        });
    let updated_at = metadata
        .get("updated_at")
        .or_else(|| metadata.get("updatedAt"))
        .and_then(|value| value.as_str())
        .and_then(parse_iso8601_seconds)
        .unwrap_or(created_at);
    let messages = root
        .get("messages")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let last_role = messages
        .iter()
        .rev()
        .find_map(|message| message.get("role").and_then(|value| value.as_str()));
    // CodeWhale's current file format updates metadata with every appended
    // message. A trailing user row therefore provides a reliable native-input
    // acknowledgement even though older rows may omit individual timestamps.
    let last_user_input_at = (last_role == Some("user")).then_some(updated_at);
    let response_state = match last_role {
        Some("user") => Some("responding".to_string()),
        Some("assistant") => Some("idle".to_string()),
        _ => None,
    };
    let assistant_preview = messages
        .iter()
        .rev()
        .filter(|message| message.get("role").and_then(|value| value.as_str()) == Some("assistant"))
        .find_map(|message| joined_preview_from_values(&[message.get("content")]))
        .or_else(|| normalized_string(metadata.get("title").and_then(|value| value.as_str())));

    Some(CodeWhaleParsedState {
        external_session_id,
        file_path: path.display().to_string(),
        model: normalized_string(metadata.get("model").and_then(|value| value.as_str())),
        assistant_preview,
        started_at: created_at,
        updated_at,
        last_user_input_at,
        completed_at: (response_state.as_deref() == Some("idle")).then_some(updated_at),
        response_state,
        has_completed_turn: last_role == Some("assistant"),
        origin: "restored".to_string(),
        total_tokens: metadata
            .get("total_tokens")
            .or_else(|| metadata.get("totalTokens"))
            .and_then(|value| value.as_i64())
            .unwrap_or(0)
            .max(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn parses_current_codewhale_json_session_for_project() {
        let root = std::env::temp_dir().join(format!("codux-codewhale-json-{}", Uuid::new_v4()));
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        let session_path = root.join("session-1.json");
        fs::write(
            &session_path,
            serde_json::json!({
                "schema_version": 1,
                "metadata": {
                    "id": "session-1",
                    "title": "hello",
                    "created_at": "2026-06-28T07:28:16Z",
                    "updated_at": "2026-06-28T07:28:20Z",
                    "total_tokens": 34448,
                    "model": "deepseek-v4-pro",
                    "workspace": project.display().to_string(),
                    "mode": "agent"
                },
                "messages": [
                    {"role": "user", "content": [{"type": "text", "text": "hi"}]},
                    {"role": "assistant", "content": [{"type": "thinking", "thinking": "..."}, {"type": "text", "text": "done"}]}
                ]
            })
            .to_string(),
        )
        .unwrap();

        let parsed =
            parse_current_codewhale_session_file(&session_path, project.to_str().unwrap()).unwrap();

        assert_eq!(parsed.external_session_id, "session-1");
        assert_eq!(parsed.file_path, session_path.display().to_string());
        assert_eq!(parsed.model.as_deref(), Some("deepseek-v4-pro"));
        assert_eq!(parsed.assistant_preview.as_deref(), Some("done"));
        assert_eq!(parsed.response_state.as_deref(), Some("idle"));
        assert_eq!(parsed.total_tokens, 34448);
        assert!(parsed.has_completed_turn);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn current_codewhale_probe_reports_json_session() {
        let root = std::env::temp_dir().join(format!("codux-codewhale-probe-{}", Uuid::new_v4()));
        let home = root.join("home");
        let sessions = home.join("sessions");
        let project = root.join("project");
        fs::create_dir_all(&sessions).unwrap();
        fs::create_dir_all(&project).unwrap();
        fs::write(
            sessions.join("thread-1.json"),
            serde_json::json!({
                "schema_version": 1,
                "metadata": {
                    "id": "thread-1",
                    "created_at": "2026-06-28T07:28:16Z",
                    "updated_at": "2026-06-28T07:28:20Z",
                    "total_tokens": 12,
                    "model": "deepseek-v4-pro",
                    "workspace": project.display().to_string()
                },
                "messages": [{"role":"user","content":[{"type":"text","text":"build"}]}]
            })
            .to_string(),
        )
        .unwrap();
        let old_home = std::env::var_os("CODEWHALE_HOME");
        unsafe {
            std::env::set_var("CODEWHALE_HOME", &home);
        }

        let snapshot = probe_codewhale_runtime(&AIRuntimeProbeRequest {
            terminal_id: "terminal-1".to_string(),
            terminal_instance_id: Some("instance-1".to_string()),
            project_id: "project-1".to_string(),
            project_path: Some(project.display().to_string()),
            tool: "codewhale".to_string(),
            external_session_id: Some("thread-1".to_string()),
            transcript_path: None,
            started_at: Some(1_782_631_690.0),
            updated_at: 1.0,
            occupied_external_session_ids: Default::default(),
        })
        .unwrap();

        assert_eq!(snapshot.external_session_id.as_deref(), Some("thread-1"));
        assert_eq!(snapshot.response_state.as_deref(), Some("responding"));
        assert_eq!(snapshot.session_origin, "fresh");
        assert_eq!(snapshot.total_tokens, 12);

        restore_env("CODEWHALE_HOME", old_home);
        let _ = fs::remove_dir_all(root);
    }

    fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
        unsafe {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}
