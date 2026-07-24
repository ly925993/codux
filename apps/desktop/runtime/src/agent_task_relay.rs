use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{Arc, OnceLock},
};

pub const AGENT_TASK_RELAY_SCHEMA_VERSION: u32 = 1;
pub const MAX_RELAY_TASKS: usize = 100;
pub const MAX_RELAY_TASK_BYTES: usize = 256 * 1024;
pub const MAX_RELAY_TASK_TOTAL_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_RELAY_RECEIPTS: usize = 200;
const AGENT_TASK_RELAY_NAMESPACE: &str = "agent-task-relay";

static COMMITTED_REVISIONS: OnceLock<parking_lot::Mutex<HashMap<(PathBuf, String), u64>>> =
    OnceLock::new();

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskRelayTarget {
    pub project_id: String,
    pub worktree_id: String,
    pub terminal_id: String,
    #[serde(default)]
    pub terminal_instance_id: Option<String>,
    pub tool: String,
    #[serde(default)]
    pub ai_session_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentTaskRelayBlockReason {
    NeedsInput,
    DeliveryUnknown,
    PersistenceError,
    TargetLost,
    SendFailed,
    Interrupted,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "reason", rename_all = "camelCase")]
pub enum AgentTaskRelayBoardState {
    Running,
    PausedByUser,
    PausedBlocked(AgentTaskRelayBlockReason),
    Completed,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentTaskRelayTaskState {
    Queued,
    Dispatching,
    AwaitingAck,
    Running,
    NeedsInput,
    Completed,
    Failed,
    DeliveryUnknown,
    Skipped,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskRelayTask {
    pub id: u64,
    pub text: Arc<str>,
    pub state: AgentTaskRelayTaskState,
    pub created_at: f64,
    #[serde(default)]
    pub dispatch_started_at: Option<f64>,
    #[serde(default)]
    pub acknowledged_at: Option<f64>,
    #[serde(default)]
    pub completion_baseline: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskRelayReceipt {
    pub task_id: u64,
    pub state: AgentTaskRelayTaskState,
    pub task_preview: Arc<str>,
    pub finished_at: f64,
    #[serde(default)]
    pub assistant_preview: Option<Arc<str>>,
    #[serde(default)]
    pub completion_event_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskRelayCounts {
    pub queued: usize,
    pub running: usize,
    pub waiting: usize,
    pub receipts: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskRelayBoard {
    pub schema_version: u32,
    pub id: String,
    pub target: AgentTaskRelayTarget,
    pub state: AgentTaskRelayBoardState,
    pub revision: u64,
    pub next_task_id: u64,
    #[serde(default = "default_true")]
    pub paused_by_user: bool,
    pub total_task_bytes: usize,
    pub counts: AgentTaskRelayCounts,
    pub tasks: VecDeque<AgentTaskRelayTask>,
    pub receipts: VecDeque<AgentTaskRelayReceipt>,
}

impl AgentTaskRelayBoard {
    pub fn new(id: impl Into<String>, target: AgentTaskRelayTarget) -> Self {
        Self {
            schema_version: AGENT_TASK_RELAY_SCHEMA_VERSION,
            id: id.into(),
            target,
            state: AgentTaskRelayBoardState::PausedByUser,
            revision: 0,
            next_task_id: 1,
            paused_by_user: true,
            total_task_bytes: 0,
            counts: AgentTaskRelayCounts::default(),
            tasks: VecDeque::new(),
            receipts: VecDeque::new(),
        }
    }

    pub fn add_task(&mut self, text: String, now: f64) -> Result<u64, &'static str> {
        if text.trim().is_empty() {
            return Err("Task text is required.");
        }
        let text_bytes = text.len();
        if text_bytes > MAX_RELAY_TASK_BYTES {
            return Err("Task text is too large.");
        }
        if self.counts.queued + self.counts.running + self.counts.waiting >= MAX_RELAY_TASKS {
            return Err("Task relay is full.");
        }
        if self.total_task_bytes.saturating_add(text_bytes) > MAX_RELAY_TASK_TOTAL_BYTES {
            return Err("Task relay text limit reached.");
        }

        let task_id = self.next_task_id;
        self.next_task_id = self.next_task_id.saturating_add(1);
        self.total_task_bytes += text_bytes;
        self.counts.queued += 1;
        self.tasks.push_back(AgentTaskRelayTask {
            id: task_id,
            text: Arc::from(text),
            state: AgentTaskRelayTaskState::Queued,
            created_at: now,
            dispatch_started_at: None,
            acknowledged_at: None,
            completion_baseline: None,
        });
        if self.state == AgentTaskRelayBoardState::Completed {
            self.state = AgentTaskRelayBoardState::PausedByUser;
            self.paused_by_user = true;
        }
        self.bump_revision();
        Ok(task_id)
    }

    pub fn start(&mut self) -> Result<(), &'static str> {
        if matches!(self.state, AgentTaskRelayBoardState::PausedBlocked(_)) {
            return Err("Resolve the blocked relay state before starting relay.");
        }
        if self.counts.waiting > 0 {
            return Err("Resolve the blocked task before starting relay.");
        }
        if self.counts.queued == 0 && self.counts.running == 0 {
            return Err("Add a task before starting relay.");
        }
        self.paused_by_user = false;
        self.state = AgentTaskRelayBoardState::Running;
        self.bump_revision();
        Ok(())
    }

    pub fn pause(&mut self) -> bool {
        if self.paused_by_user {
            return false;
        }
        self.paused_by_user = true;
        self.state = AgentTaskRelayBoardState::PausedByUser;
        self.bump_revision();
        true
    }

    /// Rebind queued work to the current lease of the same terminal. Terminal
    /// instances and Agent session IDs are intentionally short-lived, while the
    /// board ID remains stable across relaunches and repeated relay rounds.
    pub fn rebind_target(&mut self, target: AgentTaskRelayTarget) -> Result<bool, &'static str> {
        if self.target.project_id != target.project_id
            || self.target.worktree_id != target.worktree_id
            || self.target.terminal_id != target.terminal_id
        {
            return Err("Task relay target does not belong to this board.");
        }
        if self.target == target {
            return Ok(false);
        }
        // Never transfer ownership of an in-flight side effect. DeliveryUnknown
        // is the sole exception: it is already paused and only an explicit user
        // retry can turn it back into a sendable queued task.
        if self
            .active_task()
            .is_some_and(|task| task.state != AgentTaskRelayTaskState::DeliveryUnknown)
        {
            return Err("Cannot change task relay target while a task is active.");
        }
        self.target = target;
        self.bump_revision();
        Ok(true)
    }

    pub fn begin_dispatch(&mut self, now: f64, baseline: Option<f64>) -> Option<u64> {
        if self.state != AgentTaskRelayBoardState::Running
            || self
                .tasks
                .iter()
                .any(|task| is_active_task_state(task.state))
        {
            return None;
        }
        let index = self
            .tasks
            .iter()
            .position(|task| task.state == AgentTaskRelayTaskState::Queued)?;
        let task_id = self.tasks[index].id;
        self.transition_task(index, AgentTaskRelayTaskState::Dispatching);
        self.tasks[index].dispatch_started_at = Some(now);
        self.tasks[index].completion_baseline = baseline;
        self.bump_revision();
        Some(task_id)
    }

    pub fn mark_write_succeeded(&mut self, task_id: u64, _now: f64) -> bool {
        self.transition_by_id(
            task_id,
            AgentTaskRelayTaskState::Dispatching,
            AgentTaskRelayTaskState::AwaitingAck,
        )
    }

    pub fn mark_acknowledged(&mut self, task_id: u64, now: f64) -> bool {
        let Some(index) = self.task_index(task_id) else {
            return false;
        };
        if self.tasks[index].state != AgentTaskRelayTaskState::AwaitingAck {
            return false;
        }
        self.transition_task(index, AgentTaskRelayTaskState::Running);
        self.tasks[index].acknowledged_at = Some(now);
        self.bump_revision();
        true
    }

    pub fn mark_delivery_unknown(&mut self, task_id: u64) -> bool {
        let Some(index) = self.task_index(task_id) else {
            return false;
        };
        if !matches!(
            self.tasks[index].state,
            AgentTaskRelayTaskState::Dispatching | AgentTaskRelayTaskState::AwaitingAck
        ) {
            return false;
        }
        self.transition_task(index, AgentTaskRelayTaskState::DeliveryUnknown);
        self.paused_by_user = true;
        self.state =
            AgentTaskRelayBoardState::PausedBlocked(AgentTaskRelayBlockReason::DeliveryUnknown);
        self.bump_revision();
        true
    }

    pub fn mark_needs_input(&mut self, task_id: u64) -> bool {
        let Some(index) = self.task_index(task_id) else {
            return false;
        };
        if self.tasks[index].state != AgentTaskRelayTaskState::Running {
            return false;
        }
        self.transition_task(index, AgentTaskRelayTaskState::NeedsInput);
        self.state = AgentTaskRelayBoardState::PausedBlocked(AgentTaskRelayBlockReason::NeedsInput);
        self.bump_revision();
        true
    }

    pub fn mark_input_resumed(&mut self, task_id: u64, baseline: Option<f64>) -> bool {
        let Some(index) = self.task_index(task_id) else {
            return false;
        };
        if self.tasks[index].state != AgentTaskRelayTaskState::NeedsInput {
            return false;
        }
        self.transition_task(index, AgentTaskRelayTaskState::Running);
        self.tasks[index].completion_baseline = baseline;
        self.state = if self.paused_by_user {
            AgentTaskRelayBoardState::PausedByUser
        } else {
            AgentTaskRelayBoardState::Running
        };
        self.bump_revision();
        true
    }

    pub fn mark_completed(
        &mut self,
        task_id: u64,
        event_id: String,
        assistant_preview: Option<String>,
        now: f64,
    ) -> bool {
        if self
            .receipts
            .iter()
            .any(|receipt| receipt.completion_event_id.as_deref() == Some(event_id.as_str()))
        {
            return false;
        }
        let Some(index) = self.task_index(task_id) else {
            return false;
        };
        if !matches!(
            self.tasks[index].state,
            AgentTaskRelayTaskState::Running | AgentTaskRelayTaskState::NeedsInput
        ) {
            return false;
        }
        self.finish_task(
            index,
            AgentTaskRelayTaskState::Completed,
            Some(event_id),
            assistant_preview,
            now,
        );
        true
    }

    pub fn recover_after_restart(&mut self) -> bool {
        let mut changed = false;
        for index in 0..self.tasks.len() {
            if is_active_task_state(self.tasks[index].state) {
                self.transition_task(index, AgentTaskRelayTaskState::DeliveryUnknown);
                changed = true;
            }
        }
        if changed {
            self.state =
                AgentTaskRelayBoardState::PausedBlocked(AgentTaskRelayBlockReason::DeliveryUnknown);
            self.paused_by_user = true;
            self.bump_revision();
        }
        changed
    }

    pub fn edit_task(&mut self, task_id: u64, text: String) -> Result<bool, &'static str> {
        if text.trim().is_empty() {
            return Err("Task text is required.");
        }
        if text.len() > MAX_RELAY_TASK_BYTES {
            return Err("Task text is too large.");
        }
        let Some(index) = self.task_index(task_id) else {
            return Ok(false);
        };
        if self.tasks[index].state != AgentTaskRelayTaskState::Queued {
            return Err("Only queued tasks can be edited.");
        }
        let next_total = self
            .total_task_bytes
            .saturating_sub(self.tasks[index].text.len())
            .saturating_add(text.len());
        if next_total > MAX_RELAY_TASK_TOTAL_BYTES {
            return Err("Task relay text limit reached.");
        }
        if self.tasks[index].text.as_ref() == text {
            return Ok(false);
        }
        self.total_task_bytes = next_total;
        self.tasks[index].text = Arc::from(text);
        self.bump_revision();
        Ok(true)
    }

    pub fn delete_task(&mut self, task_id: u64) -> bool {
        let Some(index) = self.task_index(task_id) else {
            return false;
        };
        if self.tasks[index].state != AgentTaskRelayTaskState::Queued {
            return false;
        }
        let task = self.tasks.remove(index).expect("task index");
        self.total_task_bytes = self.total_task_bytes.saturating_sub(task.text.len());
        self.counts.queued = self.counts.queued.saturating_sub(1);
        self.complete_if_empty();
        self.bump_revision();
        true
    }

    pub fn promote_task(&mut self, task_id: u64) -> bool {
        let Some(index) = self.task_index(task_id) else {
            return false;
        };
        if self.tasks[index].state != AgentTaskRelayTaskState::Queued {
            return false;
        }
        let first_queued = self
            .tasks
            .iter()
            .position(|task| task.state == AgentTaskRelayTaskState::Queued)
            .unwrap_or(index);
        if index == first_queued {
            return false;
        }
        let task = self.tasks.remove(index).expect("task index");
        self.tasks.insert(first_queued, task);
        self.bump_revision();
        true
    }

    pub fn move_task(&mut self, task_id: u64, delta: isize) -> bool {
        let Some(index) = self.task_index(task_id) else {
            return false;
        };
        if self.tasks[index].state != AgentTaskRelayTaskState::Queued {
            return false;
        }
        let candidate = index as isize + delta;
        if candidate < 0 || candidate >= self.tasks.len() as isize {
            return false;
        }
        let candidate = candidate as usize;
        if self.tasks[candidate].state != AgentTaskRelayTaskState::Queued {
            return false;
        }
        self.tasks.swap(index, candidate);
        self.bump_revision();
        true
    }

    pub fn active_task(&self) -> Option<&AgentTaskRelayTask> {
        self.tasks
            .iter()
            .find(|task| is_active_task_state(task.state))
    }

    pub fn task(&self, task_id: u64) -> Option<&AgentTaskRelayTask> {
        self.tasks.iter().find(|task| task.id == task_id)
    }

    pub fn resolve_delivery_unknown(&mut self, task_id: u64, resend: bool, now: f64) -> bool {
        let Some(index) = self.task_index(task_id) else {
            return false;
        };
        if self.tasks[index].state != AgentTaskRelayTaskState::DeliveryUnknown {
            return false;
        }
        if resend {
            self.transition_task(index, AgentTaskRelayTaskState::Queued);
            self.tasks[index].dispatch_started_at = None;
            self.tasks[index].acknowledged_at = None;
            self.tasks[index].completion_baseline = None;
        } else {
            self.finish_task(
                index,
                AgentTaskRelayTaskState::Completed,
                None,
                Some("Marked already handled by the user.".to_string()),
                now,
            );
            return true;
        }
        self.paused_by_user = true;
        self.state = AgentTaskRelayBoardState::PausedByUser;
        self.bump_revision();
        true
    }

    pub fn mark_send_failed(&mut self, task_id: u64, now: f64) -> bool {
        let Some(index) = self.task_index(task_id) else {
            return false;
        };
        if !matches!(
            self.tasks[index].state,
            AgentTaskRelayTaskState::Dispatching | AgentTaskRelayTaskState::AwaitingAck
        ) {
            return false;
        }
        self.finish_task(index, AgentTaskRelayTaskState::Failed, None, None, now);
        self.state = AgentTaskRelayBoardState::PausedBlocked(AgentTaskRelayBlockReason::SendFailed);
        self.paused_by_user = true;
        true
    }

    pub fn mark_interrupted(&mut self, task_id: u64, now: f64) -> bool {
        let Some(index) = self.task_index(task_id) else {
            return false;
        };
        if !matches!(
            self.tasks[index].state,
            AgentTaskRelayTaskState::Running | AgentTaskRelayTaskState::NeedsInput
        ) {
            return false;
        }
        self.finish_task(index, AgentTaskRelayTaskState::Failed, None, None, now);
        self.state =
            AgentTaskRelayBoardState::PausedBlocked(AgentTaskRelayBlockReason::Interrupted);
        self.paused_by_user = true;
        true
    }

    pub fn mark_persistence_failed(&mut self) -> bool {
        if self.state
            == AgentTaskRelayBoardState::PausedBlocked(AgentTaskRelayBlockReason::PersistenceError)
        {
            return false;
        }
        self.paused_by_user = true;
        self.state =
            AgentTaskRelayBoardState::PausedBlocked(AgentTaskRelayBlockReason::PersistenceError);
        self.bump_revision();
        true
    }

    pub fn normalize_loaded(&mut self) -> Result<bool, &'static str> {
        if self.schema_version != AGENT_TASK_RELAY_SCHEMA_VERSION {
            return Err("Unsupported task relay schema version.");
        }
        self.total_task_bytes = self.tasks.iter().map(|task| task.text.len()).sum();
        self.recalculate_counts();
        let mut changed = self.recover_after_restart();
        // A clean shutdown can leave only queued work behind. Reopening Codux
        // must never resume autonomous side effects without a fresh user start.
        if !changed && self.state == AgentTaskRelayBoardState::Running {
            self.paused_by_user = true;
            self.state = AgentTaskRelayBoardState::PausedByUser;
            self.bump_revision();
            changed = true;
        }
        Ok(changed)
    }

    fn task_index(&self, task_id: u64) -> Option<usize> {
        self.tasks.iter().position(|task| task.id == task_id)
    }

    fn transition_by_id(
        &mut self,
        task_id: u64,
        expected: AgentTaskRelayTaskState,
        next: AgentTaskRelayTaskState,
    ) -> bool {
        let Some(index) = self.task_index(task_id) else {
            return false;
        };
        if self.tasks[index].state != expected {
            return false;
        }
        self.transition_task(index, next);
        self.bump_revision();
        true
    }

    fn transition_task(&mut self, index: usize, next: AgentTaskRelayTaskState) {
        let previous = self.tasks[index].state;
        adjust_count(&mut self.counts, previous, false);
        self.tasks[index].state = next;
        adjust_count(&mut self.counts, next, true);
    }

    fn finish_task(
        &mut self,
        index: usize,
        outcome: AgentTaskRelayTaskState,
        completion_event_id: Option<String>,
        assistant_preview: Option<String>,
        now: f64,
    ) {
        let task = self.tasks.remove(index).expect("task index");
        adjust_count(&mut self.counts, task.state, false);
        self.total_task_bytes = self.total_task_bytes.saturating_sub(task.text.len());
        self.receipts.push_front(AgentTaskRelayReceipt {
            task_id: task.id,
            state: outcome,
            task_preview: truncate_text(&task.text, 240),
            finished_at: now,
            assistant_preview: assistant_preview.map(|text| truncate_text(&text, 4096)),
            completion_event_id,
        });
        while self.receipts.len() > MAX_RELAY_RECEIPTS {
            self.receipts.pop_back();
        }
        self.counts.receipts = self.receipts.len();
        self.state = if self.tasks.is_empty() {
            AgentTaskRelayBoardState::Completed
        } else if self.paused_by_user {
            AgentTaskRelayBoardState::PausedByUser
        } else {
            AgentTaskRelayBoardState::Running
        };
        self.bump_revision();
    }

    fn recalculate_counts(&mut self) {
        let mut counts = AgentTaskRelayCounts {
            receipts: self.receipts.len(),
            ..AgentTaskRelayCounts::default()
        };
        for task in &self.tasks {
            adjust_count(&mut counts, task.state, true);
        }
        self.counts = counts;
    }

    fn complete_if_empty(&mut self) {
        if self.tasks.is_empty() {
            self.state = AgentTaskRelayBoardState::Completed;
        }
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
}

fn default_true() -> bool {
    true
}

fn is_active_task_state(state: AgentTaskRelayTaskState) -> bool {
    matches!(
        state,
        AgentTaskRelayTaskState::Dispatching
            | AgentTaskRelayTaskState::AwaitingAck
            | AgentTaskRelayTaskState::Running
            | AgentTaskRelayTaskState::NeedsInput
            | AgentTaskRelayTaskState::DeliveryUnknown
    )
}

fn adjust_count(
    counts: &mut AgentTaskRelayCounts,
    state: AgentTaskRelayTaskState,
    increment: bool,
) {
    let value = match state {
        AgentTaskRelayTaskState::Queued => &mut counts.queued,
        AgentTaskRelayTaskState::Dispatching
        | AgentTaskRelayTaskState::AwaitingAck
        | AgentTaskRelayTaskState::Running => &mut counts.running,
        AgentTaskRelayTaskState::NeedsInput | AgentTaskRelayTaskState::DeliveryUnknown => {
            &mut counts.waiting
        }
        AgentTaskRelayTaskState::Completed
        | AgentTaskRelayTaskState::Failed
        | AgentTaskRelayTaskState::Skipped => return,
    };
    if increment {
        *value = value.saturating_add(1);
    } else {
        *value = value.saturating_sub(1);
    }
}

fn truncate_text(text: &str, max_bytes: usize) -> Arc<str> {
    if text.len() <= max_bytes {
        return Arc::from(text);
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Arc::from(&text[..end])
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentTaskRelayLoadResult {
    pub boards: Vec<AgentTaskRelayBoard>,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct AgentTaskRelayService {
    support_dir: PathBuf,
}

impl AgentTaskRelayService {
    pub fn new(support_dir: PathBuf) -> Self {
        Self { support_dir }
    }

    pub fn load_all(&self) -> Result<AgentTaskRelayLoadResult, String> {
        let cache = crate::persistent_cache::PersistentCacheStore::for_support_dir(
            self.support_dir.clone(),
        )?;
        let values = cache.scan_json::<serde_json::Value>(AGENT_TASK_RELAY_NAMESPACE)?;
        let mut result = AgentTaskRelayLoadResult::default();
        for (key, value) in values {
            match serde_json::from_value::<AgentTaskRelayBoard>(value) {
                Ok(mut board) => match board.normalize_loaded() {
                    Ok(_) => {
                        remember_committed_revision(
                            cache.path().to_path_buf(),
                            key,
                            board.revision,
                        );
                        result.boards.push(board);
                    }
                    Err(error) => result.errors.push(format!("{key}: {error}")),
                },
                Err(error) => result.errors.push(format!("{key}: {error}")),
            }
        }
        result.boards.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(result)
    }

    /// Call this from a background executor. Returning confirms the redb
    /// transaction committed, which is the barrier required before a PTY write.
    pub fn save_durable(&self, board: &AgentTaskRelayBoard) -> Result<u64, String> {
        if board.id.trim().is_empty() {
            return Err("Task relay board id is required.".to_string());
        }
        let cache = crate::persistent_cache::PersistentCacheStore::for_support_dir(
            self.support_dir.clone(),
        )?;
        let revision_key = (cache.path().to_path_buf(), board.id.clone());
        let revisions = COMMITTED_REVISIONS.get_or_init(Default::default);
        let mut revisions = revisions.lock();
        let committed = revisions.get(&revision_key).copied().or_else(|| {
            cache
                .get_json::<AgentTaskRelayBoard>(AGENT_TASK_RELAY_NAMESPACE, &board.id)
                .ok()
                .flatten()
                .map(|stored| stored.revision)
        });
        if committed.is_some_and(|revision| revision > board.revision) {
            return Err(format!(
                "Refusing stale task relay revision {} after committed revision {}.",
                board.revision,
                committed.unwrap_or_default()
            ));
        }
        cache.put_json(AGENT_TASK_RELAY_NAMESPACE, &board.id, board)?;
        revisions.insert(revision_key, board.revision);
        Ok(board.revision)
    }

    pub fn delete_durable(&self, board_id: &str) -> Result<bool, String> {
        let cache = crate::persistent_cache::PersistentCacheStore::for_support_dir(
            self.support_dir.clone(),
        )?;
        let removed = cache.delete_json(AGENT_TASK_RELAY_NAMESPACE, board_id)?;
        if let Some(revisions) = COMMITTED_REVISIONS.get() {
            revisions
                .lock()
                .remove(&(cache.path().to_path_buf(), board_id.to_string()));
        }
        Ok(removed)
    }
}

fn remember_committed_revision(path: PathBuf, key: String, revision: u64) {
    COMMITTED_REVISIONS
        .get_or_init(Default::default)
        .lock()
        .entry((path, key))
        .and_modify(|current| *current = (*current).max(revision))
        .or_insert(revision);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn target() -> AgentTaskRelayTarget {
        AgentTaskRelayTarget {
            project_id: "project-1".to_string(),
            worktree_id: "worktree-1".to_string(),
            terminal_id: "terminal-1".to_string(),
            terminal_instance_id: Some("instance-1".to_string()),
            tool: "codex".to_string(),
            ai_session_id: Some("session-1".to_string()),
        }
    }

    #[test]
    fn relay_runs_one_task_and_keeps_a_compact_receipt() {
        let mut board = AgentTaskRelayBoard::new("board-1", target());
        let first = board.add_task("first task".to_string(), 1.0).unwrap();
        let second = board.add_task("second task".to_string(), 2.0).unwrap();
        board.start().unwrap();

        assert_eq!(board.begin_dispatch(3.0, Some(2.5)), Some(first));
        assert!(board.mark_write_succeeded(first, 3.1));
        assert!(board.mark_acknowledged(first, 3.2));
        assert!(!board.mark_acknowledged(second, 3.3));
        assert!(board.mark_completed(
            first,
            "completion-1".to_string(),
            Some("done".to_string()),
            4.0,
        ));

        assert_eq!(board.counts.queued, 1);
        assert_eq!(board.counts.running, 0);
        assert_eq!(board.counts.receipts, 1);
        assert_eq!(board.begin_dispatch(5.0, Some(4.0)), Some(second));
    }

    #[test]
    fn queued_relay_rebinds_for_a_new_terminal_lease_and_agent_session() {
        let mut board = AgentTaskRelayBoard::new("board-1", target());
        board.add_task("next round".to_string(), 1.0).unwrap();
        let mut next = target();
        next.terminal_instance_id = Some("instance-2".to_string());
        next.tool = "claude".to_string();
        next.ai_session_id = Some("session-2".to_string());

        assert!(board.rebind_target(next.clone()).unwrap());
        assert_eq!(board.target, next);
    }

    #[test]
    fn active_relay_cannot_transfer_to_another_terminal_lease() {
        let mut board = AgentTaskRelayBoard::new("board-1", target());
        let task_id = board.add_task("in flight".to_string(), 1.0).unwrap();
        board.start().unwrap();
        assert_eq!(board.begin_dispatch(2.0, None), Some(task_id));
        let mut next = target();
        next.terminal_instance_id = Some("instance-2".to_string());

        assert_eq!(
            board.rebind_target(next),
            Err("Cannot change task relay target while a task is active.")
        );
        assert_eq!(
            board.target.terminal_instance_id.as_deref(),
            Some("instance-1")
        );
    }

    #[test]
    fn delivery_unknown_can_rebind_before_an_explicit_retry() {
        let mut board = AgentTaskRelayBoard::new("board-1", target());
        let task_id = board.add_task("retry me".to_string(), 1.0).unwrap();
        board.start().unwrap();
        assert_eq!(board.begin_dispatch(2.0, None), Some(task_id));
        assert!(board.mark_write_succeeded(task_id, 2.1));
        assert!(board.mark_delivery_unknown(task_id));
        let mut next = target();
        next.terminal_instance_id = Some("instance-2".to_string());

        assert!(board.rebind_target(next.clone()).unwrap());
        assert_eq!(board.target, next);
        assert!(board.resolve_delivery_unknown(task_id, true, 3.0));
        assert_eq!(board.tasks[0].state, AgentTaskRelayTaskState::Queued);
    }

    #[test]
    fn recovered_in_flight_task_never_resends_automatically() {
        let mut board = AgentTaskRelayBoard::new("board-1", target());
        let task_id = board.add_task("uncertain".to_string(), 1.0).unwrap();
        board.start().unwrap();
        assert_eq!(board.begin_dispatch(2.0, None), Some(task_id));

        assert!(board.recover_after_restart());
        assert_eq!(
            board.tasks[0].state,
            AgentTaskRelayTaskState::DeliveryUnknown
        );
        assert_eq!(
            board.state,
            AgentTaskRelayBoardState::PausedBlocked(AgentTaskRelayBlockReason::DeliveryUnknown)
        );
        assert_eq!(board.begin_dispatch(3.0, None), None);
    }

    #[test]
    fn task_capacity_and_total_bytes_are_bounded_without_mutation() {
        let mut board = AgentTaskRelayBoard::new("board-1", target());
        assert_eq!(
            board.add_task(String::new(), 1.0),
            Err("Task text is required.")
        );
        assert_eq!(board.revision, 0);

        for index in 0..MAX_RELAY_TASKS {
            board
                .add_task(format!("task {index}"), index as f64)
                .unwrap();
        }
        let revision = board.revision;
        assert_eq!(
            board.add_task("overflow".to_string(), 200.0),
            Err("Task relay is full.")
        );
        assert_eq!(board.revision, revision);
        assert_eq!(board.tasks.len(), MAX_RELAY_TASKS);
    }

    #[test]
    fn editing_ordering_and_needs_input_keep_o1_counts_consistent() {
        let mut board = AgentTaskRelayBoard::new("board-1", target());
        let first = board.add_task("first".to_string(), 1.0).unwrap();
        let second = board.add_task("second".to_string(), 2.0).unwrap();
        assert!(board.promote_task(second));
        assert_eq!(board.tasks[0].id, second);
        assert!(board.edit_task(second, "updated".to_string()).unwrap());
        assert!(!board.delete_task(999));

        board.start().unwrap();
        assert_eq!(board.begin_dispatch(3.0, None), Some(second));
        assert!(board.mark_write_succeeded(second, 3.1));
        assert!(board.mark_acknowledged(second, 3.2));
        assert!(board.mark_needs_input(second));
        assert_eq!(board.counts.running, 0);
        assert_eq!(board.counts.waiting, 1);
        assert_eq!(board.counts.queued, 1);
        assert!(board.mark_input_resumed(second, Some(3.3)));
        assert_eq!(board.counts.running, 1);
        assert_eq!(board.counts.waiting, 0);
        assert_eq!(board.tasks[1].id, first);
    }

    #[test]
    fn acknowledgement_timeout_and_interruption_pause_without_dispatching_next_task() {
        let mut board = AgentTaskRelayBoard::new("board-1", target());
        let first = board.add_task("first".to_string(), 1.0).unwrap();
        board.add_task("second".to_string(), 1.1).unwrap();
        board.start().unwrap();
        assert_eq!(board.begin_dispatch(2.0, None), Some(first));
        assert!(board.mark_write_succeeded(first, 2.1));
        assert!(board.mark_delivery_unknown(first));
        assert_eq!(board.counts.waiting, 1);
        assert_eq!(board.counts.queued, 1);
        assert_eq!(board.begin_dispatch(3.0, None), None);

        assert!(board.resolve_delivery_unknown(first, true, 4.0));
        board.start().unwrap();
        assert_eq!(board.begin_dispatch(5.0, None), Some(first));
        assert!(board.mark_write_succeeded(first, 5.1));
        assert!(board.mark_acknowledged(first, 5.2));
        assert!(board.mark_interrupted(first, 6.0));
        assert_eq!(board.counts.running, 0);
        assert_eq!(board.counts.queued, 1);
        assert_eq!(
            board.state,
            AgentTaskRelayBoardState::PausedBlocked(AgentTaskRelayBlockReason::Interrupted)
        );
        assert_eq!(
            board.start(),
            Err("Resolve the blocked relay state before starting relay.")
        );
        assert_eq!(board.counts.queued, 1);
        assert_eq!(board.begin_dispatch(7.0, None), None);
    }

    #[test]
    fn persistence_failure_keeps_the_active_task_and_blocks_start() {
        let mut board = AgentTaskRelayBoard::new("board-1", target());
        let task_id = board.add_task("persist me".to_string(), 1.0).unwrap();
        board.start().unwrap();
        assert_eq!(board.begin_dispatch(2.0, Some(1.5)), Some(task_id));

        assert!(board.mark_persistence_failed());
        let task = board.task(task_id).expect("active task remains available");
        assert_eq!(task.state, AgentTaskRelayTaskState::Dispatching);
        assert!(board.receipts.is_empty());
        assert_eq!(
            board.state,
            AgentTaskRelayBoardState::PausedBlocked(AgentTaskRelayBlockReason::PersistenceError)
        );
        assert_eq!(
            board.start(),
            Err("Resolve the blocked relay state before starting relay.")
        );
    }

    #[test]
    fn durable_service_rejects_revision_regression_and_recovers_in_flight() {
        let support_dir = temp_support_dir("durable");
        let service = AgentTaskRelayService::new(support_dir.clone());
        let mut board = AgentTaskRelayBoard::new("board-1", target());
        let task_id = board.add_task("persist me".to_string(), 1.0).unwrap();
        board.start().unwrap();
        assert_eq!(board.begin_dispatch(2.0, None), Some(task_id));
        let committed_revision = service.save_durable(&board).unwrap();

        let mut stale = board.clone();
        stale.revision = committed_revision.saturating_sub(1);
        assert!(service.save_durable(&stale).is_err());

        let loaded = service.load_all().unwrap();
        assert!(loaded.errors.is_empty());
        assert_eq!(loaded.boards.len(), 1);
        assert_eq!(
            loaded.boards[0].tasks[0].state,
            AgentTaskRelayTaskState::DeliveryUnknown
        );
        assert!(matches!(
            loaded.boards[0].state,
            AgentTaskRelayBoardState::PausedBlocked(AgentTaskRelayBlockReason::DeliveryUnknown)
        ));
        let _ = std::fs::remove_dir_all(support_dir);
    }

    #[test]
    fn restart_pauses_a_running_board_that_only_has_queued_work() {
        let mut board = AgentTaskRelayBoard::new("board-1", target());
        board.add_task("queued".to_string(), 1.0).unwrap();
        board.start().unwrap();

        assert!(board.normalize_loaded().unwrap());
        assert_eq!(board.state, AgentTaskRelayBoardState::PausedByUser);
        assert!(board.paused_by_user);
        assert_eq!(board.begin_dispatch(2.0, None), None);
    }

    fn temp_support_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codux-agent-task-relay-{label}-{nanos}"))
    }
}
