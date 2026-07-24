use super::super::*;
use super::fixtures::*;

#[test]
fn first_prompt_notifies_when_terminal_already_has_detected_session() {
    // Process detection creates an idle session first; the first prompt event
    // then flips it to responding and notifies.
    let store = AIRuntimeStateStore::default();
    let terminal = codex_bridge_terminal();
    let detected =
        std::collections::HashMap::from([(terminal.terminal_id.clone(), "codex".to_string())]);
    assert!(
        store
            .ensure_detected_sessions(&[terminal], &detected, 1000.0)
            .did_change
    );
    assert_eq!(store.snapshot().sessions[0].state, "idle");

    let prompt = test_hook("promptSubmitted", 1001.0);
    let mutation = store.apply_hook(prompt);

    assert!(mutation.did_change);
    assert!(mutation.completion.is_none());
    let snapshot = store.snapshot();
    assert_eq!(snapshot.running_count, 1);
    assert_eq!(snapshot.sessions[0].state, "responding");
}
#[test]
fn prompt_submitted_uses_wrapper_project_even_when_hook_cwd_differs() {
    let store = AIRuntimeStateStore::default();
    let mut prompt = test_hook("promptSubmitted", 1000.0);
    prompt.project_path = Some("F:\\codux-gpui".to_string());
    prompt.metadata = Some(AIHookEventMetadata {
        cwd: Some("C:\\Users\\dux".to_string()),
        ..empty_metadata()
    });

    let mutation = store.apply_hook(prompt);

    assert!(mutation.did_change);
    let snapshot = store.snapshot();
    assert_eq!(snapshot.running_count, 1);
    assert_eq!(snapshot.sessions[0].state, "responding");
}
#[test]
fn multiple_same_tool_sessions_are_isolated_by_terminal_id() {
    let store = AIRuntimeStateStore::default();
    assert!(
        store
            .apply_hook(test_hook_for(
                "codex",
                "codex-term-1",
                "codex-session-1",
                1000.0
            ))
            .did_change
    );
    assert!(
        store
            .apply_hook(test_hook_for(
                "codex",
                "codex-term-2",
                "codex-session-2",
                1001.0
            ))
            .did_change
    );

    let snapshot = store.snapshot();
    assert_eq!(snapshot.running_count, 1);
    assert_eq!(snapshot.global_totals.running, 2);
    assert!(
        snapshot
            .sessions
            .iter()
            .any(|session| session.terminal_id == "codex-term-1"
                && session.ai_session_id.as_deref() == Some("codex-session-1")
                && session.state == "responding")
    );
    assert!(
        snapshot
            .sessions
            .iter()
            .any(|session| session.terminal_id == "codex-term-2"
                && session.ai_session_id.as_deref() == Some("codex-session-2")
                && session.state == "responding")
    );

    assert!(
        store
            .apply_hook(AIHookEventPayload {
                kind: "turnCompleted".to_string(),
                updated_at: 1010.0,
                metadata: Some(AIHookEventMetadata {
                    has_completed_turn: Some(true),
                    ..empty_metadata()
                }),
                ..test_hook_for("codex", "codex-term-1", "codex-session-1", 1010.0)
            })
            .did_change
    );

    let snapshot = store.snapshot();
    assert_eq!(snapshot.running_count, 1);
    assert_eq!(snapshot.global_totals.running, 1);
    assert!(
        snapshot
            .sessions
            .iter()
            .any(|session| session.terminal_id == "codex-term-1" && session.state == "idle")
    );
    assert!(
        snapshot
            .sessions
            .iter()
            .any(|session| session.terminal_id == "codex-term-2"
                && session.ai_session_id.as_deref() == Some("codex-session-2")
                && session.state == "responding")
    );
}
#[test]
fn multiple_claude_sessions_are_isolated_by_terminal_id_and_external_session_id() {
    let store = AIRuntimeStateStore::default();
    assert!(
        store
            .apply_hook(test_hook_for(
                "claude",
                "claude-term-1",
                "claude-external-1",
                1000.0
            ))
            .did_change
    );
    assert!(
        store
            .apply_hook(test_hook_for(
                "claude",
                "claude-term-2",
                "claude-external-2",
                1001.0
            ))
            .did_change
    );

    let snapshot = store.snapshot();
    assert_eq!(snapshot.running_count, 1);
    assert_eq!(snapshot.global_totals.running, 2);
    assert!(
        snapshot
            .sessions
            .iter()
            .any(|session| session.terminal_id == "claude-term-1"
                && session.tool == "claude"
                && session.ai_session_id.as_deref() == Some("claude-external-1"))
    );
    assert!(
        snapshot
            .sessions
            .iter()
            .any(|session| session.terminal_id == "claude-term-2"
                && session.tool == "claude"
                && session.ai_session_id.as_deref() == Some("claude-external-2"))
    );
}
#[test]
fn stale_runtime_completion_snapshot_after_prompt_stays_running() {
    let store = AIRuntimeStateStore::default();
    assert!(
        store
            .apply_hook(test_hook("sessionStarted", 1000.0))
            .did_change
    );
    assert!(
        store
            .apply_hook(test_hook("promptSubmitted", 1020.0))
            .did_change
    );

    let mutation = store.apply_runtime_snapshot(
        "terminal-1",
        AIRuntimeContextSnapshot {
            tool: "codex".to_string(),
            external_session_id: Some("session-1".to_string()),
            transcript_path: Some("/tmp/codex.jsonl".to_string()),
            model: Some("gpt-5.4".to_string()),
            assistant_preview: None,
            input_tokens: 0,
            output_tokens: 0,
            cached_input_tokens: 0,
            total_tokens: 150,
            usage_amounts: Vec::new(),
            baseline_usage_amounts: Vec::new(),
            updated_at: 1010.0,
            runtime_activity_at: None,
            last_user_input_at: None,
            started_at: Some(1000.0),
            completed_at: Some(1010.0),
            response_state: Some("idle".to_string()),
            was_interrupted: false,
            has_completed_turn: true,
            session_origin: "live".to_string(),
            source: "probe".to_string(),
            plan: None,
        },
    );

    assert!(mutation.did_change);
    assert!(mutation.completion.is_none());
    let snapshot = store.snapshot();
    assert_eq!(snapshot.running_count, 1);
    assert_eq!(snapshot.completion_count, 0);
    assert_eq!(snapshot.sessions[0].state, "responding");
    assert!(!snapshot.sessions[0].has_completed_turn);
    assert!(matches!(
        snapshot.projects[0].completed_phase,
        AIProjectPhase::Idle
    ));
}
