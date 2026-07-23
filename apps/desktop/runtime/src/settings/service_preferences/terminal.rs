impl SettingsService {
    pub fn set_terminal_scrollback_lines(&self, lines: usize) -> Result<SettingsSummary, String> {
        self.update_string(
            "terminalScrollbackLines",
            lines.clamp(200, 10_000).to_string(),
        )
    }

    pub fn set_terminal_font_size(&self, size: &str) -> Result<SettingsSummary, String> {
        let size = numeric_string(size, 14, 8, 28).to_string();
        self.update_string("terminalFontSize", size)
    }

    pub fn set_terminal_padding(&self, padding: &str) -> Result<SettingsSummary, String> {
        let padding = numeric_string(padding, 10, 0, 40).to_string();
        self.update_string("terminalPadding", padding)
    }

    pub fn set_terminal_line_height(&self, multiplier: &str) -> Result<SettingsSummary, String> {
        self.update_string(
            "terminalLineHeight",
            float_string(multiplier, 1.45, 1.0, 2.0),
        )
    }

    pub fn set_terminal_font_family(&self, family: &str) -> Result<SettingsSummary, String> {
        self.update_string("terminalFontFamily", sanitize_terminal_font_family(family))
    }

    pub fn set_terminal_shell(&self, shell: &str) -> Result<SettingsSummary, String> {
        self.update_string("terminalShell", shell.trim().to_string())
    }

    pub fn toggle_terminal_paste_images_as_paths(&self) -> Result<SettingsSummary, String> {
        let mut raw = self.raw_settings();
        let current = raw
            .get("terminalPasteImagesAsPaths")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        raw.insert(
            "terminalPasteImagesAsPaths".to_string(),
            Value::Bool(!current),
        );
        self.save_raw_settings(&raw)?;
        Ok(summary_from_raw(&raw))
    }

    pub fn toggle_terminal_copy_on_select(&self) -> Result<SettingsSummary, String> {
        let mut raw = self.raw_settings();
        let current = raw
            .get("terminalCopyOnSelect")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        raw.insert("terminalCopyOnSelect".to_string(), Value::Bool(!current));
        self.save_raw_settings(&raw)?;
        Ok(summary_from_raw(&raw))
    }

    pub fn toggle_terminal_right_click_paste(&self) -> Result<SettingsSummary, String> {
        self.toggle_terminal_boolean("terminalRightClickPaste", false)
    }

    pub fn toggle_terminal_link_navigation(&self) -> Result<SettingsSummary, String> {
        self.toggle_terminal_boolean("terminalLinkNavigation", true)
    }

    pub fn toggle_terminal_trim_trailing_whitespace_on_copy(
        &self,
    ) -> Result<SettingsSummary, String> {
        self.toggle_terminal_boolean("terminalTrimTrailingWhitespaceOnCopy", false)
    }

    pub fn toggle_terminal_trim_trailing_whitespace_on_paste(
        &self,
    ) -> Result<SettingsSummary, String> {
        self.toggle_terminal_boolean("terminalTrimTrailingWhitespaceOnPaste", false)
    }

    /// Keeps terminal booleans consistent and preserves unknown settings keys.
    fn toggle_terminal_boolean(
        &self,
        key: &str,
        default: bool,
    ) -> Result<SettingsSummary, String> {
        let mut raw = self.raw_settings();
        let current = raw.get(key).and_then(Value::as_bool).unwrap_or(default);
        raw.insert(key.to_string(), Value::Bool(!current));
        self.save_raw_settings(&raw)?;
        Ok(summary_from_raw(&raw))
    }

    pub fn set_terminal_scrollback_value(&self, lines: &str) -> Result<SettingsSummary, String> {
        let lines = numeric_string(lines, 2000, 200, 10_000).to_string();
        self.update_string("terminalScrollbackLines", lines)
    }

    pub fn cycle_terminal_font_size(&self) -> Result<SettingsSummary, String> {
        let current = numeric_string(&self.summary().terminal_font_size, 14, 8, 28);
        let next = match current {
            8 => 10,
            10 => 12,
            12 => 14,
            14 => 16,
            16 => 18,
            18 => 20,
            20 => 24,
            24 => 28,
            28 => 8,
            _ => 14,
        };
        self.update_string("terminalFontSize", next.to_string())
    }

    pub fn cycle_terminal_scrollback_lines(&self) -> Result<SettingsSummary, String> {
        let current = numeric_string(&self.summary().terminal_scrollback_lines, 2000, 200, 10_000);
        let next = match current {
            500 => 1000,
            1000 => 2000,
            2000 => 5000,
            5000 => 10_000,
            10_000 => 500,
            _ => 500,
        };
        self.update_string("terminalScrollbackLines", next.to_string())
    }
}
