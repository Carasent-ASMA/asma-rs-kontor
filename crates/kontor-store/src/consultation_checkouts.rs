//! Consultation intent/readback on the existing durable topology node.

use crate::SqliteStore;
use crate::repository::backend;
use kontor_core::id::{
    AggregateRevision, CanonicalDocument, ContentHash, ProjectId, TopologyNodeId,
};
use kontor_core::repository::{RepositoryError, RepositoryResult};
use rusqlite::{OptionalExtension, params};

fn conflict(rule: &'static str) -> RepositoryError {
    RepositoryError::Conflict {
        subject: "consultation placement",
        rule,
    }
}

/// Exact source/layout intent and its first successful Git readback.
#[derive(Debug, Clone)]
pub struct ConsultationPlacement {
    /// Immutable canonical source/layout/binding evidence.
    pub intent: CanonicalDocument,
    /// First Git result, retained across retries and native recovery.
    pub readback: Option<CanonicalDocument>,
}

impl SqliteStore {
    /// Read immutable placement without consulting current authoring sources.
    ///
    /// # Errors
    /// Refuses unreadable or hash-mismatched intent evidence.
    pub fn consultation_placement(
        &self,
        project: ProjectId,
        node: TopologyNodeId,
    ) -> RepositoryResult<Option<ConsultationPlacement>> {
        let row: Option<(Option<String>, Option<String>, Option<String>)> = self.connection.query_row(
            "SELECT consultation_intent, consultation_intent_hash, consultation_git_readback FROM topology_nodes WHERE project_id=?1 AND id=?2",
            params![project.to_string(), node.to_string()], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        ).optional().map_err(backend)?;
        let Some((intent, hash, readback)) = row else {
            return Ok(None);
        };
        let Some(intent) = intent else {
            if hash.is_some() || readback.is_some() {
                return Err(conflict("consultation.intent_missing"));
            }
            return Ok(None);
        };
        let parse = |s: &str| -> RepositoryResult<CanonicalDocument> {
            let value: serde_json::Value =
                serde_json::from_str(s).map_err(|_| conflict("consultation.intent_invalid"))?;
            Ok(CanonicalDocument::from_value(&value)?)
        };
        let intent = parse(&intent)?;
        if hash.as_deref() != Some(intent.hash().as_str()) {
            return Err(conflict("consultation.intent_hash_mismatch"));
        }
        Ok(Some(ConsultationPlacement {
            intent,
            readback: readback.as_deref().map(parse).transpose()?,
        }))
    }

    /// Freeze only a current, unbound node, with exact epic-domain and owner guards.
    ///
    /// # Errors
    /// Refuses stale revisions, foreign bindings, collisions or attempted repinning.
    pub fn freeze_consultation_placement(
        &self,
        project: ProjectId,
        node: TopologyNodeId,
        revision: AggregateRevision,
        intent: &CanonicalDocument,
    ) -> RepositoryResult<ConsultationPlacement> {
        let transaction = self.begin()?;
        if let Some(old) = self.consultation_placement(project, node)? {
            if old.intent != *intent {
                return Err(conflict("consultation.intent_binding_mismatch"));
            }
            return Ok(old);
        }
        let value: serde_json::Value = serde_json::from_str(intent.json())
            .map_err(|_| conflict("consultation.intent_invalid"))?;
        let epic = value["epic_id"]
            .as_str()
            .ok_or_else(|| conflict("consultation.epic_missing"))?;
        let destination = value["checkout"]["destination"]
            .as_str()
            .ok_or_else(|| conflict("consultation.destination_missing"))?;
        let bundle = value["bundle_hash"]
            .as_str()
            .ok_or_else(|| conflict("consultation.bundle_missing"))?;
        ContentHash::parse(bundle)?;
        if value["project_id"].as_str() != Some(project.to_string().as_str())
            || value["node_id"].as_str() != Some(node.to_string().as_str())
        {
            return Err(conflict("consultation.node_binding_mismatch"));
        }
        let valid: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM topology_nodes node JOIN mini_project_team_definition_snapshots pin ON pin.project_id=node.project_id AND pin.mini_project_id=node.mini_project_id JOIN projects project ON project.id=node.project_id WHERE node.project_id=?1 AND node.id=?2 AND node.mini_project_id=?3 AND node.revision=?4 AND pin.operational_domain_bundle_hash=?5 AND project.root_path=?6 AND NOT EXISTS(SELECT 1 FROM topology_node_containers WHERE topology_node_id=node.id))",
            params![project.to_string(),node.to_string(),epic,i64::try_from(revision.get()).map_err(|_| conflict("consultation.stale_revision"))?,bundle,value["checkout"]["project_root"].as_str()], |r| r.get(0),
        ).map_err(backend)?;
        if !valid {
            return Err(conflict("consultation.stale_revision_or_domain_binding"));
        }
        self.guard_consultation_destination(project, node, destination)?;
        // The intent is frozen before any native effect; lifecycle revision is unchanged.
        transaction.execute("UPDATE topology_nodes SET consultation_intent=?3, consultation_intent_hash=?4 WHERE project_id=?1 AND id=?2 AND consultation_intent IS NULL", params![project.to_string(),node.to_string(),intent.json(),intent.hash().as_str()]).map_err(backend)?;
        transaction.commit().map_err(backend)?;
        self.consultation_placement(project, node)?
            .ok_or_else(|| conflict("consultation.intent_missing"))
    }

    /// Realm-wide durable task, consultation and native-container destination guards.
    ///
    /// # Errors
    /// Refuses another durable owner's exact destination.
    pub fn guard_consultation_destination(
        &self,
        project: ProjectId,
        node: TopologyNodeId,
        destination: &str,
    ) -> RepositoryResult<()> {
        let collision: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_worktrees WHERE worktree=?1) OR EXISTS(SELECT 1 FROM task_checkout_placements WHERE destination=?1) OR EXISTS(SELECT 1 FROM topology_node_containers WHERE canonical_cwd=?1 AND (project_id<>?2 OR topology_node_id<>?3)) OR EXISTS(SELECT 1 FROM topology_nodes WHERE consultation_intent IS NOT NULL AND json_extract(consultation_intent,'$.checkout.destination')=?1 AND (project_id<>?2 OR id<>?3))",
            params![destination,project.to_string(),node.to_string()], |r|r.get(0),
        ).map_err(backend)?;
        if collision {
            return Err(conflict("consultation.destination_ownership_conflict"));
        }
        Ok(())
    }

    /// Prove the frozen owner, topology, domain and pending node revision still agree.
    ///
    /// # Errors
    /// Refuses a changed or stale placement before dispatch or confirmation.
    pub fn guard_consultation_placement(
        &self,
        project: ProjectId,
        node: TopologyNodeId,
    ) -> RepositoryResult<()> {
        let pin = self
            .consultation_placement(project, node)?
            .ok_or_else(|| conflict("consultation.intent_missing"))?;
        let value: serde_json::Value = serde_json::from_str(pin.intent.json())
            .map_err(|_| conflict("consultation.intent_invalid"))?;
        let valid:bool=self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM topology_nodes node JOIN mini_project_team_definition_snapshots pin ON pin.project_id=node.project_id AND pin.mini_project_id=node.mini_project_id JOIN projects project ON project.id=node.project_id WHERE node.project_id=?1 AND node.id=?2 AND node.mini_project_id=?3 AND pin.operational_domain_bundle_hash=?4 AND project.root_path=?5 AND node.spec_hash=?6 AND node.lifecycle='active' AND (node.consultation_git_readback IS NOT NULL OR node.revision=?7))",
            params![project.to_string(),node.to_string(),value["epic_id"].as_str(),value["bundle_hash"].as_str(),value["checkout"]["project_root"].as_str(),value["topology"]["canonical_hash"].as_str(),value["node_revision"].as_i64()],|r|r.get(0),
        ).map_err(backend)?;
        if !valid {
            return Err(conflict("consultation.stale_source_layout_binding"));
        }
        self.guard_consultation_destination(
            project,
            node,
            value["checkout"]["destination"]
                .as_str()
                .ok_or_else(|| conflict("consultation.destination_missing"))?,
        )
    }

    /// Keep first correlated Git readback before a native container may bind.
    ///
    /// # Errors
    /// Refuses changed intent, result or durable destination ownership.
    pub fn confirm_consultation_checkout(
        &self,
        project: ProjectId,
        node: TopologyNodeId,
        intent: &CanonicalDocument,
        readback: &CanonicalDocument,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        self.guard_consultation_placement(project, node)?;
        let pin = self
            .consultation_placement(project, node)?
            .ok_or_else(|| conflict("consultation.intent_missing"))?;
        if pin.intent != *intent {
            return Err(conflict("consultation.intent_binding_mismatch"));
        }
        let value: serde_json::Value = serde_json::from_str(readback.json())
            .map_err(|_| conflict("consultation.readback_invalid"))?;
        let frozen: serde_json::Value = serde_json::from_str(intent.json())
            .map_err(|_| conflict("consultation.intent_invalid"))?;
        if value["intent_hash"].as_str() != Some(intent.hash().as_str())
            || value["destination"] != frozen["checkout"]["destination"]
            || value["common_dir"] != frozen["checkout"]["source"]["common_dir"]
            || value["branch"] != frozen["checkout"]["branch"]
        {
            return Err(conflict("consultation.readback_binding_mismatch"));
        }
        self.guard_consultation_destination(
            project,
            node,
            value["destination"]
                .as_str()
                .ok_or_else(|| conflict("consultation.destination_missing"))?,
        )?;
        transaction.execute("UPDATE topology_nodes SET consultation_git_readback=?3 WHERE project_id=?1 AND id=?2 AND consultation_git_readback IS NULL", params![project.to_string(),node.to_string(),readback.json()]).map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }
}
