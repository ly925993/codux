use super::super::*;
use super::fixtures::*;

#[test]
fn screen_signal_refines_active_turn_only() {
    // The universal screen-scrape path: it only refines an already-active turn
    // between responding<->needsInput, and never touches an idle session.
    let mut core = AIRuntimeStateCore::default();
    assert!(apply_hook_unlocked(
        &mut core,
        test_hook_for("codex", "term-s", "ext-s", 1000.0)
    ));
    assert_eq!(core.sessions.get("term-s").unwrap().state, "responding");

    // Screen shows an approval prompt -> needsInput.
    assert!(apply_screen_signal_unlocked(
        &mut core,
        "term-s",
        ScreenSignal::Waiting,
        false
    ));
    assert_eq!(core.sessions.get("term-s").unwrap().state, "needsInput");

    // Prompt cleared / work resumed -> back to responding.
    assert!(apply_screen_signal_unlocked(
        &mut core,
        "term-s",
        ScreenSignal::Running,
        false
    ));
    assert_eq!(core.sessions.get("term-s").unwrap().state, "responding");

    // Unknown is always a no-op.
    assert!(!apply_screen_signal_unlocked(
        &mut core,
        "term-s",
        ScreenSignal::Unknown,
        false
    ));

    // A completed/idle session is never started by a screen prompt.
    apply_hook_unlocked(
        &mut core,
        AIHookEventPayload {
            kind: "turnCompleted".to_string(),
            updated_at: 1010.0,
            metadata: Some(AIHookEventMetadata {
                has_completed_turn: Some(true),
                ..empty_metadata()
            }),
            ..test_hook_for("codex", "term-s", "ext-s", 1010.0)
        },
    );
    assert_eq!(core.sessions.get("term-s").unwrap().state, "idle");
    assert!(!apply_screen_signal_unlocked(
        &mut core,
        "term-s",
        ScreenSignal::Waiting,
        false
    ));
    assert_eq!(core.sessions.get("term-s").unwrap().state, "idle");
}
#[test]
fn screen_signal_starts_kiro_turn_because_files_are_completion_delayed() {
    let mut core = AIRuntimeStateCore::default();
    assert!(apply_hook_unlocked(
        &mut core,
        AIHookEventPayload {
            kind: "sessionStarted".to_string(),
            ..test_hook_for("kiro", "term-kiro", "", 1000.0)
        }
    ));
    assert_eq!(core.sessions.get("term-kiro").unwrap().state, "idle");

    assert!(apply_screen_signal_unlocked(
        &mut core,
        "term-kiro",
        ScreenSignal::Running,
        true
    ));

    let session = core.sessions.get("term-kiro").unwrap();
    assert_eq!(session.state, "responding");
    assert!(session.is_running);
    assert!(!session.has_completed_turn);
    assert!(session.runtime_turn_started_at.is_some());
}
#[test]
fn screen_signal_starts_next_kiro_turn_after_completed_turn() {
    let mut core = AIRuntimeStateCore::default();
    assert!(apply_hook_unlocked(
        &mut core,
        AIHookEventPayload {
            kind: "sessionStarted".to_string(),
            ..test_hook_for("kiro", "term-kiro-next", "", 1000.0)
        }
    ));
    assert!(apply_runtime_snapshot_unlocked(
        &mut core,
        "term-kiro-next",
        AIRuntimeContextSnapshot {
            tool: "kiro".to_string(),
            external_session_id: Some("kiro-session-1".to_string()),
            transcript_path: Some("/tmp/kiro-session-1.json".to_string()),
            model: Some("auto".to_string()),
            assistant_preview: Some("done".to_string()),
            input_tokens: 0,
            output_tokens: 0,
            cached_input_tokens: 0,
            total_tokens: 0,
            usage_amounts: vec![crate::ai_runtime::AIUsageAmountSnapshot {
                unit: "credit".to_string(),
                value: 0.03,
            }],
            baseline_usage_amounts: Vec::new(),
            updated_at: 1010.0,
            runtime_activity_at: None,
            last_user_input_at: None,
            started_at: Some(1005.0),
            completed_at: Some(1010.0),
            response_state: Some("idle".to_string()),
            was_interrupted: false,
            has_completed_turn: true,
            session_origin: "live".to_string(),
            source: "probe".to_string(),
            plan: None,
        }
    ));
    let session = core.sessions.get("term-kiro-next").unwrap();
    assert_eq!(session.state, "idle");
    assert!(session.has_completed_turn);
    assert!(should_poll_runtime_session(session, "interval", 1015.0));
    assert!(!should_poll_runtime_session(session, "interval", 2000.0));

    assert!(apply_screen_signal_unlocked(
        &mut core,
        "term-kiro-next",
        ScreenSignal::Running,
        true
    ));

    let session = core.sessions.get("term-kiro-next").unwrap();
    assert_eq!(session.state, "responding");
    assert!(session.is_running);
    assert!(!session.has_completed_turn);
    assert_eq!(session.usage_amounts[0].unit, "credit");
}
#[test]
fn kiro_completion_snapshot_clears_screen_started_turn_without_timestamp_gate() {
    let mut core = AIRuntimeStateCore::default();
    assert!(apply_hook_unlocked(
        &mut core,
        AIHookEventPayload {
            kind: "sessionStarted".to_string(),
            ..test_hook_for("kiro", "term-kiro-complete", "", 1000.0)
        }
    ));
    assert!(apply_screen_signal_unlocked(
        &mut core,
        "term-kiro-complete",
        ScreenSignal::Running,
        true
    ));
    assert_eq!(
        core.sessions.get("term-kiro-complete").unwrap().state,
        "responding"
    );

    assert!(apply_runtime_snapshot_unlocked(
        &mut core,
        "term-kiro-complete",
        AIRuntimeContextSnapshot {
            tool: "kiro".to_string(),
            external_session_id: Some("kiro-session-complete".to_string()),
            transcript_path: Some("/tmp/kiro-session-complete.json".to_string()),
            model: Some("auto".to_string()),
            assistant_preview: Some("done".to_string()),
            input_tokens: 0,
            output_tokens: 0,
            cached_input_tokens: 0,
            total_tokens: 0,
            usage_amounts: vec![crate::ai_runtime::AIUsageAmountSnapshot {
                unit: "credit".to_string(),
                value: 0.05,
            }],
            baseline_usage_amounts: Vec::new(),
            updated_at: 1000.0,
            runtime_activity_at: None,
            last_user_input_at: None,
            started_at: Some(999.0),
            completed_at: Some(1000.0),
            response_state: Some("idle".to_string()),
            was_interrupted: false,
            has_completed_turn: true,
            session_origin: "live".to_string(),
            source: "probe".to_string(),
            plan: None,
        }
    ));

    let session = core.sessions.get("term-kiro-complete").unwrap();
    assert_eq!(session.state, "idle");
    assert!(!session.is_running);
    assert!(session.has_completed_turn);
}
#[test]
fn kiro_fast_completion_after_detection_keeps_current_credit_usage() {
    let mut core = AIRuntimeStateCore::default();
    assert!(apply_hook_unlocked(
        &mut core,
        AIHookEventPayload {
            kind: "sessionStarted".to_string(),
            ..test_hook_for("kiro", "term-kiro-fast", "", 1000.0)
        }
    ));

    assert!(apply_runtime_snapshot_unlocked(
        &mut core,
        "term-kiro-fast",
        AIRuntimeContextSnapshot {
            tool: "kiro".to_string(),
            external_session_id: Some("kiro-session-fast".to_string()),
            transcript_path: Some("/tmp/kiro-session-fast.json".to_string()),
            model: Some("auto".to_string()),
            assistant_preview: Some("done".to_string()),
            input_tokens: 0,
            output_tokens: 0,
            cached_input_tokens: 0,
            total_tokens: 0,
            usage_amounts: vec![crate::ai_runtime::AIUsageAmountSnapshot {
                unit: "credit".to_string(),
                value: 0.041,
            }],
            baseline_usage_amounts: Vec::new(),
            updated_at: 1006.0,
            runtime_activity_at: None,
            last_user_input_at: None,
            started_at: Some(999.0),
            completed_at: Some(1006.0),
            response_state: Some("idle".to_string()),
            was_interrupted: false,
            has_completed_turn: true,
            session_origin: "unknown".to_string(),
            source: "probe".to_string(),
            plan: None,
        }
    ));

    let session = core.sessions.get("term-kiro-fast").unwrap();
    assert_eq!(session.state, "idle");
    assert!(session.baseline_usage_amounts.is_empty());
    assert_eq!(session.usage_amounts[0].unit, "credit");
    assert_eq!(session.usage_amounts[0].value, 0.041);
}
#[test]
fn kiro_restored_completion_uses_prelaunch_credit_as_baseline() {
    let mut core = AIRuntimeStateCore::default();
    assert!(apply_hook_unlocked(
        &mut core,
        AIHookEventPayload {
            kind: "sessionStarted".to_string(),
            ..test_hook_for("kiro", "term-kiro-restore", "", 1_782_718_000.0)
        }
    ));
    assert!(apply_screen_signal_unlocked(
        &mut core,
        "term-kiro-restore",
        ScreenSignal::Running,
        true
    ));

    assert!(apply_runtime_snapshot_unlocked(
        &mut core,
        "term-kiro-restore",
        AIRuntimeContextSnapshot {
            tool: "kiro".to_string(),
            external_session_id: Some("0f55b186-4800-4353-a791-5ae222d2faf6".to_string()),
            transcript_path: Some("/tmp/0f55b186-4800-4353-a791-5ae222d2faf6.json".to_string(),),
            model: Some("auto".to_string()),
            assistant_preview: Some("new".to_string()),
            input_tokens: 0,
            output_tokens: 0,
            cached_input_tokens: 0,
            total_tokens: 0,
            usage_amounts: vec![crate::ai_runtime::AIUsageAmountSnapshot {
                unit: "credit".to_string(),
                value: 0.06803484112769487,
            }],
            baseline_usage_amounts: vec![crate::ai_runtime::AIUsageAmountSnapshot {
                unit: "credit".to_string(),
                value: 0.041447917081260374,
            }],
            updated_at: 1_782_718_038.0,
            runtime_activity_at: None,
            last_user_input_at: None,
            started_at: Some(1_782_718_036.0),
            completed_at: Some(1_782_718_038.0),
            response_state: Some("idle".to_string()),
            was_interrupted: false,
            has_completed_turn: true,
            session_origin: "unknown".to_string(),
            source: "probe".to_string(),
            plan: None,
        }
    ));

    let session = core.sessions.get("term-kiro-restore").unwrap();
    assert_eq!(session.state, "idle");
    assert_eq!(session.usage_amounts[0].unit, "credit");
    assert!((session.usage_amounts[0].value - 0.06803484112769487).abs() < 0.000_000_001);
    assert_eq!(session.baseline_usage_amounts[0].unit, "credit");
    assert!((session.baseline_usage_amounts[0].value - 0.041447917081260374).abs() < 0.000_000_001);
    let summary = crate::ai_runtime_state::AIRuntimeStateService::new(&std::env::temp_dir())
        .summary_from_runtime_snapshot(&summary::state_snapshot_unlocked(&core));
    let current = summary
        .sessions
        .iter()
        .find(|session| session.terminal_id == "term-kiro-restore")
        .expect("current session");
    assert_eq!(current.raw_usage_amounts[0].unit, "credit");
    assert!((current.raw_usage_amounts[0].value - 0.06803484112769487).abs() < 0.000_000_001);
    assert_eq!(current.usage_amounts[0].unit, "credit");
    assert!((current.usage_amounts[0].value - 0.026586924046434493).abs() < 0.000_000_001);
}
#[test]
fn kiro_late_prelaunch_credit_baseline_overrides_empty_resolved_baseline() {
    let mut core = AIRuntimeStateCore::default();
    assert!(apply_hook_unlocked(
        &mut core,
        AIHookEventPayload {
            kind: "sessionStarted".to_string(),
            ..test_hook_for("kiro", "term-kiro-late-baseline", "", 1_782_718_000.0)
        }
    ));

    assert!(apply_runtime_snapshot_unlocked(
        &mut core,
        "term-kiro-late-baseline",
        AIRuntimeContextSnapshot {
            tool: "kiro".to_string(),
            external_session_id: Some("kiro-session".to_string()),
            transcript_path: Some("/tmp/kiro-session.json".to_string()),
            model: Some("auto".to_string()),
            assistant_preview: None,
            input_tokens: 0,
            output_tokens: 0,
            cached_input_tokens: 0,
            total_tokens: 0,
            usage_amounts: Vec::new(),
            baseline_usage_amounts: Vec::new(),
            updated_at: 1_782_718_001.0,
            runtime_activity_at: None,
            last_user_input_at: None,
            started_at: None,
            completed_at: None,
            response_state: None,
            was_interrupted: false,
            has_completed_turn: false,
            session_origin: "unknown".to_string(),
            source: "probe".to_string(),
            plan: None,
        }
    ));
    assert!(
        core.sessions
            .get("term-kiro-late-baseline")
            .unwrap()
            .baseline_resolved
    );

    assert!(apply_runtime_snapshot_unlocked(
        &mut core,
        "term-kiro-late-baseline",
        AIRuntimeContextSnapshot {
            tool: "kiro".to_string(),
            external_session_id: Some("kiro-session".to_string()),
            transcript_path: Some("/tmp/kiro-session.json".to_string()),
            model: Some("auto".to_string()),
            assistant_preview: Some("new".to_string()),
            input_tokens: 0,
            output_tokens: 0,
            cached_input_tokens: 0,
            total_tokens: 0,
            usage_amounts: vec![crate::ai_runtime::AIUsageAmountSnapshot {
                unit: "credit".to_string(),
                value: 0.06803484112769487,
            }],
            baseline_usage_amounts: vec![crate::ai_runtime::AIUsageAmountSnapshot {
                unit: "credit".to_string(),
                value: 0.041447917081260374,
            }],
            updated_at: 1_782_718_038.0,
            runtime_activity_at: None,
            last_user_input_at: None,
            started_at: Some(1_782_718_036.0),
            completed_at: Some(1_782_718_038.0),
            response_state: Some("idle".to_string()),
            was_interrupted: false,
            has_completed_turn: true,
            session_origin: "unknown".to_string(),
            source: "probe".to_string(),
            plan: None,
        }
    ));

    let summary = crate::ai_runtime_state::AIRuntimeStateService::new(&std::env::temp_dir())
        .summary_from_runtime_snapshot(&summary::state_snapshot_unlocked(&core));
    let current = summary
        .sessions
        .iter()
        .find(|session| session.terminal_id == "term-kiro-late-baseline")
        .expect("current session");
    assert_eq!(current.usage_amounts[0].unit, "credit");
    assert!((current.usage_amounts[0].value - 0.026586924046434493).abs() < 0.000_000_001);
}
