use super::*;

pub fn app_about_metadata(
    version: impl Into<String>,
    identifier: impl Into<String>,
) -> AppAboutMetadata {
    crate::app_info::about_metadata(version, identifier)
}
pub fn app_update_status(
    settings: &AppSettings,
    repo_root: PathBuf,
    current_version: &str,
) -> Result<UpdateStatus, String> {
    Ok(crate::app_info::update_status(
        settings,
        repo_root,
        current_version,
    ))
}
pub fn app_update_install(
    settings: &AppSettings,
    repo_root: PathBuf,
    current_version: &str,
) -> Result<UpdateInstallResult, String> {
    crate::app_info::install_update(settings, repo_root, current_version)
}
pub fn diagnostics_export(
    request: DiagnosticsExportRequest,
    about: AppAboutMetadata,
    update: UpdateStatus,
    snapshot: AppDiagnosticsSnapshot,
) -> Result<DiagnosticsExportResult, String> {
    crate::app_info::export_diagnostics(request, about, update, snapshot)
}
pub fn app_open_runtime_log() -> Result<(), String> {
    crate::app_info::open_runtime_log()
}
pub fn app_open_live_log() -> Result<(), String> {
    crate::app_info::open_live_log()
}
pub fn app_open_url(url: String) -> Result<(), String> {
    crate::app_info::open_url(&url)
}
pub fn app_reveal_path(path: String) -> Result<(), String> {
    crate::files::reveal_absolute_path(&path)
}
pub fn app_request_restart() -> Result<(), String> {
    crate::app_info::request_restart()
}
pub fn app_toggle_devtools() -> bool {
    cfg!(debug_assertions)
}
pub fn app_window_close() -> bool {
    true
}
pub fn app_runtime_ready(
    service: &RuntimeService,
    visible: bool,
    focused: bool,
) -> AppRuntimeReadySnapshot {
    service.app_runtime_ready(visible, focused)
}
pub fn app_window_state(
    service: &RuntimeService,
    visible: bool,
    focused: bool,
) -> RuntimeWindowStateSnapshot {
    service.app_window_state(visible, focused)
}
pub fn runtime_trace_frontend(service: &RuntimeService, category: String, message: String) {
    service.runtime_trace_frontend(&category, &message);
}
pub fn localized_open_dialog(
    request: crate::dialog::LocalizedOpenDialogRequest,
) -> Result<Option<Vec<String>>, String> {
    crate::dialog::localized_open_dialog(request)
}
pub fn localized_save_dialog(
    request: crate::dialog::LocalizedSaveDialogRequest,
) -> Result<Option<String>, String> {
    crate::dialog::localized_save_dialog(request)
}
pub fn app_settings_get(store: &AppSettingsStore) -> AppSettings {
    store.snapshot()
}
pub fn app_settings_set(
    store: &AppSettingsStore,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    let next = store.replace(settings)?;
    sync_process_locale_preference(&next);
    Ok(next)
}
pub fn i18n_bundle_get() -> crate::i18n::I18nBundle {
    crate::i18n::i18n_bundle()
}
pub fn performance_snapshot(monitor: &PerformanceMonitor) -> PerformanceSnapshot {
    monitor.snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn app_command_names_delegate_to_runtime_system_logic() {
        let mut settings = AppSettings::default();
        settings.update.enabled = false;
        let status = app_update_status(&settings, PathBuf::new(), "1.2.3").expect("update status");
        assert_eq!(status.current_version, "1.2.3");
        assert_eq!(status.installation_mode, "disabled");

        let about = app_about_metadata("1.2.3", "com.duxweb.codux.test");
        assert_eq!(about.version, "1.2.3");
        assert_eq!(about.identifier, "com.duxweb.codux.test");

        let manager = PowerManager::default();
        assert!(!power_set_sleep_prevention(&manager, "off".to_string()).expect("power off"));
        assert!(app_window_close());
    }

    #[test]
    fn app_lifecycle_commands_delegate_to_runtime_service() {
        let support_dir =
            std::env::temp_dir().join(format!("codux-app-command-lifecycle-{}", Uuid::new_v4()));
        let project_dir = support_dir.join("project");
        std::fs::create_dir_all(&project_dir).expect("project dir");
        std::fs::write(
            support_dir.join("state.json"),
            serde_json::to_string_pretty(&json!({
                "projects": [
                    {
                        "id": "project-a",
                        "name": "Project A",
                        "path": project_dir.display().to_string()
                    }
                ],
                "selectedProjectId": "project-a"
            }))
            .expect("state json"),
        )
        .expect("write state");
        let service = RuntimeService::new(support_dir.clone());

        runtime_trace_frontend(
            &service,
            "test".to_string(),
            "lifecycle command".to_string(),
        );
        let ready = app_runtime_ready(&service, true, true);
        assert_eq!(ready.projects.projects.len(), 1);
        assert_eq!(
            ready.projects.selected_project_id.as_deref(),
            Some("project-a")
        );
        assert_eq!(
            ready.project_activity.active_project_id.as_deref(),
            Some("project-a")
        );

        let hidden = app_window_state(&service, false, false);
        assert!(!hidden.project_activity.visible);
        assert!(!hidden.project_activity.focused);

        let _ = std::fs::remove_dir_all(support_dir);
    }

    #[test]
    fn settings_i18n_and_performance_commands_match_tauri_facade_shape() {
        let support_dir =
            std::env::temp_dir().join(format!("codux-app-command-settings-{}", Uuid::new_v4()));
        let store = AppSettingsStore::from_support_dir(support_dir.clone());
        let mut settings = app_settings_get(&store);
        settings.language = "en".to_string();
        settings.theme = "dark".to_string();

        let saved = app_settings_set(&store, settings).expect("save settings");
        assert_eq!(saved.language, "en");
        assert_eq!(store.reload_snapshot().theme, "dark");

        let bundle = i18n_bundle_get();
        assert_eq!(bundle.source_language, "en");
        assert!(bundle.locales.iter().any(|locale| locale == "zh-Hans"));
        assert!(bundle.locales.iter().any(|locale| locale == "en"));

        let snapshot = performance_snapshot(&PerformanceMonitor::default());
        assert!(snapshot.cpu_percent >= 0.0);
        assert!(snapshot.memory_bytes >= snapshot.memory.main_bytes);

        let _ = std::fs::remove_dir_all(support_dir);
    }

    #[test]
    fn diagnostics_export_command_writes_redacted_runtime_report() {
        let destination = std::env::temp_dir().join(format!(
            "codux-app-command-diagnostics-{}.json",
            Uuid::new_v4()
        ));
        let request = DiagnosticsExportRequest {
            destination_path: destination.display().to_string(),
        };
        let about = app_about_metadata("1.0.0", "com.duxweb.codux.test");
        let update = UpdateStatus {
            current_version: "1.0.0".to_string(),
            installation_mode: "disabled".to_string(),
            message: "disabled".to_string(),
            ..Default::default()
        };
        let result = diagnostics_export(
            request,
            about,
            update,
            AppDiagnosticsSnapshot {
                settings: json!({
                    "ai": {
                        "providers": [
                            {
                                "apiKey": "secret",
                                "api_key": "secret",
                                "token": "secret"
                            }
                        ]
                    }
                }),
                projects: json!([]),
                ai_state: json!({}),
                performance: json!({}),
                ssh: json!({
                    "profiles": [
                        {
                            "password": "secret",
                            "privateKeyPath": "/tmp/key"
                        }
                    ]
                }),
            },
        )
        .expect("export diagnostics");

        assert!(result.bytes > 0);
        let content = std::fs::read_to_string(&destination).expect("diagnostics file");
        assert!(content.contains("\"apiKey\": \"******\""));
        assert!(content.contains("\"api_key\": \"******\""));
        assert!(content.contains("\"password\": \"******\""));
        assert!(content.contains("\"privateKeyPath\": \"******\""));
        let _ = std::fs::remove_file(destination);
    }
}
