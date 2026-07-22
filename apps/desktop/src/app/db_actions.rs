use super::*;
use crate::app::app_events::{ChildWindowUpdateKind, publish_child_window_update};
use codux_runtime::db::DBProfilesSnapshot;

impl CoduxApp {
    pub(in crate::app) fn db_text(&self, key: &str, fallback: &str) -> String {
        let locale = locale_from_language_setting(&self.state.settings.language);
        translate(&locale, key, fallback)
    }

    pub(super) fn reload_selected_project_db(&mut self) {
        let project_id = self
            .state
            .selected_project
            .as_ref()
            .map(|project| project.id.as_str());
        self.state.db = self
            .runtime_service
            .reload_db(self.runtime.root.clone(), project_id);
    }

    pub(super) fn normalize_selected_db_profile(&mut self) {
        let selected_still_exists = self
            .selected_db_profile_id
            .as_deref()
            .map(|id| {
                self.state
                    .db
                    .profiles
                    .iter()
                    .any(|profile| profile.id == id)
            })
            .unwrap_or(false);
        if !selected_still_exists {
            self.selected_db_profile_id = None;
        }
    }

    pub(super) fn select_db_profile(
        &mut self,
        profile_id: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(profile) = self
            .state
            .db
            .profiles
            .iter()
            .find(|profile| profile.id == profile_id)
        else {
            self.status_message = self.db_text(
                "db.profile.unavailable",
                "Database profile is no longer available",
            );
            self.normalize_selected_db_profile();
            self.invalidate_db_panel(cx);
            return;
        };
        self.selected_db_profile_id = Some(profile.id.clone());
        self.status_message = self
            .db_text(
                "db.profile.selected_format",
                "Selected database profile: %@",
            )
            .replace("%@", &profile.name);
        self.invalidate_db_panel(cx);
    }

    pub(super) fn apply_db_draft(&mut self, profile: DBConnectionProfile) {
        self.db_draft_id = Some(profile.id);
        self.db_draft_project_id = profile.project_id;
        self.db_draft_name = profile.name;
        self.db_draft_engine = profile.engine;
        self.db_draft_host = profile.host;
        self.db_draft_port = profile.port.to_string();
        self.db_draft_database = profile.database;
        self.db_draft_username = profile.username;
        self.db_draft_password = profile.password.unwrap_or_default();
        self.db_draft_ssl_mode = profile.ssl_mode;
        self.db_draft_read_only = profile.read_only;
    }

    pub(super) fn reset_db_draft_for_selected_project(&mut self) {
        let project_id = self
            .state
            .selected_project
            .as_ref()
            .map(|project| project.id.clone())
            .unwrap_or_default();
        self.db_draft_id = None;
        self.db_draft_project_id = project_id;
        self.db_draft_name.clear();
        self.db_draft_engine = "postgres".to_string();
        self.db_draft_host = "localhost".to_string();
        self.db_draft_port = "5432".to_string();
        self.db_draft_database.clear();
        self.db_draft_username.clear();
        self.db_draft_password.clear();
        self.db_draft_ssl_mode = "prefer".to_string();
        self.db_draft_read_only = true;
        self.db_test_result = None;
        self.db_saving = false;
        self.db_testing = false;
    }

    pub(super) fn set_db_draft_field(
        &mut self,
        field: &'static str,
        value: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match field {
            "name" => self.db_draft_name = value,
            "engine" => {
                self.db_draft_engine = value;
                if self.db_draft_engine == "mysql" && self.db_draft_port.trim() == "5432" {
                    self.db_draft_port = "3306".to_string();
                } else if self.db_draft_engine == "postgres" && self.db_draft_port.trim() == "3306"
                {
                    self.db_draft_port = "5432".to_string();
                }
            }
            "host" => self.db_draft_host = value,
            "port" => self.db_draft_port = value,
            "database" => self.db_draft_database = value,
            "username" => self.db_draft_username = value,
            "password" => self.db_draft_password = value,
            "sslMode" => self.db_draft_ssl_mode = value,
            _ => {}
        }
        self.db_test_result = None;
        self.invalidate_db_panel(cx);
    }

    pub(super) fn set_db_draft_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        self.db_draft_read_only = read_only;
        self.db_test_result = None;
        self.invalidate_db_panel(cx);
    }

    pub(super) fn db_draft_request(&self) -> Result<DBProfileUpsertRequest, String> {
        let port = self.db_draft_port.trim().parse::<u16>().map_err(|_| {
            self.db_text(
                "db.profile.port.invalid",
                "Database port must be a number from 1 to 65535.",
            )
        })?;
        Ok(DBProfileUpsertRequest {
            id: self.db_draft_id.clone(),
            project_id: self.db_draft_project_id.clone(),
            name: self.db_draft_name.clone(),
            engine: self.db_draft_engine.clone(),
            host: Some(self.db_draft_host.clone()),
            port: Some(port),
            database: self.db_draft_database.clone(),
            username: Some(self.db_draft_username.clone()),
            password: Some(self.db_draft_password.clone()),
            ssl_mode: Some(self.db_draft_ssl_mode.clone()),
            read_only: self.db_draft_read_only,
        })
    }

    pub(super) fn save_db_profile_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.db_saving || self.db_testing {
            return;
        }
        let request = match self.db_draft_request() {
            Ok(request) => request,
            Err(error) => {
                self.status_message = format!("failed to save database profile: {error}");
                self.invalidate_db_panel(cx);
                return;
            }
        };
        let requested_id = request.id.clone();
        let service = self.runtime_service.clone();
        let window_handle = window.window_handle();
        self.db_saving = true;
        self.status_message = self.db_text("db.profile.saving", "Saving database profile...");
        self.runtime_trace("database", "db_profile_save queued");
        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx| {
            let result = codux_runtime::async_runtime::run_limited_blocking(move || {
                let started_at = std::time::Instant::now();
                service.runtime_trace_frontend("database", "db_profile_save start");
                let result = service.upsert_db_profile(request);
                service.runtime_trace_frontend(
                    "database",
                    &format!(
                        "db_profile_save {} elapsed_ms={}",
                        if result.is_ok() { "ok" } else { "failed" },
                        started_at.elapsed().as_millis()
                    ),
                );
                result
            })
            .await
            .unwrap_or_else(|error| Err(format!("failed to join database profile save: {error}")));

            // Publish independently of the editor lifetime so Cmd+W cannot hide a completed save.
            if result.is_ok() {
                publish_child_window_update(ChildWindowUpdateKind::Database);
            }
            let _ = window_handle.update(cx, |_root, window, cx| {
                let _ = this.update(cx, |app, cx| {
                    app.apply_db_profile_save_result(result, requested_id, window, cx);
                });
            });
        })
        .detach();
        self.invalidate_db_panel(cx);
    }

    fn apply_db_profile_save_result(
        &mut self,
        result: Result<DBProfilesSnapshot, String>,
        requested_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.db_saving = false;
        match result {
            Ok(snapshot) => {
                self.selected_db_profile_id = requested_id.or_else(|| {
                    snapshot
                        .profiles
                        .iter()
                        .max_by_key(|profile| profile.updated_at)
                        .map(|profile| profile.id.clone())
                });
                self.status_message = self.db_text("db.profile.saved", "Database profile saved");
                if self.window_mode == AppWindowMode::DbProfileEditor {
                    window.remove_window();
                }
            }
            Err(error) => {
                self.status_message = format!("failed to save database profile: {error}");
            }
        }
        self.invalidate_db_panel(cx);
    }

    pub(super) fn delete_selected_db_profile(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project_id) = self
            .state
            .selected_project
            .as_ref()
            .map(|project| project.id.clone())
        else {
            self.status_message = "no selected project for database profile".to_string();
            self.invalidate_db_panel(cx);
            return;
        };
        let Some(profile_id) = self
            .db_draft_id
            .clone()
            .or_else(|| self.selected_db_profile_id.clone())
        else {
            self.status_message = "no database profile selected".to_string();
            self.invalidate_db_panel(cx);
            return;
        };
        match self
            .runtime_service
            .delete_db_profile(&project_id, profile_id)
        {
            Ok(_) => {
                self.reload_selected_project_db();
                self.normalize_selected_db_profile();
                self.status_message =
                    self.db_text("db.profile.deleted", "Database profile deleted");
            }
            Err(error) => {
                self.status_message = format!("failed to delete database profile: {error}");
            }
        }
        self.invalidate_db_panel(cx);
    }

    pub(super) fn test_db_profile_draft(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.db_testing || self.db_saving {
            return;
        }
        let request = match self.db_draft_request() {
            Ok(request) => request,
            Err(error) => {
                self.db_test_result = Some(DBProfileTestDisplay {
                    message: self.db_text("db.profile.test.failed", "Failed"),
                    ok: false,
                });
                self.status_message = format!("Database test failed: {error}");
                self.invalidate_db_panel(cx);
                return;
            }
        };
        let service = self.runtime_service.clone();
        let runtime_root = self.runtime.root.clone();
        self.db_testing = true;
        self.db_test_result = Some(DBProfileTestDisplay {
            message: self.db_text("db.profile.test.testing", "Testing..."),
            ok: true,
        });
        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx| {
            let result = codux_runtime::async_runtime::spawn_blocking(move || {
                service.test_db_profile(request, runtime_root)
            })
            .await
            .map_err(|error| error.to_string())
            .and_then(|result| result);

            let _ = this.update(cx, |app, cx| {
                app.db_testing = false;
                match result {
                    Ok(result) => {
                        app.db_test_result = Some(DBProfileTestDisplay {
                            message: app.db_text("db.profile.test.succeeded", "Succeeded"),
                            ok: result.ok,
                        });
                        app.status_message = result.message;
                    }
                    Err(error) => {
                        app.db_test_result = Some(DBProfileTestDisplay {
                            message: app.db_text("db.profile.test.failed", "Failed"),
                            ok: false,
                        });
                        app.status_message = format!("Database test failed: {error}");
                    }
                }
                app.invalidate_db_panel(cx);
            });
        })
        .detach();
        self.invalidate_db_panel(cx);
    }

    pub(super) fn copy_db_command(&mut self, profile_id: String, cx: &mut Context<Self>) {
        let command = format!(
            "codux-db '{}' -- 'SELECT 1;'",
            profile_id.replace('\'', "'\\''")
        );
        cx.write_to_clipboard(ClipboardItem::new_string(command));
        self.status_message = self.db_text("db.profile.command_copied", "Database command copied");
        self.invalidate_db_panel(cx);
    }

    pub(in crate::app) fn invalidate_db_panel(&mut self, cx: &mut Context<Self>) {
        if self.window_mode != AppWindowMode::Main {
            self.invalidate_ui_region(cx, UiRegion::Root);
            return;
        }
        self.invalidate_ui(
            cx,
            [
                UiRegion::WorkspaceAssistant,
                UiRegion::DbSidebar,
                UiRegion::StatusBar,
            ],
        );
    }
}
