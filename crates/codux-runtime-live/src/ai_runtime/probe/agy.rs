use crate::ai_runtime::{
    probe::paths::agy_conversation_db_for_runtime,
    snapshot::{AIRuntimeContextSnapshot, AIRuntimeProbeRequest},
    state::normalized_string,
};
use codux_ai_history::agy_db::parse_agy_conversation_db;

/// Antigravity ("agy") stores each conversation in its own SQLite DB under
/// `~/.gemini/antigravity-cli/conversations/<uuid>.db`. This probe deliberately
/// uses that single current-format source; no old JSON or transcript
/// fallback is allowed, so live status/history/tokens all resolve from the same
/// parsed DB record.
pub(crate) fn probe_agy_runtime(
    request: &AIRuntimeProbeRequest,
) -> Option<AIRuntimeContextSnapshot> {
    let project_path = normalized_string(request.project_path.as_deref())?;
    let database_path = agy_conversation_db_for_runtime(
        &project_path,
        request.external_session_id.as_deref(),
        request.started_at,
    )?;
    let conversation = parse_agy_conversation_db(&database_path)?;
    let response_state = agy_response_state(&conversation);
    let has_completed_turn = response_state.as_deref() == Some("idle");
    Some(AIRuntimeContextSnapshot {
        tool: "agy".to_string(),
        external_session_id: conversation
            .conversation_id
            .clone()
            .or_else(|| normalized_string(request.external_session_id.as_deref())),
        transcript_path: Some(database_path.display().to_string()),
        model: conversation.model.clone(),
        assistant_preview: conversation.assistant_preview.clone(),
        input_tokens: conversation.input_tokens,
        output_tokens: conversation.output_tokens,
        cached_input_tokens: conversation.cached_input_tokens,
        total_tokens: conversation.total_tokens(),
        usage_amounts: Vec::new(),
        baseline_usage_amounts: Vec::new(),
        updated_at: conversation.last_seen_at.unwrap_or(request.updated_at),
        runtime_activity_at: conversation.last_seen_at,
        last_user_input_at: conversation.last_user_at,
        started_at: conversation.last_user_at,
        completed_at: agy_completed_at(&conversation),
        response_state,
        was_interrupted: false,
        has_completed_turn,
        session_origin: "unknown".to_string(),
        source: "probe".to_string(),
        plan: None,
    })
}

fn agy_response_state(conversation: &codux_ai_history::agy_db::AgyConversation) -> Option<String> {
    let last_user_at = conversation.last_user_at.unwrap_or(0.0);
    let last_model_at = conversation.last_model_at.unwrap_or(0.0);
    if last_user_at <= 0.0 && last_model_at <= 0.0 {
        return None;
    }
    if last_user_at > last_model_at {
        Some("responding".to_string())
    } else {
        Some("idle".to_string())
    }
}

fn agy_completed_at(conversation: &codux_ai_history::agy_db::AgyConversation) -> Option<f64> {
    let last_user_at = conversation.last_user_at.unwrap_or(0.0);
    let last_model_at = conversation.last_model_at.unwrap_or(0.0);
    (last_model_at >= last_user_at && last_model_at > 0.0).then_some(last_model_at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_after_model_is_responding() {
        let conversation = codux_ai_history::agy_db::AgyConversation {
            last_user_at: Some(20.0),
            last_model_at: Some(10.0),
            ..Default::default()
        };
        assert_eq!(
            agy_response_state(&conversation).as_deref(),
            Some("responding")
        );
        assert_eq!(agy_completed_at(&conversation), None);
    }

    #[test]
    fn model_after_user_is_idle() {
        let conversation = codux_ai_history::agy_db::AgyConversation {
            last_user_at: Some(10.0),
            last_model_at: Some(20.0),
            ..Default::default()
        };
        assert_eq!(agy_response_state(&conversation).as_deref(), Some("idle"));
        assert_eq!(agy_completed_at(&conversation), Some(20.0));
    }
}
