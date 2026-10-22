//! Epic creation binds the verified selection, roster and receipt atomically.

use super::*;
use kontor_core::id::{CanonicalDocument, IdempotencyKey};
use kontor_core::realm::ReceiptEnvelope;
use kontor_core::repository::{
    CommandRepository, MiniProjectTeamDefinitionSnapshot, MiniProjectTopologySnapshot,
    ProjectRepository, ProjectTeamDefinitionDefault, StoredEpicRoster, TeamDefinitionRepository,
};
use kontor_core::spec::TeamDefinitionSnapshot;
use kontor_teams::CoreTeamRevision;
use serde::{Deserialize, Serialize};

/// Exact verified context carried by the creation intent, never a mutable path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpicDomainBinding {
    /// Published definition belonging to the selected domain.
    pub definition: TeamDefinitionSnapshot,
    /// Immutable bundle that supplied the domain and its catalog.
    pub operational_domain_bundle_hash: ContentHash,
    /// Exact seats, including frozen purposes, authorized for this epic.
    pub roster: CoreTeamRevision,
}

/// Creation authority before the graph has a durable target id.
#[derive(Debug)]
pub struct EpicApplyCommand {
    /// Generated receipt identity.
    pub receipt_id: CommandReceiptId,
    /// Stable caller operation identity.
    pub idempotency_key: IdempotencyKey,
    /// Project revision the caller read.
    pub expected_revision: AggregateRevision,
    /// Exact request and resolved domain binding.
    pub intent: CanonicalDocument,
}

impl SqliteStore {
    /// Validate an epic and its domain pins without retaining any effect.
    ///
    /// # Errors
    /// Refuses stale selections, mismatched frozen rosters and unpinned legacy epics.
    pub fn preview_epic_with_domain(
        &self,
        request: &EpicApplication<'_>,
        binding: &EpicDomainBinding,
        selection: Option<&ProjectTeamDefinitionDefault>,
    ) -> RepositoryResult<AppliedEpic> {
        let transaction = self.begin()?;
        ensure_project_exists(&transaction, request.project_id)?;
        let outcome = evaluate_epic_in(&transaction, request)?;
        self.bind_epic_domain_in(&transaction, request, &outcome, binding, selection)?;
        transaction.rollback().map_err(backend)?;
        Ok(outcome)
    }

    /// Commit the graph, pins/roster and pending local intent/result together.
    /// `None` means a concurrent call already committed: resume its stored receipt.
    ///
    /// # Errors
    /// Refuses realm, revision, selection, roster, intent or immutable-pin drift.
    pub fn apply_epic_with_domain(
        &self,
        request: &EpicApplication<'_>,
        binding: &EpicDomainBinding,
        selection: Option<&ProjectTeamDefinitionDefault>,
        envelope: &ReceiptEnvelope<EpicApplyCommand>,
    ) -> RepositoryResult<Option<AppliedEpic>> {
        let command = envelope.peek(self.realm_id())?;
        let transaction = self.begin()?;
        let intent: serde_json::Value = command.intent.deserialize()?;
        if intent.get("domain_binding")
            != Some(
                &serde_json::to_value(binding)
                    .map_err(|_| conflict("epic", "operational_domain_binding_unreadable"))?,
            )
            || intent.get("project_id") != Some(&serde_json::json!(request.project_id))
            || intent.get("expected_project_revision")
                != Some(&serde_json::json!(command.expected_revision))
        {
            return Err(conflict(
                "epic",
                "operational_domain_creation_intent_mismatch",
            ));
        }
        if let Some(receipt) = self.get_receipt_by_key(&command.idempotency_key)? {
            let mut prior: serde_json::Value = receipt.intent.deserialize()?;
            let mut incoming = intent.clone();
            // Only server-resolved context may differ across a concurrent retry.
            prior
                .as_object_mut()
                .map(|value| value.remove("domain_binding"));
            incoming
                .as_object_mut()
                .map(|value| value.remove("domain_binding"));
            if receipt.project_id != request.project_id || prior != incoming {
                return Err(conflict(
                    "epic",
                    "operational_domain_creation_replay_mismatch",
                ));
            }
            self.epic_domain_creation_result(&receipt)?;
            transaction.rollback().map_err(backend)?;
            return Ok(None);
        }
        let project = self
            .get_project(request.project_id)?
            .ok_or(RepositoryError::NotFound { subject: "project" })?;
        project
            .revision
            .expect("project", command.expected_revision)?;
        require_backlog_authority(&transaction, request.project_id)?;
        let outcome = evaluate_epic_in(&transaction, request)?;
        self.bind_epic_domain_in(&transaction, request, &outcome, binding, selection)?;
        let local = NewLocalCommand {
            project_id: request.project_id,
            receipt_id: command.receipt_id,
            idempotency_key: command.idempotency_key.clone(),
            kind: CommandKind::ApplyEpicGraph,
            target: AggregateRef::MiniProject {
                mini_project_id: outcome.mini_project_id,
            },
            target_revision: outcome.revision,
            intent: command.intent.clone(),
            created_at: request.applied_at,
        };
        let mut result = intent;
        result["result"] = serde_json::json!({
            "intent_hash": command.intent.hash(),
            "epic_id": outcome.mini_project_id,
            "domain_binding": binding,
        });
        crate::repository::record_pending_local_result(
            &transaction,
            &local,
            &CanonicalDocument::from_value(&result)?,
        )?;
        transaction.commit().map_err(backend)?;
        Ok(Some(outcome))
    }

    /// Verify the exact creation result against its receipt and frozen target.
    ///
    /// # Errors
    /// Missing results, changed intent bytes or mismatched targets refuse.
    pub fn epic_domain_creation_result(
        &self,
        receipt: &kontor_core::receipt::CommandReceipt,
    ) -> RepositoryResult<EpicDomainBinding> {
        let mismatch = || conflict("epic", "operational_domain_creation_result_mismatch");
        let (json, hash) =
            crate::repository::local_or_legacy_command_result(&self.connection, receipt)?
                .ok_or_else(mismatch)?;
        let document = CanonicalDocument::from_stored(&json, &ContentHash::parse(&hash)?)?;
        if document.hash().as_str() != hash
            || receipt.result_ref.as_ref().map(ExternalId::as_str) != Some(hash.as_str())
        {
            return Err(mismatch());
        }
        let mut value: serde_json::Value = document.deserialize()?;
        let result = value
            .as_object_mut()
            .and_then(|value| value.remove("result"))
            .ok_or_else(mismatch)?;
        if CanonicalDocument::from_value(&value)?.hash() != receipt.intent.hash()
            || result["intent_hash"] != serde_json::json!(receipt.intent.hash())
            || result["domain_binding"] != value["domain_binding"]
            || receipt.kind != CommandKind::ApplyEpicGraph
            || value["project_id"] != serde_json::json!(receipt.project_id)
        {
            return Err(mismatch());
        }
        let AggregateRef::MiniProject { mini_project_id } = receipt.target else {
            return Err(mismatch());
        };
        if result["epic_id"] != serde_json::json!(mini_project_id) {
            return Err(mismatch());
        }
        serde_json::from_value(result["domain_binding"].clone()).map_err(|_| mismatch())
    }

    fn bind_epic_domain_in(
        &self,
        transaction: &Transaction<'_>,
        request: &EpicApplication<'_>,
        outcome: &AppliedEpic,
        binding: &EpicDomainBinding,
        selection: Option<&ProjectTeamDefinitionDefault>,
    ) -> RepositoryResult<()> {
        let project = request.project_id;
        if outcome.applied != Applied::Created {
            let pin = self
                .get_mini_project_team_definition(project, outcome.mini_project_id)?
                .ok_or_else(|| conflict("epic", "operational_domain_unpinned"))?;
            let roster = self
                .get_epic_roster(project, outcome.mini_project_id)?
                .ok_or_else(|| conflict("epic", "role_purpose_unpinned"))?;
            if pin.definition != binding.definition
                || pin.operational_domain_bundle_hash.as_ref()
                    != Some(&binding.operational_domain_bundle_hash)
                || roster.core_team_version != binding.roster.version
                || roster.catalog_hash != binding.roster.catalog_hash
                || roster.seats
                    != serde_json::to_value(&binding.roster.seats)
                        .map_err(|_| conflict("epic", "operational_domain_roster_unreadable"))?
            {
                return Err(conflict("epic", "operational_domain_epic_binding_moved"));
            }
            return Ok(());
        }
        let selected =
            selection.ok_or_else(|| conflict("epic", "operational_domain_selection_moved"))?;
        if selected.project_id != project
            || selected.definition != binding.definition
            || selected.operational_domain_bundle_hash.as_ref()
                != Some(&binding.operational_domain_bundle_hash)
            || self.get_project_team_definition_default(project)?.as_ref() != Some(selected)
        {
            return Err(conflict("epic", "operational_domain_selection_moved"));
        }
        let current = self
            .get_current_core_team(project)?
            .ok_or_else(|| conflict("epic", "role_purpose_unpinned"))?;
        if current.version != binding.roster.version
            || current.catalog_hash != binding.roster.catalog_hash
            || current.seats
                != serde_json::to_value(&binding.roster.seats)
                    .map_err(|_| conflict("epic", "operational_domain_roster_unreadable"))?
        {
            return Err(conflict("epic", "operational_domain_roster_moved"));
        }
        // These helpers are the existing writers, sharing this graph transaction.
        crate::repository::pin_mini_project_team_definition_in(
            transaction,
            &MiniProjectTeamDefinitionSnapshot {
                project_id: project,
                mini_project_id: outcome.mini_project_id,
                definition: binding.definition.clone(),
                operational_domain_bundle_hash: Some(
                    binding.operational_domain_bundle_hash.clone(),
                ),
                pinned_at: request.applied_at,
            },
        )?;
        let definition = self
            .get_team_definition(
                project,
                binding.definition.definition_id,
                binding.definition.version,
            )?
            .ok_or(RepositoryError::NotFound {
                subject: "team definition",
            })?;
        crate::repository::pin_mini_project_topology_in(
            transaction,
            &MiniProjectTopologySnapshot {
                project_id: project,
                mini_project_id: outcome.mini_project_id,
                topology: definition.topology,
                pinned_at: request.applied_at,
            },
        )?;
        crate::repository::put_epic_roster_in(
            transaction,
            &StoredEpicRoster {
                project_id: project,
                mini_project_id: outcome.mini_project_id,
                core_team_version: binding.roster.version,
                catalog_hash: binding.roster.catalog_hash.clone(),
                seats: serde_json::to_value(&binding.roster.seats)
                    .map_err(|_| conflict("epic", "operational_domain_roster_unreadable"))?,
                quick_session_id: None,
                revision: AggregateRevision::INITIAL,
                pinned_at: request.applied_at,
            },
        )?;
        Ok(())
    }
}
