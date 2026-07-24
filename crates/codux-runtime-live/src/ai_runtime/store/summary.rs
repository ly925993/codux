use super::{AIRuntimeStateCore, helpers::now_seconds};
use crate::ai_runtime::log::runtime_log_line;
use crate::ai_runtime::snapshot::{
    AILatestCompletion, AIProjectPhase, AIProjectStateSnapshot, AIProjectTotals,
    AIRuntimeCompletionEvent, AIRuntimeSessionCompletionEvent, AIRuntimeStateSnapshot,
    AISessionSnapshot,
};
const NEEDS_INPUT_VISIBLE_SECONDS: f64 = 30.0;

pub(super) fn state_snapshot_unlocked(core: &AIRuntimeStateCore) -> AIRuntimeStateSnapshot {
    let now = now_seconds();
    let mut sessions = core
        .sessions
        .values()
        .cloned()
        .map(|session| visible_session_snapshot(session, now))
        .collect::<Vec<_>>();
    sessions.sort_by(|left, right| right.updated_at.total_cmp(&left.updated_at));

    // Group sessions by project in a single pass, then derive each project's
    // phase/completed/totals from its own slice. Previously every project
    // re-scanned the whole session map several times (phase + completed +
    // totals, each a full scan), which is O(projects × sessions) per snapshot;
    // this is O(sessions). Global totals and the latest completion fall out of
    // the same loop, so neither needs an extra full scan.
    let mut groups: std::collections::HashMap<&str, Vec<&AISessionSnapshot>> =
        std::collections::HashMap::new();
    for session in core.sessions.values() {
        groups
            .entry(session.project_id.as_str())
            .or_default()
            .push(session);
    }
    let mut project_ids = groups.keys().copied().collect::<Vec<_>>();
    project_ids.sort_unstable();

    let mut projects = Vec::with_capacity(project_ids.len());
    let mut global_totals = AIProjectTotals::default();
    let mut latest_completion: Option<AILatestCompletion> = None;
    for project_id in project_ids {
        let group = groups.get_mut(project_id).expect("grouped project");
        group.sort_by(|left, right| right.updated_at.total_cmp(&left.updated_at));
        let project_phase = project_phase_from(group, now);
        let completed_phase = completed_phase_from(group, core, project_id, now);
        let totals = project_totals_from(group, now);
        global_totals.total_tokens += totals.total_tokens;
        global_totals.cached_input_tokens += totals.cached_input_tokens;
        global_totals.running += totals.running;
        global_totals.needs_input += totals.needs_input;
        global_totals.completed += totals.completed;
        if let AIProjectPhase::Completed {
            tool,
            was_interrupted,
            updated_at,
        } = &completed_phase
        {
            let candidate = AILatestCompletion {
                id: format!("{project_id}:{updated_at}"),
                project_id: project_id.to_string(),
                project_name: group
                    .first()
                    .map(|session| session.project_name.clone())
                    .unwrap_or_else(|| project_id.to_string()),
                tool: tool.clone(),
                was_interrupted: *was_interrupted,
                updated_at: *updated_at,
            };
            if latest_completion
                .as_ref()
                .map(|current| candidate.updated_at > current.updated_at)
                .unwrap_or(true)
            {
                latest_completion = Some(candidate);
            }
        }
        projects.push(AIProjectStateSnapshot {
            project_id: project_id.to_string(),
            project_phase,
            completed_phase,
            totals,
        });
    }

    let needs_input_count = projects
        .iter()
        .filter(|project| matches!(project.project_phase, AIProjectPhase::NeedsInput { .. }))
        .count();
    let running_count = projects
        .iter()
        .filter(|project| matches!(project.project_phase, AIProjectPhase::Running { .. }))
        .count();
    let completion_count = projects
        .iter()
        .filter(|project| matches!(project.completed_phase, AIProjectPhase::Completed { .. }))
        .count();
    AIRuntimeStateSnapshot {
        sessions,
        projects,
        global_totals,
        needs_input_count,
        running_count,
        completion_count,
        latest_completion,
        updated_at: now,
    }
}

fn project_phase_from(sorted: &[&AISessionSnapshot], now: f64) -> AIProjectPhase {
    if let Some(session) = sorted
        .iter()
        .find(|session| session_has_active_needs_input(session, now))
    {
        return AIProjectPhase::NeedsInput {
            tool: session.tool.clone(),
        };
    }
    if let Some(session) = sorted.iter().find(|session| session.state == "responding") {
        return AIProjectPhase::Running {
            tool: session.tool.clone(),
        };
    }
    AIProjectPhase::Idle
}

fn completed_phase_from(
    sorted: &[&AISessionSnapshot],
    core: &AIRuntimeStateCore,
    project_id: &str,
    now: f64,
) -> AIProjectPhase {
    if sorted
        .iter()
        .any(|session| session_suppresses_completed_phase(session, now))
    {
        return AIProjectPhase::Idle;
    }
    let latest_active_started_at = core
        .latest_active_started_at_by_project
        .get(project_id)
        .copied()
        .unwrap_or(0.0);
    let completed = sorted.iter().find(|session| {
        session.state == "idle"
            && (session.has_completed_turn || session.was_interrupted)
            && session.updated_at >= latest_active_started_at
    });
    let Some(completed) = completed else {
        return AIProjectPhase::Idle;
    };
    let dismissed_at = core
        .dismissed_completed_at
        .get(project_id)
        .copied()
        .unwrap_or(0.0);
    if completed.updated_at <= dismissed_at {
        return AIProjectPhase::Idle;
    }
    AIProjectPhase::Completed {
        tool: completed.tool.clone(),
        was_interrupted: completed.was_interrupted,
        updated_at: completed.updated_at,
    }
}

fn project_totals_from(sorted: &[&AISessionSnapshot], now: f64) -> AIProjectTotals {
    sorted
        .iter()
        .fold(AIProjectTotals::default(), |mut total, session| {
            total.total_tokens += (session.total_tokens - session.baseline_total_tokens).max(0);
            total.cached_input_tokens +=
                (session.cached_input_tokens - session.baseline_cached_input_tokens).max(0);
            total.running += usize::from(session.state == "responding");
            total.needs_input += usize::from(session_has_active_needs_input(session, now));
            total.completed += usize::from(session.has_completed_turn);
            total
        })
}

pub(super) fn completed_phase_unlocked(
    core: &AIRuntimeStateCore,
    project_id: &str,
    now: f64,
) -> AIProjectPhase {
    completed_phase_from(
        &sorted_project_sessions(core, project_id),
        core,
        project_id,
        now,
    )
}

#[cfg(test)]
pub(super) fn project_totals_unlocked(
    core: &AIRuntimeStateCore,
    project_id: Option<&str>,
    now: f64,
) -> AIProjectTotals {
    let sessions = core
        .sessions
        .values()
        .filter(|session| {
            project_id
                .map(|project_id| session.project_id == project_id)
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    project_totals_from(&sessions, now)
}

pub(super) fn drain_completion_events_unlocked(
    core: &mut AIRuntimeStateCore,
) -> Vec<AIRuntimeCompletionEvent> {
    let mut events = Vec::new();
    let now = now_seconds();
    let mut groups: std::collections::HashMap<String, Vec<&AISessionSnapshot>> =
        std::collections::HashMap::new();
    for session in core.sessions.values() {
        groups
            .entry(session.project_id.clone())
            .or_default()
            .push(session);
    }
    for (project_id, group) in groups.iter_mut() {
        group.sort_by(|left, right| right.updated_at.total_cmp(&left.updated_at));
        let AIProjectPhase::Completed {
            tool,
            was_interrupted,
            ..
        } = completed_phase_from(group, core, project_id, now)
        else {
            continue;
        };
        let Some(session) = group
            .iter()
            .copied()
            .filter(|session| session.state == "idle")
            .filter(|session| session.has_completed_turn || session.was_interrupted)
            .max_by(|left, right| left.updated_at.total_cmp(&right.updated_at))
            .cloned()
        else {
            continue;
        };
        let completion_key = completion_event_key(&session);
        const MAX_NOTIFIED_COMPLETION_KEYS: usize = 2048;
        if core.notified_completion_keys.len() >= MAX_NOTIFIED_COMPLETION_KEYS {
            core.notified_completion_keys.clear();
        }
        if !core.notified_completion_keys.insert(completion_key.clone()) {
            continue;
        }
        let project_name = group
            .first()
            .map(|session| session.project_name.clone())
            .unwrap_or_else(|| project_id.clone());
        runtime_log_line(
            "runtime-state",
            &format!(
                "completion emit key={} project={} terminal={} session={} updated_at={:.3}",
                completion_key,
                project_id,
                session.terminal_id,
                session.ai_session_id.as_deref().unwrap_or("none"),
                session.updated_at
            ),
        );
        events.push(AIRuntimeCompletionEvent {
            id: completion_key,
            project_name,
            tool,
            was_interrupted,
            session: Some(session),
        });
    }
    events
}

pub(super) fn drain_session_completion_events_unlocked(
    core: &mut AIRuntimeStateCore,
) -> Vec<AIRuntimeSessionCompletionEvent> {
    const MAX_NOTIFIED_SESSION_COMPLETION_KEYS: usize = 4096;

    // Sort before emitting so a mutation that discovers several completions
    // has a stable chronological order regardless of HashMap iteration order.
    let mut completed_sessions = core
        .sessions
        .values()
        .filter(|session| session.state == "idle")
        .filter(|session| session.has_completed_turn || session.was_interrupted)
        .cloned()
        .collect::<Vec<_>>();
    completed_sessions.sort_by(|left, right| left.updated_at.total_cmp(&right.updated_at));

    let mut events = Vec::new();
    for session in completed_sessions {
        let completion_key = completion_event_key(&session);
        if !core
            .notified_session_completion_keys
            .insert(completion_key.clone())
        {
            continue;
        }
        core.notified_session_completion_order
            .push_back(completion_key.clone());
        while core.notified_session_completion_order.len() > MAX_NOTIFIED_SESSION_COMPLETION_KEYS {
            if let Some(expired_key) = core.notified_session_completion_order.pop_front() {
                core.notified_session_completion_keys.remove(&expired_key);
            }
        }

        let turn_started_at = session
            .completed_turn_started_at
            .or(session.active_turn_started_at)
            .or(session.runtime_turn_started_at)
            .or(session.started_at);
        events.push(AIRuntimeSessionCompletionEvent {
            id: completion_key,
            project_id: session.project_id.clone(),
            project_name: session.project_name.clone(),
            terminal_id: session.terminal_id.clone(),
            terminal_instance_id: session.terminal_instance_id.clone(),
            tool: session.tool.clone(),
            ai_session_id: session.ai_session_id.clone(),
            turn_started_at,
            completed_at: session.updated_at,
            has_completed_turn: session.has_completed_turn,
            was_interrupted: session.was_interrupted,
            latest_assistant_preview: session.latest_assistant_preview.clone(),
            total_tokens: session.total_tokens,
            cached_input_tokens: session.cached_input_tokens,
        });
    }
    events
}

fn session_has_active_needs_input(session: &AISessionSnapshot, now: f64) -> bool {
    session.state == "needsInput" && now - session.updated_at <= NEEDS_INPUT_VISIBLE_SECONDS
}

fn session_suppresses_completed_phase(session: &AISessionSnapshot, now: f64) -> bool {
    if session_has_active_needs_input(session, now) || session.state == "responding" {
        return true;
    }
    if session.has_completed_turn || session.was_interrupted {
        return false;
    }
    session.active_turn_started_at.is_some()
        || session.runtime_turn_started_at.is_some()
        || session.completed_turn_started_at.is_some()
}

fn visible_session_snapshot(mut session: AISessionSnapshot, now: f64) -> AISessionSnapshot {
    if session.state == "needsInput" && !session_has_active_needs_input(&session, now) {
        session.state = "idle".to_string();
        session.status = "idle".to_string();
        session.notification_type = None;
        session.target_tool_name = None;
        session.message = None;
    }
    session
}

fn completion_event_key(session: &AISessionSnapshot) -> String {
    let session_identity = session
        .ai_session_id
        .as_deref()
        .unwrap_or(session.terminal_id.as_str());
    let turn_identity = session
        .completed_turn_started_at
        .or(session.active_turn_started_at)
        .or(session.runtime_turn_started_at)
        .or(session.started_at)
        .unwrap_or(session.updated_at);
    format!(
        "{}:{}:{}:{:.3}",
        session.project_id, session.tool, session_identity, turn_identity
    )
}

fn sorted_project_sessions<'a>(
    core: &'a AIRuntimeStateCore,
    project_id: &str,
) -> Vec<&'a AISessionSnapshot> {
    let mut sessions = core
        .sessions
        .values()
        .filter(|session| session.project_id == project_id)
        .collect::<Vec<_>>();
    sessions.sort_by(|left, right| right.updated_at.total_cmp(&left.updated_at));
    sessions
}
