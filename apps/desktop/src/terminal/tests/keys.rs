use super::super::*;
use super::fixtures::*;

#[test]
fn maps_plain_text_and_basic_control_keys() {
    assert_eq!(bytes(keystroke("enter"), normal_mode()), b"\r");
    assert_eq!(bytes(keystroke("Return"), normal_mode()), b"\r");
    assert_eq!(bytes(keystroke("kp_enter"), normal_mode()), b"\r");
    assert_eq!(bytes(keystroke("tab"), normal_mode()), b"\t");
    assert_eq!(bytes(keystroke("Tab"), normal_mode()), b"\t");
    assert_eq!(bytes(keystroke("escape"), normal_mode()), b"\x1b");
    assert_eq!(bytes(keystroke("Esc"), normal_mode()), b"\x1b");
    assert_eq!(bytes(keystroke("backspace"), normal_mode()), b"\x7f");
}

#[test]
fn agent_queue_plain_enter_matches_all_terminal_enter_aliases() {
    for key in ["enter", "Return", "kp_enter", "numpadenter", "numpad_enter"] {
        assert!(terminal_plain_enter(&keystroke(key)), "missing alias {key}");
    }
    assert!(!terminal_plain_enter(&modified_key(
        "enter", true, false, false, false
    )));
}
#[test]
fn plain_character_without_text_input_is_not_lowercased() {
    assert!(keystroke_to_bytes(&keystroke("a"), normal_mode()).is_none());
}
#[test]
fn printable_key_chars_are_committed_by_text_input() {
    assert!(keystroke_to_bytes(&key_char("a", "a"), normal_mode()).is_none());
    assert!(keystroke_to_bytes(&key_char("a", "A"), normal_mode()).is_none());
    assert!(keystroke_to_bytes(&key_char("semicolon", ";"), normal_mode()).is_none());
}
#[test]
fn maps_terminal_interrupt_shortcut_to_etx() {
    assert_eq!(
        bytes(modified_key("c", false, false, true, false), normal_mode()),
        b"\x03"
    );
    assert_eq!(
        bytes(
            modified_key_with_char("c", "c", false, false, true, false),
            normal_mode()
        ),
        b"\x03"
    );
    assert_eq!(
        bytes(
            modified_key_with_char("c", "\x03", false, false, true, false),
            normal_mode()
        ),
        b"\x03"
    );
}
#[test]
fn maps_copy_and_paste_shortcuts_as_ui_commands() {
    assert!(is_copy_keystroke(&modified_key(
        "C", false, false, false, true
    )));
    assert!(is_paste_keystroke(&modified_key(
        "V", false, false, false, true
    )));
    assert!(!is_copy_keystroke(&modified_key(
        "c", false, false, true, false
    )));
    assert_eq!(
        is_paste_keystroke(&modified_key("v", false, false, true, false)),
        cfg!(windows)
    );
    assert_eq!(
        is_paste_keystroke(&modified_key("insert", true, false, false, false)),
        !cfg!(target_os = "macos")
    );
}
#[test]
fn shift_scroll_keeps_terminal_history_available_in_alternate_screen() {
    assert!(should_send_alternate_scroll(alternate_scroll_mode(), false));
    assert!(!should_send_alternate_scroll(alternate_scroll_mode(), true));
}
#[test]
fn maps_app_cursor_mode() {
    assert_eq!(bytes(keystroke("up"), normal_mode()), b"\x1b[A");
    assert_eq!(bytes(keystroke("down"), normal_mode()), b"\x1b[B");
    assert_eq!(bytes(keystroke("right"), normal_mode()), b"\x1b[C");
    assert_eq!(bytes(keystroke("left"), normal_mode()), b"\x1b[D");
    assert_eq!(bytes(keystroke("arrow_up"), normal_mode()), b"\x1b[A");
    assert_eq!(bytes(keystroke("down_arrow"), normal_mode()), b"\x1b[B");
    assert_eq!(bytes(keystroke("home"), normal_mode()), b"\x1b[H");
    assert_eq!(bytes(keystroke("end"), normal_mode()), b"\x1b[F");

    assert_eq!(bytes(keystroke("up"), app_cursor_mode()), b"\x1bOA");
    assert_eq!(bytes(keystroke("down"), app_cursor_mode()), b"\x1bOB");
    assert_eq!(bytes(keystroke("right"), app_cursor_mode()), b"\x1bOC");
    assert_eq!(bytes(keystroke("left"), app_cursor_mode()), b"\x1bOD");
    assert_eq!(bytes(keystroke("home"), app_cursor_mode()), b"\x1bOH");
    assert_eq!(bytes(keystroke("end"), app_cursor_mode()), b"\x1bOF");
}
#[test]
fn marked_text_filters_escape_sequences_from_navigation_keys() {
    assert_eq!(terminal_input_marked_text("\x1bOA"), "");
    assert_eq!(terminal_input_marked_text("^[OA"), "");
    assert_eq!(terminal_input_marked_text("␛OA"), "");
}
#[test]
fn marked_text_keeps_printable_composition_text() {
    assert_eq!(terminal_input_marked_text("pin"), "pin");
    assert_eq!(terminal_input_marked_text("拼"), "拼");
}
#[test]
fn committed_ime_text_drops_navigation_escape_sequences() {
    // The IME commit path (`send_filtered_input`) drops text that looks like
    // an escape sequence: a navigation key mis-delivered here as caret
    // notation ("^[OA") or a real ESC would otherwise be written verbatim
    // and the shell would echo a literal "^[OA" on top of the keystroke
    // path's real recall.
    assert!(terminal_marked_text_looks_like_escape_sequence("^[OA"));
    assert!(terminal_marked_text_looks_like_escape_sequence("\x1bOA"));
    assert!(terminal_marked_text_looks_like_escape_sequence("␛OA"));
    assert!(!terminal_marked_text_looks_like_escape_sequence("拼"));
    assert!(!terminal_marked_text_looks_like_escape_sequence("ls -la"));
}
#[test]
fn maps_modified_navigation_and_function_keys() {
    assert_eq!(
        bytes(modified_key("up", true, false, false, false), normal_mode()),
        b"\x1b[1;2A"
    );
    assert_eq!(
        bytes(
            modified_key("left", false, true, true, false),
            normal_mode()
        ),
        b"\x1b[1;7D"
    );
    assert_eq!(
        bytes(
            modified_key("home", true, false, false, false),
            normal_mode()
        ),
        b"\x1b[1;2H"
    );
    assert_eq!(bytes(keystroke("f12"), normal_mode()), b"\x1b[24~");
    assert_eq!(bytes(keystroke("f20"), normal_mode()), b"\x1b[34~");
    assert_eq!(
        bytes(modified_key("f5", false, false, true, false), normal_mode()),
        b"\x1b[15;5~"
    );
    assert_eq!(
        bytes(
            modified_key("delete", true, false, false, false),
            normal_mode()
        ),
        b"\x1b[3;2~"
    );
}
#[test]
fn maps_macos_terminal_navigation_shortcuts() {
    assert_eq!(
        bytes(
            modified_key("left", false, true, false, false),
            normal_mode()
        ),
        b"\x1bb"
    );
    assert_eq!(
        bytes(
            modified_key("right", false, true, false, false),
            normal_mode()
        ),
        b"\x1bf"
    );
    assert_eq!(
        bytes(
            modified_key("left", false, false, false, true),
            normal_mode()
        ),
        b"\x01"
    );
    assert_eq!(
        bytes(
            modified_key("right", false, false, false, true),
            normal_mode()
        ),
        b"\x05"
    );
    assert_eq!(
        bytes(
            modified_key_with_function("left", false, false, false, true, true),
            normal_mode()
        ),
        b"\x01"
    );
    assert_eq!(
        bytes(
            modified_key_with_function("right", false, false, false, true, true),
            normal_mode()
        ),
        b"\x05"
    );
    assert_eq!(
        bytes(
            modified_key_with_function("left", false, true, false, false, true),
            normal_mode()
        ),
        b"\x1bb"
    );
    assert_eq!(
        bytes(
            modified_key_with_function("right", false, true, false, false, true),
            normal_mode()
        ),
        b"\x1bf"
    );
    assert_eq!(
        bytes(
            modified_key("home", false, false, false, true),
            normal_mode()
        ),
        b"\x01"
    );
    assert_eq!(
        bytes(
            modified_key("end", false, false, false, true),
            normal_mode()
        ),
        b"\x05"
    );
    assert_eq!(
        bytes(
            modified_key("delete", false, true, false, false),
            normal_mode()
        ),
        b"\x1bd"
    );
    assert_eq!(
        bytes(
            modified_key("backspace", false, false, false, true),
            normal_mode()
        ),
        b"\x15"
    );
    assert_eq!(
        bytes(
            modified_key("back", false, false, false, true),
            normal_mode()
        ),
        b"\x15"
    );
    assert_eq!(
        bytes(
            modified_key("delete", false, false, false, true),
            normal_mode()
        ),
        b"\x0b"
    );
}
#[test]
fn keeps_macos_app_shortcuts_out_of_terminal_input() {
    for key in ["q", "h", "m", "w", "tab", "`"] {
        assert!(
            keystroke_to_bytes(&modified_key(key, false, false, false, true), normal_mode())
                .is_none(),
            "Cmd+{key} should remain an app shortcut"
        );
    }
    assert!(
        keystroke_to_bytes(&modified_key("h", false, true, false, true), normal_mode()).is_none()
    );
    assert!(
        keystroke_to_bytes(&modified_key("m", false, true, false, true), normal_mode()).is_none()
    );
    assert!(
        keystroke_to_bytes(
            &modified_key("tab", true, false, false, true),
            normal_mode()
        )
        .is_none()
    );
}
#[test]
fn preserves_control_q_for_terminal_flow_control() {
    assert_eq!(
        bytes(modified_key("q", false, false, true, false), normal_mode()),
        b"\x11"
    );
    assert_eq!(
        bytes(modified_key("Q", true, false, true, false), normal_mode()),
        b"\x11"
    );
}
#[test]
fn maps_ctrl_alt_and_shift_enter_sequences() {
    assert_eq!(
        bytes(modified_key("a", false, false, true, false), normal_mode()),
        b"\x01"
    );
    assert_eq!(
        bytes(modified_key("C", true, false, true, false), normal_mode()),
        b"\x03"
    );
    assert_eq!(
        bytes(modified_key("[", false, false, true, false), normal_mode()),
        b"\x1b"
    );
    assert_eq!(
        bytes(
            modified_key("enter", true, false, false, false),
            normal_mode()
        ),
        b"\n"
    );
    assert_eq!(
        bytes(
            modified_key("Tab", true, false, false, false),
            normal_mode()
        ),
        b"\x1b[Z"
    );
    assert_eq!(
        bytes(
            modified_key("BackTab", true, false, false, false),
            normal_mode()
        ),
        b"\x1b[Z"
    );
    assert_eq!(
        bytes(
            modified_key("enter", false, true, false, false),
            normal_mode()
        ),
        b"\x1b\r"
    );
    assert_eq!(
        bytes(modified_key("x", false, true, false, false), normal_mode()),
        b"\x1bx"
    );
}
#[test]
fn text_input_channel_drops_control_and_paste_echoes() {
    assert!(terminal_text_input_should_drop(""));
    assert!(terminal_text_input_should_drop("\u{3}"));
    assert!(terminal_text_input_should_drop(
        "\u{1b}[200~echo hi\u{1b}[201~"
    ));
    assert!(terminal_text_input_should_drop("\u{1b}[A"));
    assert!(!terminal_text_input_should_drop("echo hi"));
    assert!(!terminal_text_input_should_drop("你好"));
}
#[test]
fn key_input_uses_live_cursor_mode_before_snapshot_publish() {
    let mut state = TerminalModel::new_for_test(20, 4, 100);
    state.process_output_bytes_for_test(b"\x1b[?1h");
    assert!(!state.handle.input_mode().application_cursor);

    assert!(state.mode().application_cursor);
}
