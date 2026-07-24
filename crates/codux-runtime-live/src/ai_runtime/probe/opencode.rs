use crate::ai_runtime::{
    probe::{
        common::{json_i64, parse_iso8601_seconds},
        paths::{mimo_database_paths, opencode_database_paths, paths_equivalent},
        preview::joined_preview_from_values,
    },
    snapshot::{AIPlanItem, AIPlanSnapshot, AIRuntimeContextSnapshot, AIRuntimeProbeRequest},
    state::{canonical_tool_name, normalized_string},
};
use serde_json::Value;

pub(crate) fn probe_opencode_runtime(
    request: &AIRuntimeProbeRequest,
) -> Option<AIRuntimeContextSnapshot> {
    for database_path in opencode_database_paths() {
        if let Some(snapshot) = probe_opencode_like_runtime(request, database_path) {
            return Some(snapshot);
        }
    }
    None
}

pub(crate) fn probe_mimo_runtime(
    request: &AIRuntimeProbeRequest,
) -> Option<AIRuntimeContextSnapshot> {
    let project_path = normalized_string(request.project_path.as_deref())?;
    let external_session_id = normalized_string(request.external_session_id.as_deref());
    for database_path in mimo_database_paths() {
        if !database_path.exists() {
            continue;
        }
        let Ok(conn) = rusqlite::Connection::open(&database_path) else {
            continue;
        };
        if let Some(snapshot) = probe_mimo_schema(
            &conn,
            &database_path,
            &project_path,
            external_session_id.as_deref(),
            request,
        ) {
            return Some(snapshot);
        }
    }
    None
}

fn probe_opencode_like_runtime(
    request: &AIRuntimeProbeRequest,
    database_path: std::path::PathBuf,
) -> Option<AIRuntimeContextSnapshot> {
    let project_path = normalized_string(request.project_path.as_deref())?;
    if !database_path.exists() {
        return None;
    }
    let conn = rusqlite::Connection::open(&database_path).ok()?;
    let external_session_id =
        normalized_string(request.external_session_id.as_deref()).or_else(|| {
            newest_opencode_session_for_project(&conn, &project_path, request.started_at)
        })?;
    probe_opencode_v2_schema(
        &conn,
        &database_path,
        &project_path,
        external_session_id.as_str(),
        request,
    )
}

fn opencode_snapshot_tool(request: &AIRuntimeProbeRequest) -> String {
    canonical_tool_name(&request.tool).unwrap_or_else(|| "opencode".to_string())
}

#[derive(Default)]
struct OpenCodeParsedMessages {
    latest_model: Option<String>,
    assistant_preview: Option<String>,
    updated_at: f64,
    last_user_at: f64,
    last_completion_at: f64,
    input_tokens: i64,
    output_tokens: i64,
    cached_input_tokens: i64,
    total_tokens: i64,
    had_row: bool,
}

impl OpenCodeParsedMessages {
    fn response_state(&self) -> Option<String> {
        if self.last_user_at > 0.0 {
            if self.last_user_at > self.last_completion_at {
                Some("responding".to_string())
            } else {
                Some("idle".to_string())
            }
        } else if self.total_tokens > 0 || self.last_completion_at > 0.0 {
            Some("idle".to_string())
        } else {
            None
        }
    }

    fn has_completed_turn(&self) -> bool {
        self.last_completion_at > 0.0 && self.last_completion_at >= self.last_user_at
    }
}

fn opencode_snapshot_from_parsed(
    request: &AIRuntimeProbeRequest,
    database_path: &std::path::Path,
    session_id: &str,
    parsed: OpenCodeParsedMessages,
    session_origin: String,
    plan: Option<AIPlanSnapshot>,
) -> AIRuntimeContextSnapshot {
    let has_completed_turn = parsed.has_completed_turn();
    let response_state = parsed.response_state();
    AIRuntimeContextSnapshot {
        tool: opencode_snapshot_tool(request),
        external_session_id: Some(session_id.to_string()),
        transcript_path: Some(database_path.display().to_string()),
        model: parsed.latest_model,
        assistant_preview: parsed.assistant_preview,
        input_tokens: parsed.input_tokens.max(0),
        output_tokens: parsed.output_tokens.max(0),
        cached_input_tokens: parsed.cached_input_tokens.max(0),
        total_tokens: parsed.total_tokens.max(0),
        usage_amounts: Vec::new(),
        baseline_usage_amounts: Vec::new(),
        updated_at: parsed.updated_at.max(request.updated_at),
        runtime_activity_at: (parsed.updated_at > 0.0).then_some(parsed.updated_at),
        last_user_input_at: (parsed.last_user_at > 0.0).then_some(parsed.last_user_at),
        started_at: (parsed.last_user_at > 0.0).then_some(parsed.last_user_at),
        completed_at: has_completed_turn.then_some(parsed.last_completion_at),
        response_state,
        was_interrupted: false,
        has_completed_turn,
        session_origin,
        source: "probe".to_string(),
        plan,
    }
}

fn probe_opencode_v2_schema(
    conn: &rusqlite::Connection,
    database_path: &std::path::Path,
    project_path: &str,
    external_session_id: &str,
    request: &AIRuntimeProbeRequest,
) -> Option<AIRuntimeContextSnapshot> {
    let session = conn
        .query_row(
            r#"
            SELECT id, directory, COALESCE(path, ''), time_updated, time_created,
                   tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, model
            FROM session
            WHERE id = ?1
              AND time_archived IS NULL
            LIMIT 1;
            "#,
            [external_session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, Option<String>>(9)?,
                ))
            },
        )
        .ok()?;
    let (
        session_id,
        directory,
        sub_path,
        session_updated_at,
        session_created_at,
        input_tokens,
        output_tokens,
        reasoning_tokens,
        cached_input_tokens,
        model,
    ) = session;
    if !opencode_v2_session_matches_project(&directory, &sub_path, project_path) {
        return None;
    }

    let mut parsed = OpenCodeParsedMessages {
        latest_model: model.as_deref().and_then(opencode_model_from_value),
        input_tokens,
        output_tokens,
        cached_input_tokens,
        total_tokens: input_tokens + output_tokens + reasoning_tokens + cached_input_tokens,
        updated_at: opencode_epoch_value_seconds(session_updated_at)
            .max(opencode_epoch_value_seconds(session_created_at)),
        ..Default::default()
    };
    parse_opencode_session_message_rows(conn, &mut parsed, &session_id);
    if !parsed.had_row {
        parse_opencode_message_part_rows(conn, &mut parsed, &session_id);
    }
    if !parsed.had_row {
        return None;
    }
    Some(opencode_snapshot_from_parsed(
        request,
        database_path,
        &session_id,
        parsed,
        opencode_like_session_origin(session_created_at, request.started_at),
        opencode_plan(conn, &session_id),
    ))
}

fn parse_opencode_session_message_rows(
    conn: &rusqlite::Connection,
    parsed: &mut OpenCodeParsedMessages,
    session_id: &str,
) {
    let Ok(mut statement) = conn.prepare(
        r#"
        SELECT type, data, time_created, time_updated
        FROM session_message
        WHERE session_id = ?1
        ORDER BY seq ASC;
        "#,
    ) else {
        return;
    };
    let Ok(rows) = statement.query_map([session_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
        ))
    }) else {
        return;
    };

    for row in rows.flatten() {
        let (message_type, data, created_at_raw, updated_at_raw) = row;
        parsed.had_row = true;
        let data = serde_json::from_str::<Value>(&data).unwrap_or(Value::Null);
        let created_at = opencode_epoch_value_seconds(created_at_raw);
        let updated_at = opencode_epoch_value_seconds(updated_at_raw);
        parsed.updated_at = parsed.updated_at.max(created_at).max(updated_at);
        match message_type.as_str() {
            "user" => parsed.last_user_at = parsed.last_user_at.max(created_at),
            "assistant" => parse_opencode_v2_assistant_message(parsed, &data, created_at, false),
            _ => {}
        }
    }
}

fn parse_opencode_message_part_rows(
    conn: &rusqlite::Connection,
    parsed: &mut OpenCodeParsedMessages,
    session_id: &str,
) {
    let Ok(mut statement) = conn.prepare(
        r#"
        SELECT id, data, time_created, time_updated
        FROM message
        WHERE session_id = ?1
        ORDER BY time_created ASC, id ASC;
        "#,
    ) else {
        return;
    };
    let Ok(rows) = statement.query_map([session_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
        ))
    }) else {
        return;
    };

    for row in rows.flatten() {
        let (message_id, data, created_at_raw, updated_at_raw) = row;
        parsed.had_row = true;
        let data = serde_json::from_str::<Value>(&data).unwrap_or(Value::Null);
        let created_at = opencode_epoch_value_seconds(created_at_raw);
        let updated_at = opencode_epoch_value_seconds(updated_at_raw);
        parsed.updated_at = parsed.updated_at.max(created_at).max(updated_at);
        let role = data
            .get("role")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if role == "user" {
            parsed.last_user_at = parsed.last_user_at.max(created_at);
        } else if role == "assistant" {
            parse_opencode_message_part_assistant_message(
                conn,
                parsed,
                &message_id,
                &data,
                created_at,
            );
        }
    }
}

fn newest_opencode_session_for_project(
    conn: &rusqlite::Connection,
    project_path: &str,
    started_at: Option<f64>,
) -> Option<String> {
    let mut statement = conn
        .prepare(
            r#"
            SELECT id, directory, COALESCE(path, ''), time_updated, time_created
            FROM session
            WHERE time_archived IS NULL
            ORDER BY time_updated DESC
            LIMIT 64;
            "#,
        )
        .ok()?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .ok()?;
    rows.filter_map(Result::ok)
        .filter(|(_, directory, sub_path, updated_at, created_at)| {
            opencode_v2_session_matches_project(directory, sub_path, project_path)
                && opencode_like_session_touched_after_start(*updated_at, *created_at, started_at)
        })
        .max_by_key(|(_, _, _, updated_at, _)| *updated_at)
        .map(|(id, _, _, _, _)| id)
}

fn probe_mimo_schema(
    conn: &rusqlite::Connection,
    database_path: &std::path::Path,
    project_path: &str,
    external_session_id: Option<&str>,
    request: &AIRuntimeProbeRequest,
) -> Option<AIRuntimeContextSnapshot> {
    let session = if let Some(external_session_id) = external_session_id {
        conn.query_row(
            r#"
                SELECT id, directory, time_updated, time_created
                FROM session
                WHERE id = ?1
                  AND time_archived IS NULL
                LIMIT 1;
                "#,
            [external_session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .ok()?
    } else {
        newest_mimo_session_for_project(conn, project_path, request.started_at)?
    };
    let (session_id, directory, session_updated_at, session_created_at) = session;
    if !paths_equivalent(Some(&directory), project_path) {
        return None;
    }

    let mut parsed = OpenCodeParsedMessages {
        updated_at: opencode_epoch_value_seconds(session_updated_at)
            .max(opencode_epoch_value_seconds(session_created_at)),
        ..Default::default()
    };
    let mut statement = conn
        .prepare(
            r#"
            SELECT id, data, time_created, time_updated
            FROM message
            WHERE session_id = ?1
            ORDER BY time_created ASC, id ASC;
            "#,
        )
        .ok()?;
    let rows = statement
        .query_map([session_id.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .ok()?;

    for row in rows.flatten() {
        let (message_id, data, created_at_raw, updated_at_raw) = row;
        parsed.had_row = true;
        let data = serde_json::from_str::<Value>(&data).unwrap_or(Value::Null);
        let created_at = opencode_epoch_value_seconds(created_at_raw);
        let updated_at = opencode_epoch_value_seconds(updated_at_raw);
        parsed.updated_at = parsed.updated_at.max(created_at).max(updated_at);
        let role = data
            .get("role")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if role == "user" {
            parsed.last_user_at = parsed.last_user_at.max(created_at);
        } else if role == "assistant" {
            parse_mimo_assistant_message(conn, &mut parsed, &message_id, &data, created_at);
        }
    }

    if !parsed.had_row {
        return None;
    }
    Some(opencode_snapshot_from_parsed(
        request,
        database_path,
        &session_id,
        parsed,
        opencode_like_session_origin(session_created_at, request.started_at),
        mimo_plan(conn, &session_id),
    ))
}

fn newest_mimo_session_for_project(
    conn: &rusqlite::Connection,
    project_path: &str,
    started_at: Option<f64>,
) -> Option<(String, String, i64, i64)> {
    let mut statement = conn
        .prepare(
            r#"
            SELECT id, directory, time_updated, time_created
            FROM session
            WHERE time_archived IS NULL
            ORDER BY time_updated DESC
            LIMIT 64;
            "#,
        )
        .ok()?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .ok()?;
    rows.filter_map(Result::ok)
        .filter(|(_, directory, updated_at, created_at)| {
            paths_equivalent(Some(directory), project_path)
                && started_at
                    .map(|started_at| {
                        opencode_epoch_value_seconds(*updated_at)
                            .max(opencode_epoch_value_seconds(*created_at))
                            + 1.0
                            >= started_at
                    })
                    .unwrap_or(true)
        })
        .max_by_key(|(_, _, updated_at, _)| *updated_at)
}

fn opencode_like_session_touched_after_start(
    updated_at: i64,
    created_at: i64,
    started_at: Option<f64>,
) -> bool {
    started_at
        .map(|started_at| {
            opencode_epoch_value_seconds(updated_at).max(opencode_epoch_value_seconds(created_at))
                + 1.0
                >= started_at
        })
        .unwrap_or(true)
}

fn opencode_like_session_origin(created_at: i64, started_at: Option<f64>) -> String {
    if started_at
        .map(|started_at| opencode_epoch_value_seconds(created_at) + 1.0 >= started_at)
        .unwrap_or(false)
    {
        "fresh".to_string()
    } else {
        "restored".to_string()
    }
}

fn opencode_plan(conn: &rusqlite::Connection, session_id: &str) -> Option<AIPlanSnapshot> {
    let mut statement = conn
        .prepare(
            r#"
            SELECT content, status, priority, time_updated
            FROM todo
            WHERE session_id = ?1
            ORDER BY position ASC;
            "#,
        )
        .ok()?;
    let rows = statement
        .query_map([session_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .ok()?;
    let mut updated_at = 0.0f64;
    let mut items = Vec::new();
    for row in rows.flatten() {
        let (content, status, priority, time_updated) = row;
        let Some(text) = normalized_string(Some(&content)) else {
            continue;
        };
        updated_at = updated_at.max(time_updated as f64 / 1000.0);
        items.push(AIPlanItem {
            text,
            status: normalized_plan_status(&status),
            priority: priority
                .as_deref()
                .and_then(|priority| normalized_string(Some(priority))),
        });
    }
    (!items.is_empty()).then_some(AIPlanSnapshot {
        source: "opencode".to_string(),
        session_id: session_id.to_string(),
        updated_at,
        items,
    })
}

fn mimo_plan(conn: &rusqlite::Connection, session_id: &str) -> Option<AIPlanSnapshot> {
    let mut statement = conn
        .prepare(
            r#"
            SELECT content, status, time_updated
            FROM todo
            WHERE session_id = ?1
            ORDER BY position ASC;
            "#,
        )
        .ok()?;
    let rows = statement
        .query_map([session_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .ok()?;
    let mut updated_at = 0.0f64;
    let mut items = Vec::new();
    for row in rows.flatten() {
        let (content, status, time_updated) = row;
        let Some(text) = normalized_string(Some(&content)) else {
            continue;
        };
        updated_at = updated_at.max(opencode_epoch_value_seconds(time_updated));
        items.push(AIPlanItem {
            text,
            status: normalized_plan_status(&status),
            priority: None,
        });
    }
    (!items.is_empty()).then_some(AIPlanSnapshot {
        source: "mimo".to_string(),
        session_id: session_id.to_string(),
        updated_at,
        items,
    })
}

fn opencode_v2_session_matches_project(
    directory: &str,
    sub_path: &str,
    project_path: &str,
) -> bool {
    if paths_equivalent(Some(directory), project_path) {
        return true;
    }
    let sub_path = sub_path.trim_matches('/');
    if sub_path.is_empty() {
        return false;
    }
    let candidate = std::path::Path::new(directory).join(sub_path);
    candidate
        .to_str()
        .map(|path| paths_equivalent(Some(path), project_path))
        .unwrap_or(false)
}

fn opencode_model_from_value(value: &str) -> Option<String> {
    normalized_string(Some(value)).and_then(|value| {
        serde_json::from_str::<Value>(&value)
            .ok()
            .and_then(|root| {
                root.get("id")
                    .and_then(|value| value.as_str())
                    .or_else(|| root.get("modelID").and_then(|value| value.as_str()))
                    .or_else(|| root.get("model").and_then(|value| value.as_str()))
                    .and_then(|value| normalized_string(Some(value)))
            })
            .or(Some(value))
    })
}

fn parse_opencode_v2_assistant_message(
    parsed: &mut OpenCodeParsedMessages,
    data: &Value,
    created_at: f64,
    include_tokens: bool,
) {
    let completed_at = data
        .get("time")
        .and_then(|time| time.get("completed"))
        .and_then(opencode_value_timestamp);
    if let Some(completed_at) = completed_at {
        parsed.updated_at = parsed.updated_at.max(completed_at);
    }
    if parsed.latest_model.is_none() {
        parsed.latest_model = data
            .get("model")
            .and_then(|value| value.as_str())
            .and_then(opencode_model_from_value);
    }
    if parsed.assistant_preview.is_none() {
        parsed.assistant_preview = opencode_v2_assistant_preview(data);
    }
    if include_tokens {
        absorb_assistant_tokens(parsed, data);
    }

    let finish = data
        .get("finish")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let has_open_tool = data
        .get("content")
        .and_then(|value| value.as_array())
        .map(|items| {
            items.iter().any(|item| {
                item.get("type").and_then(|value| value.as_str()) == Some("tool")
                    && !opencode_v2_tool_state_is_terminal(
                        item.get("state").unwrap_or(&Value::Null),
                    )
            })
        })
        .unwrap_or(false);
    if !has_open_tool && is_opencode_final_assistant_finish(finish, completed_at) {
        parsed.last_completion_at = parsed
            .last_completion_at
            .max(completed_at.unwrap_or(created_at));
    }
}

fn parse_opencode_message_part_assistant_message(
    conn: &rusqlite::Connection,
    parsed: &mut OpenCodeParsedMessages,
    message_id: &str,
    data: &Value,
    created_at: f64,
) {
    let completed_at = data
        .get("time")
        .and_then(|time| time.get("completed"))
        .and_then(opencode_value_timestamp);
    if let Some(completed_at) = completed_at {
        parsed.updated_at = parsed.updated_at.max(completed_at);
    }
    if parsed.latest_model.is_none() {
        parsed.latest_model = data
            .get("model")
            .and_then(|value| value.as_str())
            .and_then(opencode_model_from_value)
            .or_else(|| {
                data.get("modelID")
                    .and_then(|value| value.as_str())
                    .and_then(opencode_model_from_value)
            });
    }

    let mut has_open_tool = false;
    let mut finish = data
        .get("finish")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    if let Some(parts) = mimo_parts_for_message(conn, message_id) {
        for part in parts {
            let part_type = part.get("type").and_then(|value| value.as_str());
            if parsed.assistant_preview.is_none() && matches!(part_type, Some("text" | "reasoning"))
            {
                parsed.assistant_preview =
                    joined_preview_from_values(&[part.get("text"), part.get("content")]);
            }
            if part_type == Some("tool")
                && !opencode_v2_tool_state_is_terminal(part.get("state").unwrap_or(&Value::Null))
            {
                has_open_tool = true;
            }
            if part_type == Some("step-finish") && finish.is_none() {
                finish = part
                    .get("reason")
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
            }
        }
    }

    if !has_open_tool
        && is_opencode_final_assistant_finish(finish.as_deref().unwrap_or(""), completed_at)
    {
        parsed.last_completion_at = parsed
            .last_completion_at
            .max(completed_at.unwrap_or(created_at));
    }
}

fn absorb_assistant_tokens(parsed: &mut OpenCodeParsedMessages, data: &Value) {
    let tokens = data.get("tokens").unwrap_or(&Value::Null);
    let input = json_i64(tokens.get("input"));
    let output = json_i64(tokens.get("output"));
    let reasoning = json_i64(tokens.get("reasoning"));
    let cache = tokens.get("cache").unwrap_or(&Value::Null);
    let cache_read = json_i64(cache.get("read"));
    parsed.input_tokens += input;
    parsed.output_tokens += output;
    parsed.cached_input_tokens += cache_read;
    parsed.total_tokens += input + output + reasoning + cache_read;
}

fn opencode_v2_assistant_preview(data: &Value) -> Option<String> {
    let content = data.get("content")?.as_array()?;
    for item in content {
        let item_type = item.get("type").and_then(|value| value.as_str());
        if !matches!(item_type, Some("text") | Some("reasoning")) {
            continue;
        }
        if let Some(preview) = joined_preview_from_values(&[
            item.get("text"),
            item.get("content"),
            item.get("summary"),
        ]) {
            return Some(preview);
        }
    }
    None
}

fn opencode_v2_tool_state_is_terminal(state: &Value) -> bool {
    matches!(
        state.get("status").and_then(|value| value.as_str()),
        Some("completed" | "error" | "failed")
    )
}

fn parse_mimo_assistant_message(
    conn: &rusqlite::Connection,
    parsed: &mut OpenCodeParsedMessages,
    message_id: &str,
    data: &Value,
    created_at: f64,
) {
    let completed_at = data
        .get("time")
        .and_then(|time| time.get("completed"))
        .and_then(opencode_value_timestamp);
    if let Some(completed_at) = completed_at {
        parsed.updated_at = parsed.updated_at.max(completed_at);
    }
    if parsed.latest_model.is_none() {
        parsed.latest_model = data
            .get("modelID")
            .and_then(|value| value.as_str())
            .and_then(|value| normalized_string(Some(value)));
    }
    absorb_assistant_tokens(parsed, data);

    let mut has_open_tool = false;
    if let Some(parts) = mimo_parts_for_message(conn, message_id) {
        for part in parts {
            let part_type = part.get("type").and_then(|value| value.as_str());
            if parsed.assistant_preview.is_none() && matches!(part_type, Some("text" | "reasoning"))
            {
                parsed.assistant_preview =
                    joined_preview_from_values(&[part.get("text"), part.get("content")]);
            }
            if part_type == Some("tool")
                && !opencode_v2_tool_state_is_terminal(part.get("state").unwrap_or(&Value::Null))
            {
                has_open_tool = true;
            }
        }
    }

    let finish = data
        .get("finish")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if !has_open_tool && is_opencode_final_assistant_finish(finish, completed_at) {
        parsed.last_completion_at = parsed
            .last_completion_at
            .max(completed_at.unwrap_or(created_at));
    }
}

fn mimo_parts_for_message(conn: &rusqlite::Connection, message_id: &str) -> Option<Vec<Value>> {
    let mut statement = conn
        .prepare(
            r#"
            SELECT data
            FROM part
            WHERE message_id = ?1
            ORDER BY time_created ASC, id ASC;
            "#,
        )
        .ok()?;
    let rows = statement
        .query_map([message_id], |row| row.get::<_, String>(0))
        .ok()?;
    Some(
        rows.filter_map(Result::ok)
            .filter_map(|data| serde_json::from_str::<Value>(&data).ok())
            .collect(),
    )
}

fn normalized_plan_status(value: &str) -> String {
    match value.trim() {
        "completed" | "complete" | "done" => "completed",
        "in_progress" | "in-progress" | "running" | "active" => "in_progress",
        _ => "pending",
    }
    .to_string()
}

fn is_opencode_final_assistant_finish(value: &str, completed_at: Option<f64>) -> bool {
    let normalized = value.trim().to_lowercase();
    if normalized.is_empty() {
        return completed_at.is_some();
    }
    normalized != "tool-calls"
}

fn opencode_epoch_value_seconds(value: i64) -> f64 {
    // OpenCode's initial migration comments say milliseconds, but the checked-in
    // SQL writes `strftime('%s', 'now')` (seconds). Accept both because users may
    // have DBs created by either implementation.
    let value = value.max(0);
    if value >= 10_000_000_000 {
        value as f64 / 1000.0
    } else {
        value as f64
    }
}

fn opencode_value_timestamp(value: &Value) -> Option<f64> {
    let raw = value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_i64().map(|value| value.to_string()))
        .or_else(|| value.as_f64().map(|value| value.to_string()))?;
    if let Ok(milliseconds) = raw.parse::<f64>() {
        return Some(milliseconds / 1000.0);
    }
    parse_iso8601_seconds(&raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v2_schema() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().expect("db");
        conn.execute_batch(
            r#"
            CREATE TABLE session (
                id TEXT PRIMARY KEY,
                directory TEXT NOT NULL,
                path TEXT,
                time_updated INTEGER NOT NULL,
                time_created INTEGER NOT NULL,
                tokens_input INTEGER NOT NULL DEFAULT 0,
                tokens_output INTEGER NOT NULL DEFAULT 0,
                tokens_reasoning INTEGER NOT NULL DEFAULT 0,
                tokens_cache_read INTEGER NOT NULL DEFAULT 0,
                model TEXT,
                time_archived INTEGER
            );
            CREATE TABLE session_message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                type TEXT NOT NULL,
                seq INTEGER NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            CREATE TABLE todo (
                session_id TEXT,
                content TEXT,
                status TEXT,
                priority TEXT,
                position INTEGER,
                time_updated INTEGER
            );
            "#,
        )
        .expect("schema");
        conn
    }

    fn mimo_schema() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().expect("db");
        conn.execute_batch(
            r#"
            CREATE TABLE session (
                id TEXT PRIMARY KEY,
                directory TEXT NOT NULL,
                time_updated INTEGER NOT NULL,
                time_created INTEGER NOT NULL,
                time_archived INTEGER
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                agent_id TEXT NOT NULL DEFAULT 'main',
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            CREATE TABLE part (
                id TEXT PRIMARY KEY,
                message_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            CREATE TABLE todo (
                session_id TEXT,
                content TEXT,
                status TEXT,
                position INTEGER,
                time_updated INTEGER
            );
            "#,
        )
        .expect("schema");
        conn
    }

    fn current_opencode_schema() -> rusqlite::Connection {
        let conn = v2_schema();
        conn.execute_batch(
            r#"
            CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            CREATE TABLE part (
                id TEXT PRIMARY KEY,
                message_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            "#,
        )
        .expect("current schema");
        conn
    }

    fn request_for_opencode_session(session_id: &str) -> AIRuntimeProbeRequest {
        AIRuntimeProbeRequest {
            terminal_id: "terminal-1".to_string(),
            terminal_instance_id: Some("instance-1".to_string()),
            project_id: "project-1".to_string(),
            project_path: Some("/tmp/project".to_string()),
            tool: "opencode".to_string(),
            external_session_id: Some(session_id.to_string()),
            transcript_path: None,
            started_at: Some(900.0),
            updated_at: 900.0,
            occupied_external_session_ids: Default::default(),
        }
    }

    #[test]
    fn v2_schema_final_assistant_message_is_idle() {
        let conn = v2_schema();
        conn.execute(
            "INSERT INTO session (id, directory, path, time_updated, time_created, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, model)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                "session-v2",
                "/tmp",
                "project",
                1_700_000_003_000i64,
                1_700_000_000_000i64,
                10i64,
                20i64,
                3i64,
                4i64,
                r#"{"id":"gpt-4.1","providerID":"openai"}"#
            ],
        )
        .expect("insert session");
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                "msg-user",
                "session-v2",
                "user",
                1i64,
                1_700_000_001_000i64,
                1_700_000_001_000i64,
                r#"{"text":"hi","time":{"created":1700000001000}}"#
            ],
        )
        .expect("insert user");
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                "msg-assistant",
                "session-v2",
                "assistant",
                2i64,
                1_700_000_002_000i64,
                1_700_000_003_000i64,
                r#"{"model":"gpt-4.1","content":[{"type":"text","id":"txt","text":"done"}],"finish":"stop","tokens":{"input":1,"output":2,"reasoning":1,"cache":{"read":1}},"time":{"created":1700000002000,"completed":1700000003000}}"#
            ],
        )
        .expect("insert assistant");

        let snapshot = probe_opencode_v2_schema(
            &conn,
            std::path::Path::new("/tmp/opencode.db"),
            "/tmp/project",
            "session-v2",
            &request_for_opencode_session("session-v2"),
        )
        .expect("snapshot");

        assert_eq!(snapshot.response_state.as_deref(), Some("idle"));
        assert!(snapshot.has_completed_turn);
        assert_eq!(snapshot.completed_at, Some(1_700_000_003.0));
        assert_eq!(snapshot.assistant_preview.as_deref(), Some("done"));
        assert_eq!(snapshot.model.as_deref(), Some("gpt-4.1"));
        assert_eq!(snapshot.input_tokens, 10);
        assert_eq!(snapshot.output_tokens, 20);
        assert_eq!(snapshot.cached_input_tokens, 4);
        assert_eq!(snapshot.total_tokens, 37);
    }

    #[test]
    fn v2_schema_open_tool_stays_responding() {
        let conn = v2_schema();
        conn.execute(
            "INSERT INTO session (id, directory, path, time_updated, time_created, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, model)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                "session-tool",
                "/tmp/project",
                "",
                1_700_000_002_000i64,
                1_700_000_000_000i64,
                0i64,
                0i64,
                0i64,
                0i64,
                Option::<String>::None
            ],
        )
        .expect("insert session");
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                "msg-user",
                "session-tool",
                "user",
                1i64,
                1_700_000_001_000i64,
                1_700_000_001_000i64,
                r#"{"text":"list files"}"#
            ],
        )
        .expect("insert user");
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                "msg-assistant",
                "session-tool",
                "assistant",
                2i64,
                1_700_000_002_000i64,
                1_700_000_002_000i64,
                r#"{"content":[{"type":"tool","id":"call","name":"list","state":{"status":"running","input":"{}"}}],"finish":"tool-calls","time":{"created":1700000002000}}"#
            ],
        )
        .expect("insert assistant");

        let snapshot = probe_opencode_v2_schema(
            &conn,
            std::path::Path::new("/tmp/opencode.db"),
            "/tmp/project",
            "session-tool",
            &request_for_opencode_session("session-tool"),
        )
        .expect("snapshot");

        assert_eq!(snapshot.response_state.as_deref(), Some("responding"));
        assert!(!snapshot.has_completed_turn);
        assert_eq!(snapshot.completed_at, None);
    }

    #[test]
    fn v2_schema_discovers_fresh_session_without_external_id() {
        let conn = v2_schema();
        conn.execute(
            "INSERT INTO session (id, directory, path, time_updated, time_created, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, model)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                "old-session",
                "/tmp/project",
                "",
                1_700_000_000_000i64,
                1_700_000_000_000i64,
                999i64,
                999i64,
                0i64,
                0i64,
                Option::<String>::None
            ],
        )
        .expect("insert old session");
        conn.execute(
            "INSERT INTO session (id, directory, path, time_updated, time_created, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, model)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                "fresh-session",
                "/tmp/project",
                "",
                1_700_000_011_000i64,
                1_700_000_010_000i64,
                11i64,
                22i64,
                3i64,
                4i64,
                r#"{"id":"gpt-5.4"}"#
            ],
        )
        .expect("insert fresh session");
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                "msg-user",
                "fresh-session",
                "user",
                1i64,
                1_700_000_010_500i64,
                1_700_000_010_500i64,
                r#"{"text":"hi"}"#
            ],
        )
        .expect("insert user");
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                "msg-assistant",
                "fresh-session",
                "assistant",
                2i64,
                1_700_000_011_000i64,
                1_700_000_011_000i64,
                r#"{"content":[{"type":"text","text":"done"}],"finish":"stop","time":{"completed":1700000011000}}"#
            ],
        )
        .expect("insert assistant");
        let mut request = request_for_opencode_session("");
        request.external_session_id = None;
        request.started_at = Some(1_700_000_009.0);

        let session_id =
            newest_opencode_session_for_project(&conn, "/tmp/project", request.started_at)
                .expect("session id");
        let snapshot = probe_opencode_v2_schema(
            &conn,
            std::path::Path::new("/tmp/opencode.db"),
            "/tmp/project",
            &session_id,
            &request,
        )
        .expect("snapshot");

        assert_eq!(session_id, "fresh-session");
        assert_eq!(
            snapshot.external_session_id.as_deref(),
            Some("fresh-session")
        );
        assert_eq!(snapshot.session_origin, "fresh");
        assert_eq!(snapshot.total_tokens, 40);
    }

    #[test]
    fn current_schema_reads_message_and_part_tables() {
        let conn = current_opencode_schema();
        conn.execute(
            "INSERT INTO session (id, directory, path, time_updated, time_created, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, model)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                "current-session",
                "/tmp/project",
                "",
                1_700_000_013_000i64,
                1_700_000_010_000i64,
                120i64,
                12i64,
                0i64,
                8704i64,
                r#"{"id":"gpt-5.4","providerID":"rightcode","variant":"high"}"#
            ],
        )
        .expect("insert session");
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "msg-user",
                "current-session",
                1_700_000_010_500i64,
                1_700_000_010_500i64,
                r#"{"role":"user","time":{"created":1700000010500},"model":{"providerID":"rightcode","modelID":"gpt-5.4","variant":"high"}}"#
            ],
        )
        .expect("insert user");
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "msg-assistant",
                "current-session",
                1_700_000_011_000i64,
                1_700_000_013_000i64,
                r#"{"role":"assistant","modelID":"gpt-5.4","providerID":"rightcode","time":{"created":1700000011000,"completed":1700000013000},"finish":"stop"}"#
            ],
        )
        .expect("insert assistant");
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                "part-text",
                "msg-assistant",
                "current-session",
                1_700_000_012_000i64,
                1_700_000_012_500i64,
                r#"{"type":"text","text":"你好，有什么需要我处理的？"}"#
            ],
        )
        .expect("insert text part");

        let snapshot = probe_opencode_v2_schema(
            &conn,
            std::path::Path::new("/tmp/opencode.db"),
            "/tmp/project",
            "current-session",
            &request_for_opencode_session("current-session"),
        )
        .expect("snapshot");

        assert_eq!(snapshot.response_state.as_deref(), Some("idle"));
        assert!(snapshot.has_completed_turn);
        assert_eq!(snapshot.completed_at, Some(1_700_000_013.0));
        assert_eq!(
            snapshot.assistant_preview.as_deref(),
            Some("你好，有什么需要我处理的？")
        );
        assert_eq!(snapshot.model.as_deref(), Some("gpt-5.4"));
        assert_eq!(snapshot.total_tokens, 8_836);
    }

    #[test]
    fn current_schema_open_assistant_stays_responding() {
        let conn = current_opencode_schema();
        conn.execute(
            "INSERT INTO session (id, directory, path, time_updated, time_created, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, model)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                "current-running",
                "/tmp/project",
                "",
                1_700_000_013_000i64,
                1_700_000_010_000i64,
                120i64,
                0i64,
                0i64,
                0i64,
                r#"{"id":"gpt-5.4"}"#
            ],
        )
        .expect("insert session");
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "msg-user",
                "current-running",
                1_700_000_010_500i64,
                1_700_000_010_500i64,
                r#"{"role":"user","time":{"created":1700000010500}}"#
            ],
        )
        .expect("insert user");
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "msg-assistant",
                "current-running",
                1_700_000_011_000i64,
                1_700_000_013_000i64,
                r#"{"role":"assistant","modelID":"gpt-5.4","time":{"created":1700000011000}}"#
            ],
        )
        .expect("insert assistant");
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                "part-start",
                "msg-assistant",
                "current-running",
                1_700_000_012_000i64,
                1_700_000_012_000i64,
                r#"{"type":"step-start"}"#
            ],
        )
        .expect("insert step start");

        let snapshot = probe_opencode_v2_schema(
            &conn,
            std::path::Path::new("/tmp/opencode.db"),
            "/tmp/project",
            "current-running",
            &request_for_opencode_session("current-running"),
        )
        .expect("snapshot");

        assert_eq!(snapshot.response_state.as_deref(), Some("responding"));
        assert!(!snapshot.has_completed_turn);
        assert_eq!(snapshot.completed_at, None);
    }

    #[test]
    fn mimo_schema_final_assistant_message_is_idle() {
        let conn = mimo_schema();
        conn.execute(
            "INSERT INTO session (id, directory, time_updated, time_created)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                "session-mimo",
                "/tmp/project",
                1_700_000_003_000i64,
                1_700_000_000_000i64,
            ],
        )
        .expect("insert session");
        conn.execute(
            "INSERT INTO message (id, session_id, data, time_created, time_updated)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "msg-user",
                "session-mimo",
                r#"{"role":"user","time":{"created":1700000001000},"model":{"providerID":"mimo","modelID":"mimo-auto"}}"#,
                1_700_000_001_000i64,
                1_700_000_001_000i64
            ],
        )
        .expect("insert user");
        conn.execute(
            "INSERT INTO message (id, session_id, data, time_created, time_updated)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "msg-assistant",
                "session-mimo",
                r#"{"role":"assistant","modelID":"mimo-auto","providerID":"mimo","finish":"stop","tokens":{"input":11,"output":22,"reasoning":3,"cache":{"read":5,"write":7}},"time":{"created":1700000002000,"completed":1700000003000}}"#,
                1_700_000_002_000i64,
                1_700_000_003_000i64
            ],
        )
        .expect("insert assistant");
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, data, time_created, time_updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                "part-text",
                "msg-assistant",
                "session-mimo",
                r#"{"type":"text","text":"done"}"#,
                1_700_000_002_100i64,
                1_700_000_002_100i64
            ],
        )
        .expect("insert text part");
        conn.execute(
            "INSERT INTO todo (session_id, content, status, position, time_updated)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "session-mimo",
                "Ship MiMo parser",
                "completed",
                0i64,
                1_700_000_003_000i64
            ],
        )
        .expect("insert todo");
        let mut request = request_for_opencode_session("session-mimo");
        request.tool = "mimo".to_string();

        let snapshot = probe_mimo_schema(
            &conn,
            std::path::Path::new("/tmp/mimocode.db"),
            "/tmp/project",
            Some("session-mimo"),
            &request,
        )
        .expect("snapshot");

        assert_eq!(snapshot.tool, "mimo");
        assert_eq!(snapshot.response_state.as_deref(), Some("idle"));
        assert!(snapshot.has_completed_turn);
        assert_eq!(snapshot.completed_at, Some(1_700_000_003.0));
        assert_eq!(snapshot.assistant_preview.as_deref(), Some("done"));
        assert_eq!(snapshot.model.as_deref(), Some("mimo-auto"));
        assert_eq!(snapshot.input_tokens, 11);
        assert_eq!(snapshot.output_tokens, 22);
        assert_eq!(snapshot.cached_input_tokens, 5);
        assert_eq!(snapshot.total_tokens, 41);
        assert_eq!(
            snapshot.plan.as_ref().map(|plan| plan.source.as_str()),
            Some("mimo")
        );

        let discovered = probe_mimo_schema(
            &conn,
            std::path::Path::new("/tmp/mimocode.db"),
            "/tmp/project",
            None,
            &request,
        )
        .expect("snapshot by project");
        assert_eq!(
            discovered.external_session_id.as_deref(),
            Some("session-mimo")
        );
        assert_eq!(discovered.response_state.as_deref(), Some("idle"));
    }

    #[test]
    fn opencode_epoch_accepts_seconds_and_milliseconds() {
        assert_eq!(opencode_epoch_value_seconds(1_000), 1_000.0);
        assert_eq!(
            opencode_epoch_value_seconds(1_000_000_000_000),
            1_000_000_000.0
        );
    }

    #[test]
    fn reads_todo_table_into_plan() {
        let conn = rusqlite::Connection::open_in_memory().expect("db");
        conn.execute_batch(
            r#"
            CREATE TABLE todo (
                session_id TEXT,
                content TEXT,
                status TEXT,
                priority TEXT,
                position INTEGER,
                time_updated INTEGER
            );
            "#,
        )
        .expect("schema");
        conn.execute(
            "INSERT INTO todo (session_id, content, status, priority, position, time_updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                "session-a",
                "Check OpenCode DB",
                "completed",
                "high",
                0,
                1000i64
            ],
        )
        .expect("insert first");
        conn.execute(
            "INSERT INTO todo (session_id, content, status, priority, position, time_updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                "session-a",
                "Render pet plan",
                "in_progress",
                Option::<String>::None,
                1,
                2000i64
            ],
        )
        .expect("insert second");

        let plan = opencode_plan(&conn, "session-a").expect("plan");

        assert_eq!(plan.source, "opencode");
        assert_eq!(plan.session_id, "session-a");
        assert_eq!(plan.updated_at, 2.0);
        assert_eq!(plan.items.len(), 2);
        assert_eq!(plan.items[0].text, "Check OpenCode DB");
        assert_eq!(plan.items[0].status, "completed");
        assert_eq!(plan.items[0].priority.as_deref(), Some("high"));
        assert_eq!(plan.items[1].status, "in_progress");
        assert_eq!(plan.items[1].priority, None);
    }
}
