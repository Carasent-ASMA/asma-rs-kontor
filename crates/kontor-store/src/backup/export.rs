//! The versioned, redacted, byte-deterministic document a Realm may hand out.
//!
//! # Typed rows, never a table dump
//!
//! Every exported table is declared below as a concrete struct with named,
//! typed columns. There is no `SELECT *`, no dynamic column discovery and no
//! "serialize whatever the schema happens to have" path, and that is a
//! *redaction* property rather than a style preference: a column added by a
//! later migration cannot appear in an export until somebody adds it here and
//! decides what it is. A dynamic dump would have exported it the day it landed.
//!
//! The same declaration is what keeps the document deterministic. Struct fields
//! serialize in declaration order, every array is read back under an explicit
//! `ORDER BY` over its primary key, and the whole document is rendered through
//! `serde_json::Value` — whose object keys are a `BTreeMap` and therefore
//! sorted. Two exports of one unchanged Realm produce identical record bytes and
//! an identical digest.
//!
//! # What is deliberately not here
//!
//! Runtime transcripts, message frames, tool calls and token deltas — none of
//! which are in the database to begin with, because
//! [`crate::events::types::ensure_control_metadata`] refuses them at the append
//! boundary and this module re-runs that check on every exported payload.
//! Runtime endpoints and provider tokens, which this process never persists.
//! The credential file, connector credentials and keychain or config-home
//! paths. The credential-*reference* resolution data on an account profile —
//! its kind, its alias and its environment mapping — of which only the opaque
//! provider identity survives, and only as provenance. Inbound external comment
//! *bodies*, which are the one place Zone C prose could reach a Kontor row: the
//! comment's identity, author, instants, digest and cursor are exported, so the
//! continuity evidence is intact and the prose is not.
//!
//! Every one of those omissions is named in the document's own
//! [`RedactionSummary`], so a reader is told what was withheld rather than
//! having to infer it from an absence.

use std::collections::BTreeMap;

use kontor_core::id::{ContentHash, RealmId, Timestamp, reject_sensitive_material};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::SqliteStore;
use crate::backup::BackupError;
use crate::events::types::ensure_control_metadata;

/// The export generation this build writes and is willing to read.
pub const EXPORT_SCHEMA_VERSION: u32 = 1;

/// How deep an embedded document is followed by the canary scan.
///
/// Persisted documents are already depth-bounded by
/// [`kontor_core::id::CanonicalDocument`]; this only stops a pathological chain
/// of JSON-inside-JSON-inside-JSON from recursing without end.
const MAX_EMBEDDED_DEPTH: u8 = 8;

/// How many records of each kind the document carries.
pub type RecordCounts = BTreeMap<String, u64>;

/// What this export withheld, and why.
///
/// It is part of the document rather than part of the documentation: an
/// importer, an auditor and a reviewer all need to know that "no comment
/// bodies" means "deliberately withheld" and not "there were none".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactionSummary {
    /// Tables no record of which is exported, by reason.
    pub excluded_tables: BTreeMap<String, String>,
    /// Columns withheld from an otherwise exported table, by reason.
    pub excluded_columns: BTreeMap<String, String>,
    /// Whether the canary scan ran over this document before it was published.
    /// It is always `true` in a published export — a `false` here means the
    /// document was assembled and never released.
    pub canary_scanned: bool,
}

/// What the export says about its own completeness.
///
/// Every value is derived from the exported records themselves, so the summary
/// cannot claim a continuity the records do not support, and two exports of one
/// unchanged Realm produce the same summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuitySummary {
    /// How many records of each kind the document carries.
    pub record_counts: RecordCounts,
    /// Recorded gaps in a runtime's own control sequence.
    pub control_gaps: u64,
    /// Recorded gaps in a runtime's session-content sequence. The content
    /// itself is runtime-owned and is not exported; the *gap* is Kontor's own
    /// evidence and is.
    pub content_gaps: u64,
    /// Command receipts that had not reached a settled state.
    pub unsettled_command_receipts: u64,
    /// Reconciliation epochs that had not completed, and whose members
    /// therefore prove nothing about what they did not reach.
    pub incomplete_reconciliation_epochs: u64,
    /// Runs whose newest confirmation was not fresh when the export was taken.
    pub unconfirmed_agent_runs: u64,
    /// The highest control-plane cursor in the document, or 0 when it carries
    /// no observations.
    pub highest_control_cursor: i64,
}

/// One Realm's exportable state, as a versioned document.
///
/// The field order is the serialization order, and `records` is last so a
/// reader can see what the document claims about itself before it reads the
/// state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KontorExportV1 {
    /// The export generation. A later one is refused rather than misread.
    pub schema_version: u32,
    /// The Realm these records came from. In the destination of an import this
    /// is a *reference*, never an authority.
    pub source_realm_id: RealmId,
    /// When the export was taken. Deliberately outside [`Self::records_hash`]:
    /// it is the one value that changes when nothing else did.
    pub exported_at: Timestamp,
    /// The database schema generation the records were read from.
    pub database_schema_version: i64,
    /// What was withheld.
    pub redaction_summary: RedactionSummary,
    /// What the document says about its own completeness.
    pub continuity_summary: ContinuitySummary,
    /// SHA-256 over the canonical bytes of `records` alone.
    pub records_hash: ContentHash,
    /// The records.
    pub records: ExportedRecords,
}

impl KontorExportV1 {
    /// The canonical bytes of the whole document: compact UTF-8 JSON with
    /// sorted keys and exactly one trailing newline.
    ///
    /// # Errors
    /// Returns [`BackupError::Redaction`] only through [`Self::canonical_value`]'s
    /// serialization failure path, which cannot happen for this type.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, BackupError> {
        canonical_bytes(&canonical_value(self)?)
    }

    /// The canonical bytes of `records` alone — the bytes
    /// [`Self::records_hash`] is taken over.
    ///
    /// # Errors
    /// As [`Self::canonical_bytes`].
    pub fn canonical_records_bytes(&self) -> Result<Vec<u8>, BackupError> {
        canonical_bytes(&canonical_value(&self.records)?)
    }

    /// Recompute the digest and compare it with the one the document carries.
    ///
    /// # Errors
    /// Returns [`BackupError::Verification`] when the records do not hash to the
    /// declared digest, and [`BackupError::UnsupportedExportVersion`] when the
    /// document is not this generation.
    pub fn verify(&self) -> Result<(), BackupError> {
        if self.schema_version != EXPORT_SCHEMA_VERSION {
            return Err(BackupError::UnsupportedExportVersion {
                found: self.schema_version,
                expected: EXPORT_SCHEMA_VERSION,
            });
        }
        if ContentHash::of(&self.canonical_records_bytes()?) != self.records_hash {
            return Err(BackupError::Verification {
                detail: "the export's records do not hash to its declared digest",
            });
        }
        Ok(())
    }

    /// Parse a document, refusing an unknown generation before anything else.
    ///
    /// The version is read from the raw JSON first, so a future export is
    /// refused with a typed error instead of failing as a shape mismatch on
    /// whichever field happened to change.
    ///
    /// # Errors
    /// Returns [`BackupError::UnsupportedExportVersion`] for another generation
    /// and [`BackupError::Verification`] when the bytes are not a document of
    /// this one.
    pub fn parse(bytes: &[u8]) -> Result<Self, BackupError> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| BackupError::Verification {
                detail: "the export is not a JSON document",
            })?;
        let found = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .ok_or(BackupError::Verification {
                detail: "the export does not declare a schema version",
            })?;
        let found = u32::try_from(found).unwrap_or(u32::MAX);
        if found != EXPORT_SCHEMA_VERSION {
            return Err(BackupError::UnsupportedExportVersion {
                found,
                expected: EXPORT_SCHEMA_VERSION,
            });
        }
        let export: Self =
            serde_json::from_value(value).map_err(|_| BackupError::Verification {
                detail: "the export is not a document of this generation",
            })?;
        export.verify()?;
        Ok(export)
    }
}

/// Render any serializable value through `serde_json::Value`, whose objects are
/// `BTreeMap`s and therefore key-sorted.
fn canonical_value<T: Serialize>(value: &T) -> Result<serde_json::Value, BackupError> {
    serde_json::to_value(value).map_err(|_| BackupError::Verification {
        detail: "the export could not be rendered as JSON",
    })
}

/// Compact bytes plus exactly one trailing newline.
fn canonical_bytes(value: &serde_json::Value) -> Result<Vec<u8>, BackupError> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| BackupError::Verification {
        detail: "the export could not be rendered as JSON",
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Export one Realm.
///
/// The document is assembled, scanned and only then returned: a canary match
/// aborts the export, and the caller never receives a document that failed the
/// scan — there is no "return it with a warning" path, because a warning is
/// something an automation ignores.
///
/// # Errors
/// Returns [`BackupError::Redaction`] when the canary scan matches,
/// [`BackupError::Domain`] when a stored control payload is not control
/// metadata, and [`BackupError::Store`] when the database cannot be read.
pub fn export_realm(store: &SqliteStore, now: Timestamp) -> Result<KontorExportV1, BackupError> {
    let records = ExportedRecords::read(&store.connection)?;
    let continuity_summary = records.continuity();
    let records_hash = ContentHash::of(&canonical_bytes(&canonical_value(&records)?)?);
    let export = KontorExportV1 {
        schema_version: EXPORT_SCHEMA_VERSION,
        source_realm_id: store.realm_id(),
        exported_at: now,
        database_schema_version: store.schema_version()?,
        redaction_summary: redaction_summary(),
        continuity_summary,
        records_hash,
        records,
    };

    // Every stored control payload is held to the same rule the append boundary
    // holds it to. A transcript that somehow reached a row does not leave the
    // machine in an export.
    for event in &export.records.runtime_events {
        let payload: serde_json::Value =
            serde_json::from_str(&event.payload).map_err(|_| BackupError::Verification {
                detail: "a stored control payload is not JSON",
            })?;
        ensure_control_metadata(&payload)?;
    }
    scan_for_canaries(&canonical_value(&export)?, 0)?;
    Ok(export)
}

/// Refuse a document that carries credential, token or Zone C material.
///
/// Two passes, because persisted documents are stored as *text*: the domain's
/// own scanner sees the structure of this document, and every string that is
/// itself JSON is parsed and scanned as structure too. Without the second pass
/// a `definition` column could carry an `api_key` member and be seen only as an
/// opaque string.
fn scan_for_canaries(value: &serde_json::Value, depth: u8) -> Result<(), BackupError> {
    reject_sensitive_material(value).map_err(|error| match error {
        kontor_core::DomainError::SensitiveMaterial { path } => BackupError::Redaction { path },
        // The scanner raises nothing else, but a future variant must not become
        // a silent success.
        other => BackupError::Domain(other),
    })?;
    if depth >= MAX_EMBEDDED_DEPTH {
        return Ok(());
    }
    match value {
        serde_json::Value::Object(members) => {
            for member in members.values() {
                scan_for_canaries(member, depth)?;
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                scan_for_canaries(item, depth)?;
            }
        }
        serde_json::Value::String(text) => {
            let trimmed = text.trim_start();
            if (trimmed.starts_with('{') || trimmed.starts_with('['))
                && let Ok(embedded) = serde_json::from_str::<serde_json::Value>(text)
            {
                scan_for_canaries(&embedded, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// What every export of this generation withholds.
fn redaction_summary() -> RedactionSummary {
    let excluded_tables = [
        (
            "resource_leases",
            "a lease is authority to act now, not durable state; it is never carried to another process or Realm",
        ),
        (
            "lease_events",
            "the history of a lease has no meaning without the lease it fenced",
        ),
    ];
    let excluded_columns = [
        (
            "account_profiles.credential_ref_kind",
            "credential-reference resolution data; only the opaque provider identity is exported, as provenance",
        ),
        (
            "account_profiles.credential_ref_alias",
            "credential-reference resolution data; the alias resolves only against a policy that is never persisted",
        ),
        (
            "account_profiles.environment_refs",
            "the environment-variable mapping a credential reference fills",
        ),
        (
            "account_profiles.environment_refs_hash",
            "the digest of the withheld environment mapping",
        ),
        (
            "command_outbox.claim_token",
            "a live dispatch claim; an imported claim would authorize a second delivery of an effect that already happened",
        ),
        (
            "external_comments.body",
            "inbound external comment prose, the one column that can carry Zone C material; identity, author, instants, digest and cursor are exported",
        ),
    ];
    RedactionSummary {
        excluded_tables: excluded_tables
            .into_iter()
            .map(|(table, reason)| (table.to_owned(), reason.to_owned()))
            .collect(),
        excluded_columns: excluded_columns
            .into_iter()
            .map(|(column, reason)| (column.to_owned(), reason.to_owned()))
            .collect(),
        canary_scanned: true,
    }
}

/// One exported record's source identity and digest.
///
/// This is what an import records as lineage: enough to say *which* source
/// record a destination row came from, and to prove the bytes have not changed
/// since, without carrying the record itself into the destination's authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordLineage {
    /// The record kind, which is the source table's name.
    pub kind: &'static str,
    /// The record's primary key, rendered as text.
    pub identity: String,
    /// SHA-256 over the record's canonical JSON.
    pub hash: ContentHash,
}

/// One exported table's contract.
trait ExportRow: Sized + Serialize {
    /// The source table, which is also the record kind.
    const KIND: &'static str;

    /// The `SELECT` this table is read with: explicit columns, explicit order.
    fn query() -> String;

    /// Read one row, by column position in [`ExportRow::query`].
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self>;

    /// The record's primary key, rendered as text.
    fn identity(&self) -> String;

    /// This record's lineage entry.
    fn lineage(&self) -> Result<RecordLineage, BackupError> {
        Ok(RecordLineage {
            kind: Self::KIND,
            identity: self.identity(),
            hash: ContentHash::of(&canonical_bytes(&canonical_value(self)?)?),
        })
    }
}

/// Read one whole table in its declared order.
fn read_table<T: ExportRow>(connection: &Connection) -> Result<Vec<T>, BackupError> {
    let query = T::query();
    let mut statement = connection
        .prepare(&query)
        .map_err(|source| BackupError::Store(source.into()))?;
    let mut rows = statement
        .query([])
        .map_err(|source| BackupError::Store(source.into()))?;
    let mut records = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|source| BackupError::Store(source.into()))?
    {
        records.push(T::read(row).map_err(|source| BackupError::Store(source.into()))?);
    }
    Ok(records)
}

/// Declare every exported table: its struct, its columns, its order and its key.
///
/// The macro exists to make one thing impossible: a table exported with columns
/// that were never written down. Every column below is named three times — as a
/// struct field, as a `SELECT` column and as a read position — from one
/// declaration, so they cannot drift apart, and adding a column is a deliberate
/// edit here rather than a consequence of a migration elsewhere.
macro_rules! exported_tables {
    ($(
        $field:ident : $row:ident from $table:literal key($($key:ident),+ ) {
            $($column:ident : $type:ty,)+
        }
    )+) => {
        $(
            #[doc = concat!("One exported row of `", $table, "`.")]
            #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
            pub struct $row {
                $(
                    #[doc = concat!("The `", stringify!($column), "` column.")]
                    pub $column: $type,
                )+
            }

            impl ExportRow for $row {
                const KIND: &'static str = $table;

                fn query() -> String {
                    format!(
                        "SELECT {} FROM {} ORDER BY {}",
                        [$(stringify!($column)),+].join(", "),
                        $table,
                        [$(stringify!($key)),+].join(", "),
                    )
                }

                fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
                    let mut index = 0;
                    $(
                        let $column: $type = row.get(index)?;
                        index += 1;
                    )+
                    let _ = index;
                    Ok(Self { $($column,)+ })
                }

                fn identity(&self) -> String {
                    [$(self.$key.to_string()),+].join("/")
                }
            }
        )+

        /// Every exported record, by kind.
        ///
        /// The field order here is the document's array order, and it follows
        /// the dependency order of the schema — identity, then work, then the
        /// evidence about that work — so a reader meets a record's owner before
        /// it meets the record.
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        pub struct ExportedRecords {
            $(
                #[doc = concat!("Every exported row of `", $table, "`.")]
                pub $field: Vec<$row>,
            )+
        }

        impl ExportedRecords {
            /// Read every exported table from one connection.
            fn read(connection: &Connection) -> Result<Self, BackupError> {
                Ok(Self {
                    $( $field: read_table(connection)?, )+
                })
            }

            /// How many records of each kind this document carries.
            #[must_use]
            pub fn record_counts(&self) -> RecordCounts {
                let mut counts = RecordCounts::new();
                $(
                    counts.insert($table.to_owned(), self.$field.len() as u64);
                )+
                counts
            }

            /// Every record's source identity and digest, in document order.
            ///
            /// # Errors
            /// Returns [`BackupError::Verification`] when a record cannot be
            /// rendered as JSON, which cannot happen for these types.
            pub fn lineage(&self) -> Result<Vec<RecordLineage>, BackupError> {
                let mut lineage = Vec::new();
                $(
                    for record in &self.$field {
                        lineage.push(ExportRow::lineage(record)?);
                    }
                )+
                Ok(lineage)
            }
        }
    };
}

exported_tables! {
    realm_metadata: RealmMetadataRow from "realm_metadata" key(realm_id) {
        realm_id: String,
        schema_version: i64,
        created_at: String,
        display_label: Option<String>,
    }
    projects: ProjectsRow from "projects" key(id) {
        id: String,
        name: String,
        root_path: String,
        revision: i64,
        created_at: String,
    }
    mini_projects: MiniProjectsRow from "mini_projects" key(id) {
        id: String,
        project_id: String,
        name: String,
        revision: i64,
        created_at: String,
    }
    tasks: TasksRow from "tasks" key(id) {
        id: String,
        project_id: String,
        mini_project_id: Option<String>,
        title: String,
        module_key: Option<String>,
        state: String,
        revision: i64,
        created_at: String,
        updated_at: String,
    }
    task_dependencies: TaskDependenciesRow from "task_dependencies" key(project_id, task_id, depends_on_task_id) {
        project_id: String,
        task_id: String,
        depends_on_task_id: String,
        created_at: String,
    }
    work_profiles: WorkProfilesRow from "work_profiles" key(project_id, profile_key, version) {
        project_id: String,
        profile_key: String,
        version: i64,
        definition: String,
        definition_hash: String,
        created_at: String,
    }
    task_workflows: TaskWorkflowsRow from "task_workflows" key(id) {
        id: String,
        project_id: String,
        task_id: String,
        profile_key: String,
        profile_version: i64,
        snapshot: String,
        snapshot_hash: String,
        current_phase: String,
        active: i64,
        revision: i64,
        created_at: String,
    }
    task_gate_evaluations: TaskGateEvaluationsRow from "task_gate_evaluations" key(project_id, workflow_id, gate_key, sequence) {
        project_id: String,
        workflow_id: String,
        gate_key: String,
        sequence: i64,
        verdict: String,
        evaluator_role: String,
        evaluator_account: String,
        evidence: String,
        recorded_at: String,
        agent_run_id: Option<String>,
        reviewer_principal: Option<String>,
        policy_evaluation_id: Option<String>,
    }
    artifact_evidence: ArtifactEvidenceRow from "artifact_evidence" key(id) {
        id: String,
        project_id: String,
        task_id: String,
        workflow_id: String,
        agent_run_id: Option<String>,
        artifact_key: String,
        locator: String,
        locator_hash: String,
        producer_role: String,
        producer_account: String,
        recorded_at: String,
    }
    gate_waivers: GateWaiversRow from "gate_waivers" key(id) {
        id: String,
        project_id: String,
        workflow_id: String,
        gate_key: String,
        sequence: i64,
        authorizing_role: String,
        authorizing_account: String,
        reason: String,
        evidence: String,
        evidence_hash: String,
        recorded_at: String,
    }
    team_templates: TeamTemplatesRow from "team_templates" key(project_id, template_id, version) {
        project_id: String,
        template_id: String,
        version: i64,
        name: String,
        definition: String,
        definition_hash: String,
        role_authority: String,
        created_at: String,
    }
    team_runs: TeamRunsRow from "team_runs" key(id) {
        id: String,
        project_id: String,
        task_id: String,
        template_id: String,
        template_version: i64,
        snapshot: String,
        snapshot_hash: String,
        lifecycle: String,
        terminal_outcome: Option<String>,
        terminal_source_kind: Option<String>,
        terminal_receipt_id: Option<String>,
        terminal_evidence_hash: Option<String>,
        closed_at: Option<String>,
        revision: i64,
        created_at: String,
    }
    agent_runs: AgentRunsRow from "agent_runs" key(id) {
        id: String,
        project_id: String,
        team_run_id: String,
        parent_agent_run_id: Option<String>,
        role_key: String,
        account_profile_id: Option<String>,
        lifecycle: String,
        desired_state: String,
        observed_state: String,
        derived_state: String,
        last_confirmed_at: Option<String>,
        last_cursor: Option<i64>,
        last_native_sequence: Option<i64>,
        terminal_outcome: Option<String>,
        terminal_source_kind: Option<String>,
        terminal_event_cursor: Option<i64>,
        terminal_receipt_id: Option<String>,
        terminal_evidence_hash: Option<String>,
        closed_at: Option<String>,
        revision: i64,
        created_at: String,
    }
    persona_scenarios: PersonaScenariosRow from "persona_scenarios" key(project_id, scenario_id, version) {
        project_id: String,
        scenario_id: String,
        version: i64,
        persona_key: String,
        gate_key: String,
        definition: String,
        definition_hash: String,
        created_at: String,
    }
    task_persona_snapshots: TaskPersonaSnapshotsRow from "task_persona_snapshots" key(project_id, task_id, scenario_id, version) {
        project_id: String,
        task_id: String,
        scenario_id: String,
        version: i64,
        workflow_id: String,
        gate_key: String,
        snapshot: String,
        snapshot_hash: String,
        created_at: String,
    }
    trigger_specs: TriggerSpecsRow from "trigger_specs" key(project_id, trigger_key, version) {
        project_id: String,
        trigger_key: String,
        version: i64,
        source_kind: String,
        source_connection: String,
        work_profile_key: String,
        work_profile_version: i64,
        team_template_id: String,
        team_template_version: i64,
        context_template: String,
        context_version: i64,
        calendar_profile_id: Option<String>,
        calendar_version: Option<i64>,
        definition: String,
        definition_hash: String,
        created_at: String,
    }
    source_events: SourceEventsRow from "source_events" key(id) {
        id: String,
        project_id: String,
        source_kind: String,
        source_connection: String,
        external_event_id: String,
        envelope: String,
        envelope_hash: String,
        external_observed_at: String,
        ingested_at: String,
        processing_state: String,
    }
    intake_receipts: IntakeReceiptsRow from "intake_receipts" key(id) {
        id: String,
        project_id: String,
        source_event_id: String,
        source_event_hash: String,
        trigger_key: String,
        trigger_version: i64,
        result: String,
        receipt: String,
        idempotency_key: String,
        dedup_key: String,
        duplicate_of: Option<String>,
        predecessor_receipt_id: Option<String>,
        decided_at: String,
    }
    jira_links: JiraLinksRow from "jira_links" key(id) {
        id: String,
        project_id: String,
        task_id: String,
        connector: String,
        external_issue_key: String,
        revision: i64,
        created_at: String,
    }
    ticket_field_specs: TicketFieldSpecsRow from "ticket_field_specs" key(project_id, connector, external_project, issue_type, version) {
        project_id: String,
        connector: String,
        external_project: String,
        issue_type: String,
        version: i64,
        definition: String,
        definition_hash: String,
        created_at: String,
    }
    external_workflow_specs: ExternalWorkflowSpecsRow from "external_workflow_specs" key(project_id, connector, external_project, issue_type, version) {
        project_id: String,
        connector: String,
        external_project: String,
        issue_type: String,
        version: i64,
        work_profile_key: Option<String>,
        work_profile_version: Option<i64>,
        definition: String,
        definition_hash: String,
        created_at: String,
    }
    ticket_sync_projections: TicketSyncProjectionsRow from "ticket_sync_projections" key(id) {
        id: String,
        project_id: String,
        link_id: String,
        link_revision: i64,
        connector: String,
        field_spec_project: String,
        field_spec_issue_type: String,
        field_spec_version: i64,
        external_issue_key: String,
        fields: String,
        comment_policy: String,
        external_comment_cursor: Option<String>,
        projection_hash: String,
        computed_at: String,
    }
    external_ticket_observations: ExternalTicketObservationsRow from "external_ticket_observations" key(id) {
        id: String,
        project_id: String,
        link_id: String,
        status_id: String,
        status_name: String,
        status_category: String,
        issue_type: String,
        assignee_account_id: Option<String>,
        assignee_display: Option<String>,
        external_version: Option<String>,
        observed_at: String,
        payload_hash: String,
    }
    status_conflicts: StatusConflictsRow from "status_conflicts" key(id) {
        id: String,
        project_id: String,
        link_id: String,
        kind: String,
        observation_id: String,
        task_revision: i64,
        spec_version: i64,
        milestone: Option<String>,
        detected_at: String,
        resolved_at: Option<String>,
        resolution_receipt_id: Option<String>,
    }
    status_transition_receipts: StatusTransitionReceiptsRow from "status_transition_receipts" key(id) {
        id: String,
        project_id: String,
        link_id: String,
        task_id: String,
        task_revision: i64,
        workflow_revision: i64,
        projection_revision: i64,
        spec_version: i64,
        prior_observation_id: String,
        milestone: String,
        target_status_id: String,
        transition_id: Option<String>,
        principal_account_id: String,
        assignment_prerequisite: i64,
        assignment_result: Option<String>,
        plan: String,
        idempotency_key: String,
        dispatched_at: String,
        acknowledged_at: Option<String>,
        confirmed_at: Option<String>,
        refetched_observation_id: Option<String>,
    }
    external_comments: ExternalCommentsRow from "external_comments" key(project_id, link_id, external_comment_id, body_hash) {
        project_id: String,
        link_id: String,
        external_comment_id: String,
        body_hash: String,
        author_account_id: String,
        author_display: Option<String>,
        external_created_at: String,
        external_updated_at: String,
        observed_at: String,
        supersedes_hash: Option<String>,
    }
    account_profiles: AccountProfilesRow from "account_profiles" key(id) {
        id: String,
        project_id: String,
        label: String,
        external_account_id: Option<String>,
        created_at: String,
        harness: Option<String>,
        routing: Option<String>,
        routing_hash: Option<String>,
        capability: Option<String>,
        capability_hash: Option<String>,
        provider_identity: Option<String>,
        enabled: Option<i64>,
        revision: Option<i64>,
        updated_at: Option<String>,
    }
    runtime_bindings: RuntimeBindingsRow from "runtime_bindings" key(id) {
        id: String,
        project_id: String,
        agent_run_id: String,
        runtime_kind: String,
        host: String,
        generation: i64,
        native_id: String,
        bound_at: String,
    }
    command_receipts: CommandReceiptsRow from "command_receipts" key(id) {
        id: String,
        project_id: String,
        idempotency_key: String,
        kind: String,
        target: String,
        target_revision: i64,
        intent: String,
        intent_hash: String,
        state: String,
        correlation: Option<String>,
        native_identity: Option<String>,
        result_ref: Option<String>,
        attempts: i64,
        created_at: String,
        updated_at: String,
    }
    command_receipt_transitions: CommandReceiptTransitionsRow from "command_receipt_transitions" key(project_id, receipt_id, sequence) {
        project_id: String,
        receipt_id: String,
        sequence: i64,
        state: String,
        correlation: Option<String>,
        native_identity: Option<String>,
        evidence_ref: Option<String>,
        recorded_at: String,
    }
    command_targets: CommandTargetsRow from "command_targets" key(project_id, receipt_id) {
        project_id: String,
        receipt_id: String,
        target_kind: String,
        target_project_id: Option<String>,
        target_mini_project_id: Option<String>,
        target_task_id: Option<String>,
        target_team_run_id: Option<String>,
        target_agent_run_id: Option<String>,
        target_ticket_link_id: Option<String>,
        target_work_calendar_id: Option<String>,
    }
    command_outbox: CommandOutboxRow from "command_outbox" key(receipt_id) {
        receipt_id: String,
        project_id: String,
        payload: String,
        payload_hash: String,
        not_before: String,
        claimed_at: Option<String>,
        dispatched_at: Option<String>,
        attempts: i64,
    }
    runtime_events: RuntimeEventsRow from "runtime_events" key(cursor) {
        cursor: i64,
        project_id: String,
        event_kind: String,
        agent_run_id: Option<String>,
        runtime_kind: Option<String>,
        host: Option<String>,
        generation: Option<i64>,
        native_id: Option<String>,
        native_event_id: Option<String>,
        native_sequence: Option<i64>,
        observed_state: Option<String>,
        contact: Option<String>,
        freshness: Option<String>,
        audit_ref: Option<String>,
        command_receipt_id: Option<String>,
        payload: String,
        payload_hash: String,
        observed_at: String,
        recorded_at: String,
    }
    runtime_replay_consumers: RuntimeReplayConsumersRow from "runtime_replay_consumers" key(project_id, consumer_key) {
        project_id: String,
        consumer_key: String,
        last_cursor: i64,
        updated_at: String,
    }
    runtime_control_gaps: RuntimeControlGapsRow from "runtime_control_gaps" key(id) {
        id: String,
        project_id: String,
        agent_run_id: String,
        runtime_kind: String,
        host: String,
        generation: i64,
        native_id: String,
        expected_sequence: i64,
        received_sequence: i64,
        detected_cursor: i64,
        audit_ref: String,
        detected_at: String,
    }
    runtime_content_gaps: RuntimeContentGapsRow from "runtime_content_gaps" key(id) {
        id: String,
        project_id: String,
        agent_run_id: String,
        content_epoch: i64,
        expected_content_sequence: i64,
        received_content_sequence: i64,
        detected_cursor: i64,
        audit_ref: String,
        detected_at: String,
    }
    recovery_episodes: RecoveryEpisodesRow from "recovery_episodes" key(id) {
        id: String,
        project_id: String,
        task_id: String,
        workflow_id: String,
        parked_agent_run_id: String,
        status: String,
        cause_evaluation_id: String,
        advisor_used: i64,
        committee_used: i64,
        effective_followups: i64,
        successor_agent_run_id: Option<String>,
        escalation_cause: Option<String>,
        revision: i64,
        created_at: String,
        closed_at: Option<String>,
    }
    recovery_steps: RecoveryStepsRow from "recovery_steps" key(project_id, episode_id, sequence) {
        project_id: String,
        episode_id: String,
        sequence: i64,
        kind: String,
        input_hash: String,
        output_hash: Option<String>,
        agent_run_id: Option<String>,
        policy_evaluation_id: Option<String>,
        artifact_evidence_id: Option<String>,
        recorded_at: String,
    }
    approval_receipts: ApprovalReceiptsRow from "approval_receipts" key(id) {
        id: String,
        project_id: String,
        scope_kind: String,
        task_id: Option<String>,
        action_domain: String,
        action_intent: String,
        action_effect: String,
        action_digest: String,
        approver_principal: String,
        approver_role: String,
        approver_account: String,
        authority_source: String,
        evidence: String,
        evidence_hash: String,
        issued_at: String,
        expires_at: String,
        consumed_at: Option<String>,
    }
    guardrail_evaluations: GuardrailEvaluationsRow from "guardrail_evaluations" key(id) {
        id: String,
        project_id: String,
        agent_run_id: String,
        rung: i64,
        verdict: String,
        evidence: String,
        evidence_hash: String,
        recorded_at: String,
    }
    policy_evaluations: PolicyEvaluationsRow from "policy_evaluations" key(id) {
        id: String,
        project_id: String,
        task_id: String,
        workflow_id: String,
        team_run_id: Option<String>,
        agent_run_id: Option<String>,
        rule_key: String,
        rule_version: i64,
        subject_kind: String,
        subject_id: String,
        inputs: String,
        inputs_hash: String,
        verdict: String,
        reason_code: String,
        evidence_refs: String,
        recorded_at: String,
    }
    run_park_closures: RunParkClosuresRow from "run_park_closures" key(project_id, agent_run_id) {
        project_id: String,
        agent_run_id: String,
        team_run_id: Option<String>,
        policy_evaluation_id: String,
        recovery_episode_id: String,
        closure_receipt_id: String,
        reason_code: String,
        evidence_hash: String,
        recorded_at: String,
    }
    calendar_profiles: CalendarProfilesRow from "calendar_profiles" key(profile_id, version) {
        profile_id: String,
        version: i64,
        name: String,
        definition: String,
        definition_hash: String,
        created_at: String,
    }
    work_calendars: WorkCalendarsRow from "work_calendars" key(id) {
        id: String,
        project_id: String,
        profile_id: String,
        profile_version: i64,
        timezone: String,
        window_override: Option<String>,
        active: i64,
        created_at: String,
        retired_at: Option<String>,
    }
    holiday_sources: HolidaySourcesRow from "holiday_sources" key(id) {
        id: String,
        profile_id: String,
        profile_version: i64,
        provider: String,
        country: String,
        subdivision: Option<String>,
        reference: String,
        range_start: String,
        range_end: String,
        retrieved_at: String,
        raw_hash: String,
        normalized_hash: String,
    }
    calendar_exceptions: CalendarExceptionsRow from "calendar_exceptions" key(id) {
        id: String,
        project_id: String,
        work_calendar_id: String,
        start_date: String,
        end_date: String,
        kind: String,
        label: String,
        provenance: String,
        supersedes: Option<String>,
        created_at: String,
    }
    execution_authorizations: ExecutionAuthorizationsRow from "execution_authorizations" key(id) {
        id: String,
        project_id: String,
        scope_kind: String,
        scope_mini_project_id: Option<String>,
        scope_task_id: Option<String>,
        selected_tasks: String,
        allowed_start: String,
        allowed_end: String,
        max_concurrency: i64,
        max_tokens: i64,
        max_commands: i64,
        max_duration_seconds: i64,
        max_cost_minor_units: i64,
        cost_currency: String,
        created_by: String,
        capability_receipt_id: String,
        created_at: String,
    }
    execution_authorization_tasks: ExecutionAuthorizationTasksRow from "execution_authorization_tasks" key(project_id, authorization_id, task_id) {
        project_id: String,
        authorization_id: String,
        task_id: String,
    }
    schedule_overrides: ScheduleOverridesRow from "schedule_overrides" key(id) {
        id: String,
        project_id: String,
        scope_kind: String,
        scope_mini_project_id: Option<String>,
        scope_task_id: Option<String>,
        reason: String,
        start_at: String,
        expiry_kind: String,
        expiry_at: Option<String>,
        expiry_mini_project_id: Option<String>,
        hard_ceiling: String,
        max_concurrency: i64,
        max_tokens: i64,
        max_commands: i64,
        max_duration_seconds: i64,
        max_cost_minor_units: i64,
        cost_currency: String,
        approved_by: String,
        approval_receipt_id: String,
        created_at: String,
        revoked_at: Option<String>,
        revoked_by: Option<String>,
        revocation_receipt_id: Option<String>,
    }
    scheduler_admission_events: SchedulerAdmissionEventsRow from "scheduler_admission_events" key(id) {
        id: String,
        project_id: String,
        task_id: String,
        decision: String,
        rejection_code: Option<String>,
        team_run_id: Option<String>,
        agent_run_id: Option<String>,
        launch_receipt_id: Option<String>,
        authorization_id: Option<String>,
        evidence: String,
        evidence_hash: String,
        decided_at: String,
    }
    runtime_reconciliation_epochs: RuntimeReconciliationEpochsRow from "runtime_reconciliation_epochs" key(epoch_id) {
        epoch_id: String,
        project_id: String,
        runtime_kind: String,
        host: String,
        generation: i64,
        reconciliation_key: String,
        census_start_cursor: i64,
        completion_cursor: Option<i64>,
        started_at: String,
        completed_at: Option<String>,
        status: String,
    }
    runtime_reconciliation_members: RuntimeReconciliationMembersRow from "runtime_reconciliation_members" key(project_id, epoch_id, native_id) {
        project_id: String,
        epoch_id: String,
        native_id: String,
        agent_run_id: Option<String>,
        observation_cursor: i64,
        observed_state: String,
        recorded_at: String,
    }
    runtime_reconciliation_results: RuntimeReconciliationResultsRow from "runtime_reconciliation_results" key(project_id, epoch_id, agent_run_id) {
        project_id: String,
        epoch_id: String,
        agent_run_id: String,
        outcome: String,
        source_revision: i64,
        resulting_revision: i64,
        source_cursor: Option<i64>,
        recorded_at: String,
    }
    context_packs: ContextPacksRow from "context_packs" key(id) {
        id: String,
        project_id: String,
        task_id: String,
        content: String,
        content_hash: String,
        created_at: String,
    }
    handoffs: HandoffsRow from "handoffs" key(id) {
        id: String,
        project_id: String,
        workflow_id: String,
        from_phase: String,
        to_phase: String,
        context_pack_id: Option<String>,
        payload: String,
        created_at: String,
    }
}

impl ExportedRecords {
    /// What this document says about its own completeness.
    ///
    /// Everything is counted from the records themselves. A summary computed by
    /// a second query could disagree with the records it describes; one
    /// computed from them cannot.
    #[must_use]
    pub fn continuity(&self) -> ContinuitySummary {
        let settled = ["succeeded", "failed", "cancelled", "superseded"];
        ContinuitySummary {
            record_counts: self.record_counts(),
            control_gaps: self.runtime_control_gaps.len() as u64,
            content_gaps: self.runtime_content_gaps.len() as u64,
            unsettled_command_receipts: self
                .command_receipts
                .iter()
                .filter(|receipt| !settled.contains(&receipt.state.as_str()))
                .count() as u64,
            incomplete_reconciliation_epochs: self
                .runtime_reconciliation_epochs
                .iter()
                .filter(|epoch| epoch.completed_at.is_none())
                .count() as u64,
            unconfirmed_agent_runs: self
                .agent_runs
                .iter()
                .filter(|run| run.last_confirmed_at.is_none() && run.closed_at.is_none())
                .count() as u64,
            highest_control_cursor: self
                .runtime_events
                .iter()
                .map(|event| event.cursor)
                .max()
                .unwrap_or_default(),
        }
    }
}
