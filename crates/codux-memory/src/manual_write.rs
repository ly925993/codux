use crate::{MemoryKind, MemoryScope, MemoryTier};
use serde::{Deserialize, Serialize};

pub const MANUAL_MEMORY_PLAN_VERSION: u32 = 1;

/// A reviewable, project-bound batch of manual memory changes.
///
/// The plan is intentionally file based so an agent can preview the exact write
/// set, show the digest to the user, and apply only that reviewed payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManualMemoryPlan {
    pub version: u32,
    pub project_id: String,
    pub reason: String,
    pub operations: Vec<ManualMemoryOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ManualMemoryOperation {
    /// Writes one canonical memory and optionally supersedes reviewed entries.
    Write {
        memory: ManualMemoryDraft,
        #[serde(default)]
        replace: Option<ManualMemoryTarget>,
        #[serde(default)]
        archive: Vec<ManualMemoryTarget>,
        #[serde(default)]
        archive_after_write: bool,
    },
    /// Archives a stale entry when no replacement memory is appropriate.
    Archive { target: ManualMemoryTarget },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManualMemoryDraft {
    pub scope: MemoryScope,
    pub module_key: String,
    pub tier: MemoryTier,
    pub kind: MemoryKind,
    pub content: String,
    #[serde(default)]
    pub rationale: Option<String>,
}

/// Targets use stable entry IDs. Preview folds each current normalized hash into
/// the confirmation digest, so apply fails closed if extraction changes a row.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManualMemoryTarget {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManualMemoryPlanPreview {
    pub digest: String,
    pub project_id: String,
    pub operation_count: usize,
    pub write_count: usize,
    pub target_entry_count: usize,
    pub resulting_archive_count: usize,
    pub privacy_redaction_count: usize,
    pub target_entry_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManualMemoryApplyResult {
    pub preview: ManualMemoryPlanPreview,
    pub written_entry_ids: Vec<String>,
    pub archived_entry_ids: Vec<String>,
    pub backup_path: String,
}

/// Active entries visible to a project-scoped manual maintenance plan.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManualMemoryEntry {
    pub id: String,
    pub scope: MemoryScope,
    pub project_id: Option<String>,
    pub module_key: String,
    pub tier: MemoryTier,
    pub kind: MemoryKind,
    pub content: String,
    pub rationale: Option<String>,
    pub updated_at: f64,
}
