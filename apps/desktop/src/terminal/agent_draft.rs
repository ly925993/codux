const MAX_TERMINAL_AGENT_DRAFT_BYTES: usize = 256 * 1024;

#[derive(Default)]
struct TerminalAgentDraft {
    text: String,
    cursor: usize,
    reliable: bool,
    overflowed: bool,
}

impl TerminalAgentDraft {
    fn new() -> Self {
        Self {
            reliable: true,
            ..Default::default()
        }
    }

    fn record_text(&mut self, text: &str) {
        if text.is_empty() || self.overflowed {
            return;
        }
        if self.text.len().saturating_add(text.len()) > MAX_TERMINAL_AGENT_DRAFT_BYTES {
            // Keep the real TUI composer untouched, but stop mirroring before
            // a large paste can allocate unbounded memory on the GPUI thread.
            self.overflowed = true;
            return;
        }
        self.cursor = self.cursor.min(self.text.len());
        if text.contains('\r') {
            // Normalize pasted Windows/macOS line endings only when needed so
            // ordinary single-character input stays allocation-free.
            let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
            self.text.insert_str(self.cursor, &normalized);
            self.cursor += normalized.len();
        } else {
            self.text.insert_str(self.cursor, text);
            self.cursor += text.len();
        }
    }

    fn record_keystroke(&mut self, keystroke: &Keystroke, bytes: &[u8]) {
        let key = terminal_agent_normalize_key(&keystroke.key);
        let modifiers = &keystroke.modifiers;
        if self.overflowed {
            if modifiers.control && key == "c" {
                self.clear();
            }
            return;
        }
        if matches!(key.as_str(), "enter" | "return" | "kp_enter")
            && modifiers.shift
            && !modifiers.control
            && !modifiers.alt
        {
            self.record_text("\n");
            return;
        }
        if modifiers.platform && matches!(key.as_str(), "left" | "home") {
            self.cursor = 0;
            return;
        }
        if modifiers.platform && matches!(key.as_str(), "right" | "end") {
            self.cursor = self.text.len();
            return;
        }
        if modifiers.alt && key == "left" {
            self.move_cursor_left(true);
            return;
        }
        if modifiers.alt && key == "right" {
            self.move_cursor_right(true);
            return;
        }
        if !modifiers.alt && !modifiers.platform && key == "left" {
            self.move_cursor_left(false);
            return;
        }
        if !modifiers.alt && !modifiers.platform && key == "right" {
            self.move_cursor_right(false);
            return;
        }
        if matches!(key.as_str(), "up" | "down") {
            // TUI history navigation can replace the entire composer without
            // echoing text back through GPUI. Fall back to native submission
            // until a clear command establishes a known empty draft again.
            self.reliable = false;
            return;
        }
        if modifiers.control && key == "b" {
            self.move_cursor_left(false);
            return;
        }
        if modifiers.control && key == "f" {
            self.move_cursor_right(false);
            return;
        }
        if matches!(key.as_str(), "home") || (modifiers.control && key == "a") {
            self.cursor = 0;
            return;
        }
        if matches!(key.as_str(), "end") || (modifiers.control && key == "e") {
            self.cursor = self.text.len();
            return;
        }
        if modifiers.alt && matches!(key.as_str(), "backspace" | "back") {
            self.remove_previous_word();
            return;
        }
        if modifiers.alt && key == "delete" {
            self.remove_next_word();
            return;
        }
        if modifiers.control && key == "w" {
            self.remove_previous_word();
            return;
        }
        if modifiers.control && key == "u" || modifiers.platform && key == "backspace" {
            self.text.drain(..self.cursor);
            self.cursor = 0;
            self.reliable = true;
            return;
        }
        if modifiers.control && key == "k" || modifiers.platform && key == "delete" {
            self.text.truncate(self.cursor);
            self.reliable = true;
            return;
        }
        if modifiers.control && key == "c" {
            self.clear();
            return;
        }
        if modifiers.control && key == "d" {
            self.remove_next_char();
            return;
        }
        if !modifiers.alt && !modifiers.platform && matches!(key.as_str(), "backspace" | "back") {
            self.remove_previous_char();
            return;
        }
        if !modifiers.alt && !modifiers.platform && key == "delete" {
            self.remove_next_char();
            return;
        }
        match bytes {
            [0x7f] | [0x08] => self.remove_previous_char(),
            [0x17] => self.remove_previous_word(),
            [0x15] => {
                self.text.drain(..self.cursor);
                self.cursor = 0;
                self.reliable = true;
            }
            [0x0b] => {
                self.text.truncate(self.cursor);
                self.reliable = true;
            }
            [0x03] => self.clear(),
            _ => {
                // Any unmodeled control/navigation shortcut may mutate an
                // Agent-specific composer without an observable text event.
                // Native Enter is safer than queueing a different prompt.
                self.reliable = false;
            }
        }
    }

    fn submission(&self) -> Option<TerminalAgentDraftSubmission> {
        if self.overflowed {
            return Some(TerminalAgentDraftSubmission::TooLarge);
        }
        (self.reliable && !self.text.trim().is_empty())
            .then(|| TerminalAgentDraftSubmission::Text(self.text.clone()))
    }

    fn is_empty_and_reliable(&self) -> bool {
        self.reliable && !self.overflowed && self.text.is_empty()
    }

    fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.reliable = true;
        self.overflowed = false;
    }

    fn mark_unreliable(&mut self) {
        if !self.text.is_empty() {
            self.reliable = false;
        }
    }

    fn move_cursor_left(&mut self, by_word: bool) {
        if by_word {
            while self.cursor > 0 {
                let (index, ch) = self.previous_char().expect("cursor is above zero");
                self.cursor = index;
                if !ch.is_whitespace() {
                    break;
                }
            }
            while self.cursor > 0 {
                let (index, ch) = self.previous_char().expect("cursor is above zero");
                if ch.is_whitespace() {
                    break;
                }
                self.cursor = index;
            }
        } else if let Some((index, _)) = self.previous_char() {
            self.cursor = index;
        }
    }

    fn move_cursor_right(&mut self, by_word: bool) {
        if by_word {
            while let Some((next, ch)) = self.next_char() {
                self.cursor = next;
                if ch.is_whitespace() {
                    break;
                }
            }
            while let Some((next, ch)) = self.next_char() {
                if !ch.is_whitespace() {
                    break;
                }
                self.cursor = next;
            }
        } else if let Some((next, _)) = self.next_char() {
            self.cursor = next;
        }
    }

    fn remove_previous_char(&mut self) {
        let Some((index, _)) = self.previous_char() else {
            return;
        };
        self.text.drain(index..self.cursor);
        self.cursor = index;
    }

    fn remove_next_char(&mut self) {
        let Some((next, _)) = self.next_char() else {
            return;
        };
        self.text.drain(self.cursor..next);
    }

    fn remove_previous_word(&mut self) {
        let end = self.cursor;
        self.move_cursor_left(true);
        self.text.drain(self.cursor..end);
    }

    fn remove_next_word(&mut self) {
        let start = self.cursor;
        self.move_cursor_right(true);
        self.text.drain(start..self.cursor);
        self.cursor = start;
    }

    fn previous_char(&self) -> Option<(usize, char)> {
        self.text[..self.cursor].char_indices().next_back()
    }

    fn next_char(&self) -> Option<(usize, char)> {
        let ch = self.text[self.cursor..].chars().next()?;
        Some((self.cursor + ch.len_utf8(), ch))
    }
}

fn terminal_agent_normalize_key(key: &str) -> String {
    let normalized = key.to_ascii_lowercase();
    match normalized.as_str() {
        "return" | "kp_enter" | "numpadenter" | "numpad_enter" => "enter".to_string(),
        "back" => "backspace".to_string(),
        "del" => "delete".to_string(),
        "arrowup" | "arrow_up" | "up_arrow" => "up".to_string(),
        "arrowdown" | "arrow_down" | "down_arrow" => "down".to_string(),
        "arrowleft" | "arrow_left" | "left_arrow" => "left".to_string(),
        "arrowright" | "arrow_right" | "right_arrow" => "right".to_string(),
        _ => normalized,
    }
}

/// Captured native-composer content passed to the application routing layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalAgentDraftSubmission {
    Text(String),
    TooLarge,
}

/// Decides whether native Enter is forwarded, consumed by the queue, or
/// rejected while preserving the user's current composer contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalAgentPromptDisposition {
    PassThrough,
    Queued,
    Rejected,
}
