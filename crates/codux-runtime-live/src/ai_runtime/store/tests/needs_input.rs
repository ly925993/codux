use super::super::*;
use super::fixtures::*;

#[test]
fn stale_needs_input_is_not_visible_in_snapshot() {
    let mut core = AIRuntimeStateCore::default();
    assert!(apply_hook_unlocked(
        &mut core,
        AIHookEventPayload {
            kind: "needsInput".to_string(),
            updated_at: 1.0,
            metadata: Some(AIHookEventMetadata {
                notification_type: Some("permission-request".to_string()),
                target_tool_name: Some("AskUserQuestion".to_string()),
                ..empty_metadata()
            }),
            ..test_hook_for("claude", "claude-term-1", "claude-external-1", 1.0)
        }
    ));

    let snapshot = state_snapshot_unlocked(&core);

    assert_eq!(snapshot.needs_input_count, 0);
    assert_eq!(snapshot.global_totals.needs_input, 0);
    assert_eq!(snapshot.sessions[0].state, "idle");
    assert!(snapshot.sessions[0].notification_type.is_none());
    assert!(matches!(
        snapshot.projects[0].project_phase,
        AIProjectPhase::Idle
    ));
}
#[test]
fn fresh_needs_input_remains_visible_in_snapshot() {
    let now = now_seconds();
    let mut core = AIRuntimeStateCore::default();
    assert!(apply_hook_unlocked(
        &mut core,
        AIHookEventPayload {
            kind: "needsInput".to_string(),
            updated_at: now,
            metadata: Some(AIHookEventMetadata {
                notification_type: Some("permission-request".to_string()),
                target_tool_name: Some("AskUserQuestion".to_string()),
                ..empty_metadata()
            }),
            ..test_hook_for("claude", "claude-term-1", "claude-external-1", now)
        }
    ));

    let snapshot = state_snapshot_unlocked(&core);

    assert_eq!(snapshot.needs_input_count, 1);
    assert_eq!(snapshot.global_totals.needs_input, 1);
    assert_eq!(snapshot.sessions[0].state, "needsInput");
    assert!(matches!(
        snapshot.projects[0].project_phase,
        AIProjectPhase::NeedsInput { .. }
    ));
}
#[test]
fn prompt_submitted_after_needs_input_restores_running_state() {
    let mut core = AIRuntimeStateCore::default();
    assert!(apply_hook_unlocked(
        &mut core,
        AIHookEventPayload {
            kind: "needsInput".to_string(),
            updated_at: 1000.0,
            metadata: Some(AIHookEventMetadata {
                notification_type: Some("permission-request".to_string()),
                target_tool_name: Some("AskUserQuestion".to_string()),
                ..empty_metadata()
            }),
            ..test_hook_for("claude", "claude-term-1", "claude-external-1", 1000.0)
        }
    ));
    assert!(apply_hook_unlocked(
        &mut core,
        AIHookEventPayload {
            kind: "promptSubmitted".to_string(),
            updated_at: 1001.0,
            metadata: Some(AIHookEventMetadata {
                source: Some("permission-auto-allowed".to_string()),
                ..empty_metadata()
            }),
            ..test_hook_for("claude", "claude-term-1", "claude-external-1", 1001.0)
        }
    ));

    let snapshot = state_snapshot_unlocked(&core);

    assert_eq!(snapshot.running_count, 1);
    assert_eq!(snapshot.needs_input_count, 0);
    assert_eq!(snapshot.sessions[0].state, "responding");
    assert!(snapshot.sessions[0].notification_type.is_none());
}
#[test]
fn claude_needs_input_clears_when_probe_sees_resume_after_completed_turn() {
    // Repro of the desktop pet sticking on "等待允许" after a manual permission
    // approval: a prior turn completed (has_completed_turn=true), the user sends
    // a new prompt, Claude asks for permission (needsInput). On approval no hook
    // fires (Claude has no "granted" hook), so the 5s probe is what must clear
    // it once Claude resumes responding.
    let mut core = AIRuntimeStateCore::default();
    assert!(apply_hook_unlocked(
        &mut core,
        test_hook_for("claude", "claude-term-1", "claude-external-1", 1000.0)
    ));
    assert!(apply_hook_unlocked(
        &mut core,
        AIHookEventPayload {
            kind: "turnCompleted".to_string(),
            updated_at: 1010.0,
            metadata: Some(AIHookEventMetadata {
                has_completed_turn: Some(true),
                ..empty_metadata()
            }),
            ..test_hook_for("claude", "claude-term-1", "claude-external-1", 1010.0)
        }
    ));
    assert!(apply_hook_unlocked(
        &mut core,
        test_hook_for("claude", "claude-term-1", "claude-external-1", 1020.0)
    ));
    assert!(apply_hook_unlocked(
        &mut core,
        AIHookEventPayload {
            kind: "needsInput".to_string(),
            updated_at: 1025.0,
            metadata: Some(AIHookEventMetadata {
                notification_type: Some("permission-request".to_string()),
                target_tool_name: Some("Skill".to_string()),
                ..empty_metadata()
            }),
            ..test_hook_for("claude", "claude-term-1", "claude-external-1", 1025.0)
        }
    ));
    assert_eq!(
        core.sessions.get("claude-term-1").unwrap().state,
        "needsInput"
    );

    // User approves; Claude resumes. The probe reads the log: the turn is still
    // responding (last user prompt newer than last completion) and new output
    // advanced `updated_at`.
    apply_runtime_snapshot_unlocked(
        &mut core,
        "claude-term-1",
        AIRuntimeContextSnapshot {
            tool: "claude".to_string(),
            external_session_id: Some("claude-external-1".to_string()),
            transcript_path: None,
            model: Some("sonnet".to_string()),
            assistant_preview: None,
            input_tokens: 0,
            output_tokens: 0,
            cached_input_tokens: 0,
            total_tokens: 200,
            usage_amounts: Vec::new(),
            baseline_usage_amounts: Vec::new(),
            updated_at: 1030.0,
            // Log's user-message time sits slightly before the hook's prompt
            // wall-clock time (real-world skew between the two clocks).
            runtime_activity_at: None,
            last_user_input_at: None,
            started_at: Some(1018.0),
            completed_at: Some(1010.0),
            response_state: Some("responding".to_string()),
            was_interrupted: false,
            has_completed_turn: false,
            session_origin: "live".to_string(),
            source: "probe".to_string(),
            plan: None,
        },
    );

    assert_eq!(
        core.sessions.get("claude-term-1").unwrap().state,
        "responding",
        "needsInput must clear once the probe sees the turn resume after approval"
    );
}
#[test]
fn probe_derived_needs_input_marks_and_clears_permission_wait() {
    // The pure-file path with NO needsInput hook: the probe alone detects a tool
    // call blocked on the user (response_state="needsInput"), the store surfaces
    // it, then recovers to responding on approval and resolves to idle on
    // completion -- the full lifecycle without a single hook event.
    let mut core = AIRuntimeStateCore::default();
    assert!(apply_hook_unlocked(
        &mut core,
        test_hook_for("claude", "claude-term-1", "claude-external-1", 1000.0)
    ));
    assert_eq!(
        core.sessions.get("claude-term-1").unwrap().state,
        "responding"
    );

    // Probe sees a pending tool call past the idle gap.
    apply_runtime_snapshot_unlocked(
        &mut core,
        "claude-term-1",
        needs_input_probe_snapshot(1005.0),
    );
    assert_eq!(
        core.sessions.get("claude-term-1").unwrap().state,
        "needsInput"
    );

    // Approval -> the probe sees the turn resume (no "granted" hook exists).
    apply_runtime_snapshot_unlocked(
        &mut core,
        "claude-term-1",
        AIRuntimeContextSnapshot {
            usage_amounts: Vec::new(),
            baseline_usage_amounts: Vec::new(),
            updated_at: 1010.0,
            response_state: Some("responding".to_string()),
            ..needs_input_probe_snapshot(1010.0)
        },
    );
    assert_eq!(
        core.sessions.get("claude-term-1").unwrap().state,
        "responding"
    );

    // Turn completes -> idle.
    apply_runtime_snapshot_unlocked(
        &mut core,
        "claude-term-1",
        AIRuntimeContextSnapshot {
            usage_amounts: Vec::new(),
            baseline_usage_amounts: Vec::new(),
            updated_at: 1020.0,
            completed_at: Some(1019.0),
            response_state: Some("idle".to_string()),
            has_completed_turn: true,
            ..needs_input_probe_snapshot(1020.0)
        },
    );
    assert_eq!(core.sessions.get("claude-term-1").unwrap().state, "idle");
}
