//! Taking another Realm's export, without taking its authority.
//!
//! An import is not a restore, and almost everything in this module exists to
//! keep the two from becoming the same operation by accident:
//!
//! * the destination is a **separately initialized** Realm — an export of this
//!   Realm is refused here (that is a restore) and so is an uninitialized
//!   destination (there is no project to own the records);
//! * the operator's intent is a value that has to be constructed and names the
//!   destination project, so an import cannot be reached as a fallback from a
//!   failed restore;
//! * every source id stays a **reference**. Nothing is looked up by it, nothing
//!   is written under it, and no destination row is keyed by it;
//! * the only records that become destination *state* are versioned
//!   specifications, and they get there by being re-validated through this
//!   build's own domain types and re-canonicalized — if the destination's
//!   digest does not match the source's, the record is refused rather than
//!   trusted;
//! * every other record is recorded as lineage. A source command receipt,
//!   status-transition receipt or dispatch row is evidence that something
//!   happened *there*, and writing it into this Realm's receipt tables would
//!   put an effect that already happened in front of a dispatcher that has
//!   never seen it.
//!
//! What an import therefore never produces: a live lease, an active dispatch
//! claim, a credential binding, a runtime session this Realm may address, or a
//! receipt a scheduler would act on. Imported evidence begins stale by
//! construction — there is no confirmation in it that this Realm made — and the
//! destination's own reconciliation is what makes anything fresh again.

use kontor_core::calendar::CalendarProfileSpec;
use kontor_core::id::{
    CanonicalDocument, ContentHash, ExternalName, ProjectId, SpecVersion, TeamTemplateId,
    Timestamp, format_utc_timestamp,
};
use kontor_core::repository::{ProjectRepository, RepositoryError, SpecRepository};
use kontor_core::spec::{PersonaScenarioSpec, TeamTemplateRevision, TriggerSpec, WorkProfileSpec};
use kontor_core::ticket::{ExternalWorkflowSpec, TicketFieldSpec};
use rusqlite::params;
use uuid::Uuid;

use crate::SqliteStore;
use crate::backup::BackupError;
use crate::backup::export::{KontorExportV1, RecordLineage};

/// The operator's explicit intent to import a foreign export.
///
/// It is a constructed value rather than a boolean flag because of what it
/// authorizes: taking records that another Realm is the authority for and
/// giving them a life in this one. A `true` passed to a function with several
/// other arguments is not a decision anybody made; naming the destination
/// project in the intent is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportPlan {
    destination_project: ProjectId,
}

impl ImportPlan {
    /// Authorize a redacted import into one destination project.
    #[must_use]
    pub const fn redacted_import_into(destination_project: ProjectId) -> Self {
        Self {
            destination_project,
        }
    }

    /// The project the import is authorized for.
    #[must_use]
    pub const fn destination_project(&self) -> ProjectId {
        self.destination_project
    }
}

/// What one import did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    /// The destination receipt this import minted.
    pub import_id: Uuid,
    /// The Realm the records came from, as a reference.
    pub source_realm_id: kontor_core::id::RealmId,
    /// The destination project the records were imported into.
    pub destination_project: ProjectId,
    /// How many source records the document carried.
    pub record_count: u64,
    /// How many became destination specifications.
    pub materialized: u64,
    /// How many the destination already had at that version.
    pub already_present: u64,
    /// How many were recorded as lineage and are deliberately not executable
    /// here.
    pub recorded: u64,
    /// How many this build refused, with the reason recorded per record.
    pub refused: u64,
    /// Always true, and stated rather than implied: nothing imported carries a
    /// confirmation this Realm made, so every imported run, binding and
    /// observation is stale until the destination's own reconciliation says
    /// otherwise.
    pub reconciliation_required: bool,
}

/// What an import did about one source record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disposition {
    /// Re-validated and inserted as a destination specification.
    Materialized,
    /// The destination already had it at that version; its own revision stands.
    AlreadyPresent,
    /// Kept as source-referenced lineage, deliberately not executable here.
    Recorded,
    /// Refused, with a stable reason code.
    Refused(&'static str),
}

impl Disposition {
    const fn column(self) -> &'static str {
        match self {
            Self::Materialized => "materialized",
            Self::AlreadyPresent => "already_present",
            Self::Recorded => "recorded",
            Self::Refused(_) => "refused",
        }
    }

    const fn reason(self) -> Option<&'static str> {
        match self {
            Self::Refused(code) => Some(code),
            _ => None,
        }
    }
}

/// Import a redacted export into a separately initialized destination Realm.
///
/// # Errors
/// Returns [`BackupError::UnsupportedExportVersion`] for another export
/// generation, [`BackupError::Verification`] when the document's digest does not
/// match its records, [`BackupError::SameRealmImport`] when the document is this
/// Realm's own export, and [`BackupError::Repository`] when the destination
/// project does not exist or the same export has already been imported into it.
/// Nothing is written when any of them is raised.
pub fn import_export(
    store: &SqliteStore,
    export: &KontorExportV1,
    plan: &ImportPlan,
    now: Timestamp,
) -> Result<ImportReport, BackupError> {
    // 1. The document is judged before the destination is touched: generation,
    //    digest, then provenance.
    export.verify()?;
    if export.source_realm_id == store.realm_id() {
        return Err(BackupError::SameRealmImport {
            realm_id: store.realm_id(),
        });
    }
    let destination = plan.destination_project();
    if store.get_project(destination)?.is_none() {
        return Err(BackupError::Repository(RepositoryError::NotFound {
            subject: "destination project",
        }));
    }

    // 2. Lineage first, and for every record: an import that materializes
    //    nothing still has to be able to say what it saw.
    let lineage = export.records.lineage()?;
    let mut dispositions: Vec<(RecordLineage, Disposition)> = lineage
        .into_iter()
        .map(|record| (record, Disposition::Recorded))
        .collect();

    // 3. Materialize the versioned specifications, in dependency order. Each one
    //    is re-validated through this build's own domain types and
    //    re-canonicalized; a document that does not reproduce its source digest
    //    is refused rather than written.
    materialize(store, export, destination, &mut dispositions)?;

    // 4. The destination's own receipt, and the lineage under it.
    let import_id = Uuid::now_v7();
    record_import(store, export, plan, now, import_id, &dispositions)?;

    let count = |wanted: &dyn Fn(Disposition) -> bool| -> u64 {
        dispositions
            .iter()
            .filter(|(_, disposition)| wanted(*disposition))
            .count() as u64
    };
    Ok(ImportReport {
        import_id,
        source_realm_id: export.source_realm_id,
        destination_project: destination,
        record_count: dispositions.len() as u64,
        materialized: count(&|d| matches!(d, Disposition::Materialized)),
        already_present: count(&|d| matches!(d, Disposition::AlreadyPresent)),
        recorded: count(&|d| matches!(d, Disposition::Recorded)),
        refused: count(&|d| matches!(d, Disposition::Refused(_))),
        reconciliation_required: true,
    })
}

/// Insert one specification and classify the outcome.
///
/// A primary-key conflict is `already_present` rather than a failure: the
/// destination's own revision of a specification is never overwritten by an
/// import, and re-running an interrupted import must converge rather than
/// refuse.
fn classify(outcome: Result<ContentHash, RepositoryError>, source: &ContentHash) -> Disposition {
    match outcome {
        Ok(hash) if &hash == source => Disposition::Materialized,
        // The destination re-canonicalized the same specification to different
        // bytes. That is this build disagreeing with the source about what the
        // document *means*, and taking it anyway is how a silently different
        // specification ends up pinned to real work.
        Ok(_) => Disposition::Refused("destination_digest_differs"),
        Err(RepositoryError::Conflict { .. }) => Disposition::AlreadyPresent,
        Err(_) => Disposition::Refused("destination_refused"),
    }
}

/// Re-validate a stored document and hand back the typed specification.
fn stored<T: for<'de> serde::Deserialize<'de>>(
    definition: &str,
    hash: &str,
) -> Result<(T, ContentHash), BackupError> {
    let digest = ContentHash::parse(hash)?;
    let document = CanonicalDocument::from_stored(definition, &digest)?;
    Ok((document.deserialize::<T>()?, digest))
}

/// Materialize every versioned specification the export carries.
///
/// The order is the dependency order of the schema — calendars and profiles
/// before the triggers that pin them — so a specification's references already
/// exist when it lands.
fn materialize(
    store: &SqliteStore,
    export: &KontorExportV1,
    destination: ProjectId,
    dispositions: &mut [(RecordLineage, Disposition)],
) -> Result<(), BackupError> {
    let records = &export.records;

    for row in &records.calendar_profiles {
        let identity = format!("{}/{}", row.profile_id, row.version);
        let outcome = match stored::<CalendarProfileSpec>(&row.definition, &row.definition_hash) {
            Ok((spec, digest)) => classify(store.insert_calendar_profile(&spec), &digest),
            Err(_) => Disposition::Refused("source_document_invalid"),
        };
        set(dispositions, "calendar_profiles", &identity, outcome);
    }

    for row in &records.work_profiles {
        let identity = format!("{}/{}/{}", row.project_id, row.profile_key, row.version);
        let outcome = match stored::<WorkProfileSpec>(&row.definition, &row.definition_hash) {
            Ok((spec, digest)) => classify(store.insert_work_profile(destination, &spec), &digest),
            Err(_) => Disposition::Refused("source_document_invalid"),
        };
        set(dispositions, "work_profiles", &identity, outcome);
    }

    for row in &records.team_templates {
        let identity = format!("{}/{}/{}", row.project_id, row.template_id, row.version);
        let outcome = materialize_team_template(store, destination, row);
        set(dispositions, "team_templates", &identity, outcome);
    }

    for row in &records.persona_scenarios {
        let identity = format!("{}/{}/{}", row.project_id, row.scenario_id, row.version);
        let outcome = match stored::<PersonaScenarioSpec>(&row.definition, &row.definition_hash) {
            Ok((spec, digest)) => {
                classify(store.insert_persona_scenario(destination, &spec), &digest)
            }
            Err(_) => Disposition::Refused("source_document_invalid"),
        };
        set(dispositions, "persona_scenarios", &identity, outcome);
    }

    for row in &records.ticket_field_specs {
        let identity = format!(
            "{}/{}/{}/{}/{}",
            row.project_id, row.connector, row.external_project, row.issue_type, row.version
        );
        let outcome = match stored::<TicketFieldSpec>(&row.definition, &row.definition_hash) {
            Ok((spec, digest)) => {
                classify(store.insert_ticket_field_spec(destination, &spec), &digest)
            }
            Err(_) => Disposition::Refused("source_document_invalid"),
        };
        set(dispositions, "ticket_field_specs", &identity, outcome);
    }

    for row in &records.external_workflow_specs {
        let identity = format!(
            "{}/{}/{}/{}/{}",
            row.project_id, row.connector, row.external_project, row.issue_type, row.version
        );
        let outcome = match stored::<ExternalWorkflowSpec>(&row.definition, &row.definition_hash) {
            Ok((spec, digest)) => classify(
                store.insert_external_workflow_spec(destination, &spec),
                &digest,
            ),
            Err(_) => Disposition::Refused("source_document_invalid"),
        };
        set(dispositions, "external_workflow_specs", &identity, outcome);
    }

    for row in &records.trigger_specs {
        let identity = format!("{}/{}/{}", row.project_id, row.trigger_key, row.version);
        let outcome = match stored::<TriggerSpec>(&row.definition, &row.definition_hash) {
            Ok((spec, digest)) => classify(store.insert_trigger_spec(destination, &spec), &digest),
            Err(_) => Disposition::Refused("source_document_invalid"),
        };
        set(dispositions, "trigger_specs", &identity, outcome);
    }

    Ok(())
}

/// A team template is the one specification whose row carries fields beside its
/// document, so it is rebuilt rather than deserialized whole.
fn materialize_team_template(
    store: &SqliteStore,
    destination: ProjectId,
    row: &crate::backup::export::TeamTemplatesRow,
) -> Disposition {
    let Ok(digest) = ContentHash::parse(&row.definition_hash) else {
        return Disposition::Refused("source_document_invalid");
    };
    let Ok(definition) = CanonicalDocument::from_stored(&row.definition, &digest) else {
        return Disposition::Refused("source_document_invalid");
    };
    let Ok(template_id) = TeamTemplateId::parse(&row.template_id) else {
        return Disposition::Refused("source_document_invalid");
    };
    let Ok(version) = u32::try_from(row.version)
        .map_err(|_| ())
        .and_then(|value| SpecVersion::parse(value).map_err(|_| ()))
    else {
        return Disposition::Refused("source_document_invalid");
    };
    let Ok(name) = ExternalName::parse(&row.name) else {
        return Disposition::Refused("source_document_invalid");
    };
    let Ok(role_authority) = serde_json::from_str(&row.role_authority) else {
        return Disposition::Refused("source_document_invalid");
    };
    let revision = TeamTemplateRevision {
        template_id,
        version,
        name,
        definition,
        role_authority,
    };
    classify(store.insert_team_template(destination, &revision), &digest)
}

/// Record what happened to one source record, by kind and source identity.
fn set(
    dispositions: &mut [(RecordLineage, Disposition)],
    kind: &str,
    identity: &str,
    outcome: Disposition,
) {
    if let Some(entry) = dispositions
        .iter_mut()
        .find(|(record, _)| record.kind == kind && record.identity == identity)
    {
        entry.1 = outcome;
    }
}

/// Mint the destination receipt and write the lineage under it, in one
/// transaction.
///
/// One transaction on purpose: an import receipt whose lineage is half written
/// would claim a provenance it cannot show.
fn record_import(
    store: &SqliteStore,
    export: &KontorExportV1,
    plan: &ImportPlan,
    now: Timestamp,
    import_id: Uuid,
    dispositions: &[(RecordLineage, Disposition)],
) -> Result<(), BackupError> {
    let materialized = dispositions
        .iter()
        .filter(|(_, disposition)| matches!(disposition, Disposition::Materialized))
        .count();
    let transaction = store.begin()?;
    transaction
        .execute(
            "INSERT INTO import_receipts
                 (id, project_id, source_realm_id, export_schema_version, source_schema_version,
                  records_hash, exported_at, imported_at, record_count, materialized_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                import_id.as_hyphenated().to_string(),
                plan.destination_project().to_string(),
                export.source_realm_id.to_string(),
                i64::from(export.schema_version),
                export.database_schema_version,
                export.records_hash.as_str(),
                format_utc_timestamp(export.exported_at),
                format_utc_timestamp(now),
                dispositions.len() as i64,
                materialized as i64,
            ],
        )
        .map_err(map_sql)?;
    for (record, disposition) in dispositions {
        transaction
            .execute(
                "INSERT INTO imported_records
                     (import_id, record_kind, source_identity, source_hash, disposition,
                      reason_code, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    import_id.as_hyphenated().to_string(),
                    record.kind,
                    record.identity,
                    record.hash.as_str(),
                    disposition.column(),
                    disposition.reason(),
                    format_utc_timestamp(now),
                ],
            )
            .map_err(map_sql)?;
    }
    transaction.commit().map_err(map_sql)?;
    Ok(())
}

/// Map a SQL failure onto a repository refusal without leaking a stored value.
fn map_sql(error: rusqlite::Error) -> BackupError {
    BackupError::Repository(crate::repository::backend(error))
}

/// Read back what an import receipt recorded, for verification and reporting.
impl SqliteStore {
    /// Every import receipt in this Realm, newest first.
    ///
    /// # Errors
    /// Returns [`BackupError::Store`] when the table cannot be read.
    pub fn import_receipts(&self) -> Result<Vec<ImportReceiptRow>, BackupError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, project_id, source_realm_id, export_schema_version,
                        source_schema_version, records_hash, exported_at, imported_at,
                        record_count, materialized_count
                 FROM import_receipts ORDER BY imported_at DESC, id DESC",
            )
            .map_err(|source| BackupError::Store(source.into()))?;
        let rows = statement
            .query_map([], |row| {
                Ok(ImportReceiptRow {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    source_realm_id: row.get(2)?,
                    export_schema_version: row.get(3)?,
                    source_schema_version: row.get(4)?,
                    records_hash: row.get(5)?,
                    exported_at: row.get(6)?,
                    imported_at: row.get(7)?,
                    record_count: row.get(8)?,
                    materialized_count: row.get(9)?,
                })
            })
            .map_err(|source| BackupError::Store(source.into()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|source| BackupError::Store(source.into()))
    }

    /// The lineage recorded under one import receipt, in kind order.
    ///
    /// # Errors
    /// Returns [`BackupError::Store`] when the table cannot be read.
    pub fn imported_records(&self, import_id: &str) -> Result<Vec<ImportedRecordRow>, BackupError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT record_kind, source_identity, source_hash, disposition, reason_code
                 FROM imported_records WHERE import_id = ?1
                 ORDER BY record_kind, source_identity",
            )
            .map_err(|source| BackupError::Store(source.into()))?;
        let rows = statement
            .query_map(params![import_id], |row| {
                Ok(ImportedRecordRow {
                    record_kind: row.get(0)?,
                    source_identity: row.get(1)?,
                    source_hash: row.get(2)?,
                    disposition: row.get(3)?,
                    reason_code: row.get(4)?,
                })
            })
            .map_err(|source| BackupError::Store(source.into()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|source| BackupError::Store(source.into()))
    }
}

/// One stored import receipt, as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReceiptRow {
    /// The destination receipt id.
    pub id: String,
    /// The destination project.
    pub project_id: String,
    /// The source Realm, as a reference.
    pub source_realm_id: String,
    /// The export generation the records were written under.
    pub export_schema_version: i64,
    /// The source database generation.
    pub source_schema_version: i64,
    /// The digest the source computed over its records.
    pub records_hash: String,
    /// The source's instant.
    pub exported_at: String,
    /// This Realm's instant.
    pub imported_at: String,
    /// How many source records the document carried.
    pub record_count: i64,
    /// How many became destination specifications.
    pub materialized_count: i64,
}

/// One stored lineage entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedRecordRow {
    /// The source table the record came from.
    pub record_kind: String,
    /// The source record's primary key, as text.
    pub source_identity: String,
    /// The source record's digest.
    pub source_hash: String,
    /// What the import did about it.
    pub disposition: String,
    /// Why, for a refusal.
    pub reason_code: Option<String>,
}
