use super::super::*;
use super::fixtures::*;

fn submission_text(draft: &TerminalAgentDraft) -> Option<String> {
    match draft.submission() {
        Some(TerminalAgentDraftSubmission::Text(text)) => Some(text),
        _ => None,
    }
}

#[test]
fn agent_draft_tracks_text_paste_cursor_and_unicode_edits() {
    let mut draft = TerminalAgentDraft::new();
    draft.record_text("你好 world");
    draft.record_keystroke(&keystroke("left"), b"\x1b[D");
    draft.record_keystroke(&keystroke("backspace"), b"\x7f");
    draft.record_text("ld!");

    assert_eq!(submission_text(&draft).as_deref(), Some("你好 world!d"));
}

#[test]
fn agent_draft_tracks_multiline_and_clear_shortcuts() {
    let mut draft = TerminalAgentDraft::new();
    draft.record_text("first");
    draft.record_keystroke(&modified_key("Return", true, false, false, false), b"\n");
    draft.record_text("second");
    assert_eq!(submission_text(&draft).as_deref(), Some("first\nsecond"));

    draft.record_keystroke(&modified_key("u", false, false, true, false), b"\x15");
    assert!(draft.submission().is_none());
}

#[test]
fn agent_draft_normalizes_pasted_line_endings() {
    let mut draft = TerminalAgentDraft::new();
    draft.record_text("one\r\ntwo\rthree");

    assert_eq!(submission_text(&draft).as_deref(), Some("one\ntwo\nthree"));
}

#[test]
fn agent_draft_falls_back_after_unobservable_history_navigation() {
    let mut draft = TerminalAgentDraft::new();
    draft.record_text("local draft");
    draft.record_keystroke(&keystroke("up"), b"\x1b[A");
    assert!(draft.submission().is_none());

    draft.record_keystroke(&modified_key("c", false, false, true, false), b"\x03");
    draft.record_text("known again");
    assert_eq!(submission_text(&draft).as_deref(), Some("known again"));
}

#[test]
fn agent_draft_models_platform_and_word_editing_shortcuts() {
    let mut draft = TerminalAgentDraft::new();
    draft.record_text("one two");
    draft.record_keystroke(&modified_key("left", false, false, false, true), b"\x01");
    draft.record_text("X");
    draft.record_keystroke(&modified_key("right", false, false, false, true), b"\x05");
    draft.record_keystroke(
        &modified_key("backspace", false, true, false, false),
        b"\x1b\x7f",
    );

    assert_eq!(submission_text(&draft).as_deref(), Some("Xone "));
}

#[test]
fn agent_draft_marks_unmodeled_control_shortcuts_unreliable() {
    let mut draft = TerminalAgentDraft::new();
    draft.record_text("abc");
    draft.record_keystroke(&modified_key("r", false, false, true, false), b"\x12");

    assert!(draft.submission().is_none());
}

#[test]
fn agent_draft_bounds_large_pastes_without_truncating_a_queued_prompt() {
    let mut draft = TerminalAgentDraft::new();
    draft.record_text(&"x".repeat(MAX_TERMINAL_AGENT_DRAFT_BYTES + 1));

    assert_eq!(
        draft.submission(),
        Some(TerminalAgentDraftSubmission::TooLarge)
    );
    draft.record_keystroke(&modified_key("c", false, false, true, false), b"\x03");
    assert!(draft.submission().is_none());
}
