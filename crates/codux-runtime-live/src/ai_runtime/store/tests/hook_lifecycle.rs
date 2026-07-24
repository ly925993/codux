use super::super::*;
use super::fixtures::*;

#[test]
fn hook_lifecycle_tracks_running_and_completion() {
    let store = AIRuntimeStateStore::default();
    let start = store.apply_hook(test_hook("promptSubmitted", 1000.0));
    assert!(start.did_change);
    assert!(start.completion.is_none());

    let snapshot = store.snapshot();
    assert_eq!(snapshot.running_count, 1);
    assert_eq!(snapshot.sessions[0].state, "responding");

    let complete = store.apply_hook(AIHookEventPayload {
        kind: "turnCompleted".to_string(),
        total_tokens: Some(150),
        updated_at: 1010.0,
        metadata: Some(AIHookEventMetadata {
            has_completed_turn: Some(true),
            ..empty_metadata()
        }),
        ..test_hook("turnCompleted", 1010.0)
    });

    assert!(complete.did_change);
    assert!(complete.completion.is_some());
    let snapshot = store.snapshot();
    assert_eq!(snapshot.running_count, 0);
    assert_eq!(snapshot.completion_count, 1);
    assert!(matches!(
        snapshot.projects[0].completed_phase,
        AIProjectPhase::Completed { .. }
    ));
}
#[test]
fn workspace_placeholder_project_name_falls_back_to_project_path_for_completion() {
    let store = AIRuntimeStateStore::default();
    let mut prompt = test_hook("promptSubmitted", 1000.0);
    prompt.project_name = "Workspace".to_string();
    prompt.project_path = Some("/tmp/codux-gpui".to_string());
    assert!(store.apply_hook(prompt).did_change);

    let complete = store.apply_hook(AIHookEventPayload {
        kind: "turnCompleted".to_string(),
        project_name: "Workspace".to_string(),
        project_path: Some("/tmp/codux-gpui".to_string()),
        total_tokens: Some(150),
        updated_at: 1010.0,
        metadata: Some(AIHookEventMetadata {
            has_completed_turn: Some(true),
            ..empty_metadata()
        }),
        ..test_hook("turnCompleted", 1010.0)
    });

    assert_eq!(
        complete.completion.expect("completion").project_name,
        "codux-gpui"
    );
}
#[test]
fn merged_mutation_preserves_multiple_completion_events() {
    let store = AIRuntimeStateStore::default();
    assert!(
        store
            .apply_hook(AIHookEventPayload {
                project_id: "project-a".to_string(),
                ..test_hook_for("codex", "terminal-a", "session-a", 1000.0)
            })
            .did_change
    );
    assert!(
        store
            .apply_hook(AIHookEventPayload {
                project_id: "project-b".to_string(),
                ..test_hook_for("codex", "terminal-b", "session-b", 1001.0)
            })
            .did_change
    );

    let first = store.apply_hook(AIHookEventPayload {
        kind: "turnCompleted".to_string(),
        project_id: "project-a".to_string(),
        metadata: Some(AIHookEventMetadata {
            has_completed_turn: Some(true),
            ..empty_metadata()
        }),
        ..test_hook_for("codex", "terminal-a", "session-a", 1010.0)
    });
    let second = store.apply_hook(AIHookEventPayload {
        kind: "turnCompleted".to_string(),
        project_id: "project-b".to_string(),
        metadata: Some(AIHookEventMetadata {
            has_completed_turn: Some(true),
            ..empty_metadata()
        }),
        ..test_hook_for("codex", "terminal-b", "session-b", 1011.0)
    });

    let mut merged = AIRuntimeStateMutation::default();
    merged.merge(first);
    merged.merge(second);

    assert_eq!(merged.completions.len(), 2);
    let projects = merged
        .completions
        .iter()
        .filter_map(|event| {
            event
                .session
                .as_ref()
                .map(|session| session.project_id.as_str())
        })
        .collect::<std::collections::HashSet<_>>();
    assert!(projects.contains("project-a"));
    assert!(projects.contains("project-b"));
}
#[test]
fn runtime_snapshot_sets_restored_session_baseline() {
    let mut core = AIRuntimeStateCore::default();
    assert!(apply_hook_unlocked(
        &mut core,
        test_hook("promptSubmitted", 1000.0)
    ));

    assert!(apply_runtime_snapshot_unlocked(
        &mut core,
        "terminal-1",
        AIRuntimeContextSnapshot {
            tool: "codex".to_string(),
            external_session_id: Some("session-1".to_string()),
            transcript_path: None,
            model: Some("gpt-5.5".to_string()),
            assistant_preview: None,
            input_tokens: 1_000,
            output_tokens: 200,
            cached_input_tokens: 3_000,
            total_tokens: 1_200,
            usage_amounts: Vec::new(),
            baseline_usage_amounts: Vec::new(),
            updated_at: 1005.0,
            runtime_activity_at: None,
            last_user_input_at: None,
            started_at: Some(900.0),
            completed_at: None,
            response_state: Some("responding".to_string()),
            was_interrupted: false,
            has_completed_turn: false,
            session_origin: "restored".to_string(),
            source: "probe".to_string(),
            plan: None,
        }
    ));

    let session = core.sessions.get("terminal-1").unwrap();
    assert_eq!(session.baseline_total_tokens, 1_200);
    assert_eq!(session.baseline_cached_input_tokens, 3_000);
    assert!(session.baseline_resolved);
    assert_eq!(
        summary::project_totals_unlocked(&core, Some("project-1"), now_seconds()).total_tokens,
        0
    );
    assert_eq!(
        summary::project_totals_unlocked(&core, Some("project-1"), now_seconds())
            .cached_input_tokens,
        0
    );
}
#[test]
fn tool_activity_without_loading_is_ignored() {
    let store = AIRuntimeStateStore::default();
    let mut event = test_hook("promptSubmitted", 1000.0);
    event.metadata = Some(AIHookEventMetadata {
        source: Some("tool-use".to_string()),
        ..empty_metadata()
    });

    let mutation = store.apply_hook(event);

    assert!(!mutation.did_change);
    assert!(store.snapshot().sessions.is_empty());
}
