pub(super) fn default_language() -> String {
    "system".to_string()
}

pub(super) fn default_true() -> bool {
    true
}

pub(super) fn default_pet_speech_mode() -> String {
    "mixed".to_string()
}

pub(super) fn default_pet_speech_frequency() -> String {
    "normal".to_string()
}

pub(super) fn default_pet_hydration_reminder_minutes() -> String {
    "60".to_string()
}

pub(super) fn default_pet_sedentary_reminder_minutes() -> String {
    "60".to_string()
}

pub(super) fn default_pet_late_night_reminder_minutes() -> String {
    "60".to_string()
}

pub(super) fn default_ai_pet_speech_mode() -> String {
    "off".to_string()
}

pub(super) fn default_ai_pet_speech_frequency() -> String {
    "normal".to_string()
}

pub(super) fn default_ai_memory_provider_id() -> String {
    "automatic".to_string()
}

pub(super) fn default_ai_pet_provider_id() -> String {
    "automatic".to_string()
}

pub(super) fn default_git_commit_message_provider_id() -> String {
    "automatic".to_string()
}

pub(super) fn default_git_commit_message_tone() -> String {
    "conventional".to_string()
}

pub(super) fn default_git_commit_message_language() -> String {
    "application".to_string()
}

pub(super) fn default_ai_tool_permission_mode() -> String {
    "default".to_string()
}

pub(super) fn default_codex_effort() -> String {
    "none".to_string()
}

pub(super) fn default_memory_user_recall() -> i32 {
    4
}

pub(super) fn default_memory_project_recall() -> i32 {
    6
}

pub(super) fn default_memory_max_active_working_entries() -> i32 {
    50
}

pub(super) fn default_memory_max_summary_versions() -> i32 {
    10
}

pub(super) fn default_memory_summary_target_token_budget() -> i32 {
    900
}

pub(super) fn default_memory_max_injected_summary_tokens() -> i32 {
    900
}

pub(super) fn default_memory_extraction_idle_delay_seconds() -> i32 {
    300
}

pub(super) fn default_memory_session_extraction_cooldown_seconds() -> i32 {
    900
}

pub(super) fn default_memory_extraction_growth_threshold_lines() -> i32 {
    8
}

pub(super) fn default_memory_max_index_sessions() -> i32 {
    20
}

pub(super) fn default_memory_max_extraction_transcript_lines() -> i32 {
    80
}

pub(super) fn default_memory_max_extraction_transcript_tokens() -> i32 {
    8000
}

pub(super) fn default_sleep_mode() -> String {
    "off".to_string()
}

pub(super) fn default_git_refresh() -> String {
    "60".to_string()
}

pub(super) fn default_ai_refresh() -> String {
    "180".to_string()
}

pub(super) fn default_ai_background_refresh() -> String {
    "600".to_string()
}

pub(super) fn default_statistics_mode() -> String {
    "normalized".to_string()
}

pub(super) fn default_file_open_default() -> String {
    "edit".to_string()
}

pub(super) fn default_theme() -> String {
    "Auto".to_string()
}

pub(super) fn default_theme_color() -> String {
    "Blue".to_string()
}

pub(super) fn default_terminal_font_size() -> String {
    "14".to_string()
}

pub(super) fn default_terminal_scrollback_lines() -> String {
    "2000".to_string()
}

pub(super) fn default_terminal_layout_mode() -> String {
    "split".to_string()
}

pub(super) fn default_terminal_paste_images_as_paths() -> bool {
    true
}

pub(super) fn default_icon_style() -> String {
    "default".to_string()
}

pub(super) fn default_window_style() -> String {
    "transparent".to_string()
}

pub(super) fn default_window_opacity() -> String {
    "80".to_string()
}

pub(super) fn default_developer_refresh() -> String {
    "3".to_string()
}

pub(super) fn default_update_channel() -> &'static str {
    release_channel_for_version(env!("CARGO_PKG_VERSION"))
}

const DEFAULT_STABLE_UPDATE_ENDPOINT: &str =
    "https://github.com/duxweb/codux/releases/latest/download/latest.json";
const DEFAULT_BETA_UPDATE_ENDPOINT: &str =
    "https://github.com/duxweb/codux/releases/download/beta/latest.json";

// Mirrors automaticReleaseChannel in prepare-release.mjs: rc prereleases
// publish to the stable channel, other prereleases to beta.
pub(super) fn release_channel_for_version(version: &str) -> &'static str {
    match version.split_once('-') {
        Some((_, prerelease))
            if prerelease != "rc"
                && !prerelease.starts_with("rc.")
                && !prerelease.starts_with("rc-")
                && prerelease != "custom"
                && !prerelease.starts_with("custom.")
                && !prerelease.starts_with("custom-") =>
        {
            "beta"
        }
        _ => "stable",
    }
}

pub(crate) fn update_endpoint_for_channel(channel: &str) -> String {
    // Private distributions inject their endpoints at build time so deployment
    // topology never needs to be committed to the public source tree.
    let configured = match channel {
        "beta" => option_env!("CODUX_BETA_UPDATE_ENDPOINT"),
        _ => option_env!("CODUX_STABLE_UPDATE_ENDPOINT"),
    };
    configured
        .map(str::trim)
        .filter(|endpoint| !endpoint.is_empty())
        .unwrap_or(match channel {
            "beta" => DEFAULT_BETA_UPDATE_ENDPOINT,
            _ => DEFAULT_STABLE_UPDATE_ENDPOINT,
        })
        .to_string()
}

pub(crate) fn is_managed_update_endpoint(endpoint: &str) -> bool {
    endpoint == update_endpoint_for_channel("stable")
        || endpoint == update_endpoint_for_channel("beta")
        || matches!(
            endpoint,
            "https://raw.githubusercontent.com/duxweb/codux/main/updates/stable/latest.json"
                | "https://raw.githubusercontent.com/duxweb/codux/main/updates/beta/latest.json"
        )
}

#[cfg(test)]
mod tests {
    use super::release_channel_for_version;

    #[test]
    fn rc_builds_default_to_the_stable_update_channel() {
        assert_eq!(release_channel_for_version("2.0.0"), "stable");
        assert_eq!(release_channel_for_version("2.0.0-rc.4"), "stable");
        assert_eq!(release_channel_for_version("2.0.0-rc"), "stable");
        assert_eq!(release_channel_for_version("2.0.0-beta.11"), "beta");
        assert_eq!(release_channel_for_version("2.0.0-alpha.1"), "beta");
        assert_eq!(release_channel_for_version("2.0.4-custom.1"), "stable");
    }
}
