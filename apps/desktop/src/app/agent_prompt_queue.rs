use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

use super::ui_helpers::codux_tooltip_container;
use super::*;

pub(super) const MAX_AGENT_PROMPTS_PER_SESSION: usize = 100;
pub(super) const MAX_AGENT_PROMPT_BYTES: usize = 256 * 1024;
pub(super) const MAX_AGENT_QUEUE_SESSIONS: usize = 32;
const NATIVE_SUBMISSION_GUARD: Duration = Duration::from_secs(5);
const AGENT_ACK_TIMEOUT: Duration = Duration::from_secs(15);
const AGENT_FOLLOW_UP_ACK_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const AGENT_QUEUE_MIN_LIST_HEIGHT: f32 = 200.0;
const AGENT_QUEUE_MAX_LIST_HEIGHT: f32 = 720.0;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct AgentPromptQueueKey {
    pub(super) terminal_id: String,
    pub(super) terminal_instance_id: String,
    pub(super) ai_session_id: Option<String>,
    pub(super) tool: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Queue ownership state for one prompt. Dispatching covers the background PTY
/// write, AwaitingAgent covers delivery acknowledgement, and only Pending or
/// Failed items may be changed by the user.
pub(super) enum AgentPromptStatus {
    Pending,
    Dispatching,
    DispatchingAcknowledged,
    AwaitingAgent,
    Failed(Arc<str>),
}

impl AgentPromptStatus {
    pub(super) fn is_editable(&self) -> bool {
        matches!(self, Self::Pending | Self::Failed(_))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AgentPromptItem {
    pub(super) id: u64,
    pub(super) text: Arc<str>,
    pub(super) preview: Arc<str>,
    pub(super) status: AgentPromptStatus,
    dispatch_started_at: Option<Instant>,
    queued_activity_at: Option<u64>,
    dispatch_user_input_at: Option<u64>,
    dispatched_while_responding: bool,
}

#[derive(Clone)]
pub(super) struct AgentPromptDispatch {
    pub(super) key: AgentPromptQueueKey,
    pub(super) item_id: u64,
    pub(super) text: Arc<str>,
}

#[derive(Default)]
pub(super) struct AgentPromptQueueStore {
    queues: HashMap<AgentPromptQueueKey, VecDeque<AgentPromptItem>>,
    /// Direct native submissions briefly block automatic dispatch until the
    /// supervisor confirms that the Agent started. Timestamps bound event-loss
    /// recovery so one missed transition cannot freeze a session forever.
    native_submissions: HashMap<AgentPromptQueueKey, Instant>,
    next_id: u64,
}

impl AgentPromptQueueStore {
    pub(super) fn len(&self, key: &AgentPromptQueueKey) -> usize {
        self.queues.get(key).map_or(0, VecDeque::len)
    }

    pub(super) fn items(&self, key: &AgentPromptQueueKey) -> Vec<AgentPromptItem> {
        self.queues
            .get(key)
            .map(|items| items.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub(super) fn keys(&self) -> Vec<AgentPromptQueueKey> {
        self.queues
            .keys()
            .chain(self.native_submissions.keys())
            .cloned()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect()
    }

    pub(super) fn note_native_submission(&mut self, key: AgentPromptQueueKey) {
        self.native_submissions.insert(key, Instant::now());
    }

    pub(super) fn native_submission_pending(&mut self, key: &AgentPromptQueueKey) -> bool {
        if self
            .native_submissions
            .get(key)
            .is_some_and(|started| started.elapsed() < NATIVE_SUBMISSION_GUARD)
        {
            return true;
        }
        self.native_submissions.remove(key);
        false
    }

    #[cfg(test)]
    pub(super) fn enqueue(
        &mut self,
        key: AgentPromptQueueKey,
        text: String,
    ) -> Result<u64, &'static str> {
        self.enqueue_at(key, text, None)
    }

    pub(super) fn enqueue_at(
        &mut self,
        key: AgentPromptQueueKey,
        text: String,
        runtime_activity_at: Option<f64>,
    ) -> Result<u64, &'static str> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err("agent queue message is empty");
        }
        if trimmed.len() > MAX_AGENT_PROMPT_BYTES {
            return Err("agent queue message is too large");
        }
        // Keep the common path allocation-free after routing hands over its
        // owned String; only surrounding whitespace requires another copy.
        let text = if trimmed.len() == text.len() {
            text
        } else {
            trimmed.to_string()
        };
        if !self.queues.contains_key(&key) && self.queues.len() >= MAX_AGENT_QUEUE_SESSIONS {
            return Err("too many agent queues are active");
        }
        let queue = self.queues.entry(key).or_default();
        if queue.len() >= MAX_AGENT_PROMPTS_PER_SESSION {
            return Err("agent queue is full");
        }

        self.next_id = self.next_id.wrapping_add(1).max(1);
        let id = self.next_id;
        queue.push_back(AgentPromptItem {
            id,
            preview: prompt_preview(&text),
            text: Arc::from(text),
            status: AgentPromptStatus::Pending,
            dispatch_started_at: None,
            queued_activity_at: runtime_activity_at.map(f64::to_bits),
            dispatch_user_input_at: None,
            dispatched_while_responding: false,
        });
        Ok(id)
    }

    pub(super) fn promote(&mut self, key: &AgentPromptQueueKey, item_id: u64) -> bool {
        let Some(queue) = self.queues.get_mut(key) else {
            return false;
        };
        let Some(index) = editable_item_index(queue, item_id) else {
            return false;
        };
        let Some(item) = queue.remove(index) else {
            return false;
        };
        // A message already being submitted keeps ownership of the head slot.
        let target = usize::from(queue.front().is_some_and(|item| !item.status.is_editable()));
        queue.insert(target, item);
        true
    }

    pub(super) fn move_item(
        &mut self,
        key: &AgentPromptQueueKey,
        item_id: u64,
        direction: isize,
    ) -> bool {
        let Some(queue) = self.queues.get_mut(key) else {
            return false;
        };
        let Some(index) = editable_item_index(queue, item_id) else {
            return false;
        };
        let target = if direction < 0 {
            index.checked_sub(1)
        } else {
            index.checked_add(1).filter(|target| *target < queue.len())
        };
        let Some(target) = target else {
            return false;
        };
        if !queue[target].status.is_editable() {
            return false;
        }
        queue.swap(index, target);
        true
    }

    pub(super) fn remove(&mut self, key: &AgentPromptQueueKey, item_id: u64) -> bool {
        let Some(queue) = self.queues.get_mut(key) else {
            return false;
        };
        let Some(index) = editable_item_index(queue, item_id) else {
            return false;
        };
        queue.remove(index);
        self.remove_empty_queue(key);
        true
    }

    pub(super) fn replace(
        &mut self,
        key: &AgentPromptQueueKey,
        item_id: u64,
        text: String,
    ) -> Result<bool, &'static str> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err("agent queue message is empty");
        }
        if trimmed.len() > MAX_AGENT_PROMPT_BYTES {
            return Err("agent queue message is too large");
        }
        let text = if trimmed.len() == text.len() {
            text
        } else {
            trimmed.to_string()
        };
        let Some(queue) = self.queues.get_mut(key) else {
            return Ok(false);
        };
        let Some(index) = editable_item_index(queue, item_id) else {
            return Ok(false);
        };
        queue[index].preview = prompt_preview(&text);
        queue[index].text = Arc::from(text);
        queue[index].status = AgentPromptStatus::Pending;
        queue[index].dispatch_started_at = None;
        queue[index].dispatch_user_input_at = None;
        queue[index].dispatched_while_responding = false;
        Ok(true)
    }

    pub(super) fn retry(&mut self, key: &AgentPromptQueueKey, item_id: u64) -> bool {
        let Some(item) = self
            .queues
            .get_mut(key)
            .and_then(|queue| queue.iter_mut().find(|item| item.id == item_id))
        else {
            return false;
        };
        if !matches!(item.status, AgentPromptStatus::Failed(_)) {
            return false;
        }
        item.status = AgentPromptStatus::Pending;
        item.dispatch_started_at = None;
        item.dispatch_user_input_at = None;
        item.dispatched_while_responding = false;
        true
    }

    #[cfg(test)]
    pub(super) fn begin_dispatch(
        &mut self,
        key: &AgentPromptQueueKey,
    ) -> Option<AgentPromptDispatch> {
        self.begin_dispatch_for_runtime(key, "idle", None, None)
    }

    pub(super) fn begin_dispatch_for_runtime(
        &mut self,
        key: &AgentPromptQueueKey,
        runtime_state: &str,
        runtime_activity_at: Option<f64>,
        last_user_input_at: Option<f64>,
    ) -> Option<AgentPromptDispatch> {
        // A native Enter has already handed a prompt to this Agent. Keep the
        // next queued item blocked until a supervisor event confirms that the
        // Agent started, including the short event-propagation race window.
        if self.native_submission_pending(key) {
            return None;
        }
        let queue = self.queues.get_mut(key)?;
        if queue.iter().any(|item| {
            matches!(
                item.status,
                AgentPromptStatus::Dispatching
                    | AgentPromptStatus::DispatchingAcknowledged
                    | AgentPromptStatus::AwaitingAgent
            )
        }) {
            return None;
        }
        // Preserve queue ordering: a failed head blocks later messages until
        // the user retries, edits, deletes, or explicitly reorders it.
        let item = queue.front_mut()?;
        if !matches!(item.status, AgentPromptStatus::Pending) {
            return None;
        }
        let dispatched_while_responding = runtime_state == "responding";
        if runtime_state != "idle"
            && (!dispatched_while_responding
                || last_user_input_at.is_none()
                || !timestamp_advanced(
                    item.queued_activity_at,
                    runtime_activity_at.map(f64::to_bits),
                ))
        {
            return None;
        }
        item.status = AgentPromptStatus::Dispatching;
        item.dispatch_started_at = Some(Instant::now());
        item.dispatch_user_input_at = last_user_input_at.map(f64::to_bits);
        item.dispatched_while_responding = dispatched_while_responding;
        Some(AgentPromptDispatch {
            key: key.clone(),
            item_id: item.id,
            text: item.text.clone(),
        })
    }

    pub(super) fn finish_write(
        &mut self,
        key: &AgentPromptQueueKey,
        item_id: u64,
        error: Option<String>,
    ) -> bool {
        let Some(queue) = self.queues.get_mut(key) else {
            return false;
        };
        let Some(index) = queue.iter().position(|item| item.id == item_id) else {
            return false;
        };
        match queue[index].status {
            // The supervisor already observed the Agent running, so the prompt
            // was accepted even if the final chunk write reported an error.
            AgentPromptStatus::DispatchingAcknowledged => {
                queue.remove(index);
                self.remove_empty_queue(key);
            }
            AgentPromptStatus::Dispatching => {
                queue[index].status = match error {
                    Some(error) => AgentPromptStatus::Failed(Arc::from(error)),
                    None => AgentPromptStatus::AwaitingAgent,
                };
                if matches!(queue[index].status, AgentPromptStatus::Failed(_)) {
                    queue[index].dispatch_started_at = None;
                }
            }
            _ => return false,
        }
        true
    }

    pub(super) fn expire_unacknowledged(&mut self, key: &AgentPromptQueueKey) -> bool {
        let Some(item) = self.queues.get_mut(key).and_then(|queue| queue.front_mut()) else {
            return false;
        };
        let timeout = if item.dispatched_while_responding {
            // Native follow-up queues may legitimately wait through a long tool
            // call before consuming the message. Keep the short timeout for an
            // idle composer, but do not mislabel normal queued steering as a
            // failure after only a few seconds.
            AGENT_FOLLOW_UP_ACK_TIMEOUT
        } else {
            AGENT_ACK_TIMEOUT
        };
        if !matches!(item.status, AgentPromptStatus::AwaitingAgent)
            || !item
                .dispatch_started_at
                .is_some_and(|started| started.elapsed() >= timeout)
        {
            return false;
        }
        item.status = AgentPromptStatus::Failed(Arc::from(
            "Agent did not confirm this message; retry only if it was not received",
        ));
        item.dispatch_started_at = None;
        true
    }

    #[cfg(test)]
    pub(super) fn acknowledge_agent_started(&mut self, key: &AgentPromptQueueKey) -> bool {
        self.acknowledge_agent_started_at(key, None)
    }

    pub(super) fn acknowledge_agent_started_at(
        &mut self,
        key: &AgentPromptQueueKey,
        last_user_input_at: Option<f64>,
    ) -> bool {
        let native_acknowledged = self.native_submissions.remove(key).is_some();
        let Some(queue) = self.queues.get_mut(key) else {
            return native_acknowledged;
        };
        let Some(index) = queue.iter().position(|item| {
            matches!(
                item.status,
                AgentPromptStatus::Dispatching | AgentPromptStatus::AwaitingAgent
            )
        }) else {
            return native_acknowledged;
        };
        if queue[index].dispatched_while_responding
            && !timestamp_advanced(
                queue[index].dispatch_user_input_at,
                last_user_input_at.map(f64::to_bits),
            )
        {
            return native_acknowledged;
        }
        if matches!(queue[index].status, AgentPromptStatus::Dispatching) {
            queue[index].status = AgentPromptStatus::DispatchingAcknowledged;
        } else {
            queue.remove(index);
            self.remove_empty_queue(key);
        }
        true
    }

    pub(super) fn adopt_session_id(
        &mut self,
        old_key: &AgentPromptQueueKey,
        ai_session_id: String,
    ) -> AgentPromptQueueKey {
        let mut new_key = old_key.clone();
        new_key.ai_session_id = Some(ai_session_id);
        if &new_key == old_key {
            return new_key;
        }
        let Some(mut queue) = self.queues.remove(old_key) else {
            if let Some(started) = self.native_submissions.remove(old_key) {
                self.native_submissions.insert(new_key.clone(), started);
            }
            return new_key;
        };
        self.queues
            .entry(new_key.clone())
            .or_default()
            .append(&mut queue);
        if let Some(started) = self.native_submissions.remove(old_key) {
            self.native_submissions.insert(new_key.clone(), started);
        }
        new_key
    }

    pub(super) fn remove_terminal(&mut self, terminal_id: &str) -> bool {
        let before_queues = self.queues.len();
        let before_native = self.native_submissions.len();
        self.queues.retain(|key, _| key.terminal_id != terminal_id);
        self.native_submissions
            .retain(|key, _| key.terminal_id != terminal_id);
        before_queues != self.queues.len() || before_native != self.native_submissions.len()
    }

    fn remove_empty_queue(&mut self, key: &AgentPromptQueueKey) {
        if self.queues.get(key).is_some_and(VecDeque::is_empty) {
            self.queues.remove(key);
        }
    }
}

fn timestamp_advanced(baseline: Option<u64>, current: Option<u64>) -> bool {
    match (baseline, current) {
        (Some(baseline), Some(current)) => f64::from_bits(current) > f64::from_bits(baseline),
        (None, Some(_)) => true,
        _ => false,
    }
}

fn editable_item_index(queue: &VecDeque<AgentPromptItem>, item_id: u64) -> Option<usize> {
    queue
        .iter()
        .position(|item| item.id == item_id && item.status.is_editable())
}

fn prompt_preview(text: &str) -> Arc<str> {
    const MAX_PREVIEW_CHARS: usize = 72;
    let mut preview = String::with_capacity(MAX_PREVIEW_CHARS + 3);
    let mut pending_space = false;
    let mut truncated = false;
    let mut preview_chars = 0usize;
    for ch in text.chars() {
        if ch.is_whitespace() {
            pending_space = !preview.is_empty();
            continue;
        }
        if pending_space && preview_chars < MAX_PREVIEW_CHARS {
            preview.push(' ');
            preview_chars += 1;
        }
        pending_space = false;
        if preview_chars >= MAX_PREVIEW_CHARS {
            truncated = true;
            break;
        }
        preview.push(ch);
        preview_chars += 1;
    }
    if truncated {
        preview.push_str("...");
    }
    Arc::from(preview)
}

#[derive(Clone, PartialEq, Eq)]
struct AgentPromptQueueSnapshot {
    key: Option<AgentPromptQueueKey>,
    runtime_state: Arc<str>,
    items: Vec<AgentPromptItem>,
    language: Arc<str>,
}

impl Default for AgentPromptQueueSnapshot {
    fn default() -> Self {
        Self {
            key: None,
            runtime_state: Arc::from(""),
            items: Vec::new(),
            language: Arc::from("en"),
        }
    }
}

pub(in crate::app) struct AgentPromptQueueView {
    app_entity: gpui::Entity<CoduxApp>,
    input: gpui::Entity<InputState>,
    scroll_handle: UniformListScrollHandle,
    snapshot: AgentPromptQueueSnapshot,
    editing: Option<(AgentPromptQueueKey, u64)>,
    collapsed: bool,
}

impl CoduxApp {
    pub(in crate::app) fn route_terminal_agent_prompt(
        &mut self,
        terminal_id: &str,
        terminal_instance_id: &str,
        submission: TerminalAgentDraftSubmission,
        cx: &mut Context<Self>,
    ) -> TerminalAgentPromptDisposition {
        let Some(session) = self
            .state
            .ai_runtime_state
            .sessions
            .iter()
            .filter(|session| session.terminal_id == terminal_id)
            .filter(|session| session.terminal_instance_id.as_deref() == Some(terminal_instance_id))
            .filter(|session| {
                codux_runtime::ai_runtime::tool_driver::is_supported_runtime_tool(&session.tool)
            })
            .max_by(|left, right| left.updated_at.total_cmp(&right.updated_at))
        else {
            // Ordinary shells and unrecognized Agents retain the exact native
            // terminal input behavior.
            return TerminalAgentPromptDisposition::PassThrough;
        };
        let mut key = AgentPromptQueueKey {
            terminal_id: terminal_id.to_string(),
            terminal_instance_id: terminal_instance_id.to_string(),
            ai_session_id: session.ai_session_id.clone(),
            tool: session.tool.clone(),
        };
        let runtime_state = session.runtime_state.clone();
        let runtime_activity_at = session.runtime_activity_at;
        if let Some(provisional) = self
            .agent_prompt_queues
            .keys()
            .into_iter()
            .find(|existing| {
                existing.terminal_id == key.terminal_id
                    && existing.terminal_instance_id == key.terminal_instance_id
                    && existing.tool == key.tool
                    && existing.ai_session_id.is_none()
            })
            && let Some(ai_session_id) = key.ai_session_id.clone()
        {
            key = self
                .agent_prompt_queues
                .adopt_session_id(&provisional, ai_session_id);
        }
        // Approval and elicitation responses belong to the live Agent turn;
        // queueing them would prevent the Agent from leaving needsInput.
        if runtime_state == "needsInput" {
            return TerminalAgentPromptDisposition::PassThrough;
        }
        if matches!(submission, TerminalAgentDraftSubmission::TooLarge) {
            self.status_message = "Agent queue message is too large".to_string();
            return TerminalAgentPromptDisposition::Rejected;
        }
        let TerminalAgentDraftSubmission::Text(prompt) = submission else {
            unreachable!("oversized submissions return above")
        };
        if runtime_state == "idle"
            && self.agent_prompt_queues.items(&key).is_empty()
            && !self.agent_prompt_queues.native_submission_pending(&key)
            && !self.agent_task_relay_owns_ack(&key)
        {
            self.agent_prompt_queues.note_native_submission(key);
            return TerminalAgentPromptDisposition::PassThrough;
        }
        match self
            .agent_prompt_queues
            .enqueue_at(key, prompt, runtime_activity_at)
        {
            Ok(_) => {
                self.refresh_agent_prompt_queue_view(cx);
                TerminalAgentPromptDisposition::Queued
            }
            Err(error) => {
                self.status_message = error.to_string();
                TerminalAgentPromptDisposition::Rejected
            }
        }
    }

    fn active_agent_prompt_snapshot(&self) -> AgentPromptQueueSnapshot {
        let Some((key, runtime_state)) = self.active_agent_prompt_target() else {
            return AgentPromptQueueSnapshot {
                language: Arc::from(self.state.settings.language.as_str()),
                ..Default::default()
            };
        };
        AgentPromptQueueSnapshot {
            runtime_state: Arc::from(runtime_state),
            items: self.agent_prompt_queues.items(&key),
            key: Some(key),
            language: Arc::from(self.state.settings.language.as_str()),
        }
    }

    pub(in crate::app) fn active_agent_prompt_target(
        &self,
    ) -> Option<(AgentPromptQueueKey, String)> {
        let (_, slot) = self.active_terminal_slot()?;
        let terminal_id = slot.terminal_id.as_deref()?.trim();
        let pane = slot.pane.as_ref()?;
        let terminal_instance_id = pane.terminal_instance_id()?;
        let session = self
            .state
            .ai_runtime_state
            .sessions
            .iter()
            .filter(|session| session.terminal_id == terminal_id)
            // Unknown instance identity is deliberately not auto-sendable: a
            // stale summary must never target a newly-created terminal.
            .filter(|session| {
                session.terminal_instance_id.as_deref() == Some(terminal_instance_id.as_str())
            })
            .filter(|session| {
                codux_runtime::ai_runtime::tool_driver::is_supported_runtime_tool(&session.tool)
            })
            .max_by(|left, right| left.updated_at.total_cmp(&right.updated_at))?;
        Some((
            AgentPromptQueueKey {
                terminal_id: terminal_id.to_string(),
                terminal_instance_id,
                ai_session_id: session.ai_session_id.clone(),
                tool: session.tool.clone(),
            },
            session.runtime_state.clone(),
        ))
    }

    pub(in crate::app) fn active_agent_prompt_queue_count(&self) -> usize {
        self.active_agent_prompt_target()
            .map(|(key, _)| self.agent_prompt_queues.len(&key))
            .unwrap_or_default()
    }

    pub(in crate::app) fn agent_prompt_queue_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Entity<AgentPromptQueueView> {
        let snapshot = self.active_agent_prompt_snapshot();
        if let Some(view) = self.agent_prompt_queue_view.clone() {
            view.update(cx, |view, cx| {
                let same_session = agent_queue_keys_share_session(
                    view.snapshot.key.as_ref(),
                    snapshot.key.as_ref(),
                );
                if !same_session {
                    // A draft must never follow the user to another terminal or
                    // Agent session, where it could be submitted accidentally.
                    view.input
                        .update(cx, |input, cx| input.set_value("", window, cx));
                    view.editing = None;
                } else if view.snapshot.key != snapshot.key
                    && let (Some((_editing_key, item_id)), Some(new_key)) =
                        (view.editing.take(), snapshot.key.clone())
                {
                    view.editing = Some((new_key, item_id));
                }
                if view.snapshot != snapshot {
                    view.snapshot = snapshot;
                    cx.notify();
                }
            });
            return view;
        }

        let placeholder =
            agent_queue_text(&snapshot.language, "ai.queue.edit", "Edit queued message");
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .rows(2)
                .placeholder(placeholder)
        });
        let submit_input = input.clone();
        cx.subscribe_in(&input, window, move |app, state, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                let text = state.read(cx).value().to_string();
                if app.submit_agent_prompt_input(text, cx) {
                    submit_input.update(cx, |input, cx| input.set_value("", window, cx));
                }
            }
        })
        .detach();
        let app_entity = cx.entity();
        let view = cx.new(|_| AgentPromptQueueView {
            app_entity,
            input,
            scroll_handle: UniformListScrollHandle::new(),
            snapshot,
            editing: None,
            collapsed: false,
        });
        self.agent_prompt_queue_view = Some(view.clone());
        view
    }

    pub(in crate::app) fn refresh_agent_prompt_queue_view(&mut self, cx: &mut Context<Self>) {
        self.invalidate_ui_region(cx, UiRegion::WorkspaceChrome);
        let Some(view) = self.agent_prompt_queue_view.clone() else {
            return;
        };
        let snapshot = self.active_agent_prompt_snapshot();
        if !agent_queue_keys_share_session(
            view.read(cx).snapshot.key.as_ref(),
            snapshot.key.as_ref(),
        ) {
            // InputState mutation needs a Window. Ask for one root render only
            // when the active terminal/session changes; the normal queue update
            // path remains isolated to this child entity.
            cx.notify();
            return;
        }
        view.update(cx, |view, cx| {
            if view.snapshot != snapshot {
                if view.snapshot.key != snapshot.key
                    && let (Some((_, item_id)), Some(new_key)) =
                        (view.editing.take(), snapshot.key.clone())
                {
                    view.editing = Some((new_key, item_id));
                }
                view.snapshot = snapshot;
                cx.notify();
            }
        });
    }

    fn submit_agent_prompt_input(&mut self, text: String, cx: &mut Context<Self>) -> bool {
        let editing = self
            .agent_prompt_queue_view
            .as_ref()
            .and_then(|view| view.read(cx).editing.clone());
        let Some((key, item_id)) = editing else {
            return false;
        };
        match self.agent_prompt_queues.replace(&key, item_id, text) {
            Ok(true) => {
                if let Some(view) = self.agent_prompt_queue_view.clone() {
                    view.update(cx, |view, cx| {
                        view.editing = None;
                        cx.notify();
                    });
                }
                self.refresh_agent_prompt_queue_view(cx);
                self.pump_agent_prompt_queues(cx);
                true
            }
            Ok(false) => false,
            Err(error) => {
                self.status_message = error.to_string();
                false
            }
        }
    }

    fn update_agent_prompt_queue(
        &mut self,
        action: impl FnOnce(&mut AgentPromptQueueStore) -> bool,
        cx: &mut Context<Self>,
    ) {
        if action(&mut self.agent_prompt_queues) {
            self.refresh_agent_prompt_queue_view(cx);
        }
        // A retry or reorder may expose a new pending head while the Agent is
        // already idle, so the same event also gets a scheduling opportunity.
        self.pump_agent_prompt_queues(cx);
    }

    fn edit_agent_prompt(
        &mut self,
        key: AgentPromptQueueKey,
        item_id: u64,
        text: Arc<str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.agent_prompt_queue_view.clone() else {
            return;
        };
        view.update(cx, |view, cx| {
            view.editing = Some((key, item_id));
            view.input.update(cx, |input, cx| {
                input.set_value(text.as_ref(), window, cx);
                input.focus(window, cx);
            });
            cx.notify();
        });
    }

    /// Reconcile queue ownership against the latest supervisor state and start
    /// at most one background write per Agent session.
    pub(in crate::app) fn pump_agent_prompt_queues(&mut self, cx: &mut Context<Self>) {
        let mut ready = Vec::new();
        let mut queue_state_changed = false;
        let editing = self
            .agent_prompt_queue_view
            .as_ref()
            .and_then(|view| view.read(cx).editing.clone());
        for old_key in self.agent_prompt_queues.keys() {
            let session = self
                .state
                .ai_runtime_state
                .sessions
                .iter()
                .filter(|session| session.terminal_id == old_key.terminal_id)
                .filter(|session| session.tool == old_key.tool)
                .filter(|session| {
                    session.terminal_instance_id.as_deref()
                        == Some(old_key.terminal_instance_id.as_str())
                })
                .filter(|session| {
                    old_key.ai_session_id.is_none()
                        || old_key.ai_session_id == session.ai_session_id
                })
                .max_by(|left, right| left.updated_at.total_cmp(&right.updated_at))
                .map(|session| {
                    (
                        session.ai_session_id.clone(),
                        session.runtime_state.clone(),
                        session.runtime_activity_at,
                        session.last_user_input_at,
                    )
                });
            let Some((ai_session_id, runtime_state, runtime_activity_at, last_user_input_at)) =
                session
            else {
                continue;
            };
            let key = match (old_key.ai_session_id.as_ref(), ai_session_id) {
                (None, Some(ai_session_id)) => self
                    .agent_prompt_queues
                    .adopt_session_id(&old_key, ai_session_id),
                _ => old_key,
            };

            queue_state_changed |= self.agent_prompt_queues.expire_unacknowledged(&key);

            if matches!(runtime_state.as_str(), "responding" | "needsInput") {
                queue_state_changed |= self
                    .agent_prompt_queues
                    .acknowledge_agent_started_at(&key, last_user_input_at);
            }
            if runtime_state == "needsInput" {
                continue;
            }
            if !matches!(runtime_state.as_str(), "idle" | "responding") {
                continue;
            }
            if self.agent_prompt_queues.native_submission_pending(&key) {
                continue;
            }
            if self.agent_task_relay_owns_ack(&key) {
                continue;
            }
            let pane = self
                .terminals
                .iter()
                .flat_map(|tab| tab.panes.iter())
                .find(|slot| slot.terminal_id.as_deref() == Some(key.terminal_id.as_str()))
                .and_then(|slot| slot.pane.as_ref())
                .filter(|pane| {
                    pane.terminal_instance_id().as_deref()
                        == Some(key.terminal_instance_id.as_str())
                })
                .cloned();
            let Some(pane) = pane else {
                continue;
            };
            if editing.as_ref().is_some_and(|(editing_key, item_id)| {
                editing_key == &key
                    && self
                        .agent_prompt_queues
                        .items(&key)
                        .first()
                        .is_some_and(|item| item.id == *item_id)
            }) {
                continue;
            }
            if !pane.view.read(cx).agent_composer_available_for_dispatch() {
                continue;
            }
            if !pane.try_reserve_agent_prompt_dispatch() {
                continue;
            }
            if let Some(dispatch) = self.agent_prompt_queues.begin_dispatch_for_runtime(
                &key,
                &runtime_state,
                runtime_activity_at,
                last_user_input_at,
            ) {
                ready.push((dispatch, pane));
            } else {
                pane.cancel_agent_prompt_dispatch();
            }
        }

        if ready.is_empty() {
            if queue_state_changed {
                self.refresh_agent_prompt_queue_view(cx);
            }
            return;
        }
        self.refresh_agent_prompt_queue_view(cx);
        for (dispatch, pane) in ready {
            cx.spawn(async move |this: gpui::WeakEntity<Self>, cx| {
                let item_id = dispatch.item_id;
                let key = dispatch.key;
                let text = dispatch.text;
                let result = codux_runtime::async_runtime::spawn_blocking(move || {
                    pane.send_agent_prompt(text.as_ref())
                })
                .await;
                let _ = this.update(cx, |app, cx| {
                    let error = match result {
                        Ok(Ok(())) => None,
                        Ok(Err(error)) => Some(error.to_string()),
                        Err(error) => Some(error.to_string()),
                    };
                    app.agent_prompt_queues
                        .finish_write(&key, item_id, error.clone());
                    if let Some(error) = error {
                        app.status_message = format!("Agent message send failed: {error}");
                    }
                    app.refresh_agent_prompt_queue_view(cx);
                    app.pump_agent_prompt_queues(cx);
                    app.pump_agent_task_relays(cx);
                });
            })
            .detach();
        }
    }

    /// Observe every intermediate supervisor snapshot before the UI summary is
    /// coalesced. This prevents a very fast Agent turn from hiding the running
    /// acknowledgement between two desktop refreshes.
    pub(in crate::app) fn observe_agent_prompt_queue_events(
        &mut self,
        events: &[codux_runtime::ai_runtime::AIRuntimeSupervisorEvent],
    ) {
        let active_sessions = events
            .iter()
            .filter_map(|event| match event {
                codux_runtime::ai_runtime::AIRuntimeSupervisorEvent::State { snapshot } => {
                    Some(snapshot.as_ref())
                }
                _ => None,
            })
            .flat_map(|snapshot| snapshot.sessions.iter())
            .filter(|session| matches!(session.state.as_str(), "responding" | "needsInput"))
            .filter_map(|session| {
                Some((
                    session.terminal_id.clone(),
                    session.terminal_instance_id.clone()?,
                    session.tool.clone(),
                    session.ai_session_id.clone(),
                    session.last_user_input_at.map(f64::to_bits),
                ))
            })
            .collect::<HashSet<_>>();

        // A drained event batch can contain many full snapshots. Deduplicate
        // session acknowledgements before matching queue keys on the UI thread.
        for (terminal_id, terminal_instance_id, tool, ai_session_id, last_user_input_at) in
            active_sessions
        {
            let matching_keys = self
                .agent_prompt_queues
                .keys()
                .into_iter()
                .filter(|key| key.terminal_id == terminal_id)
                .filter(|key| key.terminal_instance_id == terminal_instance_id)
                .filter(|key| key.tool == tool)
                .filter(|key| key.ai_session_id.is_none() || key.ai_session_id == ai_session_id)
                .collect::<Vec<_>>();
            for old_key in matching_keys {
                let key = match (&old_key.ai_session_id, &ai_session_id) {
                    (None, Some(ai_session_id)) => self
                        .agent_prompt_queues
                        .adopt_session_id(&old_key, ai_session_id.clone()),
                    _ => old_key,
                };
                self.agent_prompt_queues
                    .acknowledge_agent_started_at(&key, last_user_input_at.map(f64::from_bits));
            }
        }
    }
}

fn agent_queue_text(language: &str, key: &str, fallback: &str) -> String {
    translate(&locale_from_language_setting(language), key, fallback)
}

fn agent_queue_keys_share_session(
    left: Option<&AgentPromptQueueKey>,
    right: Option<&AgentPromptQueueKey>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.terminal_id == right.terminal_id
                && left.terminal_instance_id == right.terminal_instance_id
                && left.tool == right.tool
                && (left.ai_session_id == right.ai_session_id
                    || left.ai_session_id.is_none()
                    || right.ai_session_id.is_none())
        }
        _ => false,
    }
}

impl Render for AgentPromptQueueView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(key) = self.snapshot.key.clone() else {
            return div().h(px(0.0)).flex_none().into_any_element();
        };
        let language = self.snapshot.language.clone();
        // Include the active tool because each Agent session owns an isolated
        // queue and the user may switch between several terminals rapidly.
        let title = format!(
            "{} · {}",
            agent_queue_text(&language, "ai.queue.title", "Send Queue"),
            agent_queue_tool_label(&key.tool)
        );
        let editing = self.editing.is_some();
        let submit_label = agent_queue_text(&language, "common.save", "Save");
        let status = agent_queue_runtime_label(&language, &self.snapshot.runtime_state);
        let app_entity = self.app_entity.clone();
        let input = self.input.clone();
        let cancel_input = self.input.clone();
        let items = Rc::new(self.snapshot.items.clone());
        let count = items.len();
        if count == 0 && !editing {
            return div().h(px(0.0)).flex_none().into_any_element();
        }
        let first_editable_index = items
            .iter()
            .position(|item| item.status.is_editable())
            .unwrap_or(count);
        let list_max_height = agent_queue_list_max_height(window.viewport_size().height.as_f32());
        let list_height = (count as f32 * 68.0).min(list_max_height);
        let collapsed = self.collapsed;
        let chevron = if collapsed {
            HeroIconName::ChevronRight
        } else {
            HeroIconName::ChevronDown
        };
        let list_key = key.clone();
        let list_language = language.clone();
        let list_app_entity = app_entity.clone();

        div()
            .w_full()
            .flex_none()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(theme::vibrancy_panel(color(theme::BG_COLUMN)))
            .when(count > 0, |this| {
                this.child(
                    div()
                        .id("agent-prompt-queue-header")
                        .h(px(30.0))
                        .px(px(10.0))
                        .flex()
                        .items_center()
                        .justify_between()
                        .cursor_pointer()
                        .on_click(cx.listener(|view, _event, _window, cx| {
                            view.collapsed = !view.collapsed;
                            cx.notify();
                        }))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap_1()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(cx.theme().muted_foreground)
                                .child(Icon::new(chevron).size_3())
                                .child(div().min_w_0().truncate().child(title))
                                .child(Tag::secondary().rounded_full().child(count.to_string())),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(agent_queue_runtime_color(&self.snapshot.runtime_state))
                                .child(status),
                        ),
                )
                .when(!collapsed, |this| {
                    this.child(
                        div()
                            .id("agent-prompt-queue-scroll")
                            .h(px(list_height))
                            .overflow_hidden()
                            .child(codux_uniform_list(
                                "agent-prompt-queue-list",
                                items.clone(),
                                self.scroll_handle.clone(),
                                None,
                                cx,
                                move |item, index, _window, cx| {
                                    agent_prompt_queue_row(
                                        list_key.clone(),
                                        item,
                                        index,
                                        count,
                                        first_editable_index,
                                        list_language.clone(),
                                        list_app_entity.clone(),
                                        cx,
                                    )
                                },
                            )),
                    )
                })
            })
            .when(editing, |this| {
                this.child(
                    div()
                        .p(px(8.0))
                        .flex()
                        .items_end()
                        .gap_1()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(Input::new(&self.input).small().h(px(52.0))),
                        )
                        .child(
                            codux_tooltip_container(
                                app_entity.clone(),
                                "agent-queue-cancel-edit-tooltip",
                                agent_queue_text(&language, "common.cancel", "Cancel"),
                            )
                            .child(
                                Button::new("agent-queue-cancel-edit")
                                    .ghost()
                                    .compact()
                                    .icon(Icon::new(HeroIconName::XMark).size_3p5())
                                    .on_click(cx.listener(move |view, _event, window, cx| {
                                        view.editing = None;
                                        cancel_input.update(cx, |input, cx| {
                                            input.set_value("", window, cx)
                                        });
                                        cx.notify();
                                    })),
                            ),
                        )
                        .child(
                            codux_tooltip_container(
                                app_entity.clone(),
                                "agent-queue-send-tooltip",
                                submit_label,
                            )
                            .child(
                                Button::new("agent-queue-send")
                                    .primary()
                                    .compact()
                                    .icon(Icon::new(HeroIconName::Check).size_3p5())
                                    .on_click(move |_, _window, cx| {
                                        let text = input.read(cx).value().to_string();
                                        cx.update_entity(&app_entity, |app, cx| {
                                            if app.submit_agent_prompt_input(text, cx) {
                                                input.update(cx, |input, cx| {
                                                    input.set_value("", _window, cx)
                                                });
                                            }
                                        });
                                    }),
                            ),
                        ),
                )
            })
            .into_any_element()
    }
}

fn agent_queue_list_max_height(viewport_height: f32) -> f32 {
    // The queue now owns a full right rail. Reserve fixed space for the app
    // toolbar, panel header and editor while letting its virtual list use the
    // remainder on both compact and tall windows.
    (viewport_height - 150.0).clamp(AGENT_QUEUE_MIN_LIST_HEIGHT, AGENT_QUEUE_MAX_LIST_HEIGHT)
}

fn agent_prompt_queue_row(
    key: AgentPromptQueueKey,
    item: AgentPromptItem,
    index: usize,
    count: usize,
    first_editable_index: usize,
    language: Arc<str>,
    app_entity: gpui::Entity<CoduxApp>,
    cx: &mut Context<AgentPromptQueueView>,
) -> gpui::AnyElement {
    let editable = item.status.is_editable();
    let status_label = agent_queue_item_status_label(&language, &item.status);
    let status_color = match item.status {
        AgentPromptStatus::Failed(_) => color(theme::RED),
        AgentPromptStatus::Dispatching
        | AgentPromptStatus::DispatchingAcknowledged
        | AgentPromptStatus::AwaitingAgent => color(theme::ORANGE),
        AgentPromptStatus::Pending => cx.theme().muted_foreground,
    };
    let row_id = item.id;
    let button =
        |id: &'static str,
         icon: HeroIconName,
         label: String,
         disabled: bool,
         handler: Box<dyn Fn(&mut CoduxApp, &mut Window, &mut Context<CoduxApp>)>| {
            let entity = app_entity.clone();
            codux_tooltip_container(
                app_entity.clone(),
                format!("agent-queue-{id}-tooltip-{row_id}"),
                label,
            )
            .child(
                Button::new(format!("agent-queue-{id}-{row_id}"))
                    .ghost()
                    .compact()
                    .disabled(disabled)
                    .icon(Icon::new(icon).size_3())
                    .on_click(move |_, window, cx| {
                        cx.update_entity(&entity, |app, cx| handler(app, window, cx));
                    }),
            )
        };

    let promote_key = key.clone();
    let up_key = key.clone();
    let down_key = key.clone();
    let edit_key = key.clone();
    let delete_key = key.clone();
    let retry_key = key.clone();
    let edit_text = item.text.clone();
    let failed = matches!(item.status, AgentPromptStatus::Failed(_));

    div()
        .h(px(68.0))
        .px(px(8.0))
        .py(px(6.0))
        .border_t_1()
        .border_color(color(theme::BORDER_SOFT))
        .child(
            div()
                .min_w_0()
                .text_xs()
                .line_height(rems(1.0))
                .line_clamp(2)
                .text_color(color(theme::TEXT))
                .child(item.preview.as_ref().to_string()),
        )
        .child(
            div()
                .mt(px(3.0))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(status_color)
                        .child(status_label),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(1.0))
                        .when(failed, |this| {
                            this.child(button(
                                "retry",
                                HeroIconName::ArrowPath,
                                agent_queue_text(&language, "common.retry", "Retry"),
                                false,
                                Box::new(move |app, _window, cx| {
                                    app.update_agent_prompt_queue(
                                        |store| store.retry(&retry_key, row_id),
                                        cx,
                                    );
                                }),
                            ))
                        })
                        .child(button(
                            "top",
                            HeroIconName::BarsArrowUp,
                            agent_queue_text(&language, "ai.queue.promote", "Move to top"),
                            !editable || index <= first_editable_index,
                            Box::new(move |app, _window, cx| {
                                app.update_agent_prompt_queue(
                                    |store| store.promote(&promote_key, row_id),
                                    cx,
                                );
                            }),
                        ))
                        .child(button(
                            "up",
                            HeroIconName::ArrowUp,
                            agent_queue_text(&language, "ai.queue.move_up", "Move up"),
                            !editable || index <= first_editable_index,
                            Box::new(move |app, _window, cx| {
                                app.update_agent_prompt_queue(
                                    |store| store.move_item(&up_key, row_id, -1),
                                    cx,
                                );
                            }),
                        ))
                        .child(button(
                            "down",
                            HeroIconName::ArrowDown,
                            agent_queue_text(&language, "ai.queue.move_down", "Move down"),
                            !editable || index + 1 >= count,
                            Box::new(move |app, _window, cx| {
                                app.update_agent_prompt_queue(
                                    |store| store.move_item(&down_key, row_id, 1),
                                    cx,
                                );
                            }),
                        ))
                        .child(button(
                            "edit",
                            HeroIconName::Pencil,
                            agent_queue_text(&language, "ai.queue.edit", "Edit queued message"),
                            !editable,
                            Box::new(move |app, window, cx| {
                                app.edit_agent_prompt(
                                    edit_key.clone(),
                                    row_id,
                                    edit_text.clone(),
                                    window,
                                    cx,
                                );
                            }),
                        ))
                        .child(button(
                            "delete",
                            HeroIconName::Trash,
                            agent_queue_text(&language, "common.delete", "Delete"),
                            !editable,
                            Box::new(move |app, _window, cx| {
                                app.update_agent_prompt_queue(
                                    |store| store.remove(&delete_key, row_id),
                                    cx,
                                );
                            }),
                        )),
                ),
        )
        .into_any_element()
}

fn agent_queue_runtime_label(language: &str, state: &str) -> String {
    match state {
        "idle" => agent_queue_text(language, "ai.queue.ready", "Ready"),
        "responding" => agent_queue_text(language, "ai.queue.running", "Running"),
        "needsInput" => agent_queue_text(language, "ai.queue.needs_input", "Needs input"),
        _ => agent_queue_text(language, "ai.queue.paused", "Paused"),
    }
}

fn agent_queue_tool_label(tool: &str) -> &str {
    // Runtime driver ids are intentionally stable and locale-independent;
    // normalize only product capitalization for the compact queue header.
    match tool {
        "codex" => "Codex",
        "claude" => "Claude",
        "opencode" => "OpenCode",
        "codewhale" => "CodeWhale",
        "kimi" => "Kimi",
        "kiro" => "Kiro",
        "mimo" => "Mimo",
        "agy" => "AGY",
        "omp" => "OMP",
        other => other,
    }
}

fn agent_queue_runtime_color(state: &str) -> gpui::Hsla {
    match state {
        "idle" => color(theme::GREEN),
        "responding" => color(theme::ACCENT),
        "needsInput" => color(theme::ORANGE),
        _ => color(theme::TEXT_MUTED),
    }
}

fn agent_queue_item_status_label(language: &str, status: &AgentPromptStatus) -> String {
    match status {
        AgentPromptStatus::Pending => agent_queue_text(language, "ai.queue.pending", "Waiting"),
        AgentPromptStatus::Dispatching | AgentPromptStatus::DispatchingAcknowledged => {
            agent_queue_text(language, "ai.queue.sending", "Sending")
        }
        AgentPromptStatus::AwaitingAgent => {
            agent_queue_text(language, "ai.queue.awaiting", "Awaiting Agent")
        }
        AgentPromptStatus::Failed(error) => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> AgentPromptQueueKey {
        AgentPromptQueueKey {
            terminal_id: "terminal-1".to_string(),
            terminal_instance_id: "instance-1".to_string(),
            ai_session_id: Some("session-1".to_string()),
            tool: "codex".to_string(),
        }
    }

    #[test]
    fn dispatch_waits_for_agent_ack_before_releasing_head() {
        let mut store = AgentPromptQueueStore::default();
        store.enqueue(key(), "first".to_string()).unwrap();
        store.enqueue(key(), "second".to_string()).unwrap();

        let dispatch = store.begin_dispatch(&key()).unwrap();
        assert!(store.begin_dispatch(&key()).is_none());
        assert!(store.finish_write(&key(), dispatch.item_id, None));
        assert!(store.begin_dispatch(&key()).is_none());
        assert!(store.acknowledge_agent_started(&key()));
        assert_eq!(
            store.begin_dispatch(&key()).unwrap().text.as_ref(),
            "second"
        );
    }

    #[test]
    fn responding_queue_dispatches_the_promoted_head_at_the_next_activity_boundary() {
        let mut store = AgentPromptQueueStore::default();
        let first = store
            .enqueue_at(key(), "first".to_string(), Some(10.0))
            .unwrap();
        let second = store
            .enqueue_at(key(), "second".to_string(), Some(10.0))
            .unwrap();
        assert!(store.promote(&key(), second));

        assert!(
            store
                .begin_dispatch_for_runtime(&key(), "responding", Some(10.0), Some(5.0))
                .is_none()
        );
        let dispatch = store
            .begin_dispatch_for_runtime(&key(), "responding", Some(11.0), Some(5.0))
            .unwrap();
        assert_eq!(dispatch.item_id, second);
        assert_eq!(dispatch.text.as_ref(), "second");
        assert!(store.finish_write(&key(), dispatch.item_id, None));
        store.queues.get_mut(&key()).unwrap()[0].dispatch_started_at =
            Some(Instant::now() - AGENT_ACK_TIMEOUT);
        assert!(!store.expire_unacknowledged(&key()));

        assert!(!store.acknowledge_agent_started_at(&key(), Some(5.0)));
        assert!(store.acknowledge_agent_started_at(&key(), Some(6.0)));
        assert_eq!(store.items(&key())[0].id, first);
    }

    #[test]
    fn responding_queue_falls_back_to_idle_without_native_input_markers() {
        let mut store = AgentPromptQueueStore::default();
        store
            .enqueue_at(key(), "portable fallback".to_string(), Some(10.0))
            .unwrap();

        assert!(
            store
                .begin_dispatch_for_runtime(&key(), "responding", Some(11.0), None)
                .is_none()
        );
        assert!(
            store
                .begin_dispatch_for_runtime(&key(), "idle", Some(11.0), None)
                .is_some()
        );
    }

    #[test]
    fn agent_ack_can_arrive_before_background_write_completion() {
        let mut store = AgentPromptQueueStore::default();
        store.enqueue(key(), "fast prompt".to_string()).unwrap();
        let dispatch = store.begin_dispatch(&key()).unwrap();

        assert!(store.acknowledge_agent_started(&key()));
        assert!(matches!(
            store.items(&key())[0].status,
            AgentPromptStatus::DispatchingAcknowledged
        ));
        assert!(store.finish_write(&key(), dispatch.item_id, None));
        assert!(store.items(&key()).is_empty());
    }

    #[test]
    fn provisional_and_identified_keys_share_only_the_same_terminal_instance() {
        let mut provisional = key();
        provisional.ai_session_id = None;
        assert!(agent_queue_keys_share_session(
            Some(&provisional),
            Some(&key())
        ));

        let mut replacement = key();
        replacement.terminal_instance_id = "instance-2".to_string();
        assert!(!agent_queue_keys_share_session(
            Some(&provisional),
            Some(&replacement)
        ));
    }

    #[test]
    fn sent_item_cannot_be_reordered_edited_or_deleted() {
        let mut store = AgentPromptQueueStore::default();
        let first = store.enqueue(key(), "first".to_string()).unwrap();
        let second = store.enqueue(key(), "second".to_string()).unwrap();
        let dispatch = store.begin_dispatch(&key()).unwrap();
        assert_eq!(dispatch.item_id, first);

        assert!(!store.promote(&key(), first));
        assert!(!store.remove(&key(), first));
        assert!(!store.replace(&key(), first, "changed".to_string()).unwrap());
        assert!(!store.move_item(&key(), second, -1));
    }

    #[test]
    fn queued_items_support_priority_edit_delete_and_failure_retry() {
        let mut store = AgentPromptQueueStore::default();
        let first = store.enqueue(key(), "first".to_string()).unwrap();
        let second = store.enqueue(key(), "second".to_string()).unwrap();
        assert!(store.promote(&key(), second));
        assert!(
            store
                .replace(&key(), second, "updated second".to_string())
                .unwrap()
        );
        assert_eq!(store.items(&key())[0].text.as_ref(), "updated second");

        let dispatch = store.begin_dispatch(&key()).unwrap();
        assert!(store.finish_write(&key(), dispatch.item_id, Some("offline".to_string())));
        assert!(store.retry(&key(), dispatch.item_id));
        assert!(store.remove(&key(), first));
    }

    #[test]
    fn provisional_queue_adopts_agent_session_identity() {
        let mut provisional = key();
        provisional.ai_session_id = None;
        let mut store = AgentPromptQueueStore::default();
        store
            .enqueue(provisional.clone(), "hello".to_string())
            .unwrap();

        let adopted = store.adopt_session_id(&provisional, "session-2".to_string());
        assert!(store.items(&provisional).is_empty());
        assert_eq!(store.items(&adopted).len(), 1);
    }

    #[test]
    fn native_submission_blocks_the_next_prompt_until_agent_acknowledgement() {
        let mut store = AgentPromptQueueStore::default();
        store.note_native_submission(key());
        store.enqueue(key(), "queued second".to_string()).unwrap();

        assert!(store.native_submission_pending(&key()));
        assert!(store.begin_dispatch(&key()).is_none());
        assert!(store.acknowledge_agent_started(&key()));
        assert!(!store.native_submission_pending(&key()));
        assert_eq!(
            store.begin_dispatch(&key()).unwrap().text.as_ref(),
            "queued second"
        );
    }

    #[test]
    fn provisional_session_upgrade_preserves_native_submission_guard() {
        let mut provisional = key();
        provisional.ai_session_id = None;
        let mut store = AgentPromptQueueStore::default();
        store.note_native_submission(provisional.clone());

        let adopted = store.adopt_session_id(&provisional, "session-2".to_string());
        assert!(!store.native_submission_pending(&provisional));
        assert!(store.native_submission_pending(&adopted));
        assert!(store.acknowledge_agent_started(&adopted));
        assert!(!store.native_submission_pending(&adopted));
    }

    #[test]
    fn failed_head_blocks_following_prompts_until_user_resolves_it() {
        let mut store = AgentPromptQueueStore::default();
        store.enqueue(key(), "first".to_string()).unwrap();
        store.enqueue(key(), "second".to_string()).unwrap();
        let dispatch = store.begin_dispatch(&key()).unwrap();
        store.finish_write(&key(), dispatch.item_id, Some("offline".to_string()));

        assert!(store.begin_dispatch(&key()).is_none());
        assert!(store.retry(&key(), dispatch.item_id));
        assert_eq!(store.begin_dispatch(&key()).unwrap().text.as_ref(), "first");
    }

    #[test]
    fn queue_enforces_message_and_session_limits() {
        let mut store = AgentPromptQueueStore::default();
        for index in 0..MAX_AGENT_PROMPTS_PER_SESSION {
            store.enqueue(key(), format!("message {index}")).unwrap();
        }
        assert_eq!(
            store.enqueue(key(), "overflow".to_string()),
            Err("agent queue is full")
        );
        let mut other_key = key();
        other_key.terminal_id = "terminal-2".to_string();
        assert_eq!(
            store.enqueue(other_key, "x".repeat(MAX_AGENT_PROMPT_BYTES + 1)),
            Err("agent queue message is too large")
        );

        let mut bounded = AgentPromptQueueStore::default();
        for index in 0..MAX_AGENT_QUEUE_SESSIONS {
            let mut session_key = key();
            session_key.terminal_id = format!("terminal-{index}");
            bounded.enqueue(session_key, "pending".to_string()).unwrap();
        }
        let mut overflow_key = key();
        overflow_key.terminal_id = "terminal-overflow".to_string();
        assert_eq!(
            bounded.enqueue(overflow_key, "pending".to_string()),
            Err("too many agent queues are active")
        );
    }

    #[test]
    fn missing_acknowledgement_becomes_a_recoverable_failure() {
        let mut store = AgentPromptQueueStore::default();
        store.enqueue(key(), "uncertain".to_string()).unwrap();
        let dispatch = store.begin_dispatch(&key()).unwrap();
        store.finish_write(&key(), dispatch.item_id, None);
        store.queues.get_mut(&key()).unwrap()[0].dispatch_started_at =
            Some(Instant::now() - AGENT_ACK_TIMEOUT);

        assert!(store.expire_unacknowledged(&key()));
        assert!(matches!(
            store.items(&key())[0].status,
            AgentPromptStatus::Failed(_)
        ));
        assert!(store.retry(&key(), dispatch.item_id));
    }

    #[test]
    fn terminal_cleanup_releases_queues_and_native_guards() {
        let mut store = AgentPromptQueueStore::default();
        store.enqueue(key(), "pending".to_string()).unwrap();
        store.note_native_submission(key());

        assert!(store.remove_terminal("terminal-1"));
        assert!(store.items(&key()).is_empty());
        assert!(!store.native_submission_pending(&key()));
    }

    #[test]
    fn stale_native_submission_guard_expires() {
        let mut store = AgentPromptQueueStore::default();
        store.note_native_submission(key());
        *store.native_submissions.get_mut(&key()).unwrap() =
            Instant::now() - NATIVE_SUBMISSION_GUARD;

        assert!(!store.native_submission_pending(&key()));
    }

    #[test]
    fn queue_height_scales_with_viewport_with_stable_bounds() {
        assert_eq!(agent_queue_list_max_height(300.0), 200.0);
        assert_eq!(agent_queue_list_max_height(480.0), 330.0);
        assert_eq!(agent_queue_list_max_height(1_000.0), 720.0);
    }
}
