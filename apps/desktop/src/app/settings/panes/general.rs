use super::options::*;
use super::widgets::*;
use super::*;

pub(super) fn settings_general_pane(
    settings: &SettingsSummary,
    pending_restart_language: Option<&str>,
    terminal_font_families: &[String],
    update: &UpdateSummary,
    window: &mut Window,
    cx: &mut Context<CoduxApp>,
) -> AnyElement {
    let language = settings.language.as_str();
    let visible_language = pending_restart_language.unwrap_or(language);
    settings_form(vec![
        settings_card(
            None,
            None,
            vec![
                settings_row(
                    settings_text(language, "settings.language", "Language"),
                    Some(settings_text(
                        language,
                        "settings.language.restart_message",
                        "Restart Codux to apply the selected language.",
                    )),
                    settings_select_impl(
                        "settings-language",
                        visible_language,
                        language_options(language),
                        window,
                        cx,
                        language,
                        |app, value, window, cx| app.set_language(value, window, cx),
                    ),
                )
                .into_any_element(),
                settings_row(
                    settings_text(language, "settings.dock_badge", "Dock Badge"),
                    None,
                    settings_toggle(
                        "settings-dock-badge",
                        settings.shows_dock_badge,
                        cx,
                        |app, window, cx| app.toggle_dock_badge(window, cx),
                    ),
                )
                .into_any_element(),
                settings_row(
                    settings_text(language, "settings.sleep_prevention", "Prevent System Sleep"),
                    Some(settings_text(
                        language,
                        "settings.sleep_prevention.help",
                        "Allows the display to turn off, but prevents this device from idle sleeping while enabled.",
                    )),
                    settings_select_impl(
                        "settings-sleep-mode",
                        &settings.sleep_mode,
                        sleep_mode_options(language),
                        window,
                        cx,
                        language,
                        |app, value, window, cx| app.set_sleep_mode(value, window, cx),
                    ),
                )
                .into_any_element(),
                settings_row(
                    settings_text(language, "settings.file_open_default", "File Open Default"),
                    Some(settings_text(
                        language,
                        "settings.file_open_default.help",
                        "Used when opening files outside the Files view.",
                    )),
                    settings_select_impl(
                        "settings-file-open-default",
                        &settings.file_open_default,
                        file_open_default_options(language),
                        window,
                        cx,
                        language,
                        |app, value, window, cx| app.set_file_open_default(value, window, cx),
                    ),
                )
                .into_any_element(),
            ],
            cx,)
        .into_any_element(),
        settings_card(
            None,
            None,
            vec![
                settings_row(
                    settings_text(language, "settings.git_auto_refresh", "Git Auto Refresh"),
                    None,
                    settings_select_impl(
                        "settings-git-refresh",
                        &settings.git_refresh,
                        git_refresh_options(),
                        window,
                        cx,
                        language,
                        |app, value, window, cx| app.set_git_refresh(value, window, cx),
                    ),
                )
                .into_any_element(),
                settings_row(
                    settings_text(language, "settings.ai_auto_refresh", "AI Auto Refresh"),
                    None,
                    settings_select_impl(
                        "settings-ai-refresh",
                        &settings.ai_refresh,
                        ai_refresh_options(),
                        window,
                        cx,
                        language,
                        |app, value, window, cx| app.set_ai_refresh(value, window, cx),
                    ),
                )
                .into_any_element(),
                settings_row(
                    settings_text(
                        language,
                        "settings.ai_background_refresh",
                        "AI Background Refresh",
                    ),
                    None,
                    settings_select_impl(
                        "settings-ai-background-refresh",
                        &settings.ai_background_refresh,
                        ai_background_refresh_options(),
                        window,
                        cx,
                        language,
                        |app, value, window, cx| app.set_ai_background_refresh(value, window, cx),
                    ),
                )
                .into_any_element(),
                settings_row(
                    settings_text(language, "settings.ai_statistics_mode", "AI Statistics Mode"),
                    None,
                    settings_select_impl(
                        "settings-statistics-mode",
                        &settings.statistics_mode,
                        statistics_mode_options(language),
                        window,
                        cx,
                        language,
                        |app, value, window, cx| app.set_statistics_mode(value, window, cx),
                    ),
                )
                .into_any_element(),
            ],
            cx,)
        .into_any_element(),
        settings_card(
            Some(settings_text(language, "settings.terminal_text", "Terminal Text")),
            None,
            vec![
                settings_row(
                    settings_text(language, "settings.terminal_font_family", "Terminal Font"),
                    Some(settings_text(
                        language,
                        "settings.terminal_font_family.help",
                        "Only monospaced fonts are shown to keep terminal layout accurate.",
                    )),
                    settings_select_impl(
                        "settings-terminal-font-family",
                        &settings.terminal_font_family,
                        terminal_font_family_options(
                            language,
                            &settings.terminal_font_family,
                            terminal_font_families,
                        ),
                        window,
                        cx,
                        language,
                        |app, value, window, cx| app.set_terminal_font_family(value, window, cx),
                    ),
                )
                .into_any_element(),
                settings_row(
                    settings_text(language, "settings.terminal_font_size", "Terminal Font Size"),
                    None,
                    settings_select_impl(
                        "settings-terminal-font-size",
                        &settings.terminal_font_size,
                        terminal_font_size_options(),
                        window,
                        cx,
                        language,
                        |app, value, window, cx| app.set_terminal_font_size(value, window, cx),
                    ),
                )
                .into_any_element(),
                settings_row(
                    settings_text(language, "settings.terminal_line_height", "Terminal Line Height"),
                    None,
                    settings_select_impl(
                        "settings-terminal-line-height",
                        &settings.terminal_line_height,
                        terminal_line_height_options(),
                        window,
                        cx,
                        language,
                        |app, value, window, cx| app.set_terminal_line_height(value, window, cx),
                    ),
                )
                .into_any_element(),
                settings_row(
                    settings_text(language, "settings.terminal_padding", "Terminal Padding"),
                    None,
                    settings_select_impl(
                        "settings-terminal-padding",
                        &settings.terminal_padding,
                        terminal_padding_options(),
                        window,
                        cx,
                        language,
                        |app, value, window, cx| app.set_terminal_padding(value, window, cx),
                    ),
                )
                .into_any_element(),
                settings_row(
                    settings_text(language, "settings.terminal_scrollback", "Terminal Scrollback"),
                    Some(settings_text(
                        language,
                        "settings.terminal_scrollback.help",
                        "Limit terminal scrollback and restored output to reduce long-session memory usage.",
                    )),
                    settings_select_impl(
                        "settings-terminal-scrollback",
                        &settings.terminal_scrollback_lines,
                        terminal_scrollback_options(language),
                        window,
                        cx,
                        language,
                        |app, value, window, cx| app.set_terminal_scrollback_lines(value, window, cx),
                    ),
                )
                .into_any_element(),
                settings_row(
                    settings_text(language, "settings.terminal_shell", "Terminal Shell"),
                    Some(settings_text(
                        language,
                        "settings.terminal_shell.help",
                        "Applies to newly opened terminals.",
                    )),
                    settings_select_impl(
                        "settings-terminal-shell",
                        &settings.terminal_shell,
                        terminal_shell_options(language, &settings.terminal_shell),
                        window,
                        cx,
                        language,
                        |app, value, window, cx| app.set_terminal_shell(value, window, cx),
                    ),
                )
                .into_any_element(),
                settings_row(
                    settings_text(
                        language,
                        "settings.terminal_copy_on_select",
                        "Copy on Select",
                    ),
                    Some(settings_text(
                        language,
                        "settings.terminal_copy_on_select.help",
                        "Copy terminal text once after a mouse selection is completed.",
                    )),
                    settings_toggle(
                        "settings-terminal-copy-on-select",
                        settings.terminal_copy_on_select,
                        cx,
                        |app, window, cx| app.toggle_terminal_copy_on_select(window, cx),
                    ),
                )
                .into_any_element(),
                settings_row(
                    settings_text(
                        language,
                        "settings.terminal_right_click_paste",
                        "Right-click to Paste",
                    ),
                    Some(settings_text(
                        language,
                        "settings.terminal_right_click_paste.help",
                        "Paste with an unmodified right click. Use Shift+right-click to open the context menu.",
                    )),
                    settings_toggle(
                        "settings-terminal-right-click-paste",
                        settings.terminal_right_click_paste,
                        cx,
                        |app, window, cx| app.toggle_terminal_right_click_paste(window, cx),
                    ),
                )
                .into_any_element(),
                settings_row(
                    settings_text(
                        language,
                        "settings.terminal_trim_trailing_whitespace_on_copy",
                        "Trim Trailing Whitespace on Copy",
                    ),
                    Some(settings_text(
                        language,
                        "settings.terminal_trim_trailing_whitespace_on_copy.help",
                        "Remove spaces and tabs at hard line endings when copying a terminal selection.",
                    )),
                    settings_toggle(
                        "settings-terminal-trim-trailing-whitespace-on-copy",
                        settings.terminal_trim_trailing_whitespace_on_copy,
                        cx,
                        |app, window, cx| {
                            app.toggle_terminal_trim_trailing_whitespace_on_copy(window, cx)
                        },
                    ),
                )
                .into_any_element(),
                settings_row(
                    settings_text(
                        language,
                        "settings.terminal_trim_trailing_whitespace_on_paste",
                        "Trim Trailing Whitespace on Paste",
                    ),
                    Some(settings_text(
                        language,
                        "settings.terminal_trim_trailing_whitespace_on_paste.help",
                        "Remove spaces and tabs at line endings from plain-text pastes. File paths are unchanged.",
                    )),
                    settings_toggle(
                        "settings-terminal-trim-trailing-whitespace-on-paste",
                        settings.terminal_trim_trailing_whitespace_on_paste,
                        cx,
                        |app, window, cx| {
                            app.toggle_terminal_trim_trailing_whitespace_on_paste(window, cx)
                        },
                    ),
                )
                .into_any_element(),
                settings_row(
                    settings_text(
                        language,
                        "settings.terminal_paste_images_as_paths",
                        "Paste Images as Paths",
                    ),
                    Some(settings_text(
                        language,
                        "settings.terminal_paste_images_as_paths.help",
                        "When pasting an image into a terminal, save it to a temporary file and paste the local path instead of image data.",
                    )),
                    settings_toggle(
                        "settings-terminal-paste-images-as-paths",
                        settings.terminal_paste_images_as_paths,
                        cx,
                        |app, window, cx| app.toggle_terminal_paste_images_as_paths(window, cx),
                    ),
                )
                .into_any_element(),
            ],
            cx,)
        .into_any_element(),
        settings_card(
            Some(settings_text(language, "settings.update.section", "Updates")),
            Some(settings_text(
                language,
                "settings.update.description",
                "Updates are checked from the selected GitHub Release channel.",
            )),
            vec![
                settings_row(
                    settings_text(language, "settings.update.enabled", "Enable Update Checks"),
                    None,
                    settings_toggle(
                        "settings-update-enabled",
                        settings.update_enabled,
                        cx,
                        |app, window, cx| app.toggle_update_enabled(window, cx),
                    ),
                )
                .into_any_element(),
                settings_row(
                    settings_text(language, "settings.update.channel", "Update Channel"),
                    None,
                    settings_select_impl(
                        "settings-update-channel",
                        &settings.update_channel,
                        update_channel_options(language),
                        window,
                        cx,
                        language,
                        |app, value, window, cx| app.set_update_channel(value, window, cx),
                    ),
                )
                .into_any_element(),
                settings_row(
                    settings_text(language, "settings.update.status", "Update Status"),
                    Some(update_status_text(update, language)),
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(settings_small_button_state(
                            "settings-check-update",
                            settings_text(language, "about.updates", "Check for Updates"),
                            false,
                            !settings.update_enabled,
                            cx,
                            |app, _event, window, cx| app.open_update_dialog_window(window, cx),
                        ))
                        .into_any_element(),
                )
                .into_any_element(),
            ],
            cx,)
        .into_any_element(),
    ])
    .into_any_element()
}

pub(super) fn update_status_text(update: &UpdateSummary, language: &str) -> String {
    if let Some(error) = &update.error {
        return format!(
            "{}: {error}",
            settings_text(
                language,
                "settings.update.status.error",
                "Update check failed"
            )
        );
    }
    if let Some(version) = &update.latest_version {
        if !update.available {
            return settings_text(
                language,
                "settings.update.status.latest_format",
                "Current version %@ is up to date.",
            )
            .replace("%@", env!("CARGO_PKG_VERSION"));
        }
        let notes = update.notes_preview.trim();
        let available = settings_text(
            language,
            "settings.update.status.available_format",
            "Version %@ is available. Current version: %@.",
        )
        .replacen("%@", version, 1)
        .replacen("%@", env!("CARGO_PKG_VERSION"), 1);
        if notes.is_empty() {
            return available;
        }
        return format!("{available} · {notes}");
    }
    if update.enabled {
        String::new()
    } else {
        settings_text(
            language,
            "settings.update.status.disabled",
            "Update checks are turned off.",
        )
    }
}
