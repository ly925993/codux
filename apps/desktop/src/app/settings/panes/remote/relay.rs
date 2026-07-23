use super::*;

pub(super) fn settings_remote_relay_url_editor(
    value: &str,
    busy: bool,
    saving: bool,
    window: &mut Window,
    cx: &mut Context<CoduxApp>,
    _language: &str,
) -> AnyElement {
    let value = value.to_string();
    let state_key = SharedString::from(format!("settings-remote-relay-url-draft-{value}"));
    let state = window.use_keyed_state(state_key, cx, |window, cx| {
        InputState::new(window, cx).default_value(value.clone())
    });
    cx.subscribe_in(&state, window, |app, _state, event, _window, cx| {
        if matches!(event, InputEvent::Change) {
            app.invalidate_remote_panel(cx);
        }
    })
    .detach();
    let has_changes = state.read(cx).value().as_ref() != value.as_str();
    let input_state = state.clone();
    div()
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            Input::new(&state)
                .with_size(gpui_component::Size::Medium)
                .disabled(busy)
                .w_full(),
        )
        .when(has_changes, |this| {
            this.child(settings_icon_button_loading_state(
                "settings-remote-relay-url-apply",
                HeroIconName::Check,
                saving,
                busy,
                cx,
                move |app, _event, window, cx| {
                    app.set_remote_relay_url(input_state.read(cx).value().to_string(), window, cx)
                },
            ))
        })
        .into_any_element()
}

pub(super) fn settings_remote_relay_authentication_editor(
    value: &str,
    busy: bool,
    saving: bool,
    window: &mut Window,
    cx: &mut Context<CoduxApp>,
) -> AnyElement {
    let value = value.to_string();
    let state_key = SharedString::from(format!(
        "settings-remote-relay-authentication-draft-{}",
        value.len()
    ));
    let state = window.use_keyed_state(state_key, cx, |window, cx| {
        InputState::new(window, cx)
            .default_value(value.clone())
            .masked(true)
    });
    cx.subscribe_in(&state, window, |app, _state, event, _window, cx| {
        if matches!(event, InputEvent::Change) {
            app.invalidate_remote_panel(cx);
        }
    })
    .detach();
    let has_changes = state.read(cx).value().as_ref() != value.as_str();
    let input_state = state.clone();
    div()
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            Input::new(&state)
                .with_size(gpui_component::Size::Medium)
                .disabled(busy)
                .w_full(),
        )
        .when(has_changes, |this| {
            this.child(settings_icon_button_loading_state(
                "settings-remote-relay-authentication-apply",
                HeroIconName::Check,
                saving,
                busy,
                cx,
                move |app, _event, window, cx| {
                    app.set_remote_relay_authentication(
                        input_state.read(cx).value().to_string(),
                        window,
                        cx,
                    )
                },
            ))
        })
        .into_any_element()
}

pub(super) fn settings_remote_relay_custom_fields(
    settings: &SettingsSummary,
    busy: bool,
    saving: bool,
    window: &mut Window,
    cx: &mut Context<CoduxApp>,
    language: &str,
) -> AnyElement {
    if settings.remote_relay_preset != "custom" {
        return div().into_any_element();
    }
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(settings_row(
            settings_text(language, "settings.remote.relay_url", "Custom Relay URL"),
            Some(settings_text(
                language,
                "settings.remote.relay_url.help",
                "Leave empty to use the public network. Pair again after changing it",
            )),
            settings_remote_relay_url_editor(
                settings.remote_relay_url.as_str(),
                busy,
                saving,
                window,
                cx,
                language,
            ),
        ))
        .child(settings_row(
            settings_text(
                language,
                "settings.remote.relay_authentication",
                "Relay Authentication",
            ),
            Some(settings_text(
                language,
                "settings.remote.relay_authentication.help",
                "Optional Bearer token for custom Iroh relays. Pair again after changing it",
            )),
            settings_remote_relay_authentication_editor(
                settings.remote_relay_authentication.as_str(),
                busy,
                saving,
                window,
                cx,
            ),
        ))
        .into_any_element()
}
