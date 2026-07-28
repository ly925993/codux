use super::{
    helpers::{normalized_memory_content, sha256_hex, should_skip_memory_candidate},
    types::{MemoryCandidate, MemoryDecisionLog, MemoryWriteDecision, MemoryWriteDecisionKind},
};
use crate::{
    MANUAL_MEMORY_PLAN_VERSION, ManualMemoryApplyResult, ManualMemoryDraft,
    ManualMemoryEntry, ManualMemoryOperation, ManualMemoryPlan, ManualMemoryPlanPreview,
    MemoryEntryStatus, MemoryKind, MemoryScope, MemoryService, MemoryTier, now_seconds,
    privacy::privacy_scrub,
};
use rusqlite::{Connection, OptionalExtension, backup::Backup, params};
use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    time::Duration,
};
use uuid::Uuid;

const MAX_MANUAL_MEMORY_OPERATIONS: usize = 200;
const MAX_MANUAL_MEMORY_CONTENT_CHARS: usize = 4_000;
const MAX_MANUAL_MEMORY_RATIONALE_CHARS: usize = 1_000;
const MAX_MANUAL_MEMORY_MODULE_CHARS: usize = 64;

#[derive(Clone)]
struct PreparedMemoryDraft {
    draft: ManualMemoryDraft,
    project_id: Option<String>,
}

#[derive(Clone)]
enum PreparedManualOperation {
    Write {
        memory: PreparedMemoryDraft,
        replace_id: Option<String>,
        archive_ids: Vec<String>,
        archive_after_write: bool,
    },
    Archive {
        target_id: String,
    },
}

struct PreparedManualPlan {
    preview: ManualMemoryPlanPreview,
    operations: Vec<PreparedManualOperation>,
}

impl MemoryService {
    /// Lists the active user and project entries that a project-bound plan may
    /// reference. Internal hashes and database details stay private.
    pub fn list_manual_entries(&self, project_id: &str) -> Result<Vec<ManualMemoryEntry>, String> {
        let project_id = project_id.trim();
        if Uuid::parse_str(project_id).is_err() {
            return Err("Manual memory project id must be a UUID.".to_string());
        }
        let conn = self.open_read_only_connection()?;
        let mut statement = conn
            .prepare(
                r#"
                SELECT id, scope, project_id, COALESCE(module_key, 'general'), tier, kind,
                       content, rationale, updated_at
                FROM memory_entries
                WHERE status = 'active'
                  AND superseded_by IS NULL
                  AND (scope = 'user' OR project_id = ?1)
                ORDER BY CASE tier WHEN 'core' THEN 0 WHEN 'working' THEN 1 ELSE 2 END,
                         updated_at DESC, created_at DESC;
                "#,
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![project_id], |row| {
                Ok(ManualMemoryEntry {
                    id: row.get(0)?,
                    scope: MemoryScope::from_token(row.get::<_, String>(1)?.as_str()),
                    project_id: row.get(2)?,
                    module_key: row.get(3)?,
                    tier: MemoryTier::from_token(row.get::<_, String>(4)?.as_str()),
                    kind: MemoryKind::from_token(row.get::<_, String>(5)?.as_str()),
                    content: row.get(6)?,
                    rationale: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    /// Validates a manual plan without writing and returns the state-bound digest
    /// that must be presented again to apply the same reviewed change set.
    pub fn preview_manual_plan(
        &self,
        plan: &ManualMemoryPlan,
    ) -> Result<ManualMemoryPlanPreview, String> {
        let conn = self.open_read_only_connection()?;
        Ok(self.prepare_manual_plan(&conn, plan)?.preview)
    }

    /// Applies a reviewed plan atomically after creating a consistent SQLite
    /// backup. The confirmation digest binds both JSON input and target hashes.
    pub fn apply_manual_plan(
        &self,
        plan: &ManualMemoryPlan,
        confirmation_digest: &str,
    ) -> Result<ManualMemoryApplyResult, String> {
        let preview = self.preview_manual_plan(plan)?;
        if preview.digest != confirmation_digest.trim() {
            return Err(
                "Manual memory confirmation digest does not match the current plan and database state."
                    .to_string(),
            );
        }

        // Schema maintenance is itself a write, so it happens only after the
        // caller has supplied the current reviewed confirmation digest.
        self.ensure_queue_schema()?;
        let backup_path = self.backup_manual_memory_database(&preview.digest)?;
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let prepared = self.prepare_manual_plan(&tx, plan)?;
        if prepared.preview.digest != confirmation_digest.trim() {
            return Err(
                "Manual memory targets changed after preview; no changes were committed."
                    .to_string(),
            );
        }

        let mut written_entry_ids = Vec::new();
        let mut archived_entry_ids = Vec::new();
        for operation in prepared.operations {
            match operation {
                PreparedManualOperation::Write {
                    memory,
                    replace_id,
                    archive_ids,
                    archive_after_write,
                } => {
                    let decision = replace_id.clone().map_or_else(
                        || MemoryWriteDecision {
                            kind: MemoryWriteDecisionKind::Create,
                            target_entry_id: None,
                            reason: "reviewed manual memory plan created a canonical entry"
                                .to_string(),
                        },
                        |target_entry_id| MemoryWriteDecision {
                            kind: MemoryWriteDecisionKind::Replace,
                            target_entry_id: Some(target_entry_id),
                            reason: "reviewed manual memory plan replaced a canonical entry"
                                .to_string(),
                        },
                    );
                    let entry = self
                        .write_candidate_with_decision(
                            &tx,
                            manual_candidate(memory, &prepared.preview.digest),
                            Some(decision),
                        )?
                        .ok_or_else(|| "Manual memory write was unexpectedly skipped.".to_string())?;
                    written_entry_ids.push(entry.id.clone());
                    if let Some(target_id) = replace_id {
                        archived_entry_ids.push(target_id);
                    }
                    for target_id in archive_ids {
                        self.supersede_entry(&tx, &target_id, &entry.id)?;
                        self.record_memory_decision(
                            &tx,
                            MemoryDecisionLog {
                                kind: MemoryWriteDecisionKind::Replace,
                                entry_id: Some(entry.id.clone()),
                                target_entry_id: Some(target_id.clone()),
                                reason: "reviewed manual memory plan consolidated a duplicate"
                                    .to_string(),
                                created_at: now_seconds(),
                            },
                        )?;
                        archived_entry_ids.push(target_id);
                    }
                    if archive_after_write {
                        self.archive_entries(&tx, std::slice::from_ref(&entry.id))?;
                        self.record_memory_decision(
                            &tx,
                            MemoryDecisionLog {
                                kind: MemoryWriteDecisionKind::Archive,
                                entry_id: Some(entry.id.clone()),
                                target_entry_id: Some(entry.id.clone()),
                                reason: "reviewed manual memory plan created an archival index"
                                    .to_string(),
                                created_at: now_seconds(),
                            },
                        )?;
                        archived_entry_ids.push(entry.id);
                    }
                }
                PreparedManualOperation::Archive { target_id } => {
                    self.archive_entries(&tx, std::slice::from_ref(&target_id))?;
                    self.record_memory_decision(
                        &tx,
                        MemoryDecisionLog {
                            kind: MemoryWriteDecisionKind::Archive,
                            entry_id: None,
                            target_entry_id: Some(target_id.clone()),
                            reason: "reviewed manual memory plan archived a stale entry".to_string(),
                            created_at: now_seconds(),
                        },
                    )?;
                    archived_entry_ids.push(target_id);
                }
            }
        }
        tx.commit().map_err(|error| error.to_string())?;

        written_entry_ids.sort();
        written_entry_ids.dedup();
        archived_entry_ids.sort();
        archived_entry_ids.dedup();
        Ok(ManualMemoryApplyResult {
            preview: prepared.preview,
            written_entry_ids,
            archived_entry_ids,
            backup_path: backup_path.display().to_string(),
        })
    }

    fn prepare_manual_plan(
        &self,
        conn: &Connection,
        plan: &ManualMemoryPlan,
    ) -> Result<PreparedManualPlan, String> {
        validate_manual_plan_header(plan)?;
        let mut target_ids = HashSet::new();
        let mut operations = Vec::with_capacity(plan.operations.len());
        let mut write_count = 0;
        let mut resulting_archive_count = 0;
        let mut privacy_redaction_count = 0;

        for operation in &plan.operations {
            match operation {
                ManualMemoryOperation::Write {
                    memory,
                    replace,
                    archive,
                    archive_after_write,
                } => {
                    write_count += 1;
                    resulting_archive_count +=
                        usize::from(replace.is_some()) + archive.len() + usize::from(*archive_after_write);
                    let memory = prepare_memory_draft(
                        memory,
                        &plan.project_id,
                        &mut privacy_redaction_count,
                    )?;
                    ensure_manual_memory_is_new(conn, &memory)?;
                    let replace_id = replace
                        .as_ref()
                        .map(|target| register_target(&mut target_ids, &target.id))
                        .transpose()?;
                    let archive_ids = archive
                        .iter()
                        .map(|target| register_target(&mut target_ids, &target.id))
                        .collect::<Result<Vec<_>, _>>()?;
                    operations.push(PreparedManualOperation::Write {
                        memory,
                        replace_id,
                        archive_ids,
                        archive_after_write: *archive_after_write,
                    });
                }
                ManualMemoryOperation::Archive { target } => {
                    resulting_archive_count += 1;
                    operations.push(PreparedManualOperation::Archive {
                        target_id: register_target(&mut target_ids, &target.id)?,
                    });
                }
            }
        }

        let mut target_states = target_ids
            .iter()
            .map(|id| load_manual_target_state(conn, id, &plan.project_id))
            .collect::<Result<Vec<_>, _>>()?;
        target_states.sort();
        let digest = manual_confirmation_digest(plan, &target_states)?;
        let target_entry_ids = target_states
            .iter()
            .map(|state| state.id.clone())
            .collect::<Vec<_>>();
        Ok(PreparedManualPlan {
            preview: ManualMemoryPlanPreview {
                digest,
                project_id: plan.project_id.trim().to_string(),
                operation_count: plan.operations.len(),
                write_count,
                target_entry_count: target_entry_ids.len(),
                resulting_archive_count,
                privacy_redaction_count,
                target_entry_ids,
            },
            operations,
        })
    }

    fn backup_manual_memory_database(&self, digest: &str) -> Result<PathBuf, String> {
        let parent = self
            .database_path
            .parent()
            .ok_or_else(|| "Memory database has no parent directory.".to_string())?;
        let backup_dir = parent.join("memory-backups");
        fs::create_dir_all(&backup_dir).map_err(|error| error.to_string())?;
        let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
        let backup_path = backup_dir.join(format!(
            "memory-before-manual-write-{timestamp}-{}.sqlite3",
            &digest[..12]
        ));
        if backup_path.exists() {
            return Err("Manual memory backup path already exists.".to_string());
        }

        let source = self.open_connection()?;
        let mut destination = Connection::open(&backup_path).map_err(|error| error.to_string())?;
        {
            let backup = Backup::new(&source, &mut destination).map_err(|error| error.to_string())?;
            backup
                .run_to_completion(128, Duration::from_millis(5), None)
                .map_err(|error| error.to_string())?;
        }
        Ok(backup_path)
    }
}

#[derive(Eq, Ord, PartialEq, PartialOrd)]
struct ManualTargetState {
    id: String,
    normalized_hash: String,
}

fn validate_manual_plan_header(plan: &ManualMemoryPlan) -> Result<(), String> {
    if plan.version != MANUAL_MEMORY_PLAN_VERSION {
        return Err(format!(
            "Unsupported manual memory plan version {}; expected {}.",
            plan.version, MANUAL_MEMORY_PLAN_VERSION
        ));
    }
    if Uuid::parse_str(plan.project_id.trim()).is_err() {
        return Err("Manual memory plan projectId must be a UUID.".to_string());
    }
    if plan.reason.trim().chars().count() < 8 {
        return Err("Manual memory plan reason must explain the intended change.".to_string());
    }
    if plan.operations.is_empty() || plan.operations.len() > MAX_MANUAL_MEMORY_OPERATIONS {
        return Err(format!(
            "Manual memory plan must contain 1-{MAX_MANUAL_MEMORY_OPERATIONS} operations."
        ));
    }
    Ok(())
}

fn prepare_memory_draft(
    memory: &ManualMemoryDraft,
    project_id: &str,
    privacy_redaction_count: &mut usize,
) -> Result<PreparedMemoryDraft, String> {
    let module_key = memory.module_key.trim();
    if module_key.is_empty() || module_key.chars().count() > MAX_MANUAL_MEMORY_MODULE_CHARS {
        return Err(format!(
            "Manual memory moduleKey must contain 1-{MAX_MANUAL_MEMORY_MODULE_CHARS} characters."
        ));
    }
    let raw_content = memory.content.trim();
    if raw_content.chars().count() > MAX_MANUAL_MEMORY_CONTENT_CHARS {
        return Err(format!(
            "Manual memory content exceeds {MAX_MANUAL_MEMORY_CONTENT_CHARS} characters."
        ));
    }
    let content = privacy_scrub(raw_content);
    if content != raw_content {
        *privacy_redaction_count += 1;
    }
    let rationale = memory
        .rationale
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            if value.chars().count() > MAX_MANUAL_MEMORY_RATIONALE_CHARS {
                return Err(format!(
                    "Manual memory rationale exceeds {MAX_MANUAL_MEMORY_RATIONALE_CHARS} characters."
                ));
            }
            let scrubbed = privacy_scrub(value);
            if scrubbed != value {
                *privacy_redaction_count += 1;
            }
            Ok(scrubbed)
        })
        .transpose()?;
    let draft = ManualMemoryDraft {
        scope: memory.scope.clone(),
        module_key: module_key.to_string(),
        tier: memory.tier.clone(),
        kind: memory.kind.clone(),
        content,
        rationale,
    };
    let candidate = manual_candidate(
        PreparedMemoryDraft {
            project_id: (draft.scope == MemoryScope::Project)
                .then(|| project_id.trim().to_string()),
            draft: draft.clone(),
        },
        "preview",
    );
    if should_skip_memory_candidate(&candidate) {
        return Err("Manual memory content is too short or low signal.".to_string());
    }
    Ok(PreparedMemoryDraft {
        project_id: (draft.scope == MemoryScope::Project)
            .then(|| project_id.trim().to_string()),
        draft,
    })
}

fn register_target(target_ids: &mut HashSet<String>, id: &str) -> Result<String, String> {
    let id = id.trim();
    if Uuid::parse_str(id).is_err() {
        return Err(format!("Manual memory target id is not a UUID: {id}"));
    }
    if !target_ids.insert(id.to_string()) {
        return Err(format!(
            "Manual memory target appears more than once in the plan: {id}"
        ));
    }
    Ok(id.to_string())
}

/// Manual writes never inherit the broader extraction path's upsert behavior:
/// changing or reactivating an existing row requires an explicit plan target.
fn ensure_manual_memory_is_new(
    conn: &Connection,
    memory: &PreparedMemoryDraft,
) -> Result<(), String> {
    let normalized_hash = sha256_hex(&normalized_memory_content(&memory.draft.content));
    let existing_id = conn
        .query_row(
            r#"
            SELECT id
            FROM memory_entries
            WHERE scope = ?1
              AND COALESCE(project_id, '') = COALESCE(?2, '')
              AND tool_id IS NULL
              AND COALESCE(module_key, '') = ?3
              AND normalized_hash = ?4
            LIMIT 1;
            "#,
            params![
                memory.draft.scope.as_str(),
                memory.project_id.as_deref(),
                memory.draft.module_key,
                normalized_hash,
            ],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some(existing_id) = existing_id {
        return Err(format!(
            "Manual memory content already exists as entry {existing_id}; reference existing entries explicitly instead of writing a duplicate."
        ));
    }
    Ok(())
}

fn load_manual_target_state(
    conn: &Connection,
    id: &str,
    project_id: &str,
) -> Result<ManualTargetState, String> {
    let row = conn
        .query_row(
            r#"
            SELECT normalized_hash, status, superseded_by
            FROM memory_entries
            WHERE id = ?1
              AND (scope = 'user' OR project_id = ?2)
            LIMIT 1;
            "#,
            params![id, project_id.trim()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Manual memory target is outside this project or missing: {id}"))?;
    if MemoryEntryStatus::from_str(&row.1) != MemoryEntryStatus::Active || row.2.is_some() {
        return Err(format!("Manual memory target is no longer active: {id}"));
    }
    Ok(ManualTargetState {
        id: id.to_string(),
        normalized_hash: row.0,
    })
}

fn manual_confirmation_digest(
    plan: &ManualMemoryPlan,
    targets: &[ManualTargetState],
) -> Result<String, String> {
    let mut canonical = serde_json::to_string(plan).map_err(|error| error.to_string())?;
    for target in targets {
        canonical.push('\n');
        canonical.push_str(&target.id);
        canonical.push(':');
        canonical.push_str(&target.normalized_hash);
    }
    Ok(sha256_hex(&canonical))
}

fn manual_candidate(memory: PreparedMemoryDraft, digest: &str) -> MemoryCandidate {
    MemoryCandidate {
        scope: memory.draft.scope,
        project_id: memory.project_id,
        tool_id: None,
        module_key: Some(memory.draft.module_key),
        tier: memory.draft.tier,
        kind: memory.draft.kind,
        content: memory.draft.content,
        rationale: memory.draft.rationale,
        source_tool: Some("codux-memory".to_string()),
        source_session_id: None,
        source_fingerprint: Some(format!("manual-plan:{digest}")),
    }
}
