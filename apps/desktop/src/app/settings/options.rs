use super::*;

#[derive(Clone, Copy)]
pub(super) struct TerminalThemePreview {
    pub(super) background: u32,
    pub(super) foreground: u32,
    pub(super) muted_foreground: u32,
    pub(super) selection: u32,
}

#[derive(Clone, Copy)]
pub(super) struct ThemeColorValue {
    pub(super) label: &'static str,
    pub(super) color: u32,
}

#[derive(Clone, Copy)]
pub(super) struct IconStyleValue {
    pub(super) value: &'static str,
    pub(super) label_key: &'static str,
    pub(super) fallback: &'static str,
}

pub(super) fn opt(value: &'static str, label: &'static str) -> (String, SharedString) {
    (value.to_string(), SharedString::from(label))
}

pub(super) fn language_options(language: &str) -> Vec<(String, SharedString)> {
    vec![
        (
            "system",
            settings_text(language, "settings.language.follow_system", "Follow System"),
        ),
        ("simplifiedChinese", "简体中文".to_string()),
        ("traditionalChinese", "繁體中文".to_string()),
        ("english", "English".to_string()),
        ("japanese", "日本語".to_string()),
        ("korean", "한국어".to_string()),
        ("french", "Français".to_string()),
        ("german", "Deutsch".to_string()),
        ("spanish", "Español".to_string()),
        ("portugueseBrazil", "Português (Brasil)".to_string()),
        ("russian", "Русский".to_string()),
    ]
    .into_iter()
    .map(|(value, label)| (value.to_string(), SharedString::from(label)))
    .collect()
}

pub(super) fn sleep_mode_options(language: &str) -> Vec<(String, SharedString)> {
    vec![
        (
            "off",
            settings_text(language, "settings.sleep_prevention.mode.off", "Off"),
        ),
        (
            "always",
            settings_text(language, "settings.sleep_prevention.mode.always", "Always"),
        ),
        (
            "powerAdapterOnly",
            settings_text(
                language,
                "settings.sleep_prevention.mode.power_adapter_only",
                "On Power Only",
            ),
        ),
    ]
    .into_iter()
    .map(|(value, label)| (value.to_string(), SharedString::from(label)))
    .collect()
}

pub(super) fn git_refresh_options() -> Vec<(String, SharedString)> {
    interval_options(&[
        ("30", "30 sec"),
        ("60", "1 min"),
        ("120", "2 min"),
        ("300", "5 min"),
        ("600", "10 min"),
    ])
}

pub(super) fn ai_refresh_options() -> Vec<(String, SharedString)> {
    interval_options(&[
        ("60", "1 min"),
        ("120", "2 min"),
        ("180", "3 min"),
        ("300", "5 min"),
        ("600", "10 min"),
    ])
}

pub(super) fn ai_background_refresh_options() -> Vec<(String, SharedString)> {
    interval_options(&[
        ("300", "5 min"),
        ("600", "10 min"),
        ("900", "15 min"),
        ("1800", "30 min"),
    ])
}

pub(super) fn developer_refresh_options() -> Vec<(String, SharedString)> {
    interval_options(&[
        ("1", "1 sec"),
        ("2", "2 sec"),
        ("3", "3 sec"),
        ("5", "5 sec"),
        ("10", "10 sec"),
    ])
}

pub(super) fn interval_options(
    options: &[(&'static str, &'static str)],
) -> Vec<(String, SharedString)> {
    options
        .iter()
        .map(|(value, label)| opt(value, label))
        .collect()
}

pub(super) fn statistics_mode_options(language: &str) -> Vec<(String, SharedString)> {
    vec![
        (
            "normalized",
            settings_text(
                language,
                "settings.ai_statistics_mode.normalized",
                "Exclude Cache",
            ),
        ),
        (
            "includingCache",
            settings_text(
                language,
                "settings.ai_statistics_mode.including_cache",
                "Include Cache",
            ),
        ),
    ]
    .into_iter()
    .map(|(value, label)| (value.to_string(), SharedString::from(label)))
    .collect()
}

pub(super) fn file_open_default_options(language: &str) -> Vec<(String, SharedString)> {
    vec![
        (
            "edit",
            settings_text(language, "settings.file_open_default.edit", "Editor"),
        ),
        (
            "preview",
            settings_text(language, "settings.file_open_default.preview", "Preview"),
        ),
        (
            "split",
            settings_text(language, "settings.file_open_default.split", "Split"),
        ),
    ]
    .into_iter()
    .map(|(value, label)| (value.to_string(), SharedString::from(label)))
    .collect()
}

pub(super) fn update_channel_options(language: &str) -> Vec<(String, SharedString)> {
    vec![
        (
            "stable",
            settings_text(language, "settings.update.channel.stable", "Stable"),
        ),
        (
            "beta",
            settings_text(language, "settings.update.channel.beta", "Beta"),
        ),
    ]
    .into_iter()
    .map(|(value, label)| (value.to_string(), SharedString::from(label)))
    .collect()
}

pub(super) fn system_theme_options() -> Vec<(&'static str, &'static str)> {
    vec![("Auto", "Follow System")]
}

pub(super) fn dark_theme_options() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Codux Dark", "Codux Dark"),
        ("Deep Ocean", "Deep Ocean"),
        ("Arctic Night", "Arctic Night"),
        ("Forest Night", "Forest Night"),
        ("Ember", "Ember"),
        ("Amethyst Dusk", "Amethyst Dusk"),
        ("Rose Noir", "Rose Noir"),
        ("Carbon", "Carbon"),
    ]
}

pub(super) fn light_theme_options() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Codux Light", "Codux Light"),
        ("Glacier", "Glacier"),
        ("Morning Mist", "Morning Mist"),
        ("Matcha", "Matcha"),
        ("Ivory", "Ivory"),
        ("Lavender", "Lavender"),
        ("Rosewater", "Rosewater"),
        ("Sandstone", "Sandstone"),
    ]
}

pub(super) fn terminal_theme_preview(value: &str) -> TerminalThemePreview {
    let palette = theme::terminal_theme_palette(value);
    TerminalThemePreview {
        background: palette.background,
        foreground: palette.foreground,
        muted_foreground: palette.muted_foreground,
        selection: palette.selection,
    }
}

pub(super) fn theme_color_values() -> Vec<ThemeColorValue> {
    vec![
        ThemeColorValue {
            label: "Blue",
            color: 0x3B82F6,
        },
        ThemeColorValue {
            label: "Sky",
            color: 0x0EA5E9,
        },
        ThemeColorValue {
            label: "Cyan",
            color: 0x06B6D4,
        },
        ThemeColorValue {
            label: "Teal",
            color: 0x14B8A6,
        },
        ThemeColorValue {
            label: "Emerald",
            color: 0x10B981,
        },
        ThemeColorValue {
            label: "Green",
            color: 0x22C55E,
        },
        ThemeColorValue {
            label: "Lime",
            color: 0x84CC16,
        },
        ThemeColorValue {
            label: "Amber",
            color: 0xF59E0B,
        },
        ThemeColorValue {
            label: "Orange",
            color: 0xF97316,
        },
        ThemeColorValue {
            label: "Red",
            color: 0xEF4444,
        },
        ThemeColorValue {
            label: "Rose",
            color: 0xF43F5E,
        },
        ThemeColorValue {
            label: "Pink",
            color: 0xEC4899,
        },
        ThemeColorValue {
            label: "Fuchsia",
            color: 0xD946EF,
        },
        ThemeColorValue {
            label: "Purple",
            color: 0xA855F7,
        },
        ThemeColorValue {
            label: "Violet",
            color: 0x8B5CF6,
        },
        ThemeColorValue {
            label: "Indigo",
            color: 0x6366F1,
        },
    ]
}

pub(super) fn icon_style_values() -> Vec<IconStyleValue> {
    vec![
        IconStyleValue {
            value: "default",
            label_key: "settings.app_icon.option.default",
            fallback: "Default",
        },
        IconStyleValue {
            value: "cobalt",
            label_key: "settings.app_icon.option.cobalt",
            fallback: "Cobalt",
        },
        IconStyleValue {
            value: "sunset",
            label_key: "settings.app_icon.option.sunset",
            fallback: "Sunset",
        },
        IconStyleValue {
            value: "forest",
            label_key: "settings.app_icon.option.forest",
            fallback: "Forest",
        },
    ]
}

pub(super) fn app_icon_asset_path(style: &str) -> &'static str {
    match style {
        "cobalt" => "app-icons/codux-cobalt.svg",
        "sunset" => "app-icons/codux-sunset.svg",
        "forest" => "app-icons/codux-forest.svg",
        _ => "app-icons/codux-default.svg",
    }
}

pub(super) fn terminal_scrollback_options(language: &str) -> Vec<(String, SharedString)> {
    ["500", "1000", "2000", "5000", "10000"]
        .into_iter()
        .map(|value| {
            let label = settings_text(
                language,
                "settings.terminal_scrollback.option_format",
                "%@ lines",
            )
            .replace("%@", value);
            (value.to_string(), SharedString::from(label))
        })
        .collect()
}

pub(super) fn terminal_layout_mode_options(language: &str) -> Vec<(String, SharedString)> {
    vec![
        (
            "split",
            settings_text(language, "settings.terminal_layout_mode.split", "Split"),
        ),
        (
            "tabs",
            settings_text(language, "settings.terminal_layout_mode.tabs", "Tabs"),
        ),
    ]
    .into_iter()
    .map(|(value, label)| (value.to_string(), SharedString::from(label)))
    .collect()
}

pub(super) fn terminal_font_family_options(
    language: &str,
    selected: &str,
    families: &[String],
) -> Vec<(String, SharedString)> {
    let mut options = vec![(
        String::new(),
        SharedString::from(settings_text(
            language,
            "settings.terminal_font_family.default",
            "System Default",
        )),
    )];
    let selected = selected.trim();
    for family in families {
        let family = family.trim();
        if family.is_empty() {
            continue;
        }
        options.push((family.to_string(), SharedString::from(family.to_string())));
    }
    if !selected.is_empty()
        && !options
            .iter()
            .any(|(value, _)| value.eq_ignore_ascii_case(selected))
    {
        options.push((
            selected.to_string(),
            SharedString::from(selected.to_string()),
        ));
    }
    options
}

pub(super) fn terminal_shell_options(
    language: &str,
    selected: &str,
) -> Vec<(String, SharedString)> {
    let mut options = vec![(
        String::new(),
        SharedString::from(settings_text(
            language,
            "settings.terminal_shell.default",
            "System Default",
        )),
    )];
    for (value, label) in detected_terminal_shells() {
        options.push((value, SharedString::from(label)));
    }
    let selected = selected.trim();
    if !selected.is_empty() && !options.iter().any(|(value, _)| value == selected) {
        options.push((
            selected.to_string(),
            SharedString::from(selected.to_string()),
        ));
    }
    options
}

/// Installed shells offered in the picker; the value is the absolute path handed to the PTY.
fn detected_terminal_shells() -> Vec<(String, String)> {
    let mut shells: Vec<(String, String)> = Vec::new();
    let mut push = |path: std::path::PathBuf, label: &str| {
        if path.is_file() && !shells.iter().any(|(_, existing)| existing == label) {
            shells.push((path.display().to_string(), label.to_string()));
        }
    };
    #[cfg(target_os = "windows")]
    {
        for base in ["ProgramFiles", "ProgramW6432"] {
            if let Some(dir) = std::env::var_os(base) {
                push(
                    std::path::Path::new(&dir)
                        .join("PowerShell")
                        .join("7")
                        .join("pwsh.exe"),
                    "PowerShell 7 (pwsh)",
                );
                push(
                    std::path::Path::new(&dir)
                        .join("Git")
                        .join("bin")
                        .join("bash.exe"),
                    "Git Bash",
                );
            }
        }
        // Per-user installs (no admin) land under LOCALAPPDATA\Programs.
        if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
            let programs = std::path::Path::new(&dir).join("Programs");
            push(
                programs.join("PowerShell").join("7").join("pwsh.exe"),
                "PowerShell 7 (pwsh)",
            );
            push(
                programs.join("Git").join("bin").join("bash.exe"),
                "Git Bash",
            );
        }
        // Store/scoop installs put pwsh.exe on PATH instead of Program Files.
        if let Some(path_var) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path_var) {
                push(dir.join("pwsh.exe"), "PowerShell 7 (pwsh)");
            }
        }
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            let system32 = std::path::Path::new(&system_root).join("System32");
            push(
                system32
                    .join("WindowsPowerShell")
                    .join("v1.0")
                    .join("powershell.exe"),
                "Windows PowerShell",
            );
            push(system32.join("cmd.exe"), "Command Prompt");
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        for (path, label) in [
            ("/bin/zsh", "zsh"),
            ("/usr/bin/zsh", "zsh"),
            ("/bin/bash", "bash"),
            ("/usr/bin/bash", "bash"),
            ("/opt/homebrew/bin/fish", "fish"),
            ("/usr/local/bin/fish", "fish"),
            ("/usr/bin/fish", "fish"),
            ("/opt/homebrew/bin/nu", "nushell"),
            ("/usr/local/bin/nu", "nushell"),
            ("/usr/bin/nu", "nushell"),
        ] {
            push(std::path::PathBuf::from(path), label);
        }
    }
    shells
}

pub(super) fn terminal_font_size_options() -> Vec<(String, SharedString)> {
    (8..=28)
        .map(|value| {
            let value = value.to_string();
            (value.clone(), SharedString::from(value))
        })
        .collect()
}

pub(super) fn terminal_padding_options() -> Vec<(String, SharedString)> {
    (0..=40)
        .step_by(2)
        .map(|value| {
            let value = value.to_string();
            (value.clone(), SharedString::from(value))
        })
        .collect()
}

pub(super) fn terminal_line_height_options() -> Vec<(String, SharedString)> {
    ["1", "1.1", "1.2", "1.3", "1.45", "1.6", "1.8", "2"]
        .into_iter()
        .map(|value| (value.to_string(), SharedString::from(value)))
        .collect()
}

pub(super) fn pet_speech_mode_options(language: &str) -> Vec<(String, SharedString)> {
    ["mixed", "off", "encourage", "roast", "flirty", "chuunibyou"]
        .into_iter()
        .map(|value| {
            (
                value.to_string(),
                SharedString::from(settings_text(
                    language,
                    &format!("pet.speech.mode.{value}"),
                    value,
                )),
            )
        })
        .collect()
}

pub(super) fn pet_speech_frequency_options(language: &str) -> Vec<(String, SharedString)> {
    ["quiet", "normal", "lively", "chatterbox"]
        .into_iter()
        .map(|value| {
            (
                value.to_string(),
                SharedString::from(pet_speech_frequency_option_label(language, value)),
            )
        })
        .collect()
}

pub(super) fn pet_speech_frequency_option_label(language: &str, value: &str) -> String {
    let (hourly, cooldown_seconds) = pet_speech_frequency_config(value);
    let cooldown = pet_speech_cooldown_label(language, cooldown_seconds);
    settings_text(
        language,
        "settings.pet.speech.frequency_option_format",
        "%@ · %@/hour · cooldown %@",
    )
    .replacen(
        "%@",
        &settings_text(language, &format!("pet.speech.frequency.{value}"), value),
        1,
    )
    .replacen("%@", hourly, 1)
    .replacen("%@", &cooldown, 1)
}

pub(super) fn pet_speech_frequency_config(value: &str) -> (&'static str, u32) {
    match value {
        "quiet" => ("0-1", 300),
        "lively" => ("3-8", 30),
        "chatterbox" => ("8-15", 30),
        _ => ("1-3", 60),
    }
}

pub(super) fn pet_speech_cooldown_label(language: &str, seconds: u32) -> String {
    if seconds >= 60 {
        settings_text(
            language,
            "settings.pet.speech.cooldown.minutes_format",
            "%d min",
        )
        .replace("%d", &(seconds / 60).to_string())
    } else {
        settings_text(
            language,
            "settings.pet.speech.cooldown.seconds_format",
            "%d sec",
        )
        .replace("%d", &seconds.to_string())
    }
}

pub(super) fn pet_reminder_interval_options(language: &str) -> Vec<(String, SharedString)> {
    ["15", "30", "45", "60", "90", "120", "180", "240"]
        .into_iter()
        .map(|value| {
            let label = settings_text(language, "settings.interval.minutes_format", "%@ min")
                .replace("%@", value);
            (value.to_string(), SharedString::from(label))
        })
        .collect()
}

pub(super) fn runtime_tool_permission_options(language: &str) -> Vec<(String, SharedString)> {
    vec![
        (
            "default",
            settings_text(language, "settings.tools.permission.default", "Default"),
        ),
        (
            "fullAccess",
            settings_text(
                language,
                "settings.tools.permission.full_access",
                "Full Access",
            ),
        ),
    ]
    .into_iter()
    .map(|(value, label)| (value.to_string(), SharedString::from(label)))
    .collect()
}

pub(super) fn remote_relay_preset_options(language: &str) -> Vec<(String, SharedString)> {
    let _ = language;
    codux_runtime::remote::remote_relay_presets()
        .iter()
        .map(|preset| (preset.key.clone(), SharedString::from(preset.name.clone())))
        .collect()
}
pub(super) fn codex_effort_options(language: &str) -> Vec<(String, SharedString)> {
    vec![
        (
            "none".to_string(),
            SharedString::from(settings_text(
                language,
                "settings.ai.tool.reasoning_effort.default",
                "Default",
            )),
        ),
        (
            "minimal".to_string(),
            SharedString::from(settings_text(
                language,
                "settings.ai.tool.reasoning_effort.minimal",
                "Minimal",
            )),
        ),
        (
            "low".to_string(),
            SharedString::from(settings_text(
                language,
                "settings.ai.tool.reasoning_effort.low",
                "Low",
            )),
        ),
        (
            "medium".to_string(),
            SharedString::from(settings_text(
                language,
                "settings.ai.tool.reasoning_effort.medium",
                "Medium",
            )),
        ),
        (
            "high".to_string(),
            SharedString::from(settings_text(
                language,
                "settings.ai.tool.reasoning_effort.high",
                "High",
            )),
        ),
        (
            "xhigh".to_string(),
            SharedString::from(settings_text(
                language,
                "settings.ai.tool.reasoning_effort.xhigh",
                "XHigh",
            )),
        ),
    ]
}

pub(super) fn git_provider_options(
    settings: &SettingsSummary,
    language: &str,
) -> Vec<(String, SharedString)> {
    let mut options = vec![
        (
            "automatic".to_string(),
            SharedString::from(settings_text(
                language,
                "settings.ai.git_commit_message_provider.automatic",
                "Automatic",
            )),
        ),
        (
            "off".to_string(),
            SharedString::from(settings_text(
                language,
                "settings.ai.git_commit_message_provider.off",
                "Off",
            )),
        ),
    ];
    options.extend(
        settings
            .ai_providers
            .iter()
            .filter(|provider| provider.enabled && provider.kind != "localLlama")
            .map(|provider| {
                (
                    provider.id.clone(),
                    SharedString::from(provider.display_name.clone()),
                )
            }),
    );
    options
}

pub(super) fn git_tone_options() -> Vec<(String, SharedString)> {
    vec![
        ("conventional", "Conventional Commits"),
        ("concise", "Concise"),
        ("sentence", "Sentence"),
        ("changelog", "Changelog"),
    ]
    .into_iter()
    .map(|(value, label)| opt(value, label))
    .collect()
}

pub(super) fn git_language_options(language: &str) -> Vec<(String, SharedString)> {
    let mut options = vec![(
        "application".to_string(),
        SharedString::from(settings_text(
            language,
            "settings.ai.git_commit_message_language.follow",
            "Follow App",
        )),
    )];
    options.extend(
        language_options(language)
            .into_iter()
            .filter(|(value, _)| value != "system"),
    );
    options
}

pub(super) fn ai_provider_kind_options() -> Vec<(String, SharedString)> {
    vec![
        ("openai", "OpenAI"),
        ("openAICompatible", "OpenAI-Compatible API"),
        ("anthropic", "Claude API"),
        ("deepseek", "DeepSeek"),
        ("gemini", "Gemini"),
        ("groq", "Groq"),
        ("openrouter", "OpenRouter"),
        ("ollama", "Ollama"),
    ]
    .into_iter()
    .map(|(value, label)| opt(value, label))
    .collect()
}

pub(super) fn ai_provider_options(
    settings: &SettingsSummary,
    purpose: &str,
    language: &str,
) -> Vec<(String, SharedString)> {
    let mut providers = settings
        .ai_providers
        .iter()
        .filter(|provider| {
            provider.enabled
                && provider.kind != "localLlama"
                && (purpose != "memory" || provider.memory_extraction)
        })
        .cloned()
        .collect::<Vec<_>>();
    providers.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.display_name.cmp(&right.display_name))
    });

    let mut options = vec![(
        "automatic".to_string(),
        SharedString::from(settings_text(
            language,
            "settings.ai.memory.extraction_provider.automatic",
            "Automatic",
        )),
    )];
    options.extend(
        providers
            .into_iter()
            .map(|provider| (provider.id, SharedString::from(provider.display_name))),
    );
    options
}

pub(super) fn provider_allows_empty_api_key(kind: &str) -> bool {
    matches!(kind, "ollama" | "localLlama")
}

pub(super) fn memory_extraction_interval_options() -> Vec<(String, SharedString)> {
    interval_options(&[
        ("60", "1 min"),
        ("120", "2 min"),
        ("300", "5 min"),
        ("600", "10 min"),
        ("900", "15 min"),
    ])
}

pub(super) fn memory_max_index_options(language: &str) -> Vec<(String, SharedString)> {
    ["5", "10", "20", "50", "100"]
        .into_iter()
        .map(|value| {
            let label = settings_text(
                language,
                "settings.ai.memory.max_index_sessions.option_format",
                "%@ sessions",
            )
            .replace("%@", value);
            (value.to_string(), SharedString::from(label))
        })
        .collect()
}

pub(super) fn notification_endpoint_label(channel_id: &str, language: &str) -> String {
    let fallback = match channel_id {
        "bark" => "Server URL",
        "ntfy" => "Topic URL",
        "wxpusher" => "SPT Token",
        "telegram" => "Chat ID",
        "webhook" => "Request URL",
        _ => "Webhook URL",
    };
    settings_text(
        language,
        &format!("settings.notifications.channel.{channel_id}.endpoint"),
        fallback,
    )
}

pub(super) fn notification_token_label(channel_id: &str, language: &str) -> String {
    let fallback = match channel_id {
        "bark" => "Device Key",
        "ntfy" => "Bearer Token",
        "wxpusher" => "Token",
        "feishu" => "Hook Token",
        "dingtalk" => "Access Token",
        "wecom" => "Webhook Key",
        "telegram" => "Bot Token",
        "discord" | "slack" => "Optional Auth Token",
        "webhook" => "Bearer Token",
        _ => "Token",
    };
    settings_text(
        language,
        &format!("settings.notifications.channel.{channel_id}.token"),
        fallback,
    )
}

pub(super) fn notification_channel_description(channel_id: &str, language: &str) -> String {
    let fallback = match channel_id {
        "bark" => "Send push notifications through Bark service and device key.",
        "ntfy" => "Publish notifications to an ntfy topic.",
        "wxpusher" => "Send notifications to a WxPusher SPT target.",
        "feishu" => "Send notifications through a Feishu bot webhook.",
        "dingtalk" => "Send notifications through a DingTalk bot webhook.",
        "wecom" => "Send notifications to a WeCom group bot.",
        "telegram" => "Send notifications with a Telegram bot token and chat id.",
        "discord" => "Send notifications to a Discord webhook.",
        "slack" => "Send notifications to a Slack incoming webhook.",
        "webhook" => "Send a JSON POST request to a custom endpoint.",
        _ => "Custom notification channel.",
    };
    settings_text(
        language,
        &format!("settings.notifications.channel.{channel_id}.description"),
        fallback,
    )
}

pub(super) fn remote_status_label(remote: &RemoteSummary, language: &str) -> String {
    match remote.status.as_str() {
        "connected" => settings_text(language, "remote.status.connected_label", "Connected"),
        "connecting" => settings_text(language, "remote.status.connecting_label", "Connecting"),
        "failed" => settings_text(language, "settings.ai.local_model.status.failed", "Failed"),
        _ => settings_text(language, "remote.status.disconnected_label", "Disconnected"),
    }
}

pub(super) fn remote_status_color(remote: &RemoteSummary) -> u32 {
    match remote.status.as_str() {
        "connected" => theme::GREEN,
        "connecting" => theme::ORANGE,
        "failed" => theme::RED,
        _ => theme::TEXT_DIM,
    }
}
