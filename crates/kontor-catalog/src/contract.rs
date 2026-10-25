//! The frozen catalog data contract.
//!
//! These are the public types a reader sees. They are deliberately independent
//! of the storage schema: a migration may add columns and change no field here,
//! and a field may only change with a contract-version bump.
//!
//! # Three identities, never one
//!
//! A *logical plan* is what people mean by "that plan". An *occurrence* is one
//! exact file in one checkout or worktree. A *content revision* is the bytes
//! read from it. Renaming a file changes the occurrence and leaves the logical
//! plan; editing it changes the revision; checking the same branch out twice
//! creates two occurrences that may legitimately carry different revisions.
//! Collapsing any of the three into another is how a catalog starts reporting
//! one worktree's content as another's.
//!
//! # What this contract deliberately cannot claim
//!
//! An association collected here is `declared` or `explicit_operator`. None is
//! `verified`: TASK-001 has no Jira observation, so a plan-level `ws:<key>` or
//! epic join is a declaration, not a proof. [`Association::verified`] is
//! therefore always `false` in this contract version, and a later collector
//! that can prove one must say so through its own evidence rather than by
//! silently turning a declaration into a verification.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// The contract version carried by every catalog response.
pub const CATALOG_CONTRACT_VERSION: u32 = 1;

/// The single host-local scope this build collects.
pub const HOST_SCOPE: &str = "host";

/// The observer status of the whole work catalog.
///
/// It answers freshness, coverage and collector ownership together because a
/// caller that reads only `entries` cannot tell a healthy empty catalog from a
/// collector that never ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CatalogStatus {
    /// The contract version this body satisfies.
    pub schema_version: u32,
    /// Whether this answer came from the database or the fallback snapshot.
    pub source: ReadSource,
    /// A host-local catalog identity, stable across restarts of this database.
    pub catalog_id: String,
    /// The most recent complete generation, or `0` before the first one.
    pub generation: u64,
    /// How far collection has attempted; may be ahead of `generation`.
    pub attempted_generation: u64,
    /// The instant the latest complete generation finished, if any.
    pub as_of: Option<String>,
    /// Whether an owner currently holds the collector lease.
    pub collector: CollectorStatus,
    /// The latest collection attempt and its coverage.
    pub collection: CollectionStatus,
    /// Current work-item counts at the active generation.
    pub counts: CatalogCounts,
    /// Current registry projection, including diagnostics.
    pub registry: RegistryStatus,
    /// Effective operator configuration provenance.
    pub config: ConfigStatus,
    /// Whether the atomic fallback snapshot is present and at which generation.
    pub snapshot: SnapshotStatus,
    /// Diagnostics that are not attached to one entry.
    pub diagnostics: Vec<Diagnostic>,
    /// Conflicts that are not attached to one entry, or all current conflicts.
    pub conflicts: Vec<Conflict>,
    /// Dangling explicit associations retained across removal.
    pub dangling_associations: Vec<DanglingAssociation>,
}

/// The collector lease, which makes one owner per scope explicit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CollectorStatus {
    /// The current owner, if a live lease exists.
    pub owner: Option<String>,
    /// When the current lease is allowed to be taken over.
    pub lease_until: Option<String>,
    /// Whether this process is the current owner.
    pub owner_here: bool,
    /// The last collection failure, structurally redacted.
    pub last_error: Option<String>,
}

/// The state of the latest collection attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CollectionState {
    /// No sweep has ever run.
    NeverCollected,
    /// The latest sweep completed over every configured repository.
    Complete,
    /// The latest sweep stopped before every repository was read.
    Incomplete,
    /// The latest sweep could not start or read its configuration.
    Failed,
}

/// One collection attempt, complete or not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CollectionStatus {
    /// The attempt's own state.
    pub state: CollectionState,
    /// The generation this attempt would have produced.
    pub generation: u64,
    /// When the attempt started.
    pub started_at: Option<String>,
    /// When the attempt finished.
    pub finished_at: Option<String>,
    /// Measured wall-clock duration in milliseconds.
    pub duration_ms: Option<u64>,
    /// Configured repository count.
    pub repositories_configured: usize,
    /// Repositories read in this attempt.
    pub repositories_scanned: usize,
    /// Tracked and untracked Markdown files considered.
    pub files_seen: usize,
    /// Plan documents observed.
    pub entries_observed: usize,
    /// A structural failure, never file content and never a credential.
    pub error: Option<String>,
    /// Attempt-level structural diagnostics. Unknown registry keys are not
    /// registry entries and appear here as diagnostics, never as work.
    pub diagnostics: Vec<String>,
    /// Per-repository coverage for this attempt.
    pub coverage: Vec<RepositoryCoverage>,
}

/// One configured repository's coverage in the latest attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RepositoryCoverage {
    /// The configured root, as resolved on this host.
    pub root: String,
    /// The repository identity derived from the git common directory.
    pub repository_id: Option<String>,
    /// The checked-out commit at scan time.
    pub head: Option<String>,
    /// What happened to this root.
    pub state: RepositoryState,
    /// Files considered in this root.
    pub files_seen: usize,
    /// Plan documents observed in this root.
    pub entries_observed: usize,
    /// Whether `.github/plans` was explicitly permitted for this root.
    pub github_plans_permitted: bool,
    /// A structural failure for this root only.
    pub error: Option<String>,
}

/// Whether one configured root was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryState {
    /// Read to completion within the sweep.
    Scanned,
    /// Not reached because the sweep budget was spent.
    NotReached,
    /// Could not be read; other roots keep their own results.
    Failed,
}

/// Current counts, all derived from the active generation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CatalogCounts {
    /// Logical plans, including removed ones.
    pub logical_plans: usize,
    /// Active occurrences.
    pub occurrences: usize,
    /// Distinct content revisions retained.
    pub content_revisions: usize,
    /// Removed logical plans retained as history.
    pub tombstones: usize,
    /// Explicit associations whose plan or source is no longer present.
    pub dangling_associations: usize,
    /// Diagnostics at the active generation.
    pub diagnostics: usize,
    /// Conflicts at the active generation.
    pub conflicts: usize,
}

/// The selected workstream registry's projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RegistryStatus {
    /// The file read, if one was configured.
    pub path: Option<String>,
    /// The SHA-256 of the exact registry bytes.
    pub revision: Option<String>,
    /// Whether a registry was absent, readable or refused.
    pub state: RegistryState,
    /// The registered workstream count. Always zero when absent or invalid.
    pub workstreams: usize,
    /// Structural diagnostics. Unknown plan keys never add entries here.
    pub diagnostics: Vec<String>,
}

/// Whether the registry was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RegistryState {
    /// No registry path is configured.
    Absent,
    /// The exact file parsed as the selected registry.
    Ready,
    /// A path was configured but the file was missing or malformed.
    Invalid,
}

/// Where the effective configuration came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfigOrigin {
    /// The durable `<state_root>/catalog.json` was read.
    CatalogFile,
    /// The effective plugin environment was migrated once.
    MigratedPluginEnv,
    /// The plugin configuration files were migrated once.
    MigratedPluginFile,
    /// Nothing is configured; zero repositories is a valid catalog.
    None,
}

/// Effective configuration provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ConfigStatus {
    /// How this configuration was obtained.
    pub origin: ConfigOrigin,
    /// The SHA-256 of the effective settings document.
    pub revision: Option<String>,
    /// Configured repositories.
    pub repositories: usize,
    /// Imported explicit operator sources.
    pub explicit_sources: usize,
    /// When a migrated configuration was materialized.
    pub imported_at: Option<String>,
    /// Structural diagnostics from resolution.
    pub diagnostics: Vec<String>,
}

/// Whether the fallback snapshot matches the active generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SnapshotStatus {
    /// Whether a snapshot file exists.
    pub present: bool,
    /// The generation inside it.
    pub generation: Option<u64>,
    /// When it was written.
    pub written_at: Option<String>,
    /// Whether it is the same generation a reader would get from the database.
    pub current: bool,
    /// Why it could not be read, when it exists but is unusable.
    pub error: Option<String>,
}

/// What a catalog entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// A plan document that may name an epic.
    Plan,
    /// A QNR execution unit. Never an epic and never a plan replacement.
    ExecutionUnit,
}

impl EntryKind {
    /// The stable wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::ExecutionUnit => "execution_unit",
        }
    }

    /// Parse the wire spelling.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "plan" => Some(Self::Plan),
            "execution_unit" => Some(Self::ExecutionUnit),
            _ => None,
        }
    }
}

/// Whether an entry is currently observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EntryState {
    /// At least one live occurrence was observed in the active generation.
    Active,
    /// A complete sweep proved no occurrence remains. The entry is retained.
    Removed,
}

/// How a file was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    /// Tracked by git in the configured checkout.
    Tracked,
    /// Present, not ignored by git, and not tracked. A draft, visibly.
    UntrackedDraft,
    /// An explicit operator source outside repository discovery.
    ExternalSource,
}

/// The operator's declared revision intent, preserved from the source catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RevisionKind {
    /// A working copy of a plan.
    WorkingCopy,
    /// A saved draft.
    SavedDraft,
    /// An approved revision. A non-null pin is required by the source contract.
    ApprovedRevision,
}

/// A known identity of one logical plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Alias {
    /// Why this alias exists.
    pub kind: AliasKind,
    /// The prior or explicit value.
    pub value: String,
    /// When the alias was observed.
    pub observed_at: String,
}

/// The kind of a retained identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AliasKind {
    /// A single unambigous rename proved by identical content and one absent
    /// predecessor in the same repository.
    RenamedFrom,
    /// The explicit operator source id that names this plan.
    SourceId,
    /// A validated explicit plan id declared in front matter.
    ExplicitId,
}

/// One operator or plan association.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Association {
    /// The relation named.
    pub relation: AssociationRelation,
    /// The relation's value: a workstream key, an epic key or a project id.
    pub value: String,
    /// Which authority stated it.
    pub kind: AssociationKind,
    /// The operator source id when `kind` is explicit.
    pub source_id: Option<String>,
    /// Always `false` in contract version 1. A later verified join must not
    /// reuse this field without an evidence contract.
    pub verified: bool,
    /// Whether the associated plan is removed or the source vanished.
    pub dangling: bool,
    /// A structural note, never file content.
    pub note: Option<String>,
}

/// What an association relates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssociationRelation {
    /// A workstream registry key.
    Workstream,
    /// A Jira epic key.
    Epic,
    /// A governed or direct project id.
    Project,
}

/// Who stated an association.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssociationKind {
    /// An operator source catalog entry. This is authoritative for the
    /// operator's own choice, but it is not a Jira verification.
    ExplicitOperator,
    /// A plan file's own declaration.
    PlanDeclaration,
}

/// A one-line diagnostic with no file content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Diagnostic {
    /// A stable machine code.
    pub code: String,
    /// How severe it is.
    pub severity: DiagnosticSeverity,
    /// A structural message that never contains source bytes or credentials.
    pub message: String,
    /// The occurrence or source the diagnostic is about.
    pub subject: Option<String>,
    /// The logical plan it belongs to, when known.
    pub logical_plan_id: Option<String>,
    /// When it was observed.
    pub observed_at: String,
}

/// Diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    /// Informational, such as a migrated configuration.
    Info,
    /// A visible ambiguity or unsupported declaration.
    Warning,
    /// A scope that could not be read at all.
    Error,
}

/// A visible identity or content conflict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Conflict {
    /// The kind of conflict.
    pub kind: ConflictKind,
    /// The logical plan it belongs to, when a single plan is implicated.
    pub logical_plan_id: Option<String>,
    /// A structural description.
    pub message: String,
    /// When it was observed.
    pub observed_at: String,
}

/// The conflicts this contract can expose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    /// The same logical plan has different content in two worktrees.
    DivergentWorktree,
    /// More than one predecessor matched a rename; nothing was merged.
    AmbiguousRename,
    /// Two files declared the same explicit plan id.
    DuplicateExplicitId,
    /// More than one logical plan declares the same epic.
    DuplicateEpic,
    /// An explicit pin does not match the bytes read.
    PinMismatch,
    /// An explicit operator declaration disagrees with a plan declaration.
    AssociationConflict,
}

/// One exact file in one checkout or worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Occurrence {
    /// The occurrence's stable id.
    pub occurrence_id: String,
    /// The repository identity (shared across worktrees of one repository).
    pub repository_id: String,
    /// The configured or derived worktree root.
    pub repository_root: String,
    /// The path relative to the repository root.
    pub relative_path: String,
    /// How the file was found.
    pub content_kind: ContentKind,
    /// The operator's declared revision intent.
    pub revision_kind: RevisionKind,
    /// The exact bytes' revision.
    pub content: ContentRevision,
    /// The explicit source ids that name this occurrence.
    pub source_ids: Vec<String>,
    /// The operator's expected hash, when one was declared.
    pub expected_sha256: Option<String>,
    /// Whether the declared pin matches the bytes read. `None` without a pin.
    pub pin_matches: Option<bool>,
    /// Whether the occurrence is live at the active generation.
    pub state: EntryState,
    /// When this occurrence was first observed.
    pub first_seen: String,
    /// When it was last observed in a complete sweep.
    pub last_seen: String,
}

/// One exact content revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ContentRevision {
    /// The SHA-256 of the exact bytes.
    pub revision_id: String,
    /// The byte length read.
    pub byte_length: u64,
    /// The file's modification time.
    pub modified_at: String,
    /// The title taken from the first H1, or the path.
    pub title: String,
    /// The declared epic, when unambiguous.
    pub epic: Option<String>,
    /// The declared workstream, when valid.
    pub workstream: Option<String>,
}

/// One logical plan with all its occurrences and associations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CatalogEntry {
    /// The stable logical plan id.
    pub logical_plan_id: String,
    /// A plan or a QNR execution unit.
    pub kind: EntryKind,
    /// The current display title.
    pub title: String,
    /// Whether it is live or retained as removed history.
    pub state: EntryState,
    /// Known aliases, including an unambiguous rename predecessor.
    pub aliases: Vec<Alias>,
    /// Every association, operator-explicit or declared.
    pub associations: Vec<Association>,
    /// Every occurrence at the active generation.
    pub occurrences: Vec<Occurrence>,
    /// Entry-level diagnostics.
    pub diagnostics: Vec<Diagnostic>,
    /// Entry-level conflicts.
    pub conflicts: Vec<Conflict>,
    /// When the logical plan was first observed.
    pub first_seen: String,
    /// When it was last observed in a complete sweep.
    pub last_seen: String,
}

/// A retained explicit association whose plan or source is gone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DanglingAssociation {
    /// The relation.
    pub relation: AssociationRelation,
    /// The value.
    pub value: String,
    /// The operator source id that stated it.
    pub source_id: Option<String>,
    /// The removed logical plan, when one was identified.
    pub logical_plan_id: Option<String>,
    /// When it was last seen attached.
    pub last_seen: String,
}

/// The generation-consistent entries answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct EntriesView {
    /// The contract version.
    pub schema_version: u32,
    /// The active complete generation this answer describes.
    pub generation: u64,
    /// The instant that generation completed.
    pub as_of: Option<String>,
    /// Whether the answer came from the database or the fallback snapshot.
    pub source: ReadSource,
    /// The matching entries.
    pub entries: Vec<CatalogEntry>,
    /// Whether a further page exists. Equivalent to `next_cursor.is_some()`.
    pub truncated: bool,
    /// An opaque continuation for the next page, bound to this generation and
    /// filter. A reader that presents it after the generation advanced is
    /// refused rather than shown a page from a mix of generations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Where a read answer came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadSource {
    /// The catalog's SQLite database, inside one read transaction.
    Database,
    /// The atomic fallback snapshot, because the database was unreadable.
    Snapshot,
}

/// A basic entry query.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntryFilter {
    /// Only this kind.
    pub kind: Option<EntryKind>,
    /// A repository id, worktree root, relative path prefix or workstream key.
    pub scope: Option<String>,
    /// Include tombstoned entries. Defaults to false.
    pub include_removed: bool,
    /// A hard row bound. The reader clamps it.
    pub limit: usize,
    /// An opaque continuation from a prior [`EntriesView::next_cursor`].
    pub cursor: Option<String>,
}

/// One source of observations in the shared catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSource {
    /// Local plan and execution-unit discovery.
    Plan,
    /// Read-only Jira observation.
    Jira,
    /// Headless native Paseo census.
    Native,
}

/// The state of one source's latest attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSourceState {
    /// The source has no active configuration for this catalog.
    NotConfigured,
    /// The source is configured but no complete generation exists.
    NeverCollected,
    /// The latest attempt read everything it needed.
    Complete,
    /// The latest attempt stopped before every fact was answered.
    Incomplete,
    /// The latest attempt could not start or lost its channel.
    Failed,
}

/// One bounded coverage line for a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CatalogSourceCoverage {
    /// What the line covers, such as a repository or a sweep mode.
    pub subject: String,
    /// The stable state spelling for this line.
    pub state: String,
    /// A bounded structural detail, never file bytes or a credential.
    pub detail: String,
}

/// One conflict observed at a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CatalogSourceConflict {
    /// A stable conflict kind.
    pub kind: String,
    /// The plan, issue or runtime the conflict is about.
    pub subject: Option<String>,
    /// A structural description.
    pub message: String,
    /// When the conflict was observed, when the source records it.
    pub observed_at: Option<String>,
}

/// Freshness, coverage, errors and conflicts for one source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CatalogSourceStatus {
    /// Which source this describes.
    pub source: CatalogSource,
    /// The latest attempt's state.
    pub state: CatalogSourceState,
    /// The last complete generation, or `0` before the first one.
    pub generation: u64,
    /// How far collection has attempted; may be ahead of `generation`.
    pub attempted_generation: u64,
    /// The instant the latest complete generation finished. Never the request
    /// instants or the instant this status was read.
    pub as_of: Option<String>,
    /// The latest structural failure, never source content.
    pub error: Option<String>,
    /// The earliest instant this source will retry, when it named one.
    pub retry_at: Option<String>,
    /// Bounded coverage lines.
    pub coverage: Vec<CatalogSourceCoverage>,
    /// Bounded visible conflicts.
    pub conflicts: Vec<CatalogSourceConflict>,
    /// Associations this source has not verified. The frozen v1 association
    /// contract cannot claim verification, so plan declarations remain pending
    /// until a collector with its own evidence contract proves them.
    pub pending_verification: usize,
}

/// The per-source observer status across every catalog source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CatalogSourcesView {
    /// The contract version.
    pub schema_version: u32,
    /// One row per source, in plan, Jira, native order.
    pub sources: Vec<CatalogSourceStatus>,
}

/// The fallback snapshot document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CatalogSnapshot {
    /// The contract version.
    pub schema_version: u32,
    /// The generation the snapshot is consistent at.
    pub generation: u64,
    /// When it was written.
    pub written_at: String,
    /// The status projected at write time.
    pub status: CatalogStatus,
    /// The complete generation's entries, including removed ones.
    pub entries: Vec<CatalogEntry>,
}
