use super::*;

impl CoduxApp {
    pub(in crate::app) fn open_settings_window(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_settings_window_with_pane(SettingsPane::General, cx);
    }

    pub(in crate::app) fn open_settings_window_with_pane(
        &mut self,
        pane: SettingsPane,
        cx: &mut Context<Self>,
    ) {
        let pane_label = pane.label(&self.state.settings.language);
        let parent_main_window = cx.entity().downgrade();
        let opened = self.open_auxiliary_window(
            AuxiliaryWindowSpec {
                slot: AuxiliaryWindowSlot::Settings,
                title: SharedString::from("Codux Settings"),
                size: size(px(980.0), px(720.0)),
                min_size: size(px(760.0), px(560.0)),
                already_open_message: "settings window already opened",
                opened_message: "settings window opened",
                failed_prefix: "failed to open settings window",
            },
            cx,
            move |state, runtime, runtime_service, window, cx| {
                let mut app =
                    CoduxApp::new_settings_window_from_state(state, runtime, runtime_service);
                app.active_settings_pane = pane;
                // Settings save locally, then notify the owning main window
                // immediately; its polling loop remains a recovery fallback.
                app.parent_main_window = Some(parent_main_window);
                let _ = (window, cx);
                app
            },
            |view, _window, cx| {
                view.update(cx, |app, cx| app.start_settings_remote_snapshot_loop(cx));
            },
        );

        if opened {
            self.status_message = format!("{}: {pane_label}", self.status_message);
        }
        self.invalidate_status_bar(cx);
    }

    pub(in crate::app) fn open_remote_settings_window(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_settings_window_with_pane(SettingsPane::Remote, cx);
    }

    pub(in crate::app) fn open_git_clone_window(&mut self, cx: &mut Context<Self>) {
        let labels = GitSidebarLabels::load(&self.state.settings.language);
        self.open_auxiliary_window(
            AuxiliaryWindowSpec {
                slot: AuxiliaryWindowSlot::GitClone,
                title: SharedString::from(labels.clone_repository.clone()),
                size: size(px(420.0), px(204.0)),
                min_size: size(px(360.0), px(190.0)),
                already_open_message: "Git clone window already opened",
                opened_message: "Git clone window opened",
                failed_prefix: "failed to open Git clone window",
            },
            cx,
            |state, runtime, runtime_service, _window, _cx| {
                let mut app =
                    CoduxApp::new_settings_window_from_state(state, runtime, runtime_service);
                app.window_mode = AppWindowMode::GitClone;
                app.status_message = "Git clone window ready".to_string();
                app
            },
            |_view, _window, _cx| {},
        );
        self.invalidate_status_bar(cx);
    }

    pub(in crate::app) fn open_git_credentials_window(&mut self, cx: &mut Context<Self>) {
        let labels = GitSidebarLabels::load(&self.state.settings.language);
        let project_id = self.git_credential_project_id.clone();
        let project_name = self.git_credential_project_name.clone();
        let project_path = self.git_credential_project_path.clone();
        let remote_url = self.git_credential_remote_url.clone();
        let username = self.git_credential_username.clone();
        let error = self.git_credential_error.clone();
        self.open_auxiliary_window(
            AuxiliaryWindowSpec {
                slot: AuxiliaryWindowSlot::GitCredentials,
                title: SharedString::from(labels.credentials_title.clone()),
                size: size(
                    px(GIT_CREDENTIALS_WINDOW_WIDTH),
                    px(GIT_CREDENTIALS_COMPACT_HEIGHT),
                ),
                min_size: size(px(380.0), px(GIT_CREDENTIALS_COMPACT_HEIGHT)),
                already_open_message: "Git credentials window already opened",
                opened_message: "Git credentials window opened",
                failed_prefix: "failed to open Git credentials window",
            },
            cx,
            move |state, runtime, runtime_service, _window, _cx| {
                let mut app =
                    CoduxApp::new_settings_window_from_state(state, runtime, runtime_service);
                app.window_mode = AppWindowMode::GitCredentials;
                app.status_message = "Git credentials window ready".to_string();
                app.git_credential_project_id = project_id;
                app.git_credential_project_name = project_name;
                app.git_credential_project_path = project_path;
                app.git_credential_remote_url = remote_url;
                app.git_credential_username = username;
                app.git_credential_error = error;
                app
            },
            |view, window, cx| {
                let expanded = view.read(cx).git_credential_error.is_some();
                resize_git_credentials_window(window, expanded);
            },
        );
        self.invalidate_status_bar(cx);
    }

    pub(in crate::app) fn open_ssh_profile_dialog(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_ssh_profile_editor(None, cx);
    }

    pub(in crate::app) fn open_db_profile_dialog(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_db_profile_editor(None, cx);
    }

    pub(in crate::app) fn open_selected_ssh_profile_editor(
        &mut self,
        profile_id: String,
        cx: &mut Context<Self>,
    ) {
        self.open_ssh_profile_editor(Some(profile_id), cx);
    }

    pub(in crate::app) fn open_selected_db_profile_editor(
        &mut self,
        profile_id: String,
        cx: &mut Context<Self>,
    ) {
        self.open_db_profile_editor(Some(profile_id), cx);
    }

    pub(in crate::app) fn open_ssh_profile_editor(
        &mut self,
        profile_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.state.ssh = self.runtime_service.reload_ssh(self.runtime.root.clone());
        self.normalize_selected_ssh_profile();
        if Self::activate_child_window(&mut self.ssh_profile_editor_window, cx) {
            self.status_message = "SSH profile editor already opened".to_string();
            self.invalidate_remote_panel(cx);
            return;
        }

        let profile = if let Some(profile_id) = profile_id {
            let snapshot = self.runtime_service.ssh_profiles();
            let Some(profile) = snapshot
                .profiles
                .into_iter()
                .find(|profile| profile.id == profile_id)
            else {
                self.status_message = "SSH profile is no longer available".to_string();
                self.invalidate_remote_panel(cx);
                return;
            };
            Some(profile)
        } else {
            None
        };
        let title = if profile.is_some() {
            "Edit SSH Profile"
        } else {
            "Add SSH Profile"
        };
        self.open_auxiliary_window(
            AuxiliaryWindowSpec {
                slot: AuxiliaryWindowSlot::SshProfileEditor,
                title: SharedString::from(title),
                size: size(px(520.0), px(430.0)),
                min_size: size(px(460.0), px(390.0)),
                already_open_message: "SSH profile editor already opened",
                opened_message: "SSH profile editor opened",
                failed_prefix: "failed to open SSH profile editor",
            },
            cx,
            move |state, runtime, runtime_service, _window, _cx| {
                CoduxApp::new_ssh_profile_editor_window_from_state(
                    profile,
                    state,
                    runtime,
                    runtime_service,
                )
            },
            |_view, _window, _cx| {},
        );
        self.invalidate_remote_panel(cx);
    }

    pub(in crate::app) fn open_db_profile_editor(
        &mut self,
        profile_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(project_id) = self
            .state
            .selected_project
            .as_ref()
            .map(|project| project.id.clone())
        else {
            self.status_message = self.db_text(
                "db.profile.no_project",
                "Select a project before adding a database profile",
            );
            self.invalidate_db_panel(cx);
            return;
        };
        self.reload_selected_project_db();
        self.normalize_selected_db_profile();
        if Self::activate_child_window(&mut self.db_profile_editor_window, cx) {
            self.status_message = self.db_text(
                "db.profile.editor.already_open",
                "Database profile editor already opened",
            );
            self.invalidate_db_panel(cx);
            return;
        }

        let profile = if let Some(profile_id) = profile_id {
            let snapshot = self.runtime_service.db_profiles(Some(&project_id));
            let Some(profile) = snapshot
                .profiles
                .into_iter()
                .find(|profile| profile.id == profile_id)
            else {
                self.status_message = self.db_text(
                    "db.profile.unavailable",
                    "Database profile is no longer available",
                );
                self.invalidate_db_panel(cx);
                return;
            };
            Some(profile)
        } else {
            None
        };
        let locale = locale_from_language_setting(&self.state.settings.language);
        let title = if profile.is_some() {
            translate(&locale, "db.profile.edit_window", "Edit Database Profile")
        } else {
            translate(&locale, "db.profile.add_window", "Add Database Profile")
        };
        self.open_auxiliary_window(
            AuxiliaryWindowSpec {
                slot: AuxiliaryWindowSlot::DbProfileEditor,
                title: SharedString::from(title),
                size: size(px(520.0), px(520.0)),
                min_size: size(px(460.0), px(430.0)),
                already_open_message: "Database profile editor already opened",
                opened_message: "Database profile editor opened",
                failed_prefix: "failed to open database profile editor",
            },
            cx,
            move |state, runtime, runtime_service, _window, _cx| {
                CoduxApp::new_db_profile_editor_window_from_state(
                    profile,
                    state,
                    runtime,
                    runtime_service,
                )
            },
            |_view, _window, _cx| {},
        );
        self.invalidate_db_panel(cx);
    }
}
