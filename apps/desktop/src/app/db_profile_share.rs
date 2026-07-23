use super::*;
use gpui_component::checkbox::Checkbox;

#[derive(Clone)]
struct DbProfileShareLabels {
    share: String,
    shared_projects: String,
    selected_projects: String,
    current_project: String,
    no_projects: String,
    cancel: String,
    save: String,
}

impl DbProfileShareLabels {
    fn load(language: &str) -> Self {
        let locale = locale_from_language_setting(language);
        let tr = |key: &str, fallback: &str| translate(&locale, key, fallback);
        Self {
            share: tr("db.profile.share", "Share to projects"),
            shared_projects: tr("db.profile.shared_projects", "Available in projects"),
            selected_projects: tr("db.profile.selected_projects", "%@ projects selected"),
            current_project: tr("db.profile.current_project", "Current"),
            no_projects: tr("db.profile.no_projects", "No projects available"),
            cancel: tr("common.cancel", "Cancel"),
            save: tr("common.save", "Save"),
        }
    }
}

pub(in crate::app) fn db_profile_share_workspace(
    app: &CoduxApp,
    db_saving: bool,
    _window: &mut Window,
    cx: &mut Context<CoduxApp>,
) -> impl IntoElement {
    let labels = DbProfileShareLabels::load(&app.state.settings.language);
    let current_project_id = app
        .state
        .selected_project
        .as_ref()
        .map(|project| project.id.clone());
    // Membership is rebuilt in memory from a small draft; checkbox renders never touch storage.
    let selected_project_ids = app
        .db_draft_project_ids
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let original_project_ids = app
        .db_share_original_project_ids
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let selection_changed = selected_project_ids != original_project_ids;
    let selected_summary = labels
        .selected_projects
        .replace("%@", &selected_project_ids.len().to_string());
    let projects = app.db_share_projects.clone();
    let projects_empty = projects.is_empty();
    let scroll_handle = app.db_share_scroll_handle.clone();
    let app_entity = cx.entity();
    let current_label = labels.current_project.clone();
    let footer = div()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .truncate()
                .text_size(rems(0.75))
                .text_color(color(theme::RED))
                .children(app.db_share_error.clone()),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    dialog_cancel_button(
                        "db-share-cancel",
                        labels.cancel.clone(),
                        cx,
                        |_app, _event, window, _cx| window.remove_window(),
                    )
                    .disabled(db_saving),
                )
                .child(
                    dialog_primary_button(
                        "db-share-save",
                        labels.save.clone(),
                        cx,
                        |app, _event, window, cx| app.save_db_profile_sharing(window, cx),
                    )
                    .loading(db_saving)
                    .disabled(db_saving || !selection_changed),
                ),
        );

    child_window_shell(format!("{} · {}", labels.share, app.db_draft_name), cx)
        .child(
            div()
                .flex_1()
                .min_h_0()
                .p(px(18.0))
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.0))
                        .child(
                            div()
                                .text_size(rems(0.75))
                                .text_color(color(theme::TEXT_MUTED))
                                .child(labels.shared_projects),
                        )
                        .child(
                            div()
                                .text_size(rems(0.75))
                                .text_color(cx.theme().muted_foreground)
                                .child(selected_summary),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(cx.theme().border)
                        .p(px(6.0))
                        .child(if projects_empty {
                            div()
                                .h_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(rems(0.8125))
                                .text_color(cx.theme().muted_foreground)
                                .child(labels.no_projects)
                                .into_any_element()
                        } else {
                            // GPUI creates only visible rows on both macOS and Windows.
                            codux_uniform_list(
                                "db-share-project-list",
                                projects,
                                scroll_handle,
                                Some(px(2.0)),
                                cx,
                                move |project, _index, _window, cx| {
                                    let project_id = project.id.clone();
                                    let selected = selected_project_ids.contains(&project.id);
                                    let current =
                                        current_project_id.as_deref() == Some(project.id.as_str());
                                    let app_entity = app_entity.clone();
                                    Checkbox::new(SharedString::from(format!(
                                        "db-share-project-{project_id}"
                                    )))
                                    .checked(selected)
                                    .disabled(db_saving || current)
                                    .with_size(Size::Small)
                                    .w_full()
                                    .px_2()
                                    .py_2()
                                    .on_click(move |_, _window, cx| {
                                        cx.update_entity(&app_entity, |app, cx| {
                                            app.toggle_db_share_project(project_id.clone(), cx);
                                        });
                                    })
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_size(rems(0.8125))
                                                    .text_color(cx.theme().foreground)
                                                    .child(project.name),
                                            )
                                            .child(
                                                div()
                                                    .mt(px(2.0))
                                                    .truncate()
                                                    .text_size(rems(0.6875))
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(project.path),
                                            ),
                                    )
                                    .when(current, |this| {
                                        this.child(
                                            div()
                                                .flex_none()
                                                .text_size(rems(0.6875))
                                                .text_color(cx.theme().muted_foreground)
                                                .child(current_label.clone()),
                                        )
                                    })
                                    .into_any_element()
                                },
                            )
                            .into_any_element()
                        }),
                ),
        )
        .child(dialog_footer_bar(footer, cx))
}
