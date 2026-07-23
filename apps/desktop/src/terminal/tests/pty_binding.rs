use super::super::*;

struct HostedTestController {
    actions: std::sync::Mutex<Vec<String>>,
    inputs: std::sync::Mutex<Vec<Vec<u8>>>,
    terminals: Vec<String>,
    accept_input: bool,
}

impl Default for HostedTestController {
    fn default() -> Self {
        Self {
            actions: std::sync::Mutex::new(Vec::new()),
            inputs: std::sync::Mutex::new(Vec::new()),
            terminals: Vec::new(),
            accept_input: true,
        }
    }
}

impl HostedTestController {
    fn with_terminals(terminals: &[&str]) -> Self {
        Self {
            actions: std::sync::Mutex::new(Vec::new()),
            inputs: std::sync::Mutex::new(Vec::new()),
            terminals: terminals.iter().map(|value| value.to_string()).collect(),
            accept_input: true,
        }
    }

    fn record(&self, action: impl Into<String>) {
        self.actions.lock().unwrap().push(action.into());
    }
}

impl RuntimeTerminalController for HostedTestController {
    fn open_terminal(&self, config: &TerminalPtyConfig) -> Result<String, String> {
        let session_id = config
            .terminal_id
            .clone()
            .ok_or_else(|| "terminal id missing".to_string())?;
        self.record(format!("open:{session_id}"));
        Ok(session_id)
    }

    fn list_terminals(&self) -> Result<serde_json::Value, String> {
        self.record("list");
        Ok(serde_json::json!({
            "terminals": self
                .terminals
                .iter()
                .map(|id| serde_json::json!({ "id": id }))
                .collect::<Vec<_>>()
        }))
    }

    fn terminal_input(&self, _session_id: &str, bytes: &[u8]) -> bool {
        self.inputs.lock().unwrap().push(bytes.to_vec());
        self.accept_input
    }

    fn terminal_resize(&self, _session_id: &str, _cols: u16, _rows: u16) -> bool {
        true
    }

    fn close_terminal(&self, _session_id: &str) -> Result<(), String> {
        Ok(())
    }

    fn close_terminal_fire(&self, _session_id: &str) -> bool {
        true
    }

    fn register_terminal_output(
        &self,
        session_id: &str,
        _forwarder: codux_runtime::runtime_terminal::RuntimeTerminalOutputForwarder,
    ) {
        self.record(format!("output:{session_id}"));
    }

    fn unregister_terminal_output(&self, _session_id: &str) {}

    fn register_terminal_events(
        &self,
        session_id: &str,
        _forwarder: codux_runtime::runtime_terminal::RuntimeTerminalEventForwarder,
    ) {
        self.record(format!("events:{session_id}"));
    }
}

#[test]
fn pending_terminal_binding_matches_requested_config_before_attach() {
    let config = terminal_pty_config_with_view(
        TerminalPtyConfig {
            cwd: Some("/tmp/project".to_string()),
            project_id: Some("project-1".to_string()),
            terminal_id: Some("terminal-1".to_string()),
            session_key: Some("gpui:project-1:terminal-1".to_string()),
            ..Default::default()
        },
        &terminal_config(),
    );

    let (binding, _initial_layout_rx) = TerminalSessionBinding::pending(config.clone());

    assert!(binding.matches_pty_config(&config));

    let mut different_terminal = config;
    different_terminal.terminal_id = Some("terminal-2".to_string());
    assert!(!binding.matches_pty_config(&different_terminal));
}

#[test]
fn pending_terminal_binding_rejects_different_runtime_target() {
    let config = TerminalPtyConfig {
        cwd: Some("/tmp/project".to_string()),
        project_id: Some("project-1".to_string()),
        terminal_id: Some("terminal-1".to_string()),
        session_key: Some("gpui:project-1:terminal-1".to_string()),
        ..Default::default()
    };
    let (binding, _initial_layout_rx) = TerminalSessionBinding::pending(config.clone());
    let mut wsl_config = config;
    wsl_config.runtime_target = ProjectRuntimeTarget::Wsl {
        distribution: "Ubuntu-24.04".to_string(),
    };

    assert!(!binding.matches_pty_config(&wsl_config));
}

#[test]
fn hosted_terminal_binding_rejects_different_distribution() {
    let config = TerminalPtyConfig {
        terminal_id: Some("terminal-1".to_string()),
        runtime_target: ProjectRuntimeTarget::Wsl {
            distribution: "Ubuntu-24.04".to_string(),
        },
        ..Default::default()
    };
    let (binding, _initial_layout_rx) = TerminalSessionBinding::pending(config.clone());
    binding.attach_hosted(
        Arc::new(HostedTestController::default()),
        "terminal-1".to_string(),
        flume::unbounded().0,
        flume::unbounded().0,
        flume::unbounded().0,
        config.clone(),
    );
    let mut other_distribution = config;
    other_distribution.runtime_target = ProjectRuntimeTarget::Wsl {
        distribution: "Debian".to_string(),
    };

    assert!(!binding.matches_pty_config(&other_distribution));
}

#[test]
fn hosted_restore_recreates_missing_session_after_registering_forwarders() {
    let controller = HostedTestController::default();
    let config = TerminalPtyConfig {
        terminal_id: Some("terminal-1".to_string()),
        ..Default::default()
    };
    let (output_tx, _) = flume::unbounded();
    let (event_tx, _) = flume::unbounded();
    let (wake_tx, _) = flume::unbounded();

    let session_id = restore_hosted_session(
        &controller,
        "terminal-1",
        &config,
        &output_tx,
        &event_tx,
        &wake_tx,
    )
    .unwrap();

    assert_eq!(session_id, "terminal-1");
    assert_eq!(
        *controller.actions.lock().unwrap(),
        [
            "list",
            "output:terminal-1",
            "events:terminal-1",
            "open:terminal-1"
        ]
    );
}

#[test]
fn hosted_restore_reuses_existing_session_without_reopening() {
    let controller = HostedTestController::with_terminals(&["terminal-1"]);
    let config = TerminalPtyConfig {
        terminal_id: Some("terminal-1".to_string()),
        ..Default::default()
    };
    let (output_tx, _) = flume::unbounded();
    let (event_tx, _) = flume::unbounded();
    let (wake_tx, _) = flume::unbounded();

    let session_id = restore_hosted_session(
        &controller,
        "terminal-1",
        &config,
        &output_tx,
        &event_tx,
        &wake_tx,
    )
    .unwrap();

    assert_eq!(session_id, "terminal-1");
    assert_eq!(*controller.actions.lock().unwrap(), ["list"]);
}

#[test]
fn reconnect_event_clears_terminal_failure_state() {
    let mut model = TerminalModel::new_for_test(80, 24, 100);

    assert!(model.apply_ui_event(TerminalUiEvent::Exit));
    assert!(model.apply_ui_event(TerminalUiEvent::Error("runtime exited".to_string())));
    assert!(model.exited);
    assert!(model.title.is_some());

    assert!(model.apply_ui_event(TerminalUiEvent::Reconnected));
    assert!(!model.exited);
    assert!(model.title.is_none());
}

#[test]
fn reserved_agent_prompt_precedes_input_typed_during_dispatch() {
    let controller = Arc::new(HostedTestController::default());
    let config = TerminalPtyConfig {
        terminal_id: Some("terminal-1".to_string()),
        ..Default::default()
    };
    let (binding, _initial_layout_rx) = TerminalSessionBinding::pending(config.clone());
    binding.attach_hosted(
        controller.clone(),
        "terminal-1".to_string(),
        flume::unbounded().0,
        flume::unbounded().0,
        flume::unbounded().0,
        config,
    );

    assert!(binding.try_reserve_agent_prompt_dispatch());
    binding.write(b"new draft").unwrap();
    binding
        .write_reserved_agent_prompt(b"queued prompt")
        .unwrap();

    assert_eq!(
        *controller.inputs.lock().unwrap(),
        [
            b"queued prompt".to_vec(),
            b"\r".to_vec(),
            b"new draft".to_vec()
        ]
    );
}

#[test]
fn queued_agent_prompt_keeps_submit_outside_the_bracketed_paste_write() {
    assert_eq!(
        frame_agent_prompt("first line\nsecond line"),
        b"\x1b[200~first line\nsecond line\x1b[201~"
    );
}

#[test]
fn hosted_input_rejection_is_reported_to_queue_dispatch() {
    let controller = Arc::new(HostedTestController {
        accept_input: false,
        ..Default::default()
    });
    let config = TerminalPtyConfig {
        terminal_id: Some("terminal-1".to_string()),
        ..Default::default()
    };
    let (binding, _initial_layout_rx) = TerminalSessionBinding::pending(config.clone());
    binding.attach_hosted(
        controller.clone(),
        "terminal-1".to_string(),
        flume::unbounded().0,
        flume::unbounded().0,
        flume::unbounded().0,
        config,
    );

    assert!(binding.try_reserve_agent_prompt_dispatch());
    assert!(binding.write_reserved_agent_prompt(b"prompt").is_err());
    assert_eq!(*controller.inputs.lock().unwrap(), [b"prompt".to_vec()]);
}
