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
    CanonicalDocument, CommandReceiptId, ContentHash, ExternalName, ProjectId, SpecVersion, TaskId,
    TaskWorkflowId, TeamTemplateId, Timestamp, WorkProfileKey, format_utc_timestamp,
    parse_utc_timestamp,
};
use kontor_core::receipt::AggregateRef;
use kontor_core::repository::{ProjectRepository, RepositoryError};
use kontor_core::spec::{
    PersonaScenarioSpec, ResolvedWorkProfileSnapshot, TeamTemplateRevision, TriggerSpec,
    WorkProfileSpec,
};
use kontor_core::ticket::{ExternalWorkflowSpec, TicketFieldSpec};
use rusqlite::{Transaction, params};
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
    validate_profile_selection_outcomes(export)?;

    // 2. Lineage first, and for every record: an import that materializes
    //    nothing still has to be able to say what it saw.
    let lineage = export.records.lineage()?;
    let mut identities = std::collections::BTreeSet::new();
    if lineage
        .iter()
        .any(|record| !identities.insert((record.kind, record.identity.clone())))
    {
        return Err(BackupError::Verification {
            detail: "an export repeats a source record identity",
        });
    }
    let mut dispositions: Vec<(RecordLineage, Disposition)> = lineage
        .into_iter()
        .map(|record| (record, Disposition::Recorded))
        .collect();

    // 3. Materialization and provenance are one write, in dependency order.
    //    Each specification is re-validated and re-canonicalized before its
    //    insert. No specification may become live before the destination
    //    receipt that explains it, and a late uniqueness/FK failure rolls every
    //    inserted specification back.
    let import_id = kontor_core::id::generate_uuid_v7();
    let transaction = store.begin()?;
    materialize(&transaction, export, destination, now, &mut dispositions)?;
    record_import(&transaction, export, plan, now, import_id, &dispositions)?;
    transaction.commit().map_err(map_sql)?;

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

/// Verify every exported selection result against the exact source records it
/// claims to bind.
///
/// An import never makes those records live, but it must not preserve a
/// self-consistent hash over a fabricated relationship. The typed identifiers,
/// closed outcome vocabulary and source receipt/workflow/specification rows are
/// all checked before the destination transaction begins.
fn validate_profile_selection_outcomes(export: &KontorExportV1) -> Result<(), BackupError> {
    let records = &export.records;
    let mut identities = std::collections::BTreeSet::new();
    for row in &records.profile_selection_outcomes {
        ProjectId::parse(&row.project_id)?;
        CommandReceiptId::parse(&row.receipt_id)?;
        let task_id = TaskId::parse(&row.task_id)?;
        TaskWorkflowId::parse(&row.workflow_id)?;
        let profile_key = WorkProfileKey::parse(&row.profile_key)?;
        let profile_version =
            SpecVersion::parse(u32::try_from(row.profile_version).map_err(|_| {
                BackupError::Verification {
                    detail: "an exported profile selection has an invalid profile version",
                }
            })?)?;
        let profile_hash = ContentHash::parse(&row.profile_hash)?;
        let recorded_at = parse_utc_timestamp(&row.recorded_at)?;
        if !matches!(row.applied.as_str(), "created" | "unchanged") {
            return Err(BackupError::Verification {
                detail: "an exported profile selection has an invalid applied outcome",
            });
        }
        if !identities.insert((&row.project_id, &row.receipt_id)) {
            return Err(BackupError::Verification {
                detail: "an export repeats a profile selection outcome identity",
            });
        }

        let team = match (
            &row.team_template_id,
            row.team_template_version,
            &row.team_template_hash,
        ) {
            (None, None, None) => None,
            (Some(id), Some(version), Some(hash)) => {
                let id = TeamTemplateId::parse(id)?;
                let version = SpecVersion::parse(u32::try_from(version).map_err(|_| {
                    BackupError::Verification {
                        detail: "an exported profile selection has an invalid team version",
                    }
                })?)?;
                let hash = ContentHash::parse(hash)?;
                Some((id, version, hash))
            }
            _ => {
                return Err(BackupError::Verification {
                    detail: "an exported profile selection has a partial team binding",
                });
            }
        };

        let Some(receipt) = records
            .command_receipts
            .iter()
            .find(|receipt| receipt.project_id == row.project_id && receipt.id == row.receipt_id)
        else {
            return Err(invalid_selection_lineage());
        };
        let receipt_matches = receipt.kind == "select_task_profile"
            && receipt.execution_mode == "local"
            && receipt.state == "confirmed"
            && receipt.result_ref.as_deref() == Some(receipt.intent_hash.as_str())
            && receipt.created_at == row.recorded_at
            && serde_json::from_str::<AggregateRef>(&receipt.target)
                .is_ok_and(|target| target == (AggregateRef::Task { task_id }))
            && !records.command_outbox.iter().any(|outbox| {
                outbox.project_id == row.project_id && outbox.receipt_id == row.receipt_id
            });

        let Some(profile_row) = records.work_profiles.iter().find(|profile| {
            profile.project_id == row.project_id
                && profile.profile_key == row.profile_key
                && profile.version == row.profile_version
        }) else {
            return Err(invalid_selection_lineage());
        };
        let (profile, stored_profile_hash) =
            stored::<WorkProfileSpec>(&profile_row.definition, &profile_row.definition_hash)?;
        let canonical_profile = profile.canonicalize()?;
        let profile_matches = profile.id == profile_key
            && profile.version == profile_version
            && stored_profile_hash == profile_hash
            && canonical_profile.hash() == &profile_hash
            && canonical_profile.json() == profile_row.definition;

        let Some(workflow) = records.task_workflows.iter().find(|workflow| {
            workflow.project_id == row.project_id && workflow.id == row.workflow_id
        }) else {
            return Err(invalid_selection_lineage());
        };
        let snapshot_hash = ContentHash::parse(&workflow.snapshot_hash)?;
        let snapshot_document = CanonicalDocument::from_stored(&workflow.snapshot, &snapshot_hash)?;
        let snapshot = snapshot_document.deserialize::<ResolvedWorkProfileSnapshot>()?;
        snapshot.verify()?;
        let workflow_created_at = parse_utc_timestamp(&workflow.created_at)?;
        let applied_matches = match row.applied.as_str() {
            "created" => workflow_created_at == recorded_at,
            "unchanged" => workflow_created_at < recorded_at,
            _ => false,
        };
        let workflow_matches = workflow.task_id == row.task_id
            && workflow.profile_key == row.profile_key
            && workflow.profile_version == row.profile_version
            && snapshot.definition == profile
            && snapshot.definition_hash == profile_hash
            && applied_matches;

        let snapshot_team_matches = match (snapshot.definition.team_template, team.as_ref()) {
            (None, None) => true,
            (Some(pin), Some((id, version, _))) => {
                pin.template_id == *id && pin.version == *version
            }
            _ => false,
        };
        let team_matches = team.as_ref().is_none_or(|(id, version, hash)| {
            records.team_templates.iter().any(|template| {
                if template.project_id != row.project_id
                    || template.template_id != id.to_string()
                    || template.version != i64::from(version.get())
                    || template.definition_hash != hash.as_str()
                {
                    return false;
                }
                CanonicalDocument::from_stored(&template.definition, hash).is_ok()
            })
        });
        let task_matches = records
            .tasks
            .iter()
            .any(|task| task.project_id == row.project_id && task.id == row.task_id);
        if !(receipt_matches
            && workflow_matches
            && task_matches
            && profile_matches
            && snapshot_team_matches
            && team_matches)
        {
            return Err(BackupError::Verification {
                detail: "an exported profile selection does not match its source lineage",
            });
        }
    }
    Ok(())
}

const fn invalid_selection_lineage() -> BackupError {
    BackupError::Verification {
        detail: "an exported profile selection does not match its source lineage",
    }
}

/// Classify one targeted `INSERT .. ON CONFLICT(primary-key) DO NOTHING`.
fn inserted(outcome: rusqlite::Result<usize>) -> Disposition {
    match outcome {
        Ok(1) => Disposition::Materialized,
        Ok(0) => Disposition::AlreadyPresent,
        Ok(_) | Err(_) => Disposition::Refused("destination_refused"),
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
    transaction: &Transaction<'_>,
    export: &KontorExportV1,
    destination: ProjectId,
    now: Timestamp,
    dispositions: &mut [(RecordLineage, Disposition)],
) -> Result<(), BackupError> {
    let records = &export.records;
    let created_at = format_utc_timestamp(now);

    for row in &records.calendar_profiles {
        let identity = format!("{}/{}", row.profile_id, row.version);
        let outcome = match stored::<CalendarProfileSpec>(&row.definition, &row.definition_hash) {
            Ok((spec, digest)) => match spec.canonicalize() {
                Ok(document) if document.hash() == &digest => inserted(transaction.execute(
                    "INSERT INTO calendar_profiles
                         (profile_id, version, name, definition, definition_hash, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(profile_id, version) DO NOTHING",
                    params![
                        spec.profile_id.to_string(),
                        i64::from(spec.version.get()),
                        spec.name.as_str(),
                        document.json(),
                        document.hash().as_str(),
                        created_at,
                    ],
                )),
                Ok(_) => Disposition::Refused("destination_digest_differs"),
                Err(_) => Disposition::Refused("source_document_invalid"),
            },
            Err(_) => Disposition::Refused("source_document_invalid"),
        };
        set(dispositions, "calendar_profiles", &identity, outcome);
    }

    for row in &records.work_profiles {
        let identity = format!("{}/{}/{}", row.project_id, row.profile_key, row.version);
        let outcome = match stored::<WorkProfileSpec>(&row.definition, &row.definition_hash) {
            Ok((spec, digest)) => match spec.canonicalize() {
                Ok(document) if document.hash() == &digest => inserted(transaction.execute(
                    "INSERT INTO work_profiles
                         (project_id, profile_key, version, definition, definition_hash, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(project_id, profile_key, version) DO NOTHING",
                    params![
                        destination.to_string(),
                        spec.id.as_str(),
                        i64::from(spec.version.get()),
                        document.json(),
                        document.hash().as_str(),
                        created_at,
                    ],
                )),
                Ok(_) => Disposition::Refused("destination_digest_differs"),
                Err(_) => Disposition::Refused("source_document_invalid"),
            },
            Err(_) => Disposition::Refused("source_document_invalid"),
        };
        set(dispositions, "work_profiles", &identity, outcome);
    }

    for row in &records.team_templates {
        let identity = format!("{}/{}/{}", row.project_id, row.template_id, row.version);
        let outcome = materialize_team_template(transaction, destination, now, row);
        set(dispositions, "team_templates", &identity, outcome);
    }

    for row in &records.persona_scenarios {
        let identity = format!("{}/{}/{}", row.project_id, row.scenario_id, row.version);
        let outcome = match stored::<PersonaScenarioSpec>(&row.definition, &row.definition_hash) {
            Ok((spec, digest)) => match spec.canonicalize() {
                Ok(document) if document.hash() == &digest => inserted(transaction.execute(
                    "INSERT INTO persona_scenarios
                         (project_id, scenario_id, version, persona_key, gate_key, definition,
                          definition_hash, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(project_id, scenario_id, version) DO NOTHING",
                    params![
                        destination.to_string(),
                        spec.scenario_id.to_string(),
                        i64::from(spec.version.get()),
                        spec.persona.as_str(),
                        spec.gate_under_test.as_str(),
                        document.json(),
                        document.hash().as_str(),
                        created_at,
                    ],
                )),
                Ok(_) => Disposition::Refused("destination_digest_differs"),
                Err(_) => Disposition::Refused("source_document_invalid"),
            },
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
            Ok((spec, digest)) => match spec.canonicalize() {
                Ok(document) if document.hash() == &digest => inserted(transaction.execute(
                    "INSERT INTO ticket_field_specs
                         (project_id, connector, external_project, issue_type, version,
                          definition, definition_hash, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(project_id, connector, external_project, issue_type, version)
                     DO NOTHING",
                    params![
                        destination.to_string(),
                        spec.connector.as_str(),
                        spec.project.as_str(),
                        spec.issue_type.as_str(),
                        i64::from(spec.version.get()),
                        document.json(),
                        document.hash().as_str(),
                        created_at,
                    ],
                )),
                Ok(_) => Disposition::Refused("destination_digest_differs"),
                Err(_) => Disposition::Refused("source_document_invalid"),
            },
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
            Ok((spec, digest)) => match spec.canonicalize() {
                Ok(document) if document.hash() == &digest => inserted(transaction.execute(
                    "INSERT INTO external_workflow_specs
                         (project_id, connector, external_project, issue_type, version,
                          work_profile_key, work_profile_version, definition, definition_hash,
                          created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                     ON CONFLICT(project_id, connector, external_project, issue_type, version)
                     DO NOTHING",
                    params![
                        destination.to_string(),
                        spec.connector.as_str(),
                        spec.project.as_str(),
                        spec.issue_type.as_str(),
                        i64::from(spec.version.get()),
                        spec.work_profile.as_ref().map(WorkProfileKey::as_str),
                        spec.work_profile_version.map(|version| i64::from(version.get())),
                        document.json(),
                        document.hash().as_str(),
                        created_at,
                    ],
                )),
                Ok(_) => Disposition::Refused("destination_digest_differs"),
                Err(_) => Disposition::Refused("source_document_invalid"),
            },
            Err(_) => Disposition::Refused("source_document_invalid"),
        };
        set(dispositions, "external_workflow_specs", &identity, outcome);
    }

    for row in &records.trigger_specs {
        let identity = format!("{}/{}/{}", row.project_id, row.trigger_key, row.version);
        let outcome = match stored::<TriggerSpec>(&row.definition, &row.definition_hash) {
            Ok((spec, digest)) => match spec.canonicalize() {
                Ok(document) if document.hash() == &digest => inserted(transaction.execute(
                    "INSERT INTO trigger_specs
                         (project_id, trigger_key, version, source_kind, source_connection,
                          work_profile_key, work_profile_version, team_template_id,
                          team_template_version, context_template, context_version,
                          calendar_profile_id, calendar_version, definition, definition_hash,
                          created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                             ?15, ?16)
                     ON CONFLICT(project_id, trigger_key, version) DO NOTHING",
                    params![
                        destination.to_string(),
                        spec.id.as_str(),
                        i64::from(spec.version.get()),
                        spec.source_kind.as_str(),
                        spec.source_connection.as_str(),
                        spec.work_profile.as_str(),
                        i64::from(spec.work_profile_version.get()),
                        spec.team_template.template_id.to_string(),
                        i64::from(spec.team_template.version.get()),
                        spec.context_template.template.as_str(),
                        i64::from(spec.context_template.version.get()),
                        spec.calendar_policy
                            .as_ref()
                            .map(|policy| policy.profile_id.to_string()),
                        spec.calendar_policy
                            .as_ref()
                            .map(|policy| i64::from(policy.version.get())),
                        document.json(),
                        document.hash().as_str(),
                        created_at,
                    ],
                )),
                Ok(_) => Disposition::Refused("destination_digest_differs"),
                Err(_) => Disposition::Refused("source_document_invalid"),
            },
            Err(_) => Disposition::Refused("source_document_invalid"),
        };
        set(dispositions, "trigger_specs", &identity, outcome);
    }

    Ok(())
}

/// A team template is the one specification whose row carries fields beside its
/// document, so it is rebuilt rather than deserialized whole.
fn materialize_team_template(
    transaction: &Transaction<'_>,
    destination: ProjectId,
    now: Timestamp,
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
    let Ok(authority) = serde_json::to_string(&revision.role_authority) else {
        return Disposition::Refused("source_document_invalid");
    };
    inserted(transaction.execute(
        "INSERT INTO team_templates
             (project_id, template_id, version, name, definition, definition_hash,
              role_authority, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(project_id, template_id, version) DO NOTHING",
        params![
            destination.to_string(),
            revision.template_id.to_string(),
            i64::from(revision.version.get()),
            revision.name.as_str(),
            revision.definition.json(),
            digest.as_str(),
            authority,
            format_utc_timestamp(now),
        ],
    ))
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
    transaction: &Transaction<'_>,
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
    for row in &export.records.profile_selection_outcomes {
        let identity = format!("{}/{}", row.project_id, row.receipt_id);
        let source_hash = dispositions
            .iter()
            .find(|(record, _)| {
                record.kind == "profile_selection_outcomes" && record.identity == identity
            })
            .map(|(record, _)| record.hash.as_str())
            .ok_or(BackupError::Verification {
                detail: "an exported profile selection has no lineage digest",
            })?;
        transaction
            .execute(
                "INSERT INTO imported_profile_selection_outcomes
                     (project_id, import_id, source_project_id, source_receipt_id, source_task_id,
                      source_workflow_id, profile_key, profile_version, profile_hash,
                      team_template_id, team_template_version, team_template_hash, applied,
                      source_recorded_at, source_record_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    plan.destination_project().to_string(),
                    import_id.as_hyphenated().to_string(),
                    row.project_id,
                    row.receipt_id,
                    row.task_id,
                    row.workflow_id,
                    row.profile_key,
                    row.profile_version,
                    row.profile_hash,
                    row.team_template_id,
                    row.team_template_version,
                    row.team_template_hash,
                    row.applied,
                    row.recorded_at,
                    source_hash,
                ],
            )
            .map_err(map_sql)?;
    }
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

    /// Exact profile-selection outcomes preserved as non-executable lineage
    /// under one destination import receipt.
    ///
    /// # Errors
    /// Returns [`BackupError::Store`] when the table cannot be read.
    pub fn imported_profile_selection_outcomes(
        &self,
        import_id: &str,
    ) -> Result<Vec<ImportedProfileSelectionOutcomeRow>, BackupError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT source_project_id, source_receipt_id, source_task_id,
                        source_workflow_id, profile_key, profile_version, profile_hash,
                        team_template_id, team_template_version, team_template_hash, applied,
                        source_recorded_at, source_record_hash
                 FROM imported_profile_selection_outcomes WHERE import_id = ?1
                 ORDER BY source_project_id, source_receipt_id",
            )
            .map_err(|source| BackupError::Store(source.into()))?;
        let rows = statement
            .query_map(params![import_id], |row| {
                Ok(ImportedProfileSelectionOutcomeRow {
                    source_project_id: row.get(0)?,
                    source_receipt_id: row.get(1)?,
                    source_task_id: row.get(2)?,
                    source_workflow_id: row.get(3)?,
                    profile_key: row.get(4)?,
                    profile_version: row.get(5)?,
                    profile_hash: row.get(6)?,
                    team_template_id: row.get(7)?,
                    team_template_version: row.get(8)?,
                    team_template_hash: row.get(9)?,
                    applied: row.get(10)?,
                    source_recorded_at: row.get(11)?,
                    source_record_hash: row.get(12)?,
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

/// One exact imported profile-selection result kept as source-referenced
/// lineage, never as a live command or workflow effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedProfileSelectionOutcomeRow {
    /// Source project reference.
    pub source_project_id: String,
    /// Source command receipt reference.
    pub source_receipt_id: String,
    /// Source task reference.
    pub source_task_id: String,
    /// Source workflow reference.
    pub source_workflow_id: String,
    /// Exact source profile key.
    pub profile_key: String,
    /// Exact source profile version.
    pub profile_version: i64,
    /// Exact source profile hash.
    pub profile_hash: String,
    /// Exact source team id, when pinned.
    pub team_template_id: Option<String>,
    /// Exact source team version, when pinned.
    pub team_template_version: Option<i64>,
    /// Exact source team hash, when pinned.
    pub team_template_hash: Option<String>,
    /// Whether the source selection created or reused its workflow.
    pub applied: String,
    /// Source instant.
    pub source_recorded_at: String,
    /// Canonical hash of the complete exported source row.
    pub source_record_hash: String,
}
