use super::*;
use codux_runtime::agent_task_relay::{
    AgentTaskRelayBlockReason, AgentTaskRelayBoard, AgentTaskRelayBoardState,
    AgentTaskRelayService, AgentTaskRelayTarget, AgentTaskRelayTaskState, MAX_RELAY_TASKS,
};

#[derive(Clone, PartialEq)]
pub(in crate::app) struct AgentTaskRelaySnapshot {
    language: Arc<str>,
    target: Option<AgentTaskRelayTarget>,
    board: Option<AgentTaskRelayBoard>,
    loaded: bool,
    error: Option<Arc<str>>,
}

pub(in crate::app) struct AgentTaskRelayView {
    app_entity: gpui::Entity<CoduxApp>,
    input: gpui::Entity<InputState>,
    snapshot: AgentTaskRelaySnapshot,
    editing_task_id: Option<u64>,
    editing_scope: Option<RelayEditorScope>,
    showing_receipts: bool,
    task_scroll_handle: UniformListScrollHandle,
    receipt_scroll_handle: UniformListScrollHandle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RelayEditorScope {
    board_id: String,
}

impl AgentTaskRelayView {
    fn set_snapshot(&mut self, snapshot: AgentTaskRelaySnapshot, cx: &mut Context<Self>) {
        if self.snapshot == snapshot {
            return;
        }
        if self.editing_task_id.is_some()
            && self.editing_scope.as_ref() != relay_editor_scope(&snapshot).as_ref()
        {
            self.editing_task_id = None;
            self.editing_scope = None;
        }
        self.snapshot = snapshot;
        cx.notify();
    }
}

impl CoduxApp {
    pub(in crate::app) fn load_agent_task_relays(&mut self, cx: &mut Context<Self>) {
        if self.agent_task_relay_loaded || self.agent_task_relay_loading {
            return;
        }
        self.agent_task_relay_loading = true;
        let service = AgentTaskRelayService::new(self.state.support_dir.clone());
        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx| {
            let result =
                codux_runtime::async_runtime::spawn_blocking(move || service.load_all()).await;
            let _ = this.update(cx, |app, cx| {
                app.agent_task_relay_loading = false;
                match result {
                    Ok(Ok(result)) => {
                        app.agent_task_relay_boards = result
                            .boards
                            .into_iter()
                            .map(|board| (board.id.clone(), board))
                            .collect();
                        app.agent_task_relay_loaded = true;
                        app.agent_task_relay_error =
                            (!result.errors.is_empty()).then(|| result.errors.join("\n"));
                    }
                    Ok(Err(error)) => {
                        app.agent_task_relay_error = Some(error.to_string());
                    }
                    Err(error) => app.agent_task_relay_error = Some(error.to_string()),
                }
                app.refresh_agent_task_relay_ui(cx);
            });
        })
        .detach();
    }

    pub(in crate::app) fn agent_task_relay_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Entity<AgentTaskRelayView> {
        let snapshot = self.active_agent_task_relay_snapshot();
        if let Some(view) = self.agent_task_relay_view.clone() {
            view.update(cx, |view, cx| view.set_snapshot(snapshot, cx));
            return view;
        }
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .rows(8)
                .placeholder("请输入一个完整任务")
        });
        let app_entity = cx.entity();
        let view = cx.new(|_| AgentTaskRelayView {
            app_entity,
            input,
            snapshot,
            editing_task_id: None,
            editing_scope: None,
            showing_receipts: false,
            task_scroll_handle: UniformListScrollHandle::new(),
            receipt_scroll_handle: UniformListScrollHandle::new(),
        });
        self.agent_task_relay_view = Some(view.clone());
        view
    }

    pub(in crate::app) fn refresh_agent_task_relay_ui(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = self.agent_task_relay_view.clone() {
            let snapshot = self.active_agent_task_relay_snapshot();
            view.update(cx, |view, cx| view.set_snapshot(snapshot, cx));
        }
        self.invalidate_ui(
            cx,
            [UiRegion::WorkspaceChrome, UiRegion::WorkspaceAssistant],
        );
    }

    pub(in crate::app) fn active_agent_task_relay_counts(&self) -> (usize, usize, bool) {
        let Some(target) = self.active_agent_task_relay_target() else {
            return (0, 0, false);
        };
        let Some(board) = self.agent_task_relay_boards.get(&relay_board_id(&target)) else {
            return (0, 0, false);
        };
        (
            board.counts.queued,
            board.counts.receipts,
            matches!(board.state, AgentTaskRelayBoardState::PausedBlocked(_)),
        )
    }

    fn active_agent_task_relay_snapshot(&self) -> AgentTaskRelaySnapshot {
        let target = self.active_agent_task_relay_target();
        let board = target
            .as_ref()
            .and_then(|target| self.agent_task_relay_boards.get(&relay_board_id(target)))
            .cloned();
        AgentTaskRelaySnapshot {
            language: Arc::from(self.state.settings.language.as_str()),
            target,
            board,
            loaded: self.agent_task_relay_loaded,
            error: self.agent_task_relay_error.as_deref().map(Arc::from),
        }
    }

    fn active_agent_task_relay_target(&self) -> Option<AgentTaskRelayTarget> {
        let (key, _) = self.active_agent_prompt_target()?;
        let project_id = self.state.selected_project.as_ref()?.id.clone();
        let worktree_id = super::ai_runtime_status::terminal_layout_owner_id(&self.state)?;
        Some(AgentTaskRelayTarget {
            project_id,
            worktree_id,
            terminal_id: key.terminal_id,
            terminal_instance_id: Some(key.terminal_instance_id),
            tool: key.tool,
            ai_session_id: key.ai_session_id,
        })
    }

    fn mutate_active_agent_task_relay(
        &mut self,
        action: impl FnOnce(&mut AgentTaskRelayBoard) -> Result<bool, String>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(target) = self.active_agent_task_relay_target() else {
            self.status_message = "请先选择一个已识别的 Agent 终端。".to_string();
            return false;
        };
        let board_id = relay_board_id(&target);
        let board = self
            .agent_task_relay_boards
            .entry(board_id.clone())
            .or_insert_with(|| AgentTaskRelayBoard::new(board_id, target.clone()));
        let result = board
            .rebind_target(target)
            .map_err(str::to_string)
            .and_then(|rebound| action(board).map(|changed| changed || rebound));
        match result {
            Ok(true) => {
                let board = board.clone();
                self.persist_agent_task_relay(board, cx);
                self.refresh_agent_task_relay_ui(cx);
                true
            }
            Ok(false) => false,
            Err(error) => {
                self.status_message = relay_error_label(&error).to_string();
                false
            }
        }
    }

    fn persist_agent_task_relay(&mut self, board: AgentTaskRelayBoard, cx: &mut Context<Self>) {
        let board_id = board.id.clone();
        let revision = board.revision;
        self.agent_task_relay_persisting
            .insert(board_id.clone(), revision);
        let service = AgentTaskRelayService::new(self.state.support_dir.clone());
        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx| {
            let result =
                codux_runtime::async_runtime::spawn_blocking(move || service.save_durable(&board))
                    .await;
            let _ = this.update(cx, |app, cx| {
                if app.agent_task_relay_persisting.get(&board_id) == Some(&revision) {
                    app.agent_task_relay_persisting.remove(&board_id);
                }
                let error = match result {
                    Ok(Ok(_)) => None,
                    Ok(Err(error)) => Some(error),
                    Err(error) => Some(error.to_string()),
                };
                if let Some(error) = error {
                    // A lower revision can legitimately finish after a newer
                    // commit. Only surface failures that still describe the
                    // current board revision.
                    if app
                        .agent_task_relay_boards
                        .get(&board_id)
                        .is_some_and(|current| current.revision <= revision)
                    {
                        app.agent_task_relay_error = Some(error);
                        if let Some(board) = app.agent_task_relay_boards.get_mut(&board_id) {
                            board.mark_persistence_failed();
                        }
                    }
                }
                app.refresh_agent_task_relay_ui(cx);
            });
        })
        .detach();
    }

    fn add_agent_task_relay_task(&mut self, text: String, cx: &mut Context<Self>) -> bool {
        self.mutate_active_agent_task_relay(
            |board| {
                board
                    .add_task(text, app_now_seconds())
                    .map(|_| true)
                    .map_err(str::to_string)
            },
            cx,
        )
    }

    fn edit_agent_task_relay_task(
        &mut self,
        task_id: u64,
        text: String,
        cx: &mut Context<Self>,
    ) -> bool {
        self.mutate_active_agent_task_relay(
            |board| board.edit_task(task_id, text).map_err(str::to_string),
            cx,
        )
    }

    fn start_or_pause_agent_task_relay(&mut self, cx: &mut Context<Self>) {
        let running = self
            .active_agent_task_relay_snapshot()
            .board
            .as_ref()
            .is_some_and(|board| board.state == AgentTaskRelayBoardState::Running);
        let changed = self.mutate_active_agent_task_relay(
            |board| {
                if running {
                    Ok(board.pause())
                } else {
                    board.start().map(|_| true).map_err(str::to_string)
                }
            },
            cx,
        );
        if changed && !running {
            self.pump_agent_task_relays(cx);
        }
    }

    fn update_agent_task_relay_task(
        &mut self,
        task_id: u64,
        action: RelayTaskAction,
        cx: &mut Context<Self>,
    ) {
        let changed = self.mutate_active_agent_task_relay(
            |board| match action {
                RelayTaskAction::Promote => Ok(board.promote_task(task_id)),
                RelayTaskAction::MoveUp => Ok(board.move_task(task_id, -1)),
                RelayTaskAction::MoveDown => Ok(board.move_task(task_id, 1)),
                RelayTaskAction::Delete => Ok(board.delete_task(task_id)),
                RelayTaskAction::Resend => retry_agent_task_relay_task(board, task_id),
                RelayTaskAction::MarkHandled => {
                    Ok(board.resolve_delivery_unknown(task_id, false, app_now_seconds()))
                }
            },
            cx,
        );
        if changed && matches!(action, RelayTaskAction::Resend) {
            self.pump_agent_task_relays(cx);
        }
    }

    pub(in crate::app) fn agent_task_relay_owns_ack(
        &self,
        key: &super::agent_prompt_queue::AgentPromptQueueKey,
    ) -> bool {
        self.agent_task_relay_boards.values().any(|board| {
            relay_target_matches_key(&board.target, key)
                && board.active_task().is_some_and(|task| {
                    matches!(
                        task.state,
                        AgentTaskRelayTaskState::Dispatching | AgentTaskRelayTaskState::AwaitingAck
                    )
                })
        })
    }

    pub(in crate::app) fn observe_agent_task_relay_events(
        &mut self,
        events: &[codux_runtime::ai_runtime::AIRuntimeSupervisorEvent],
        cx: &mut Context<Self>,
    ) {
        let mut changed_ids = HashSet::new();
        for event in events {
            match event {
                codux_runtime::ai_runtime::AIRuntimeSupervisorEvent::State { snapshot } => {
                    for session in &snapshot.sessions {
                        for board in self.agent_task_relay_boards.values_mut() {
                            if !relay_target_matches_session(&board.target, session) {
                                continue;
                            }
                            let Some(task) = board.active_task().cloned() else {
                                continue;
                            };
                            let changed = match task.state {
                                AgentTaskRelayTaskState::AwaitingAck
                                    if matches!(
                                        session.state.as_str(),
                                        "responding" | "needsInput"
                                    ) && session.last_user_input_at.is_some_and(
                                        |observed| {
                                            task.dispatch_started_at
                                                .is_some_and(|started| observed + 0.001 >= started)
                                        },
                                    ) =>
                                {
                                    board.mark_acknowledged(task.id, session.updated_at)
                                }
                                AgentTaskRelayTaskState::Running
                                    if session.state == "needsInput" =>
                                {
                                    board.mark_needs_input(task.id)
                                }
                                AgentTaskRelayTaskState::NeedsInput
                                    if session.state == "responding" =>
                                {
                                    board.mark_input_resumed(task.id, session.runtime_activity_at)
                                }
                                _ => false,
                            };
                            if changed {
                                changed_ids.insert(board.id.clone());
                            }
                        }
                    }
                }
                codux_runtime::ai_runtime::AIRuntimeSupervisorEvent::SessionCompletion {
                    completion,
                } => {
                    for board in self.agent_task_relay_boards.values_mut() {
                        if !relay_target_matches_completion(&board.target, completion) {
                            continue;
                        }
                        let Some(task) = board.active_task().cloned() else {
                            continue;
                        };
                        let changed = match relay_completion_disposition(&task, completion) {
                            RelayCompletionDisposition::Ignore => false,
                            RelayCompletionDisposition::Interrupted => {
                                board.mark_interrupted(task.id, completion.completed_at)
                            }
                            RelayCompletionDisposition::Completed => board.mark_completed(
                                task.id,
                                completion.id.clone(),
                                completion.latest_assistant_preview.clone(),
                                completion.completed_at,
                            ),
                        };
                        if changed {
                            changed_ids.insert(board.id.clone());
                        }
                    }
                }
                codux_runtime::ai_runtime::AIRuntimeSupervisorEvent::TerminalStatus { status } => {
                    for board in self.agent_task_relay_boards.values_mut() {
                        if !relay_target_matches_terminal_status(&board.target, status) {
                            continue;
                        }
                        let Some(task) = board.active_task().cloned() else {
                            continue;
                        };
                        if status.updated_at + 0.001
                            < task.dispatch_started_at.unwrap_or(status.updated_at)
                        {
                            continue;
                        }
                        use codux_runtime::ai_runtime::TerminalStatusState;
                        let changed = if relay_terminal_status_acknowledges_task(
                            &board.target,
                            &task,
                            status,
                        ) {
                            // PTY activity is a portable acknowledgement when
                            // Claude/Codex hook files are delayed or unavailable.
                            board.mark_acknowledged(task.id, status.updated_at)
                        } else {
                            match (status.state, task.state) {
                                (
                                    TerminalStatusState::Completed,
                                    AgentTaskRelayTaskState::Running,
                                ) => board.mark_completed(
                                    task.id,
                                    relay_terminal_completion_id(status),
                                    None,
                                    status.updated_at,
                                ),
                                (
                                    TerminalStatusState::Waiting,
                                    AgentTaskRelayTaskState::Running,
                                ) => board.mark_needs_input(task.id),
                                (
                                    TerminalStatusState::Working,
                                    AgentTaskRelayTaskState::NeedsInput,
                                ) => board.mark_input_resumed(task.id, Some(status.updated_at)),
                                (
                                    TerminalStatusState::Error,
                                    AgentTaskRelayTaskState::Dispatching
                                    | AgentTaskRelayTaskState::AwaitingAck,
                                ) => board.mark_send_failed(task.id, status.updated_at),
                                (
                                    TerminalStatusState::Error,
                                    AgentTaskRelayTaskState::Running
                                    | AgentTaskRelayTaskState::NeedsInput,
                                ) => board.mark_interrupted(task.id, status.updated_at),
                                _ => false,
                            }
                        };
                        if changed {
                            changed_ids.insert(board.id.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        if changed_ids.is_empty() {
            return;
        }
        for board_id in changed_ids {
            if let Some(board) = self.agent_task_relay_boards.get(&board_id).cloned() {
                self.persist_agent_task_relay(board, cx);
            }
        }
        self.refresh_agent_task_relay_ui(cx);
    }

    /// Schedule at most one independent task per call. The existing send queue
    /// remains ahead of relay work and the durable dispatch intent is committed
    /// before the PTY side effect starts.
    pub(in crate::app) fn pump_agent_task_relays(&mut self, cx: &mut Context<Self>) {
        if self.agent_task_relay_boards.is_empty() {
            return;
        }
        let board_ids = self
            .agent_task_relay_boards
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        // One lock/snapshot per pump keeps relay scheduling independent of the
        // number of boards. Lookups below are allocation-free borrowed keys.
        let terminal_statuses = self.runtime_service.ai_runtime_terminal_statuses();
        let mut terminal_statuses_by_target = HashMap::new();
        for status in &terminal_statuses {
            let Some(instance_id) = status.terminal_instance_id.as_deref() else {
                continue;
            };
            let key = (status.terminal_id.as_str(), instance_id);
            terminal_statuses_by_target
                .entry(key)
                .and_modify(
                    |current: &mut &codux_runtime::ai_runtime::TerminalStatusEvent| {
                        if status.updated_at > current.updated_at {
                            *current = status;
                        }
                    },
                )
                .or_insert(status);
        }
        for board_id in board_ids {
            let expired_task_id = self
                .agent_task_relay_boards
                .get(&board_id)
                .and_then(|board| board.active_task())
                .filter(|task| task.state == AgentTaskRelayTaskState::AwaitingAck)
                .filter(|task| {
                    task.dispatch_started_at
                        .is_some_and(|started| app_now_seconds() - started >= 15.0)
                })
                .map(|task| task.id);
            if let Some(task_id) = expired_task_id {
                if let Some(board) = self.agent_task_relay_boards.get_mut(&board_id)
                    && board.mark_delivery_unknown(task_id)
                {
                    let board = board.clone();
                    self.persist_agent_task_relay(board, cx);
                    self.refresh_agent_task_relay_ui(cx);
                }
                continue;
            }
            let Some(board_snapshot) = self.agent_task_relay_boards.get(&board_id).cloned() else {
                continue;
            };
            if board_snapshot.state != AgentTaskRelayBoardState::Running
                || board_snapshot.active_task().is_some()
            {
                continue;
            }
            let target = board_snapshot.target.clone();
            let Some(session) = self
                .state
                .ai_runtime_state
                .sessions
                .iter()
                .filter(|session| relay_target_matches_summary(&target, session))
                .max_by(|left, right| left.updated_at.total_cmp(&right.updated_at))
                .cloned()
            else {
                continue;
            };
            let terminal_status = target
                .terminal_instance_id
                .as_deref()
                .and_then(|instance_id| {
                    terminal_statuses_by_target
                        .get(&(target.terminal_id.as_str(), instance_id))
                        .copied()
                });
            if !relay_runtime_allows_dispatch(
                &session.tool,
                &session.runtime_state,
                session.last_user_input_at,
                terminal_status,
            ) {
                continue;
            }
            let key = super::agent_prompt_queue::AgentPromptQueueKey {
                terminal_id: target.terminal_id.clone(),
                terminal_instance_id: target.terminal_instance_id.clone().unwrap_or_default(),
                ai_session_id: target.ai_session_id.clone(),
                tool: target.tool.clone(),
            };
            if !self.agent_prompt_queues.items(&key).is_empty()
                || self.agent_prompt_queues.native_submission_pending(&key)
            {
                continue;
            }
            let pane = self
                .terminals
                .iter()
                .flat_map(|tab| tab.panes.iter())
                .find(|slot| slot.terminal_id.as_deref() == Some(target.terminal_id.as_str()))
                .and_then(|slot| slot.pane.as_ref())
                .filter(|pane| pane.terminal_instance_id() == target.terminal_instance_id)
                .cloned();
            let Some(pane) = pane else {
                continue;
            };
            if !pane.view.read(cx).agent_composer_available_for_dispatch()
                || !pane.try_reserve_agent_prompt_dispatch()
            {
                continue;
            }
            let Some(board) = self.agent_task_relay_boards.get_mut(&board_id) else {
                pane.cancel_agent_prompt_dispatch();
                continue;
            };
            let Some(task_id) = board.begin_dispatch(app_now_seconds(), Some(session.updated_at))
            else {
                pane.cancel_agent_prompt_dispatch();
                continue;
            };
            // Completed/Idle may lead the Windows ConPTY composer by a few
            // frames. Carry the exact readiness event into the background
            // writer so persistence time counts toward the settle window.
            let terminal_ready_at = terminal_status
                .filter(|status| {
                    matches!(
                        status.state,
                        codux_runtime::ai_runtime::TerminalStatusState::Completed
                            | codux_runtime::ai_runtime::TerminalStatusState::Idle
                    )
                })
                .map(|status| status.updated_at)
                .or_else(|| (session.runtime_state == "idle").then_some(session.updated_at));
            let text = board
                .task(task_id)
                .map(|task| task.text.clone())
                .unwrap_or_default();
            let dispatch_board = board.clone();
            let service = AgentTaskRelayService::new(self.state.support_dir.clone());
            self.refresh_agent_task_relay_ui(cx);
            cx.spawn(async move |this: gpui::WeakEntity<Self>, cx| {
                let result = codux_runtime::async_runtime::spawn_blocking(move || {
                    if let Err(error) = service.save_durable(&dispatch_board) {
                        pane.cancel_agent_prompt_dispatch();
                        return RelayDispatchResult::BarrierFailed(error);
                    }
                    super::agent_prompt_queue::wait_for_agent_composer_settle(terminal_ready_at);
                    match pane.send_agent_prompt(text.as_ref()) {
                        Ok(()) => RelayDispatchResult::Sent,
                        Err(error) => RelayDispatchResult::DeliveryUnknown(error.to_string()),
                    }
                })
                .await;
                let _ = this.update(cx, |app, cx| {
                    let now = app_now_seconds();
                    let Some(board) = app.agent_task_relay_boards.get_mut(&board_id) else {
                        return;
                    };
                    match result {
                        Ok(RelayDispatchResult::Sent) => {
                            board.mark_write_succeeded(task_id, now);
                        }
                        Ok(RelayDispatchResult::BarrierFailed(error)) => {
                            board.mark_persistence_failed();
                            app.agent_task_relay_error = Some(error);
                        }
                        Ok(RelayDispatchResult::DeliveryUnknown(error)) => {
                            board.mark_delivery_unknown(task_id);
                            app.agent_task_relay_error = Some(error);
                        }
                        Err(error) => {
                            board.mark_delivery_unknown(task_id);
                            app.agent_task_relay_error = Some(error.to_string());
                        }
                    }
                    let board = board.clone();
                    app.persist_agent_task_relay(board, cx);
                    app.refresh_agent_task_relay_ui(cx);
                });
            })
            .detach();
        }
    }
}

#[derive(Clone, Copy)]
enum RelayTaskAction {
    Promote,
    MoveUp,
    MoveDown,
    Delete,
    Resend,
    MarkHandled,
}

enum RelayDispatchResult {
    Sent,
    BarrierFailed(String),
    DeliveryUnknown(String),
}

fn retry_agent_task_relay_task(
    board: &mut AgentTaskRelayBoard,
    task_id: u64,
) -> Result<bool, String> {
    if !board.resolve_delivery_unknown(task_id, true, app_now_seconds()) {
        return Ok(false);
    }
    // "Resend" is a command, not a queue-edit action. Resume the paused board
    // in the same transaction so the caller can immediately pump the retry.
    board.start().map(|_| true).map_err(str::to_string)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RelayCompletionDisposition {
    Ignore,
    Completed,
    Interrupted,
}

fn relay_completion_disposition(
    task: &codux_runtime::agent_task_relay::AgentTaskRelayTask,
    completion: &codux_runtime::ai_runtime::AIRuntimeSessionCompletionEvent,
) -> RelayCompletionDisposition {
    if !matches!(
        task.state,
        AgentTaskRelayTaskState::Running | AgentTaskRelayTaskState::NeedsInput
    ) {
        return RelayCompletionDisposition::Ignore;
    }

    let completion_floor = [
        task.dispatch_started_at,
        task.acknowledged_at,
        task.completion_baseline,
    ]
    .into_iter()
    .flatten()
    .max_by(f64::total_cmp);
    if completion_floor.is_some_and(|floor| completion.completed_at + 0.001 < floor) {
        return RelayCompletionDisposition::Ignore;
    }
    if let (Some(turn_started_at), Some(baseline)) =
        (completion.turn_started_at, task.completion_baseline)
        && turn_started_at + 0.001 < baseline
    {
        return RelayCompletionDisposition::Ignore;
    }
    if completion.was_interrupted {
        return RelayCompletionDisposition::Interrupted;
    }
    if !completion.has_completed_turn {
        return RelayCompletionDisposition::Ignore;
    }
    RelayCompletionDisposition::Completed
}

fn relay_board_id(target: &AgentTaskRelayTarget) -> String {
    format!(
        "{}:{}:{}",
        target.project_id, target.worktree_id, target.terminal_id
    )
}

fn relay_editor_scope(snapshot: &AgentTaskRelaySnapshot) -> Option<RelayEditorScope> {
    let target = snapshot.target.clone()?;
    Some(RelayEditorScope {
        board_id: relay_board_id(&target),
    })
}

fn relay_target_matches_key(
    target: &AgentTaskRelayTarget,
    key: &super::agent_prompt_queue::AgentPromptQueueKey,
) -> bool {
    target.terminal_id == key.terminal_id
        && target.terminal_instance_id.as_deref() == Some(key.terminal_instance_id.as_str())
        && target.tool == key.tool
        && target
            .ai_session_id
            .as_ref()
            .is_none_or(|session_id| key.ai_session_id.as_ref() == Some(session_id))
}

fn relay_target_matches_summary(
    target: &AgentTaskRelayTarget,
    session: &codux_runtime::ai_runtime_state::AIRuntimeSessionSummary,
) -> bool {
    target.terminal_id == session.terminal_id
        && target.terminal_instance_id == session.terminal_instance_id
        && target.tool == session.tool
        && target
            .ai_session_id
            .as_ref()
            .is_none_or(|session_id| session.ai_session_id.as_ref() == Some(session_id))
}

fn relay_target_matches_session(
    target: &AgentTaskRelayTarget,
    session: &codux_runtime::ai_runtime::AISessionSnapshot,
) -> bool {
    target.terminal_id == session.terminal_id
        && target.terminal_instance_id == session.terminal_instance_id
        && target.tool == session.tool
        && target
            .ai_session_id
            .as_ref()
            .is_none_or(|session_id| session.ai_session_id.as_ref() == Some(session_id))
}

fn relay_target_matches_completion(
    target: &AgentTaskRelayTarget,
    completion: &codux_runtime::ai_runtime::AIRuntimeSessionCompletionEvent,
) -> bool {
    target.project_id == completion.project_id
        && target.terminal_id == completion.terminal_id
        && target.terminal_instance_id == completion.terminal_instance_id
        && target.tool == completion.tool
        && target
            .ai_session_id
            .as_ref()
            .is_none_or(|session_id| completion.ai_session_id.as_ref() == Some(session_id))
}

fn relay_target_matches_terminal_status(
    target: &AgentTaskRelayTarget,
    status: &codux_runtime::ai_runtime::TerminalStatusEvent,
) -> bool {
    target.terminal_id == status.terminal_id
        && target.terminal_instance_id == status.terminal_instance_id
}

fn relay_terminal_status_acknowledges_task(
    target: &AgentTaskRelayTarget,
    task: &codux_runtime::agent_task_relay::AgentTaskRelayTask,
    status: &codux_runtime::ai_runtime::TerminalStatusEvent,
) -> bool {
    relay_target_matches_terminal_status(target, status)
        && status.state == codux_runtime::ai_runtime::TerminalStatusState::Working
        && task.state == AgentTaskRelayTaskState::AwaitingAck
        && task
            .dispatch_started_at
            .is_some_and(|started| status.updated_at + 0.001 >= started)
}

fn relay_terminal_completion_id(status: &codux_runtime::ai_runtime::TerminalStatusEvent) -> String {
    // The source and microsecond timestamp keep independent OSC completion
    // signals unique while repeated delivery of the same event stays idempotent.
    format!(
        "terminal-status:{}:{}:{}:{:.6}",
        status.terminal_id,
        status.terminal_instance_id.as_deref().unwrap_or("none"),
        status.source,
        status.updated_at
    )
}

/// Agent session files can remain in `responding` briefly after a turn ends.
/// Completed is an explicit full-turn boundary for every supported Agent;
/// Claude also needs Idle as a bounded fallback because its terminal bridge may
/// omit Completed. The caller already enforces exact terminal-instance identity.
fn relay_runtime_allows_dispatch(
    tool: &str,
    runtime_state: &str,
    last_user_input_at: Option<f64>,
    terminal_status: Option<&codux_runtime::ai_runtime::TerminalStatusEvent>,
) -> bool {
    if runtime_state == "idle" {
        return true;
    }
    if runtime_state != "responding" {
        return false;
    }
    let Some(last_user_input_at) = last_user_input_at else {
        return false;
    };
    terminal_status.is_some_and(|status| {
        let is_completed =
            status.state == codux_runtime::ai_runtime::TerminalStatusState::Completed;
        let is_claude_idle = tool.eq_ignore_ascii_case("claude")
            && status.state == codux_runtime::ai_runtime::TerminalStatusState::Idle;
        (is_completed || is_claude_idle) && status.updated_at + 0.001 >= last_user_input_at
    })
}

impl Render for AgentTaskRelayView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let app_entity = self.app_entity.clone();
        let language = self.snapshot.language.clone();
        let board = self.snapshot.board.clone();
        let target = self.snapshot.target.clone();
        let counts = board.as_ref().map(|board| board.counts).unwrap_or_default();
        let running = board
            .as_ref()
            .is_some_and(|board| board.state == AgentTaskRelayBoardState::Running);
        let can_start = target.is_some() && counts.queued > 0 && counts.waiting == 0;
        let editing = self.editing_task_id.is_some();
        let input = self.input.clone();

        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            // The fixed-width assistant rail uses a compact two-level type
            // hierarchy so headings and controls stay balanced at 320 px.
            .child(
                div()
                    .h(px(44.0))
                    .flex_none()
                    .px_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(Icon::new(HeroIconName::QueueList).size_4())
                            .child(relay_text(&language, "ai.relay.title", "Task Relay")),
                    )
                    .child(
                        Button::new("agent-relay-toggle")
                            .compact()
                            .disabled(!running && !can_start)
                            .icon(
                                Icon::new(if running {
                                    HeroIconName::Pause
                                } else {
                                    HeroIconName::Play
                                })
                                .size_3p5(),
                            )
                            .child(div().text_xs().child(if running {
                                relay_text(&language, "ai.relay.pause", "Pause")
                            } else {
                                format!(
                                    "{} {}",
                                    relay_text(&language, "ai.relay.start", "Start Relay"),
                                    counts.queued
                                )
                            }))
                            .on_click(move |_, _window, cx| {
                                cx.update_entity(&app_entity, |app, cx| {
                                    app.start_or_pause_agent_task_relay(cx)
                                });
                            }),
                    ),
            )
            .child(relay_target_row(target.as_ref(), cx))
            .child(relay_counts_row(counts, cx))
            .when_some(
                board.as_ref().and_then(relay_block_reason),
                |this, reason| {
                    this.child(relay_block_band(
                        reason,
                        board.as_ref(),
                        self.app_entity.clone(),
                        cx,
                    ))
                },
            )
            .child(relay_tabs(
                self.showing_receipts,
                counts,
                self.app_entity.clone(),
                cx,
            ))
            .when_some(self.snapshot.error.clone(), |this, error| {
                this.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_xs()
                        .text_color(color(theme::RED))
                        .child(SharedString::from(error.to_string())),
                )
            })
            .when(!self.snapshot.loaded, |this| {
                this.child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(relay_text(
                            &language,
                            "ai.relay.loading",
                            "Loading relay tasks...",
                        )),
                )
            })
            .when(self.snapshot.loaded && editing, |this| {
                this.child(relay_editor(
                    self.input.clone(),
                    self.editing_task_id,
                    self.app_entity.clone(),
                    window,
                    cx,
                ))
            })
            .when(
                self.snapshot.loaded && !editing && self.showing_receipts,
                |this| {
                    this.child(relay_receipts(
                        board.as_ref(),
                        self.receipt_scroll_handle.clone(),
                        cx,
                    ))
                },
            )
            .when(
                self.snapshot.loaded && !editing && !self.showing_receipts,
                |this| {
                    this.child(relay_tasks(
                        board.as_ref(),
                        self.app_entity.clone(),
                        self.task_scroll_handle.clone(),
                        cx,
                    ))
                    .child(
                        Button::new("agent-relay-add")
                            .ghost()
                            .w_full()
                            .h(px(40.0))
                            .disabled(
                                counts.queued + counts.running + counts.waiting >= MAX_RELAY_TASKS,
                            )
                            .icon(Icon::new(HeroIconName::Plus).size_3p5())
                            .child(div().text_sm().child(relay_text(
                                &language,
                                "ai.relay.add",
                                "Add task",
                            )))
                            .on_click(cx.listener(move |view, _event, window, cx| {
                                view.editing_task_id = Some(0);
                                view.editing_scope = relay_editor_scope(&view.snapshot);
                                input.update(cx, |input, cx| {
                                    input.set_value("", window, cx);
                                    input.focus(window, cx);
                                });
                                cx.notify();
                            })),
                    )
                },
            )
            .into_any_element()
    }
}

fn relay_target_row(
    target: Option<&AgentTaskRelayTarget>,
    cx: &mut Context<AgentTaskRelayView>,
) -> impl IntoElement {
    div()
        .h(px(38.0))
        .flex_none()
        .px_3()
        .flex()
        .items_center()
        .justify_between()
        .border_b_1()
        .border_color(cx.theme().border)
        .text_xs()
        .child(div().min_w_0().truncate().child(target.map_or_else(
            || "未选择已识别的 Agent".to_string(),
            |target| format!("{} · {}", target.tool, target.terminal_id),
        )))
        .child(
            div()
                .flex_none()
                .text_color(if target.is_some() {
                    color(theme::GREEN)
                } else {
                    cx.theme().muted_foreground
                })
                .child(if target.is_some() {
                    "支持自动接力"
                } else {
                    "不可用"
                }),
        )
}

fn relay_counts_row(
    counts: codux_runtime::agent_task_relay::AgentTaskRelayCounts,
    cx: &mut Context<AgentTaskRelayView>,
) -> impl IntoElement {
    div()
        .h(px(46.0))
        .flex_none()
        .grid()
        .grid_cols(3)
        .border_b_1()
        .border_color(cx.theme().border)
        .child(relay_count("执行中", counts.running, color(theme::GREEN)))
        .child(relay_count("等待中", counts.waiting, color(theme::ORANGE)))
        .child(relay_count(
            "排队中",
            counts.queued,
            cx.theme().muted_foreground,
        ))
}

fn relay_count(label: &'static str, value: usize, accent: gpui::Hsla) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(accent)
                .child(value.to_string()),
        )
        .child(
            div()
                .text_xs()
                .text_color(color(theme::TEXT_DIM))
                .child(label),
        )
}

fn relay_tabs(
    receipts: bool,
    counts: codux_runtime::agent_task_relay::AgentTaskRelayCounts,
    app_entity: gpui::Entity<CoduxApp>,
    cx: &mut Context<AgentTaskRelayView>,
) -> impl IntoElement {
    let _ = app_entity;
    div()
        .h(px(36.0))
        .flex_none()
        .px_2()
        .flex()
        .items_center()
        .gap_1()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            Button::new("agent-relay-tasks-tab")
                .ghost()
                .compact()
                .when(!receipts, |button| button.bg(cx.theme().accent))
                .child(div().text_xs().child(format!(
                    "任务 {}",
                    counts.queued + counts.running + counts.waiting
                )))
                .on_click(cx.listener(|view, _, _, cx| {
                    view.showing_receipts = false;
                    cx.notify();
                })),
        )
        .child(
            Button::new("agent-relay-receipts-tab")
                .ghost()
                .compact()
                .when(receipts, |button| button.bg(cx.theme().accent))
                .child(div().text_xs().child(format!("回执 {}", counts.receipts)))
                .on_click(cx.listener(|view, _, _, cx| {
                    view.showing_receipts = true;
                    cx.notify();
                })),
        )
}

fn relay_tasks(
    board: Option<&AgentTaskRelayBoard>,
    app_entity: gpui::Entity<CoduxApp>,
    scroll_handle: UniformListScrollHandle,
    cx: &mut Context<AgentTaskRelayView>,
) -> impl IntoElement {
    let tasks = board
        .map(|board| board.tasks.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let empty = tasks.is_empty();
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .when(empty, |this| {
            this.flex().items_center().justify_center().child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("暂无接力任务"),
            )
        })
        .when(!empty, |this| {
            this.child(codux_uniform_list(
                "agent-task-relay-tasks",
                Rc::new(tasks),
                scroll_handle,
                None,
                cx,
                move |task, _, _, cx| {
                    relay_task_row(task, app_entity.clone(), cx).into_any_element()
                },
            ))
        })
}

fn relay_task_row(
    task: codux_runtime::agent_task_relay::AgentTaskRelayTask,
    app_entity: gpui::Entity<CoduxApp>,
    cx: &mut Context<AgentTaskRelayView>,
) -> impl IntoElement {
    let promote_entity = app_entity.clone();
    let up_entity = app_entity.clone();
    let down_entity = app_entity.clone();
    let delete_entity = app_entity;
    let task_id = task.id;
    let editable = task.state == AgentTaskRelayTaskState::Queued;
    let edit_text = task.text.clone();
    div()
        .h(px(64.0))
        .flex_none()
        .px_2()
        .flex()
        .items_center()
        .gap_1()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            Icon::new(if editable {
                HeroIconName::Bars3
            } else {
                relay_task_icon(task.state)
            })
            .size_3p5()
            .text_color(cx.theme().muted_foreground),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .line_clamp(2)
                        .child(task.text.as_ref().to_string()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!(
                            "#{task_id} · {}",
                            relay_task_state_label(task.state)
                        )),
                ),
        )
        .when(editable, |this| {
            this.child(
                Button::new(SharedString::from(format!("relay-edit-{task_id}")))
                    .ghost()
                    .compact()
                    .w(px(24.0))
                    .icon(Icon::new(HeroIconName::PencilSquare).size_3())
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.editing_task_id = Some(task_id);
                        view.editing_scope = relay_editor_scope(&view.snapshot);
                        view.input.update(cx, |input, cx| {
                            input.set_value(edit_text.as_ref(), window, cx);
                            input.focus(window, cx);
                        });
                        cx.notify();
                    })),
            )
            .child(relay_icon_button(
                format!("relay-promote-{task_id}"),
                HeroIconName::ArrowUp,
                move |cx| {
                    cx.update_entity(&promote_entity, |app, cx| {
                        app.update_agent_task_relay_task(task_id, RelayTaskAction::Promote, cx)
                    });
                },
            ))
            .child(relay_icon_button(
                format!("relay-up-{task_id}"),
                HeroIconName::ChevronUp,
                move |cx| {
                    cx.update_entity(&up_entity, |app, cx| {
                        app.update_agent_task_relay_task(task_id, RelayTaskAction::MoveUp, cx)
                    });
                },
            ))
            .child(relay_icon_button(
                format!("relay-down-{task_id}"),
                HeroIconName::ChevronDown,
                move |cx| {
                    cx.update_entity(&down_entity, |app, cx| {
                        app.update_agent_task_relay_task(task_id, RelayTaskAction::MoveDown, cx)
                    });
                },
            ))
            .child(relay_icon_button(
                format!("relay-delete-{task_id}"),
                HeroIconName::Trash,
                move |cx| {
                    cx.update_entity(&delete_entity, |app, cx| {
                        app.update_agent_task_relay_task(task_id, RelayTaskAction::Delete, cx)
                    });
                },
            ))
        })
}

fn relay_icon_button(
    id: String,
    icon: HeroIconName,
    action: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    Button::new(SharedString::from(id))
        .ghost()
        .compact()
        .w(px(24.0))
        .icon(Icon::new(icon).size_3())
        .on_click(move |_, _, cx| action(cx))
}

fn relay_receipts(
    board: Option<&AgentTaskRelayBoard>,
    scroll_handle: UniformListScrollHandle,
    cx: &mut Context<AgentTaskRelayView>,
) -> impl IntoElement {
    let receipts = board
        .map(|board| board.receipts.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let empty = receipts.is_empty();
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .when(empty, |this| {
            this.flex().items_center().justify_center().child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("暂无任务回执"),
            )
        })
        .when(!empty, |this| {
            this.child(codux_uniform_list(
                "agent-task-relay-receipts",
                Rc::new(receipts),
                scroll_handle,
                None,
                cx,
                move |receipt, _, _, cx| {
                    div()
                        .h(px(72.0))
                        .flex_none()
                        .px_3()
                        .py_2()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(
                            div()
                                .text_sm()
                                .line_clamp(2)
                                .child(receipt.task_preview.as_ref().to_string()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "#{} · {}",
                                    receipt.task_id,
                                    relay_task_state_label(receipt.state)
                                )),
                        )
                        .into_any_element()
                },
            ))
        })
}

fn relay_editor(
    input: gpui::Entity<InputState>,
    editing_task_id: Option<u64>,
    app_entity: gpui::Entity<CoduxApp>,
    _window: &mut Window,
    cx: &mut Context<AgentTaskRelayView>,
) -> impl IntoElement {
    let save_input = input.clone();
    let cancel_input = input.clone();
    let save_entity = app_entity.clone();
    let editor_view = cx.entity();
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .p_3()
        .gap_3()
        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(
            if editing_task_id == Some(0) {
                "新建接力任务"
            } else {
                "编辑接力任务"
            },
        ))
        .child(div().flex_1().min_h_0().child(Input::new(&input).h_full()))
        .child(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("relay-editor-cancel")
                        .ghost()
                        .child(div().text_sm().child("取消"))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.editing_task_id = None;
                            view.editing_scope = None;
                            cancel_input.update(cx, |input, cx| input.set_value("", window, cx));
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("relay-editor-save")
                        .primary()
                        .child(div().text_sm().child("保存"))
                        .on_click(move |_, window, cx| {
                            let text = save_input.read(cx).value().to_string();
                            let (task_id, scope_matches) = {
                                let view = editor_view.read(cx);
                                (
                                    view.editing_task_id,
                                    view.editing_scope.as_ref()
                                        == relay_editor_scope(&view.snapshot).as_ref(),
                                )
                            };
                            if !scope_matches {
                                return;
                            }
                            // The parent update refreshes this child view. Run it
                            // outside a child listener update to avoid GPUI entity
                            // re-entry and the resulting process abort.
                            let saved = cx.update_entity(&save_entity, |app, cx| {
                                if let Some(task_id) = task_id.filter(|task_id| *task_id != 0) {
                                    app.edit_agent_task_relay_task(task_id, text, cx)
                                } else {
                                    app.add_agent_task_relay_task(text, cx)
                                }
                            });
                            if saved {
                                cx.update_entity(&editor_view, |view, cx| {
                                    view.editing_task_id = None;
                                    view.editing_scope = None;
                                    save_input
                                        .update(cx, |input, cx| input.set_value("", window, cx));
                                    cx.notify();
                                });
                            }
                        }),
                ),
        )
}

fn relay_block_reason(board: &AgentTaskRelayBoard) -> Option<AgentTaskRelayBlockReason> {
    match &board.state {
        AgentTaskRelayBoardState::PausedBlocked(reason) => Some(reason.clone()),
        _ => None,
    }
}

fn relay_block_band(
    reason: AgentTaskRelayBlockReason,
    board: Option<&AgentTaskRelayBoard>,
    app_entity: gpui::Entity<CoduxApp>,
    _cx: &mut Context<AgentTaskRelayView>,
) -> impl IntoElement {
    let task_id = board
        .and_then(|board| board.active_task())
        .map(|task| task.id);
    let resend_entity = app_entity.clone();
    div()
        .min_h(px(36.0))
        .flex_none()
        .px_3()
        .py_2()
        .flex()
        .items_center()
        .gap_2()
        .bg(color(theme::ORANGE).opacity(0.12))
        .child(Icon::new(HeroIconName::ExclamationTriangle).size_3p5())
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .child(format!("{}", relay_block_label(&reason))),
        )
        .when(
            reason == AgentTaskRelayBlockReason::DeliveryUnknown && task_id.is_some(),
            |this| {
                let task_id = task_id.unwrap_or_default();
                this.child(
                    Button::new("relay-recovery-handled")
                        .ghost()
                        .compact()
                        .child(div().text_xs().child("已处理"))
                        .on_click(move |_, _, cx| {
                            cx.update_entity(&app_entity, |app, cx| {
                                app.update_agent_task_relay_task(
                                    task_id,
                                    RelayTaskAction::MarkHandled,
                                    cx,
                                )
                            });
                        }),
                )
                .child(
                    Button::new("relay-recovery-resend")
                        .ghost()
                        .compact()
                        .child(div().text_xs().child("重新发送"))
                        .on_click(move |_, _, cx| {
                            cx.update_entity(&resend_entity, |app, cx| {
                                app.update_agent_task_relay_task(
                                    task_id,
                                    RelayTaskAction::Resend,
                                    cx,
                                )
                            });
                        }),
                )
            },
        )
}

fn relay_block_label(reason: &AgentTaskRelayBlockReason) -> &'static str {
    match reason {
        AgentTaskRelayBlockReason::NeedsInput => "等待用户输入",
        AgentTaskRelayBlockReason::DeliveryUnknown => "无法确认任务是否已发送",
        AgentTaskRelayBlockReason::PersistenceError => "无法保存任务接力状态",
        AgentTaskRelayBlockReason::TargetLost => "目标 Agent 当前不可用",
        AgentTaskRelayBlockReason::SendFailed => "任务发送失败",
        AgentTaskRelayBlockReason::Interrupted => "Agent 会话已中断",
    }
}

fn relay_task_state_label(state: AgentTaskRelayTaskState) -> &'static str {
    match state {
        AgentTaskRelayTaskState::Queued => "排队中",
        AgentTaskRelayTaskState::Dispatching => "正在发送",
        AgentTaskRelayTaskState::AwaitingAck => "等待 Agent 确认",
        AgentTaskRelayTaskState::Running => "执行中",
        AgentTaskRelayTaskState::NeedsInput => "等待输入",
        AgentTaskRelayTaskState::Completed => "已完成",
        AgentTaskRelayTaskState::Failed => "失败",
        AgentTaskRelayTaskState::DeliveryUnknown => "发送状态未知",
        AgentTaskRelayTaskState::Skipped => "已跳过",
    }
}

fn relay_error_label(error: &str) -> &str {
    match error {
        "Task text is required." => "请输入任务内容。",
        "Task text is too large." => "任务内容过长。",
        "Task relay is full." => "任务接力队列已满。",
        "Task relay text limit reached." => "任务接力内容已达到容量上限。",
        "Only queued tasks can be edited." => "只能编辑排队中的任务。",
        "Resolve the blocked relay state before starting relay." => {
            "请先处理当前阻塞状态，再开始接力。"
        }
        "Resolve the blocked task before starting relay." => "请先处理当前阻塞任务，再开始接力。",
        "Add a task before starting relay." => "请先添加任务，再开始接力。",
        "Cannot change task relay target while a task is active." => {
            "当前任务仍在执行，暂时不能切换接力终端。"
        }
        "Task relay target does not belong to this board." => "接力任务与当前终端不匹配。",
        _ => error,
    }
}

fn relay_task_icon(state: AgentTaskRelayTaskState) -> HeroIconName {
    match state {
        AgentTaskRelayTaskState::Queued => HeroIconName::Clock,
        AgentTaskRelayTaskState::Dispatching | AgentTaskRelayTaskState::AwaitingAck => {
            HeroIconName::PaperAirplane
        }
        AgentTaskRelayTaskState::Running => HeroIconName::Play,
        AgentTaskRelayTaskState::NeedsInput | AgentTaskRelayTaskState::DeliveryUnknown => {
            HeroIconName::ExclamationTriangle
        }
        AgentTaskRelayTaskState::Completed => HeroIconName::Check,
        AgentTaskRelayTaskState::Failed => HeroIconName::XMark,
        AgentTaskRelayTaskState::Skipped => HeroIconName::Forward,
    }
}

fn relay_text(language: &str, key: &str, fallback: &str) -> String {
    // This panel currently owns Chinese-only domain labels. Keeping its
    // translated actions together prevents a mixed-language control surface.
    let localized = match key {
        "ai.relay.title" => Some("任务接力"),
        "ai.relay.pause" => Some("暂停"),
        "ai.relay.start" => Some("开始接力"),
        "ai.relay.add" => Some("添加任务"),
        "ai.relay.loading" => Some("正在加载接力任务..."),
        _ => None,
    };
    if let Some(localized) = localized {
        return localized.to_string();
    }
    let locale = locale_from_language_setting(language);
    translate(&locale, key, fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(instance: &str, session: &str) -> AgentTaskRelayTarget {
        AgentTaskRelayTarget {
            project_id: "project-1".to_string(),
            worktree_id: "worktree-1".to_string(),
            terminal_id: "terminal-1".to_string(),
            terminal_instance_id: Some(instance.to_string()),
            tool: "codex".to_string(),
            ai_session_id: Some(session.to_string()),
        }
    }

    fn running_task() -> codux_runtime::agent_task_relay::AgentTaskRelayTask {
        codux_runtime::agent_task_relay::AgentTaskRelayTask {
            id: 1,
            text: Arc::from("relay task"),
            state: AgentTaskRelayTaskState::Running,
            created_at: 1.0,
            dispatch_started_at: Some(10.0),
            acknowledged_at: Some(12.0),
            completion_baseline: Some(9.0),
        }
    }

    fn completion() -> codux_runtime::ai_runtime::AIRuntimeSessionCompletionEvent {
        codux_runtime::ai_runtime::AIRuntimeSessionCompletionEvent {
            id: "completion-1".to_string(),
            project_id: "project-1".to_string(),
            project_name: "Project".to_string(),
            terminal_id: "terminal-1".to_string(),
            terminal_instance_id: Some("instance-1".to_string()),
            tool: "codex".to_string(),
            ai_session_id: Some("session-1".to_string()),
            turn_started_at: Some(10.0),
            completed_at: 20.0,
            has_completed_turn: true,
            was_interrupted: false,
            latest_assistant_preview: Some("done".to_string()),
            total_tokens: 100,
            cached_input_tokens: 10,
        }
    }

    fn terminal_status(
        instance: &str,
        state: codux_runtime::ai_runtime::TerminalStatusState,
        updated_at: f64,
    ) -> codux_runtime::ai_runtime::TerminalStatusEvent {
        codux_runtime::ai_runtime::TerminalStatusEvent {
            terminal_id: "terminal-1".to_string(),
            terminal_instance_id: Some(instance.to_string()),
            project_id: Some("project-1".to_string()),
            worktree_id: Some("worktree-1".to_string()),
            state,
            updated_at,
            source: "test".to_string(),
        }
    }

    #[test]
    fn relay_completion_requires_matching_flags_and_time_fences() {
        let task = running_task();
        assert_eq!(
            relay_completion_disposition(&task, &completion()),
            RelayCompletionDisposition::Completed
        );

        let mut stale_completion = completion();
        stale_completion.completed_at = 11.0;
        assert_eq!(
            relay_completion_disposition(&task, &stale_completion),
            RelayCompletionDisposition::Ignore
        );

        let mut stale_turn = completion();
        stale_turn.turn_started_at = Some(8.0);
        assert_eq!(
            relay_completion_disposition(&task, &stale_turn),
            RelayCompletionDisposition::Ignore
        );

        let mut incomplete = completion();
        incomplete.has_completed_turn = false;
        assert_eq!(
            relay_completion_disposition(&task, &incomplete),
            RelayCompletionDisposition::Ignore
        );

        let mut interrupted = completion();
        interrupted.has_completed_turn = false;
        interrupted.was_interrupted = true;
        assert_eq!(
            relay_completion_disposition(&task, &interrupted),
            RelayCompletionDisposition::Interrupted
        );
    }

    #[test]
    fn relay_editor_scope_tracks_the_stable_board_not_the_session_lease() {
        let snapshot = |target| AgentTaskRelaySnapshot {
            language: Arc::from("en"),
            target: Some(target),
            board: None,
            loaded: true,
            error: None,
        };
        let first = relay_editor_scope(&snapshot(target("instance-1", "session-1"))).unwrap();
        let same = relay_editor_scope(&snapshot(target("instance-1", "session-1"))).unwrap();
        let new_instance =
            relay_editor_scope(&snapshot(target("instance-2", "session-1"))).unwrap();
        let new_session = relay_editor_scope(&snapshot(target("instance-1", "session-2"))).unwrap();
        let mut other_terminal_target = target("instance-1", "session-1");
        other_terminal_target.terminal_id = "terminal-2".to_string();
        let other_board = relay_editor_scope(&snapshot(other_terminal_target)).unwrap();

        assert_eq!(first, same);
        assert_eq!(first, new_instance);
        assert_eq!(first, new_session);
        assert_ne!(first, other_board);
    }

    #[test]
    fn relay_working_status_acknowledges_only_the_current_dispatch_instance() {
        let mut task = running_task();
        task.state = AgentTaskRelayTaskState::AwaitingAck;
        task.acknowledged_at = None;
        let current = target("instance-1", "session-1");

        assert!(relay_terminal_status_acknowledges_task(
            &current,
            &task,
            &terminal_status(
                "instance-1",
                codux_runtime::ai_runtime::TerminalStatusState::Working,
                10.0
            )
        ));
        assert!(!relay_terminal_status_acknowledges_task(
            &current,
            &task,
            &terminal_status(
                "instance-1",
                codux_runtime::ai_runtime::TerminalStatusState::Working,
                9.0
            )
        ));
        assert!(!relay_terminal_status_acknowledges_task(
            &current,
            &task,
            &terminal_status(
                "instance-2",
                codux_runtime::ai_runtime::TerminalStatusState::Working,
                11.0
            )
        ));
    }

    #[test]
    fn stale_responding_state_uses_a_fresh_completed_terminal_signal() {
        let completed = terminal_status(
            "instance-1",
            codux_runtime::ai_runtime::TerminalStatusState::Completed,
            20.0,
        );
        assert!(relay_runtime_allows_dispatch(
            "claude",
            "responding",
            Some(19.0),
            Some(&completed)
        ));

        let stale = terminal_status(
            "instance-1",
            codux_runtime::ai_runtime::TerminalStatusState::Completed,
            18.0,
        );
        assert!(!relay_runtime_allows_dispatch(
            "claude",
            "responding",
            Some(19.0),
            Some(&stale)
        ));
        assert!(!relay_runtime_allows_dispatch(
            "claude",
            "needsInput",
            Some(19.0),
            Some(&completed)
        ));
        assert!(relay_runtime_allows_dispatch(
            "codex",
            "responding",
            Some(19.0),
            Some(&completed)
        ));
        let idle = terminal_status(
            "instance-1",
            codux_runtime::ai_runtime::TerminalStatusState::Idle,
            20.0,
        );
        assert!(relay_runtime_allows_dispatch(
            "claude",
            "responding",
            Some(19.0),
            Some(&idle)
        ));
        assert!(!relay_runtime_allows_dispatch(
            "codex",
            "responding",
            Some(19.0),
            Some(&idle)
        ));
        assert!(relay_runtime_allows_dispatch(
            "codex",
            "idle",
            Some(19.0),
            None
        ));
    }

    #[test]
    fn terminal_completion_signal_finishes_a_running_task_without_session_event() {
        let mut board = AgentTaskRelayBoard::new("board-1", target("instance-1", "session-1"));
        let task_id = board.add_task("first".to_string(), 1.0).unwrap();
        board.add_task("second".to_string(), 2.0).unwrap();
        board.start().unwrap();
        assert_eq!(board.begin_dispatch(10.0, Some(9.0)), Some(task_id));
        assert!(board.mark_write_succeeded(task_id, 10.1));
        assert!(board.mark_acknowledged(task_id, 10.2));
        let status = terminal_status(
            "instance-1",
            codux_runtime::ai_runtime::TerminalStatusState::Completed,
            11.0,
        );

        assert!(board.mark_completed(
            task_id,
            relay_terminal_completion_id(&status),
            None,
            status.updated_at
        ));
        assert_eq!(board.counts.queued, 1);
        assert_eq!(board.counts.running, 0);
        assert_eq!(board.counts.receipts, 1);
    }

    #[test]
    fn resend_command_requeues_and_immediately_resumes_the_board() {
        let mut board = AgentTaskRelayBoard::new("board-1", target("instance-1", "session-1"));
        let task_id = board.add_task("retry".to_string(), 1.0).unwrap();
        board.start().unwrap();
        assert_eq!(board.begin_dispatch(2.0, None), Some(task_id));
        assert!(board.mark_write_succeeded(task_id, 2.1));
        assert!(board.mark_delivery_unknown(task_id));

        assert!(retry_agent_task_relay_task(&mut board, task_id).unwrap());
        assert_eq!(board.state, AgentTaskRelayBoardState::Running);
        assert!(!board.paused_by_user);
        assert_eq!(board.tasks[0].state, AgentTaskRelayTaskState::Queued);
    }
}
