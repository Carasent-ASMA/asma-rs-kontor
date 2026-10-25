//! The headless native runtime census.
//!
//! # What this observes
//!
//! Every native runtime instance the operator configured, its projects,
//! workspaces and sessions, read through supported list operations with no
//! plugin screen, no mounted client and no governed project. The census is an
//! observation, never an execution surface: it cannot create, prompt, stop or
//! archive a session, and it never edits a runtime.
//!
//! # Identity
//!
//! A native identity is the pair `(runtime_instance, native_id)`. A title is
//! display data. Two sessions named the same thing in one workspace are two
//! rows with two ids; a census that grouped by title would be the exact defect
//! this module exists to avoid.
//!
//! # Completeness and deletion
//!
//! The protocol mirrors plan discovery because the hazard is the same:
//! a page is evidence, a complete walk is the only proof of absence. Pages are
//! committed with their checkpoint; only a finished walk that read every
//! configured runtime promotes observations and tombstones what it proved
//! absent. An incomplete walk may insert a previously unknown row hidden from
//! readers, and never updates or removes an existing one. Native absence is not
//! task completion and never touches the scheduler.

use std::collections::BTreeMap;

use async_trait::async_trait;
use kontor_core::id::{SignedDuration, Timestamp};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::contract::{CollectionState, CollectorStatus, EntryState, ReadSource, SnapshotStatus};
use crate::store::{Database, OwnerStatus, StoreError};

/// The version of the native census documents.
pub const NATIVE_CENSUS_VERSION: u32 = 1;

/// The atomic fallback snapshot for native observations.
pub const NATIVE_SNAPSHOT_FILE: &str = "catalog-native-snapshot.json";

/// The plugin's own bound, adopted so the two readers cannot disagree about
/// what "bounded pagination" means.
pub const MAX_NATIVE_PAGES: usize = 1_000;

/// The daemon's maximum directory page.
pub const NATIVE_PAGE_LIMIT: u32 = 200;

/// One headless census of one runtime instance.
///
/// A source that could not reach its runtime still answers with this shape and
/// `complete = false`; refusing with an error would lose the distinction
/// between "Paseo is unavailable" and "the catalog database failed".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NativeCensus {
    /// Stable identity of the runtime instance, not of one boot.
    pub runtime_instance: String,
    /// The persistent connection identity this census used.
    pub client_id: String,
    /// The runtime's own boot identity, when it announced one.
    pub server_id: Option<String>,
    /// The runtime application version.
    pub server_version: Option<String>,
    /// The session protocol version this client requested.
    pub protocol_version: u32,
    /// Semantic permissions the runtime attached to this connection.
    pub permissions: Option<Vec<String>>,
    /// Advertised runtime features.
    pub features: BTreeMap<String, bool>,
    /// Whether archived sessions were requested and therefore covered.
    pub include_archived: bool,
    /// Whether every project, workspace and session page was read.
    pub complete: bool,
    /// The first structural failure, when the walk did not finish.
    pub error: Option<String>,
    /// Structural attempt diagnostics.
    pub diagnostics: Vec<String>,
    /// Pages followed for each list.
    pub pages: NativePageCounts,
    /// Observed projects.
    pub projects: Vec<NativeProject>,
    /// Observed workspaces.
    pub workspaces: Vec<NativeWorkspace>,
    /// Observed sessions, archived included when `include_archived`.
    pub sessions: Vec<NativeSession>,
}

impl NativeCensus {
    /// An answer for a runtime that could not be reached.
    #[must_use]
    pub fn unavailable(
        runtime_instance: impl Into<String>,
        client_id: impl Into<String>,
        error: impl Into<String>,
    ) -> Self {
        Self {
            runtime_instance: runtime_instance.into(),
            client_id: client_id.into(),
            server_id: None,
            server_version: None,
            protocol_version: 1,
            permissions: None,
            features: BTreeMap::new(),
            include_archived: false,
            complete: false,
            error: Some(error.into()),
            diagnostics: Vec::new(),
            pages: NativePageCounts::default(),
            projects: Vec::new(),
            workspaces: Vec::new(),
            sessions: Vec::new(),
        }
    }

    /// Sessions carrying an archive stamp.
    #[must_use]
    pub fn archived_sessions(&self) -> usize {
        self.sessions
            .iter()
            .filter(|session| session.archived_at.is_some())
            .count()
    }
}

/// How many pages each list required.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NativePageCounts {
    /// Project list pages. Paseo answers projects in one list.
    pub project_pages: u32,
    /// Workspace directory pages.
    pub workspace_pages: u32,
    /// Session directory pages.
    pub session_pages: u32,
}

/// One native project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NativeProject {
    /// Stable runtime instance identity.
    pub runtime_instance: String,
    /// Native project id.
    pub project_id: String,
    /// Resolved display name.
    pub display_name: String,
    /// Raw custom name, when one exists.
    pub custom_name: Option<String>,
    /// The project's remote or directory key, as the daemon reports it.
    pub project_key: Option<String>,
    /// Project root path.
    pub root_path: String,
    /// Native project kind.
    pub project_kind: Option<String>,
}

/// One native workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NativeWorkspace {
    /// Stable runtime instance identity.
    pub runtime_instance: String,
    /// Native workspace id.
    pub workspace_id: String,
    /// Native project id.
    pub project_id: String,
    /// Resolved display name.
    pub name: String,
    /// Raw title override, when one exists.
    pub title: Option<String>,
    /// Absolute working directory.
    pub directory: String,
    /// Native workspace kind.
    pub kind: String,
    /// Archive intent, when Paseo is retiring the workspace.
    pub archiving_at: Option<String>,
    /// Paseo-owned worktree basename, when one exists.
    pub worktree_slug: Option<String>,
    /// Project display name carried on the workspace descriptor.
    pub project_display_name: Option<String>,
    /// Project custom name carried on the workspace descriptor.
    pub project_custom_name: Option<String>,
    /// Project root path carried on the workspace descriptor.
    pub project_root_path: Option<String>,
    /// Project kind carried on the workspace descriptor.
    pub project_kind: Option<String>,
}

/// Checkout facts carried by one directory entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NativeCheckout {
    /// Working directory.
    pub cwd: Option<String>,
    /// Checked-out branch.
    pub branch: Option<String>,
    /// Linked worktree root.
    pub worktree_root: Option<String>,
    /// Whether the directory is a Git checkout.
    pub is_git: Option<bool>,
    /// Whether Paseo provisioned the worktree itself.
    pub is_paseo_owned_worktree: Option<bool>,
}

/// One native session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NativeSession {
    /// Stable runtime instance identity.
    pub runtime_instance: String,
    /// Native session id. Never derived from a title.
    pub session_id: String,
    /// Native workspace id, when one is attached.
    pub workspace_id: Option<String>,
    /// Native project id, resolved from the workspace or the directory entry.
    pub project_id: Option<String>,
    /// Project display name carried by the directory entry.
    pub project_name: Option<String>,
    /// Workspace display name carried by the directory entry.
    pub workspace_name: Option<String>,
    /// Display title.
    pub title: Option<String>,
    /// Provider name.
    pub provider: String,
    /// Model name.
    pub model: Option<String>,
    /// Native lifecycle status.
    pub status: String,
    /// The archive stamp. Presence is native archive evidence.
    pub archived_at: Option<String>,
    /// Paseo's attention hint.
    pub attention_reason: Option<String>,
    /// Working directory.
    pub cwd: String,
    /// Native creation instant.
    pub created_at: Option<String>,
    /// Native update instant.
    pub updated_at: Option<String>,
    /// Last user-message instant.
    pub last_user_message_at: Option<String>,
    /// Native parent session, when the entry reports one.
    pub parent_session_id: Option<String>,
    /// Placement checkout facts.
    pub checkout: Option<NativeCheckout>,
}

/// The bounded adapter contract for a native census.
///
/// The Paseo implementation is the only one this build composes. A future
/// runtime adds one implementation of this read-only contract; it does not
/// gain a second scheduler, a second store or a mutation surface.
#[async_trait]
pub trait NativeCensusSource: Send + Sync + std::fmt::Debug {
    /// Stable identity of the runtime instance this source observes.
    fn runtime_instance(&self) -> &str;

    /// The persistent connection identity this source uses.
    fn client_id(&self) -> &str;

    /// Observe one complete or partial census.
    ///
    /// Infallible by design: a runtime that cannot be reached is an incomplete
    /// observation with an error, which the catalog records honestly. The only
    /// failure left is the catalog's own store.
    async fn observe(&self) -> NativeCensus;
}

/// One census attempt, complete or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSweepOutcome {
    /// Whether every configured source finished its walk.
    pub complete: bool,
    /// Per-runtime coverage.
    pub coverage: Vec<NativeRuntimeCoverage>,
    /// Projects observed in this attempt.
    pub projects_seen: usize,
    /// Workspaces observed in this attempt.
    pub workspaces_seen: usize,
    /// Sessions observed in this attempt.
    pub sessions_seen: usize,
    /// Archived sessions observed in this attempt.
    pub archived_sessions_seen: usize,
    /// Whether archive scope was requested.
    pub include_archived: bool,
    /// Structural attempt diagnostics.
    pub diagnostics: Vec<String>,
    /// The first structural failure, when the walk did not finish.
    pub error: Option<String>,
    /// The earliest instant the collector may retry.
    pub retry_at: Option<String>,
}

/// One in-flight native sweep, which is also its fence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSweepId {
    /// The single host scope.
    pub scope: String,
    /// The monotonic native attempt generation.
    pub generation: u64,
    /// The owner that may write pages for it.
    pub owner_id: String,
}

/// One runtime instance's coverage in the latest attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NativeRuntimeCoverage {
    /// Stable runtime instance identity.
    pub runtime_instance: String,
    /// Persistent connection identity.
    pub client_id: String,
    /// The runtime's boot identity.
    pub server_id: Option<String>,
    /// The runtime application version.
    pub server_version: Option<String>,
    /// The requested session protocol version.
    pub protocol_version: u32,
    /// Semantic permissions on the connection.
    pub permissions: Option<Vec<String>>,
    /// Advertised runtime features.
    pub features: BTreeMap<String, bool>,
    /// Whether archive scope was requested.
    pub include_archived: bool,
    /// What happened to this runtime.
    pub state: NativeRuntimeState,
    /// Project list pages.
    pub project_pages: u32,
    /// Workspace pages.
    pub workspace_pages: u32,
    /// Session pages.
    pub session_pages: u32,
    /// Projects observed.
    pub projects: usize,
    /// Workspaces observed.
    pub workspaces: usize,
    /// Sessions observed.
    pub sessions: usize,
    /// Archived sessions observed.
    pub archived_sessions: usize,
    /// A structural failure for this runtime only.
    pub error: Option<String>,
    /// Runtime-specific diagnostics.
    pub diagnostics: Vec<String>,
}

impl NativeRuntimeCoverage {
    /// Project one census into its coverage row.
    #[must_use]
    pub fn from_census(census: &NativeCensus) -> Self {
        let state = if census.complete {
            NativeRuntimeState::Scanned
        } else if census.server_id.is_some() {
            // The runtime answered and the walk stopped partway: a permission
            // loss or a failed page is not proof of anything.
            NativeRuntimeState::Incomplete
        } else {
            NativeRuntimeState::Failed
        };
        Self {
            runtime_instance: census.runtime_instance.clone(),
            client_id: census.client_id.clone(),
            server_id: census.server_id.clone(),
            server_version: census.server_version.clone(),
            protocol_version: census.protocol_version,
            permissions: census.permissions.clone(),
            features: census.features.clone(),
            include_archived: census.include_archived,
            state,
            project_pages: census.pages.project_pages,
            workspace_pages: census.pages.workspace_pages,
            session_pages: census.pages.session_pages,
            projects: census.projects.len(),
            workspaces: census.workspaces.len(),
            sessions: census.sessions.len(),
            archived_sessions: census.archived_sessions(),
            error: census.error.clone(),
            diagnostics: census.diagnostics.clone(),
        }
    }
}

/// Whether one runtime was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NativeRuntimeState {
    /// Read to completion.
    Scanned,
    /// The runtime answered but the walk did not finish.
    Incomplete,
    /// The runtime could not be reached.
    Failed,
}

/// The latest native collection attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NativeCollectionStatus {
    /// The attempt's own state.
    pub state: CollectionState,
    /// The last complete native generation.
    pub generation: u64,
    /// The latest attempted generation, which may be ahead.
    pub attempt_generation: u64,
    /// When the attempt started.
    pub started_at: Option<String>,
    /// When the attempt finished.
    pub finished_at: Option<String>,
    /// Measured wall-clock duration.
    pub duration_ms: Option<u64>,
    /// The earliest instant this catalog will retry after a failure.
    pub retry_at: Option<String>,
    /// Runtime instances configured for this attempt.
    pub runtimes_configured: usize,
    /// Runtime instances read to completion.
    pub runtimes_complete: usize,
    /// Projects observed in this attempt.
    pub projects_observed: usize,
    /// Workspaces observed in this attempt.
    pub workspaces_observed: usize,
    /// Sessions observed in this attempt.
    pub sessions_observed: usize,
    /// Archived sessions observed in this attempt.
    pub archived_sessions: usize,
    /// Whether archive scope was requested.
    pub include_archived: bool,
    /// A structural failure, never a credential.
    pub error: Option<String>,
    /// Attempt-level diagnostics.
    pub diagnostics: Vec<String>,
    /// Per-runtime coverage.
    pub coverage: Vec<NativeRuntimeCoverage>,
}

/// Counts at the active native generation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NativeCounts {
    /// Active native projects.
    pub projects: usize,
    /// Active native workspaces.
    pub workspaces: usize,
    /// Active native sessions, archived included.
    pub sessions: usize,
    /// Active sessions carrying an archive stamp.
    pub archived_sessions: usize,
    /// Removed native observations visible at this generation.
    pub tombstones: usize,
}

/// The native census's freshness, coverage and counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NativeCatalogStatus {
    /// The native census document version.
    pub schema_version: u32,
    /// Whether this answer came from the database or the fallback snapshot.
    pub source: ReadSource,
    /// The last complete native generation.
    pub generation: u64,
    /// The latest attempted native generation.
    pub attempted_generation: u64,
    /// When the last complete native generation finished.
    pub as_of: Option<String>,
    /// The collector lease.
    pub collector: CollectorStatus,
    /// The latest native attempt and its coverage.
    pub collection: NativeCollectionStatus,
    /// Current native counts.
    pub counts: NativeCounts,
    /// Whether the native fallback snapshot is present and at which generation.
    pub snapshot: SnapshotStatus,
}

/// One native observation with catalog state and history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NativeProjectRecord {
    /// Stable runtime instance identity.
    pub runtime_instance: String,
    /// Native project id.
    pub project_id: String,
    /// Resolved display name.
    pub display_name: String,
    /// Raw custom name.
    pub custom_name: Option<String>,
    /// Project key as the daemon reports it.
    pub project_key: Option<String>,
    /// Project root path.
    pub root_path: String,
    /// Native project kind.
    pub project_kind: Option<String>,
    /// Active or removed at this generation.
    pub state: EntryState,
    /// When this project was first observed.
    pub first_seen: String,
    /// When it was last observed in a complete census.
    pub last_seen: String,
}

/// One native workspace observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NativeWorkspaceRecord {
    /// Stable runtime instance identity.
    pub runtime_instance: String,
    /// Native workspace id.
    pub workspace_id: String,
    /// Native project id.
    pub project_id: String,
    /// Resolved display name.
    pub name: String,
    /// Raw title override.
    pub title: Option<String>,
    /// Absolute working directory.
    pub directory: String,
    /// Native workspace kind.
    pub kind: String,
    /// Archive intent.
    pub archiving_at: Option<String>,
    /// Paseo-owned worktree basename.
    pub worktree_slug: Option<String>,
    /// Project display name.
    pub project_display_name: Option<String>,
    /// Project custom name.
    pub project_custom_name: Option<String>,
    /// Project root path.
    pub project_root_path: Option<String>,
    /// Project kind.
    pub project_kind: Option<String>,
    /// Active or removed at this generation.
    pub state: EntryState,
    /// When this workspace was first observed.
    pub first_seen: String,
    /// When it was last observed in a complete census.
    pub last_seen: String,
}

/// One native session observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NativeSessionRecord {
    /// Stable runtime instance identity.
    pub runtime_instance: String,
    /// Native session id.
    pub session_id: String,
    /// Native workspace id.
    pub workspace_id: Option<String>,
    /// Native project id.
    pub project_id: Option<String>,
    /// Project display name.
    pub project_name: Option<String>,
    /// Workspace display name.
    pub workspace_name: Option<String>,
    /// Display title.
    pub title: Option<String>,
    /// Provider name.
    pub provider: String,
    /// Model name.
    pub model: Option<String>,
    /// Native lifecycle status.
    pub status: String,
    /// The archive stamp.
    pub archived_at: Option<String>,
    /// Paseo's attention hint.
    pub attention_reason: Option<String>,
    /// Working directory.
    pub cwd: String,
    /// Native creation instant.
    pub created_at: Option<String>,
    /// Native update instant.
    pub updated_at: Option<String>,
    /// Last user-message instant.
    pub last_user_message_at: Option<String>,
    /// Native parent session.
    pub parent_session_id: Option<String>,
    /// Placement checkout facts.
    pub checkout: Option<NativeCheckout>,
    /// Active or removed at this generation.
    pub state: EntryState,
    /// When this session was first observed.
    pub first_seen: String,
    /// When it was last observed in a complete census.
    pub last_seen: String,
}

/// One generation-consistent native inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NativeInventoryView {
    /// The native census document version.
    pub schema_version: u32,
    /// The active complete generation.
    pub generation: u64,
    /// When that generation finished.
    pub as_of: Option<String>,
    /// Whether this answer came from the database or the fallback snapshot.
    pub source: ReadSource,
    /// Whether the walk that produced this generation requested archived sessions.
    pub archive_scope: bool,
    /// Native projects.
    pub projects: Vec<NativeProjectRecord>,
    /// Native workspaces.
    pub workspaces: Vec<NativeWorkspaceRecord>,
    /// Native sessions.
    pub sessions: Vec<NativeSessionRecord>,
    /// Whether a further page exists. Equivalent to `next_cursor.is_some()`.
    pub truncated: bool,
    /// An opaque continuation bound to this generation and filter. It carries
    /// one offset per collection, so a page never mixes generations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// A native inventory query.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeInventoryFilter {
    /// Include observations removed at the active generation.
    pub include_removed: bool,
    /// Only this runtime instance.
    pub runtime_instance: Option<String>,
    /// A hard row bound per collection.
    pub limit: usize,
    /// An opaque continuation from a prior [`NativeInventoryView::next_cursor`].
    pub cursor: Option<String>,
}

/// One finished native sweep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSweepReport {
    /// The generation attempted.
    pub generation: u64,
    /// Whether the walk finished everywhere.
    pub complete: bool,
    /// The state recorded for this attempt.
    pub state: CollectionState,
    /// Projects observed.
    pub projects: usize,
    /// Workspaces observed.
    pub workspaces: usize,
    /// Sessions observed.
    pub sessions: usize,
    /// Archived sessions observed.
    pub archived_sessions: usize,
    /// Atomic checkpoint pages committed by this attempt.
    pub pages: u32,
    /// Measured duration.
    pub duration_ms: u64,
    /// Structural diagnostics.
    pub diagnostics: Vec<String>,
    /// Whether the native fallback snapshot was replaced.
    pub snapshot_written: bool,
}

/// The fallback snapshot document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NativeCatalogSnapshot {
    /// The native census document version.
    pub schema_version: u32,
    /// The generation this snapshot is consistent at.
    pub generation: u64,
    /// The instant the generation completed, never the read instant.
    pub written_at: String,
    /// The status projected at write time.
    pub status: NativeCatalogStatus,
    /// Projects at that generation, removals included.
    pub projects: Vec<NativeProjectRecord>,
    /// Workspaces at that generation, removals included.
    pub workspaces: Vec<NativeWorkspaceRecord>,
    /// Sessions at that generation, removals included.
    pub sessions: Vec<NativeSessionRecord>,
}

/// Everything a native status projection needs from storage.
#[derive(Debug, Clone)]
pub(crate) struct NativeStoreStatus {
    pub(crate) generation: u64,
    pub(crate) attempted_generation: u64,
    pub(crate) as_of: Option<String>,
    pub(crate) owner: OwnerStatus,
    pub(crate) collection: NativeCollectionStatus,
    pub(crate) counts: NativeCounts,
}

/// A generation-consistent native inventory from storage.
#[derive(Debug, Clone)]
pub(crate) struct NativeStoreInventory {
    pub(crate) generation: u64,
    pub(crate) as_of: Option<String>,
    pub(crate) archive_scope: bool,
    pub(crate) projects: Vec<NativeProjectRecord>,
    pub(crate) workspaces: Vec<NativeWorkspaceRecord>,
    pub(crate) sessions: Vec<NativeSessionRecord>,
}

fn generation_to_sql(generation: u64) -> i64 {
    i64::try_from(generation).unwrap_or(i64::MAX)
}

fn bounded(text: &str) -> String {
    text.chars().take(4096).collect()
}

fn parse_state(state: &str) -> EntryState {
    if state == "removed" {
        EntryState::Removed
    } else {
        EntryState::Active
    }
}

pub(crate) fn retry_after(now: &Timestamp, seconds: u64) -> String {
    (*now + SignedDuration::from_secs(i64::try_from(seconds).unwrap_or(i64::MAX))).to_string()
}

impl Database {
    /// Begin one native census attempt, allocating the next native generation.
    ///
    /// # Errors
    /// [`StoreError::NotOwner`] when this process no longer holds the lease.
    pub(crate) fn begin_native_sweep(
        &self,
        owner_id: &str,
        now: &Timestamp,
    ) -> Result<NativeSweepId, StoreError> {
        let now_text = now.to_string();
        let mut connection = self.lock();
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let (current_owner, lease_until, generation): (Option<String>, Option<String>, i64) =
            transaction.query_row(
                "SELECT owner_id, lease_until, native_generation
                 FROM catalog_scope WHERE scope = ?1",
                [crate::contract::HOST_SCOPE],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
        if current_owner.as_deref() != Some(owner_id)
            || !lease_until.is_some_and(|until| until > now_text)
        {
            return Err(StoreError::NotOwner);
        }
        let generation = u64::try_from(generation)
            .map_err(|_| StoreError::Corrupt("negative native generation".to_owned()))?
            + 1;
        transaction.execute(
            "INSERT INTO catalog_native_sweeps
                 (scope, generation, owner_id, state, started_at, pages_committed,
                  projects_seen, workspaces_seen, sessions_seen,
                  archived_sessions_seen, runtimes_json, diagnostics_json)
             VALUES (?1, ?2, ?3, 'running', ?4, 0, 0, 0, 0, 0, '[]', '[]')",
            params![
                crate::contract::HOST_SCOPE,
                generation_to_sql(generation),
                owner_id,
                now_text
            ],
        )?;
        transaction.execute(
            "UPDATE catalog_scope SET native_generation = ?1, updated_at = ?2
             WHERE scope = ?3",
            params![
                generation_to_sql(generation),
                now_text,
                crate::contract::HOST_SCOPE
            ],
        )?;
        transaction.commit()?;
        Ok(NativeSweepId {
            scope: crate::contract::HOST_SCOPE.to_owned(),
            generation,
            owner_id: owner_id.to_owned(),
        })
    }

    /// Commit one page of native observations plus its page checkpoint.
    ///
    /// # Errors
    /// [`StoreError::Fenced`] when a newer sweep or owner displaced this one.
    pub(crate) fn commit_native_page<T: Serialize>(
        &self,
        sweep: &NativeSweepId,
        page_index: u32,
        kind: &str,
        observed_at: &Timestamp,
        items: &[T],
    ) -> Result<(), StoreError> {
        let page_json = serde_json::to_string(items)?;
        let mut connection = self.lock();
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let state: Option<(String, String)> = transaction
            .query_row(
                "SELECT state, owner_id FROM catalog_native_sweeps
                 WHERE scope = ?1 AND generation = ?2",
                params![sweep.scope, generation_to_sql(sweep.generation)],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let current: i64 = transaction.query_row(
            "SELECT native_generation FROM catalog_scope WHERE scope = ?1",
            [crate::contract::HOST_SCOPE],
            |row| row.get(0),
        )?;
        match state {
            Some((state, owner))
                if state == "running"
                    && owner == sweep.owner_id
                    && current == generation_to_sql(sweep.generation) => {}
            Some(_) => return Err(StoreError::Fenced),
            None => return Err(StoreError::MissingSweep),
        }
        transaction.execute(
            "INSERT INTO catalog_native_sweep_pages
                 (scope, generation, page_index, kind, observed_at, page_json, items_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (scope, generation, page_index) DO UPDATE SET
                 kind = excluded.kind,
                 observed_at = excluded.observed_at,
                 page_json = excluded.page_json,
                 items_seen = excluded.items_seen",
            params![
                sweep.scope,
                generation_to_sql(sweep.generation),
                i64::from(page_index),
                kind,
                observed_at.to_string(),
                page_json,
                i64::try_from(items.len()).unwrap_or(i64::MAX)
            ],
        )?;
        transaction.execute(
            "UPDATE catalog_native_sweeps
             SET pages_committed = (SELECT COUNT(*) FROM catalog_native_sweep_pages
                                    WHERE scope = ?1 AND generation = ?2)
             WHERE scope = ?1 AND generation = ?2",
            params![sweep.scope, generation_to_sql(sweep.generation)],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Finish one native sweep and commit its checkpoint.
    ///
    /// A complete sweep promotes every staged page, advances the active native
    /// generation and tombstones what it proved absent. An incomplete sweep only
    /// inserts previously unknown rows, hidden from readers.
    ///
    /// # Errors
    /// [`StoreError::Fenced`], [`StoreError::MissingSweep`] or a SQL/JSON
    /// refusal.
    pub(crate) fn finalize_native_sweep(
        &self,
        sweep: &NativeSweepId,
        outcome: &NativeSweepOutcome,
        now: &Timestamp,
    ) -> Result<(), StoreError> {
        let mut connection = self.lock();
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let state: Option<(String, String, String)> = transaction
            .query_row(
                "SELECT state, owner_id, started_at FROM catalog_native_sweeps
                 WHERE scope = ?1 AND generation = ?2",
                params![sweep.scope, generation_to_sql(sweep.generation)],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let current: i64 = transaction.query_row(
            "SELECT native_generation FROM catalog_scope WHERE scope = ?1",
            [crate::contract::HOST_SCOPE],
            |row| row.get(0),
        )?;
        let Some((state, owner, started_at)) = state else {
            return Err(StoreError::MissingSweep);
        };
        if state != "running"
            || owner != sweep.owner_id
            || current != generation_to_sql(sweep.generation)
        {
            return Err(StoreError::Fenced);
        }

        let mut projects = Vec::new();
        let mut workspaces = Vec::new();
        let mut sessions = Vec::new();
        {
            let mut statement = transaction.prepare(
                "SELECT kind, page_json FROM catalog_native_sweep_pages
                 WHERE scope = ?1 AND generation = ?2 ORDER BY page_index",
            )?;
            let rows = statement.query_map(
                params![sweep.scope, generation_to_sql(sweep.generation)],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )?;
            for row in rows {
                let (kind, json) = row?;
                match kind.as_str() {
                    "projects" => {
                        let mut page: Vec<NativeProject> = serde_json::from_str(&json)
                            .map_err(|error| StoreError::Corrupt(error.to_string()))?;
                        projects.append(&mut page);
                    }
                    "workspaces" => {
                        let mut page: Vec<NativeWorkspace> = serde_json::from_str(&json)
                            .map_err(|error| StoreError::Corrupt(error.to_string()))?;
                        workspaces.append(&mut page);
                    }
                    "sessions" => {
                        let mut page: Vec<NativeSession> = serde_json::from_str(&json)
                            .map_err(|error| StoreError::Corrupt(error.to_string()))?;
                        sessions.append(&mut page);
                    }
                    other => {
                        return Err(StoreError::Corrupt(format!(
                            "unknown native page kind `{other}`"
                        )));
                    }
                }
            }
        }

        if outcome.complete {
            promote_native_complete(
                &transaction,
                sweep.generation,
                now,
                &outcome.coverage,
                &projects,
                &workspaces,
                &sessions,
            )?;
        } else {
            promote_native_incomplete(
                &transaction,
                sweep.generation,
                now,
                &projects,
                &workspaces,
                &sessions,
            )?;
        }

        let runtimes_json = serde_json::to_string(&outcome.coverage)?;
        let diagnostics_json = serde_json::to_string(&outcome.diagnostics)?;
        let state_text = native_attempt_state(outcome);
        transaction.execute(
            "UPDATE catalog_native_sweeps
             SET state = ?1, finished_at = ?2, error = ?3, runtimes_json = ?4,
                 diagnostics_json = ?5, projects_seen = ?6, workspaces_seen = ?7,
                 sessions_seen = ?8, archived_sessions_seen = ?9
             WHERE scope = ?10 AND generation = ?11",
            params![
                state_text,
                now.to_string(),
                outcome.error.as_deref().map(bounded),
                runtimes_json,
                diagnostics_json,
                i64::try_from(outcome.projects_seen).unwrap_or(i64::MAX),
                i64::try_from(outcome.workspaces_seen).unwrap_or(i64::MAX),
                i64::try_from(outcome.sessions_seen).unwrap_or(i64::MAX),
                i64::try_from(outcome.archived_sessions_seen).unwrap_or(i64::MAX),
                sweep.scope,
                generation_to_sql(sweep.generation)
            ],
        )?;
        if outcome.complete {
            transaction.execute(
                "UPDATE catalog_scope SET native_active_generation = ?1, updated_at = ?2
                 WHERE scope = ?3",
                params![
                    generation_to_sql(sweep.generation),
                    now.to_string(),
                    sweep.scope
                ],
            )?;
        }
        let duration_ms = started_at.parse::<Timestamp>().ok().and_then(|started| {
            u64::try_from(now.as_millisecond() - started.as_millisecond()).ok()
        });
        let runtimes_complete = outcome
            .coverage
            .iter()
            .filter(|coverage| coverage.state == NativeRuntimeState::Scanned)
            .count();
        transaction.execute(
            "INSERT INTO catalog_native_collection
                 (scope, generation, attempt_generation, state, started_at, finished_at,
                  duration_ms, retry_at, runtimes_configured, runtimes_complete,
                  projects_observed, workspaces_observed, sessions_observed,
                  archived_sessions, include_archived, runtimes_json,
                  diagnostics_json, error)
             VALUES (?1,
                     CASE WHEN ?2 = 1 THEN ?3 ELSE COALESCE(
                         (SELECT generation FROM catalog_native_collection WHERE scope = ?1), 0) END,
                     ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
             ON CONFLICT (scope) DO UPDATE SET
                 generation = CASE WHEN ?2 = 1 THEN ?3
                                   ELSE catalog_native_collection.generation END,
                 attempt_generation = ?3,
                 state = ?4,
                 started_at = ?5,
                 finished_at = ?6,
                 duration_ms = ?7,
                 retry_at = ?8,
                 runtimes_configured = ?9,
                 runtimes_complete = ?10,
                 projects_observed = ?11,
                 workspaces_observed = ?12,
                 sessions_observed = ?13,
                 archived_sessions = ?14,
                 include_archived = ?15,
                 runtimes_json = ?16,
                 diagnostics_json = ?17,
                 error = ?18",
            params![
                sweep.scope,
                if outcome.complete { 1 } else { 0 },
                generation_to_sql(sweep.generation),
                state_text,
                started_at,
                now.to_string(),
                duration_ms.map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
                outcome.retry_at,
                i64::try_from(outcome.coverage.len()).unwrap_or(i64::MAX),
                i64::try_from(runtimes_complete).unwrap_or(i64::MAX),
                i64::try_from(outcome.projects_seen).unwrap_or(i64::MAX),
                i64::try_from(outcome.workspaces_seen).unwrap_or(i64::MAX),
                i64::try_from(outcome.sessions_seen).unwrap_or(i64::MAX),
                i64::try_from(outcome.archived_sessions_seen).unwrap_or(i64::MAX),
                if outcome.include_archived { 1 } else { 0 },
                serde_json::to_string(&outcome.coverage)?,
                serde_json::to_string(&outcome.diagnostics)?,
                outcome.error.as_deref().map(bounded)
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Record a native sweep that could not commit its observations.
    ///
    /// # Errors
    /// [`StoreError::Fenced`], [`StoreError::MissingSweep`] or a SQL refusal.
    pub(crate) fn fail_native_sweep(
        &self,
        sweep: &NativeSweepId,
        error: &str,
        diagnostics: &[String],
        coverage: &[NativeRuntimeCoverage],
        retry_at: Option<&str>,
        now: &Timestamp,
    ) -> Result<(), StoreError> {
        let mut connection = self.lock();
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let updated = transaction.execute(
            "UPDATE catalog_native_sweeps
             SET state = 'failed', finished_at = ?1, error = ?2, diagnostics_json = ?3,
                 runtimes_json = ?4
             WHERE scope = ?5 AND generation = ?6 AND owner_id = ?7 AND state = 'running'",
            params![
                now.to_string(),
                bounded(error),
                serde_json::to_string(diagnostics)?,
                serde_json::to_string(coverage)?,
                sweep.scope,
                generation_to_sql(sweep.generation),
                sweep.owner_id
            ],
        )?;
        if updated == 0 {
            return Err(StoreError::Fenced);
        }
        transaction.execute(
            "INSERT INTO catalog_native_collection
                 (scope, generation, attempt_generation, state, started_at, finished_at,
                  duration_ms, retry_at, runtimes_configured, runtimes_complete,
                  projects_observed, workspaces_observed, sessions_observed,
                  archived_sessions, include_archived, runtimes_json,
                  diagnostics_json, error)
             VALUES (?1, 0, ?2, 'failed', ?3, ?3, 0, ?4, ?5, 0, 0, 0, 0, 0, 1, ?6, ?7, ?8)
             ON CONFLICT (scope) DO UPDATE SET
                 attempt_generation = ?2,
                 state = 'failed',
                 finished_at = ?3,
                 retry_at = ?4,
                 runtimes_configured = ?5,
                 runtimes_complete = 0,
                 runtimes_json = ?6,
                 diagnostics_json = ?7,
                 error = ?8",
            params![
                sweep.scope,
                generation_to_sql(sweep.generation),
                now.to_string(),
                retry_at,
                i64::try_from(coverage.len()).unwrap_or(i64::MAX),
                serde_json::to_string(coverage)?,
                serde_json::to_string(diagnostics)?,
                bounded(error)
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// The current native status projection.
    ///
    /// # Errors
    /// SQLite refused.
    pub(crate) fn read_native_status(&self) -> Result<NativeStoreStatus, StoreError> {
        let connection = self.lock();
        let (generation, active): (i64, i64) = connection.query_row(
            "SELECT native_generation, native_active_generation
             FROM catalog_scope WHERE scope = ?1",
            [crate::contract::HOST_SCOPE],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let generation = u64::try_from(generation)
            .map_err(|_| StoreError::Corrupt("negative native generation".to_owned()))?;
        let active = u64::try_from(active)
            .map_err(|_| StoreError::Corrupt("negative native active generation".to_owned()))?;
        let as_of = if active == 0 {
            None
        } else {
            connection
                .query_row(
                    "SELECT finished_at FROM catalog_native_sweeps
                     WHERE scope = ?1 AND generation = ?2 AND state = 'complete'",
                    params![crate::contract::HOST_SCOPE, generation_to_sql(active)],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()?
                .flatten()
        };
        let owner = connection
            .query_row(
                "SELECT owner_id, lease_until FROM catalog_scope WHERE scope = ?1",
                [crate::contract::HOST_SCOPE],
                |row| {
                    Ok(OwnerStatus {
                        owner: row.get(0)?,
                        lease_until: row.get(1)?,
                    })
                },
            )
            .optional()?
            .unwrap_or_default();
        let collection = read_native_collection(&connection, active)?;
        let counts = read_native_counts(&connection, active)?;
        Ok(NativeStoreStatus {
            generation: active,
            attempted_generation: generation,
            as_of,
            owner,
            collection,
            counts,
        })
    }

    /// Every native observation at the active generation, in one read
    /// transaction.
    ///
    /// # Errors
    /// SQLite refused.
    pub(crate) fn read_native_inventory(
        &self,
        include_removed: bool,
        runtime_instance: Option<&str>,
    ) -> Result<NativeStoreInventory, StoreError> {
        let connection = self.lock();
        let (active, include_archived): (i64, i64) = connection.query_row(
            "SELECT s.native_active_generation,
                    COALESCE((SELECT include_archived FROM catalog_native_collection
                              WHERE scope = s.scope), 0)
             FROM catalog_scope s WHERE s.scope = ?1",
            [crate::contract::HOST_SCOPE],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let active = u64::try_from(active)
            .map_err(|_| StoreError::Corrupt("negative native active generation".to_owned()))?;
        let archive_scope = include_archived != 0;
        if active == 0 {
            return Ok(NativeStoreInventory {
                generation: 0,
                as_of: None,
                archive_scope,
                projects: Vec::new(),
                workspaces: Vec::new(),
                sessions: Vec::new(),
            });
        }
        let as_of = connection
            .query_row(
                "SELECT finished_at FROM catalog_native_sweeps
                 WHERE scope = ?1 AND generation = ?2 AND state = 'complete'",
                params![crate::contract::HOST_SCOPE, generation_to_sql(active)],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        let projects =
            read_native_projects(&connection, active, include_removed, runtime_instance)?;
        let workspaces =
            read_native_workspaces(&connection, active, include_removed, runtime_instance)?;
        let sessions =
            read_native_sessions(&connection, active, include_removed, runtime_instance)?;
        Ok(NativeStoreInventory {
            generation: active,
            as_of,
            archive_scope,
            projects,
            workspaces,
            sessions,
        })
    }

    /// The projection used by the native fallback snapshot.
    ///
    /// # Errors
    /// SQLite refused.
    pub(crate) fn native_snapshot_projection(
        &self,
    ) -> Result<NativeSnapshotProjection, StoreError> {
        let inventory = self.read_native_inventory(true, None)?;
        Ok(NativeSnapshotProjection {
            generation: inventory.generation,
            projects: inventory.projects,
            workspaces: inventory.workspaces,
            sessions: inventory.sessions,
        })
    }
}

/// The generation-consistent rows a fallback snapshot carries.
pub(crate) struct NativeSnapshotProjection {
    pub(crate) generation: u64,
    pub(crate) projects: Vec<NativeProjectRecord>,
    pub(crate) workspaces: Vec<NativeWorkspaceRecord>,
    pub(crate) sessions: Vec<NativeSessionRecord>,
}

fn native_attempt_state(outcome: &NativeSweepOutcome) -> &'static str {
    if outcome.complete {
        "complete"
    } else if !outcome.coverage.is_empty()
        && outcome
            .coverage
            .iter()
            .all(|coverage| coverage.state == NativeRuntimeState::Failed)
    {
        "failed"
    } else {
        "incomplete"
    }
}

fn promote_native_complete(
    transaction: &rusqlite::Transaction<'_>,
    generation: u64,
    now: &Timestamp,
    coverage: &[NativeRuntimeCoverage],
    projects: &[NativeProject],
    workspaces: &[NativeWorkspace],
    sessions: &[NativeSession],
) -> Result<(), StoreError> {
    let observed_at = now.to_string();
    for project in projects {
        upsert_native_project(transaction, generation, &observed_at, project)?;
    }
    for workspace in workspaces {
        upsert_native_workspace(transaction, generation, &observed_at, workspace)?;
    }
    for session in sessions {
        upsert_native_session(transaction, generation, &observed_at, session)?;
    }

    // Only the runtime instances this complete attempt observed may prove
    // absence. A runtime removed from configuration is not absence, and its
    // last facts stay visible.
    for runtime in coverage {
        transaction.execute(
            "UPDATE catalog_native_projects
             SET state = 'removed', removed_at = ?1, visible_generation = ?2
             WHERE runtime_instance = ?3 AND state = 'active'
               AND last_seen_generation <> ?2",
            params![
                observed_at,
                generation_to_sql(generation),
                runtime.runtime_instance
            ],
        )?;
        transaction.execute(
            "UPDATE catalog_native_workspaces
             SET state = 'removed', removed_at = ?1, visible_generation = ?2
             WHERE runtime_instance = ?3 AND state = 'active'
               AND last_seen_generation <> ?2",
            params![
                observed_at,
                generation_to_sql(generation),
                runtime.runtime_instance
            ],
        )?;
        transaction.execute(
            "UPDATE catalog_native_sessions
             SET state = 'removed', removed_at = ?1, visible_generation = ?2
             WHERE runtime_instance = ?3 AND state = 'active'
               AND last_seen_generation <> ?2",
            params![
                observed_at,
                generation_to_sql(generation),
                runtime.runtime_instance
            ],
        )?;
    }
    Ok(())
}

fn promote_native_incomplete(
    transaction: &rusqlite::Transaction<'_>,
    generation: u64,
    now: &Timestamp,
    projects: &[NativeProject],
    workspaces: &[NativeWorkspace],
    sessions: &[NativeSession],
) -> Result<(), StoreError> {
    let observed_at = now.to_string();
    for project in projects {
        insert_native_project_hidden(transaction, generation, &observed_at, project)?;
    }
    for workspace in workspaces {
        insert_native_workspace_hidden(transaction, generation, &observed_at, workspace)?;
    }
    for session in sessions {
        insert_native_session_hidden(transaction, generation, &observed_at, session)?;
    }
    Ok(())
}

fn upsert_native_project(
    transaction: &rusqlite::Transaction<'_>,
    generation: u64,
    observed_at: &str,
    project: &NativeProject,
) -> Result<(), StoreError> {
    transaction.execute(
        "INSERT INTO catalog_native_projects
             (runtime_instance, project_id, display_name, custom_name, project_key,
              root_path, project_kind, state, first_seen, last_seen,
              first_seen_generation, last_seen_generation, visible_generation, removed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'active', ?8, ?8, ?9, ?9, ?9, NULL)
         ON CONFLICT (runtime_instance, project_id) DO UPDATE SET
             display_name = excluded.display_name,
             custom_name = excluded.custom_name,
             project_key = excluded.project_key,
             root_path = excluded.root_path,
             project_kind = excluded.project_kind,
             state = 'active',
             last_seen = excluded.last_seen,
             last_seen_generation = excluded.last_seen_generation,
             visible_generation = excluded.visible_generation,
             removed_at = NULL",
        params![
            project.runtime_instance,
            project.project_id,
            project.display_name,
            project.custom_name,
            project.project_key,
            project.root_path,
            project.project_kind,
            observed_at,
            generation_to_sql(generation)
        ],
    )?;
    Ok(())
}

fn insert_native_project_hidden(
    transaction: &rusqlite::Transaction<'_>,
    generation: u64,
    observed_at: &str,
    project: &NativeProject,
) -> Result<(), StoreError> {
    transaction.execute(
        "INSERT OR IGNORE INTO catalog_native_projects
             (runtime_instance, project_id, display_name, custom_name, project_key,
              root_path, project_kind, state, first_seen, last_seen,
              first_seen_generation, last_seen_generation, visible_generation, removed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'active', ?8, ?8, ?9, ?9, NULL, NULL)",
        params![
            project.runtime_instance,
            project.project_id,
            project.display_name,
            project.custom_name,
            project.project_key,
            project.root_path,
            project.project_kind,
            observed_at,
            generation_to_sql(generation)
        ],
    )?;
    Ok(())
}

fn upsert_native_workspace(
    transaction: &rusqlite::Transaction<'_>,
    generation: u64,
    observed_at: &str,
    workspace: &NativeWorkspace,
) -> Result<(), StoreError> {
    transaction.execute(
        "INSERT INTO catalog_native_workspaces
             (runtime_instance, workspace_id, project_id, name, title, directory, kind,
              archiving_at, worktree_slug, project_display_name, project_custom_name,
              project_root_path, project_kind, state, first_seen, last_seen,
              first_seen_generation, last_seen_generation, visible_generation, removed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                 'active', ?14, ?14, ?15, ?15, ?15, NULL)
         ON CONFLICT (runtime_instance, workspace_id) DO UPDATE SET
             project_id = excluded.project_id,
             name = excluded.name,
             title = excluded.title,
             directory = excluded.directory,
             kind = excluded.kind,
             archiving_at = excluded.archiving_at,
             worktree_slug = excluded.worktree_slug,
             project_display_name = excluded.project_display_name,
             project_custom_name = excluded.project_custom_name,
             project_root_path = excluded.project_root_path,
             project_kind = excluded.project_kind,
             state = 'active',
             last_seen = excluded.last_seen,
             last_seen_generation = excluded.last_seen_generation,
             visible_generation = excluded.visible_generation,
             removed_at = NULL",
        params![
            workspace.runtime_instance,
            workspace.workspace_id,
            workspace.project_id,
            workspace.name,
            workspace.title,
            workspace.directory,
            workspace.kind,
            workspace.archiving_at,
            workspace.worktree_slug,
            workspace.project_display_name,
            workspace.project_custom_name,
            workspace.project_root_path,
            workspace.project_kind,
            observed_at,
            generation_to_sql(generation)
        ],
    )?;
    Ok(())
}

fn insert_native_workspace_hidden(
    transaction: &rusqlite::Transaction<'_>,
    generation: u64,
    observed_at: &str,
    workspace: &NativeWorkspace,
) -> Result<(), StoreError> {
    transaction.execute(
        "INSERT OR IGNORE INTO catalog_native_workspaces
             (runtime_instance, workspace_id, project_id, name, title, directory, kind,
              archiving_at, worktree_slug, project_display_name, project_custom_name,
              project_root_path, project_kind, state, first_seen, last_seen,
              first_seen_generation, last_seen_generation, visible_generation, removed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                 'active', ?14, ?14, ?15, ?15, NULL, NULL)",
        params![
            workspace.runtime_instance,
            workspace.workspace_id,
            workspace.project_id,
            workspace.name,
            workspace.title,
            workspace.directory,
            workspace.kind,
            workspace.archiving_at,
            workspace.worktree_slug,
            workspace.project_display_name,
            workspace.project_custom_name,
            workspace.project_root_path,
            workspace.project_kind,
            observed_at,
            generation_to_sql(generation)
        ],
    )?;
    Ok(())
}

fn upsert_native_session(
    transaction: &rusqlite::Transaction<'_>,
    generation: u64,
    observed_at: &str,
    session: &NativeSession,
) -> Result<(), StoreError> {
    let checkout = session.checkout.clone().unwrap_or_default();
    transaction.execute(
        "INSERT INTO catalog_native_sessions
             (runtime_instance, session_id, workspace_id, project_id, project_name,
              workspace_name, title, provider, model, status, archived_at,
              attention_reason, cwd, created_at, updated_at, last_user_message_at,
              parent_session_id, checkout_cwd, checkout_branch, worktree_root,
              checkout_is_git, checkout_is_paseo_owned_worktree, state, first_seen,
              last_seen, first_seen_generation, last_seen_generation,
              visible_generation, removed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                 ?16, ?17, ?18, ?19, ?20, ?21, ?22, 'active', ?23, ?23, ?24, ?24, ?24, NULL)
         ON CONFLICT (runtime_instance, session_id) DO UPDATE SET
             workspace_id = excluded.workspace_id,
             project_id = excluded.project_id,
             project_name = excluded.project_name,
             workspace_name = excluded.workspace_name,
             title = excluded.title,
             provider = excluded.provider,
             model = excluded.model,
             status = excluded.status,
             archived_at = excluded.archived_at,
             attention_reason = excluded.attention_reason,
             cwd = excluded.cwd,
             created_at = excluded.created_at,
             updated_at = excluded.updated_at,
             last_user_message_at = excluded.last_user_message_at,
             parent_session_id = excluded.parent_session_id,
             checkout_cwd = excluded.checkout_cwd,
             checkout_branch = excluded.checkout_branch,
             worktree_root = excluded.worktree_root,
             checkout_is_git = excluded.checkout_is_git,
             checkout_is_paseo_owned_worktree = excluded.checkout_is_paseo_owned_worktree,
             state = 'active',
             last_seen = excluded.last_seen,
             last_seen_generation = excluded.last_seen_generation,
             visible_generation = excluded.visible_generation,
             removed_at = NULL",
        params![
            session.runtime_instance,
            session.session_id,
            session.workspace_id,
            session.project_id,
            session.project_name,
            session.workspace_name,
            session.title,
            session.provider,
            session.model,
            session.status,
            session.archived_at,
            session.attention_reason,
            session.cwd,
            session.created_at,
            session.updated_at,
            session.last_user_message_at,
            session.parent_session_id,
            checkout.cwd,
            checkout.branch,
            checkout.worktree_root,
            checkout.is_git.map(i64::from),
            checkout.is_paseo_owned_worktree.map(i64::from),
            observed_at,
            generation_to_sql(generation)
        ],
    )?;
    Ok(())
}

fn insert_native_session_hidden(
    transaction: &rusqlite::Transaction<'_>,
    generation: u64,
    observed_at: &str,
    session: &NativeSession,
) -> Result<(), StoreError> {
    let checkout = session.checkout.clone().unwrap_or_default();
    transaction.execute(
        "INSERT OR IGNORE INTO catalog_native_sessions
             (runtime_instance, session_id, workspace_id, project_id, project_name,
              workspace_name, title, provider, model, status, archived_at,
              attention_reason, cwd, created_at, updated_at, last_user_message_at,
              parent_session_id, checkout_cwd, checkout_branch, worktree_root,
              checkout_is_git, checkout_is_paseo_owned_worktree, state, first_seen,
              last_seen, first_seen_generation, last_seen_generation,
              visible_generation, removed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                 ?16, ?17, ?18, ?19, ?20, ?21, ?22, 'active', ?23, ?23, ?24, ?24, NULL, NULL)",
        params![
            session.runtime_instance,
            session.session_id,
            session.workspace_id,
            session.project_id,
            session.project_name,
            session.workspace_name,
            session.title,
            session.provider,
            session.model,
            session.status,
            session.archived_at,
            session.attention_reason,
            session.cwd,
            session.created_at,
            session.updated_at,
            session.last_user_message_at,
            session.parent_session_id,
            checkout.cwd,
            checkout.branch,
            checkout.worktree_root,
            checkout.is_git.map(i64::from),
            checkout.is_paseo_owned_worktree.map(i64::from),
            observed_at,
            generation_to_sql(generation)
        ],
    )?;
    Ok(())
}

type NativeCollectionRow = (
    i64,
    i64,
    String,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    String,
    String,
    Option<String>,
);

fn read_native_collection(
    connection: &rusqlite::Connection,
    active: u64,
) -> Result<NativeCollectionStatus, StoreError> {
    let row: Option<NativeCollectionRow> = connection
        .query_row(
            "SELECT generation, attempt_generation, state, started_at, finished_at,
                    duration_ms, retry_at, runtimes_configured, runtimes_complete,
                    projects_observed, workspaces_observed, sessions_observed,
                    archived_sessions, include_archived, runtimes_json,
                    diagnostics_json, error
             FROM catalog_native_collection WHERE scope = ?1",
            [crate::contract::HOST_SCOPE],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                    row.get(11)?,
                    row.get(12)?,
                    row.get(13)?,
                    row.get(14)?,
                    row.get(15)?,
                    row.get(16)?,
                ))
            },
        )
        .optional()?;
    let Some((
        _generation,
        attempt_generation,
        state,
        started_at,
        finished_at,
        duration_ms,
        retry_at,
        runtimes_configured,
        runtimes_complete,
        projects_observed,
        workspaces_observed,
        sessions_observed,
        archived_sessions,
        include_archived,
        runtimes_json,
        diagnostics_json,
        error,
    )) = row
    else {
        return Ok(NativeCollectionStatus {
            state: CollectionState::NeverCollected,
            generation: active,
            attempt_generation: 0,
            started_at: None,
            finished_at: None,
            duration_ms: None,
            retry_at: None,
            runtimes_configured: 0,
            runtimes_complete: 0,
            projects_observed: 0,
            workspaces_observed: 0,
            sessions_observed: 0,
            archived_sessions: 0,
            include_archived: false,
            error: None,
            diagnostics: Vec::new(),
            coverage: Vec::new(),
        });
    };
    let coverage: Vec<NativeRuntimeCoverage> =
        serde_json::from_str(&runtimes_json).unwrap_or_default();
    Ok(NativeCollectionStatus {
        state: match state.as_str() {
            "complete" => CollectionState::Complete,
            "incomplete" => CollectionState::Incomplete,
            "failed" => CollectionState::Failed,
            _ => CollectionState::NeverCollected,
        },
        generation: active,
        attempt_generation: u64::try_from(attempt_generation).unwrap_or(0),
        started_at,
        finished_at,
        duration_ms: duration_ms.and_then(|value| u64::try_from(value).ok()),
        retry_at,
        runtimes_configured: usize::try_from(runtimes_configured).unwrap_or(0),
        runtimes_complete: usize::try_from(runtimes_complete).unwrap_or(0),
        projects_observed: usize::try_from(projects_observed).unwrap_or(0),
        workspaces_observed: usize::try_from(workspaces_observed).unwrap_or(0),
        sessions_observed: usize::try_from(sessions_observed).unwrap_or(0),
        archived_sessions: usize::try_from(archived_sessions).unwrap_or(0),
        include_archived: include_archived != 0,
        error,
        diagnostics: serde_json::from_str(&diagnostics_json).unwrap_or_default(),
        coverage,
    })
}

fn read_native_counts(
    connection: &rusqlite::Connection,
    active: u64,
) -> Result<NativeCounts, StoreError> {
    if active == 0 {
        return Ok(NativeCounts::default());
    }
    let generation = generation_to_sql(active);
    let count = |table: &str, extra: &str| -> Result<usize, StoreError> {
        let sql = format!("SELECT COUNT(*) FROM {table} WHERE visible_generation = ?1 AND {extra}");
        let value: i64 = connection.query_row(&sql, [generation], |row| row.get(0))?;
        Ok(usize::try_from(value).unwrap_or(0))
    };
    let projects = count("catalog_native_projects", "state = 'active'")?;
    let workspaces = count("catalog_native_workspaces", "state = 'active'")?;
    let sessions = count("catalog_native_sessions", "state = 'active'")?;
    let archived_sessions = count(
        "catalog_native_sessions",
        "state = 'active' AND archived_at IS NOT NULL",
    )?;
    let tombstones = count("catalog_native_projects", "state = 'removed'")?
        + count("catalog_native_workspaces", "state = 'removed'")?
        + count("catalog_native_sessions", "state = 'removed'")?;
    Ok(NativeCounts {
        projects,
        workspaces,
        sessions,
        archived_sessions,
        tombstones,
    })
}

fn read_native_projects(
    connection: &rusqlite::Connection,
    active: u64,
    include_removed: bool,
    runtime_instance: Option<&str>,
) -> Result<Vec<NativeProjectRecord>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT runtime_instance, project_id, display_name, custom_name, project_key,
                root_path, project_kind, state, first_seen, last_seen
         FROM catalog_native_projects
         WHERE visible_generation = ?1
           AND (?2 = 1 OR state = 'active')
           AND (?3 IS NULL OR runtime_instance = ?3)
         ORDER BY runtime_instance, project_id",
    )?;
    let rows = statement.query_map(
        params![
            generation_to_sql(active),
            i64::from(include_removed),
            runtime_instance
        ],
        |row| {
            Ok(NativeProjectRecord {
                runtime_instance: row.get(0)?,
                project_id: row.get(1)?,
                display_name: row.get(2)?,
                custom_name: row.get(3)?,
                project_key: row.get(4)?,
                root_path: row.get(5)?,
                project_kind: row.get(6)?,
                state: parse_state(&row.get::<_, String>(7)?),
                first_seen: row.get(8)?,
                last_seen: row.get(9)?,
            })
        },
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn read_native_workspaces(
    connection: &rusqlite::Connection,
    active: u64,
    include_removed: bool,
    runtime_instance: Option<&str>,
) -> Result<Vec<NativeWorkspaceRecord>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT runtime_instance, workspace_id, project_id, name, title, directory,
                kind, archiving_at, worktree_slug, project_display_name,
                project_custom_name, project_root_path, project_kind, state,
                first_seen, last_seen
         FROM catalog_native_workspaces
         WHERE visible_generation = ?1
           AND (?2 = 1 OR state = 'active')
           AND (?3 IS NULL OR runtime_instance = ?3)
         ORDER BY runtime_instance, project_id, workspace_id",
    )?;
    let rows = statement.query_map(
        params![
            generation_to_sql(active),
            i64::from(include_removed),
            runtime_instance
        ],
        |row| {
            Ok(NativeWorkspaceRecord {
                runtime_instance: row.get(0)?,
                workspace_id: row.get(1)?,
                project_id: row.get(2)?,
                name: row.get(3)?,
                title: row.get(4)?,
                directory: row.get(5)?,
                kind: row.get(6)?,
                archiving_at: row.get(7)?,
                worktree_slug: row.get(8)?,
                project_display_name: row.get(9)?,
                project_custom_name: row.get(10)?,
                project_root_path: row.get(11)?,
                project_kind: row.get(12)?,
                state: parse_state(&row.get::<_, String>(13)?),
                first_seen: row.get(14)?,
                last_seen: row.get(15)?,
            })
        },
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn read_native_sessions(
    connection: &rusqlite::Connection,
    active: u64,
    include_removed: bool,
    runtime_instance: Option<&str>,
) -> Result<Vec<NativeSessionRecord>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT runtime_instance, session_id, workspace_id, project_id, project_name,
                workspace_name, title, provider, model, status, archived_at,
                attention_reason, cwd, created_at, updated_at, last_user_message_at,
                parent_session_id, checkout_cwd, checkout_branch, worktree_root,
                checkout_is_git, checkout_is_paseo_owned_worktree, state,
                first_seen, last_seen
         FROM catalog_native_sessions
         WHERE visible_generation = ?1
           AND (?2 = 1 OR state = 'active')
           AND (?3 IS NULL OR runtime_instance = ?3)
         ORDER BY runtime_instance, workspace_id, session_id",
    )?;
    let rows = statement.query_map(
        params![
            generation_to_sql(active),
            i64::from(include_removed),
            runtime_instance
        ],
        |row| {
            let checkout = NativeCheckout {
                cwd: row.get(17)?,
                branch: row.get(18)?,
                worktree_root: row.get(19)?,
                is_git: row.get::<_, Option<i64>>(20)?.map(|value| value != 0),
                is_paseo_owned_worktree: row.get::<_, Option<i64>>(21)?.map(|value| value != 0),
            };
            Ok(NativeSessionRecord {
                runtime_instance: row.get(0)?,
                session_id: row.get(1)?,
                workspace_id: row.get(2)?,
                project_id: row.get(3)?,
                project_name: row.get(4)?,
                workspace_name: row.get(5)?,
                title: row.get(6)?,
                provider: row.get(7)?,
                model: row.get(8)?,
                status: row.get(9)?,
                archived_at: row.get(10)?,
                attention_reason: row.get(11)?,
                cwd: row.get(12)?,
                created_at: row.get(13)?,
                updated_at: row.get(14)?,
                last_user_message_at: row.get(15)?,
                parent_session_id: row.get(16)?,
                checkout: Some(checkout),
                state: parse_state(&row.get::<_, String>(22)?),
                first_seen: row.get(23)?,
                last_seen: row.get(24)?,
            })
        },
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_native_identity_is_the_runtime_and_the_native_id() {
        let census = NativeCensus {
            runtime_instance: "paseo:ws://127.0.0.1:6767/ws".to_owned(),
            client_id: "kontor-census".to_owned(),
            server_id: Some("srv-1".to_owned()),
            server_version: Some("0.11.4".to_owned()),
            protocol_version: 1,
            permissions: None,
            features: BTreeMap::new(),
            include_archived: true,
            complete: true,
            error: None,
            diagnostics: Vec::new(),
            pages: NativePageCounts::default(),
            projects: Vec::new(),
            workspaces: Vec::new(),
            sessions: vec![
                NativeSession {
                    runtime_instance: "paseo:ws://127.0.0.1:6767/ws".to_owned(),
                    session_id: "agt_1".to_owned(),
                    workspace_id: Some("wks_1".to_owned()),
                    project_id: Some("prj_1".to_owned()),
                    project_name: Some("DESK • ADAM".to_owned()),
                    workspace_name: Some("ADAM".to_owned()),
                    title: Some("Same title".to_owned()),
                    provider: "claude".to_owned(),
                    model: Some("opus".to_owned()),
                    status: "idle".to_owned(),
                    archived_at: None,
                    attention_reason: None,
                    cwd: "/w/1".to_owned(),
                    created_at: None,
                    updated_at: None,
                    last_user_message_at: None,
                    parent_session_id: None,
                    checkout: None,
                },
                NativeSession {
                    runtime_instance: "paseo:ws://127.0.0.1:6767/ws".to_owned(),
                    session_id: "agt_2".to_owned(),
                    workspace_id: Some("wks_1".to_owned()),
                    project_id: Some("prj_1".to_owned()),
                    project_name: Some("DESK • ADAM".to_owned()),
                    workspace_name: Some("ADAM".to_owned()),
                    title: Some("Same title".to_owned()),
                    provider: "codex".to_owned(),
                    model: None,
                    status: "closed".to_owned(),
                    archived_at: Some("2026-10-25T00:00:00Z".to_owned()),
                    attention_reason: None,
                    cwd: "/w/1".to_owned(),
                    created_at: None,
                    updated_at: None,
                    last_user_message_at: None,
                    parent_session_id: None,
                    checkout: None,
                },
            ],
        };
        assert_eq!(census.archived_sessions(), 1);
        let coverage = NativeRuntimeCoverage::from_census(&census);
        assert_eq!(coverage.state, NativeRuntimeState::Scanned);
        assert_eq!(coverage.sessions, 2);
        assert_eq!(coverage.archived_sessions, 1);
    }

    #[tokio::test]
    async fn a_stale_native_page_and_a_superseded_finalizer_are_fenced() {
        let directory = tempfile::TempDir::new().expect("a state root");
        let database =
            Database::open(&directory.path().join("catalog.db")).expect("the catalog database");
        let now = Timestamp::now();
        database
            .acquire_owner("fence-owner", 120, &now)
            .expect("the fence lease");
        let stale = database
            .begin_native_sweep("fence-owner", &now)
            .expect("the first native sweep");
        let current = database
            .begin_native_sweep("fence-owner", &now)
            .expect("the second native sweep");
        assert_eq!(stale.generation + 1, current.generation);
        assert!(matches!(
            database.commit_native_page::<NativeProject>(&stale, 0, "projects", &now, &[]),
            Err(StoreError::Fenced)
        ));
        assert!(matches!(
            database.finalize_native_sweep(
                &stale,
                &NativeSweepOutcome {
                    complete: true,
                    coverage: Vec::new(),
                    projects_seen: 0,
                    workspaces_seen: 0,
                    sessions_seen: 0,
                    archived_sessions_seen: 0,
                    include_archived: true,
                    diagnostics: Vec::new(),
                    error: None,
                    retry_at: None,
                },
                &now,
            ),
            Err(StoreError::Fenced)
        ));
        database
            .commit_native_page::<NativeProject>(&current, 0, "projects", &now, &[])
            .expect("the current page commits");
    }

    #[test]
    fn an_unreachable_runtime_is_failed_not_an_empty_directory() {
        let census = NativeCensus::unavailable("paseo:test", "census-1", "socket refused");
        assert!(!census.complete);
        let coverage = NativeRuntimeCoverage::from_census(&census);
        assert_eq!(coverage.state, NativeRuntimeState::Failed);
        assert_eq!(coverage.sessions, 0);
        assert_eq!(coverage.error.as_deref(), Some("socket refused"));
    }
}
