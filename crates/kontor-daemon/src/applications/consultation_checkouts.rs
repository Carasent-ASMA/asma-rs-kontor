//! Native consultation A: pinned policy, explicit root source, intent then effects.
use super::*;
use kontor_runtime::checkout::{self, CheckoutError, TaskCheckout};
use kontor_runtime::consultation_checkout::ConsultationCheckout;

impl Services {
    pub(super) fn consultation_checkout(
        &self,
        project: ProjectId,
        node: TopologyNodeId,
    ) -> Result<Option<ConsultationCheckout>, ApiError> {
        self.state()?
            .with_store(|store| store.consultation_placement(project, node))
            .map_err(|e| self.refuse(&e))?
            .map(|pin| {
                serde_json::from_str(pin.intent.json()).map_err(|_| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "consultation.intent_invalid",
                    )
                })
            })
            .transpose()
    }

    /// A legacy path is evidence only when this exact node already has a binding.
    fn bound_legacy_consultation_root(
        &self,
        project: &kontor_core::repository::Project,
        node: &SessionTopologyNode,
        key: Option<&TrackerKey>,
    ) -> Result<WorkspaceRoot, ApiError> {
        let state = self.state()?;
        let bound = state
            .with_store(|s| s.get_topology_node_container(project.id, node.id))
            .map_err(|e| self.refuse(&e))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "consultation.legacy_binding_missing",
                )
            })?;
        let target = bound.canonical_cwd.ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "consultation.legacy_path_unpinned",
            )
        })?;
        let branch = checkout::observed_branch(target.as_str())
            .map_err(|e| self.deny(ApiErrorCode::PlacementBlocked, e.0))?;
        let old = format!(
            "{}/.worktrees/consultation-{}",
            project.root_path.as_str(),
            node.id
        );
        let keyed = key.map(|key| {
            BranchName::derive(BranchType::Chore, key, &format!("consultation {}", node.id))
        });
        let exact = (target.as_str() == old && branch == format!("consultation-{}", node.id))
            || keyed.as_ref().is_some_and(|b| {
                branch == b.as_str()
                    && target.as_str() == managed_worktree_path(project.root_path.as_str(), b)
            });
        if !exact {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "consultation.legacy_binding_mismatch",
            ));
        }
        checkout::verify_legacy_consultation(project.root_path.as_str(), target.as_str(), &branch)
            .map_err(|e| self.deny(ApiErrorCode::PlacementBlocked, e.0))?;
        state
            .with_store(|s| s.guard_consultation_destination(project.id, node.id, target.as_str()))
            .map_err(|e| self.refuse(&e))?;
        WorkspaceRoot::parse(target.as_str()).map_err(|e| self.refuse_domain(&e))
    }

    /// Resolve/freeze one exact node; directory existence never grants ownership.
    pub(super) fn consultation_root(
        &self,
        project_id: ProjectId,
        node_id: TopologyNodeId,
    ) -> Result<WorkspaceRoot, ApiError> {
        let state = self.state()?;
        let project = self.project_row(project_id)?;
        let node = state
            .with_store(|s| s.get_topology_node(project_id, node_id))
            .map_err(|e| self.refuse(&e))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "consultation.node_missing_or_foreign",
                )
            })?;
        let epic = node.mini_project_id.ok_or_else(|| {
            self.deny(ApiErrorCode::PlacementBlocked, "consultation.epic_missing")
        })?;
        let key = self.epic_tracker_key(project_id, epic)?;
        let observed = state
            .with_store(|s| s.confirmed_jira_epic_key(project_id, epic))
            .map_err(|e| self.refuse(&e))?;
        if let (Some(selected), Some(observed)) = (&key, observed.as_ref())
            && selected.as_str() != observed.as_str()
        {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "consultation.epic_binding_conflict",
            ));
        }
        if let Some(pin) = self.consultation_checkout(project_id, node_id)? {
            self.validate_consultation_pin(&project, &node, &pin, key.as_ref())?;
            return WorkspaceRoot::parse(&pin.checkout.destination)
                .map_err(|e| self.refuse_domain(&e));
        }
        if state
            .with_store(|s| s.get_topology_node_container(project_id, node_id))
            .map_err(|e| self.refuse(&e))?
            .is_some()
        {
            return self.bound_legacy_consultation_root(&project, &node, key.as_ref());
        }
        let legacy = PathBuf::from(project.root_path.as_str())
            .join(".worktrees")
            .join(format!("consultation-{node_id}"));
        let keyed = key.as_ref().map(|key| {
            managed_worktree_path(
                project.root_path.as_str(),
                &BranchName::derive(BranchType::Chore, key, &format!("consultation {node_id}")),
            )
        });
        if legacy.try_exists().map_err(|_| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "consultation.legacy_path_unreadable",
            )
        })? || keyed
            .as_ref()
            .is_some_and(|path| PathBuf::from(path).exists())
        {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "consultation.legacy_binding_missing",
            ));
        }
        let context = self.runtime_domain(project_id, Some(epic))?;
        let manifest = self
            .fleet
            .verify_bundle(&context.bundle_hash)
            .map_err(|e| self.refuse_fleet_policy(&e))?
            .manifest;
        let policy = manifest.consultation_layout.ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "project.consultation_layout.not_configured",
            )
        })?;
        let key = key.ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "consultation.binding_unconfirmed_or_ambiguous",
            )
        })?;
        let (relative, managed, branch) = policy
            .render(&key, node_id)
            .map_err(|e| self.deny(ApiErrorCode::PlacementBlocked, e))?;
        let inputs = self
            .selected_config
            .as_ref()
            .and_then(|folder| folder.checkouts.as_ref())
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "host.checkouts.root.not_configured",
                )
            })?;
        let source = inputs
            .select(project.root_path.as_str(), None, &policy.base_branch)
            .map_err(|e| self.deny(ApiErrorCode::PlacementBlocked, e.0))?;
        let target = format!("{}/{relative}", project.root_path.as_str());
        let pin = ConsultationCheckout {
            schema_version: 1,
            project_id,
            epic_id: epic,
            node_id,
            node_revision: node.revision,
            container_binding_id: ContainerBindingId::generate().to_string(),
            topology: node.topology.clone(),
            bundle_hash: context.bundle_hash,
            policy,
            key: key.to_string(),
            checkout: TaskCheckout {
                schema_version: 1,
                source,
                project_root: project.root_path.as_str().to_owned(),
                claim: target.clone(),
                destination: target,
                branch: branch.to_string(),
                confirmed_keys: vec![key.to_string()],
                module: None,
                managed_root: Some(format!("{}/{managed}", project.root_path.as_str())),
            },
        };
        self.validate_consultation_pin(&project, &node, &pin, Some(&key))?;
        // A path prepared by an interrupted *persisted* intent is recoverable.
        // A pre-existing path without that intent/binding is never adopted.
        if PathBuf::from(&pin.checkout.destination)
            .try_exists()
            .map_err(|_| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "consultation.destination_unreadable",
                )
            })?
        {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "consultation.destination_unbound",
            ));
        }
        let intent =
            CanonicalDocument::from_serializable(&pin).map_err(|e| self.refuse_domain(&e))?;
        if let Err(error) = state.with_store(|s| {
            s.freeze_consultation_placement(project_id, node_id, node.revision, &intent)
        }) {
            if let Some(winner) = self.consultation_checkout(project_id, node_id)? {
                self.validate_consultation_pin(&project, &node, &winner, Some(&key))?;
                return WorkspaceRoot::parse(&winner.checkout.destination)
                    .map_err(|e| self.refuse_domain(&e));
            }
            return Err(self.refuse(&error));
        }
        WorkspaceRoot::parse(&pin.checkout.destination).map_err(|e| self.refuse_domain(&e))
    }

    pub(super) fn validate_consultation_pin(
        &self,
        project: &kontor_core::repository::Project,
        node: &SessionTopologyNode,
        pin: &ConsultationCheckout,
        key: Option<&TrackerKey>,
    ) -> Result<(), ApiError> {
        let context = self.runtime_domain(project.id, node.mini_project_id)?;
        let manifest = self
            .fleet
            .verify_bundle(&pin.bundle_hash)
            .map_err(|e| self.refuse_fleet_policy(&e))?
            .manifest;
        if pin.project_id != project.id
            || node.project_id != project.id
            || pin.node_id != node.id
            || Some(pin.epic_id) != node.mini_project_id
            || pin.topology != node.topology
            || pin.bundle_hash != context.bundle_hash
            || manifest.consultation_layout.as_ref() != Some(&pin.policy)
            || key.map(TrackerKey::as_str) != Some(pin.key.as_str())
            || pin.checkout.project_root != project.root_path.as_str()
        {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "consultation.source_layout_binding_mismatch",
            ));
        }
        let stored = self
            .state()?
            .with_store(|s| s.consultation_placement(project.id, node.id))
            .map_err(|e| self.refuse(&e))?;
        if stored.as_ref().is_some_and(|p| p.readback.is_none())
            && pin.node_revision != node.revision
        {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "consultation.stale_node_revision",
            ));
        }
        if let Some(bound) = self
            .state()?
            .with_store(|s| s.get_topology_node_container(project.id, node.id))
            .map_err(|e| self.refuse(&e))?
            && (bound.canonical_cwd.as_ref().map(ExternalName::as_str)
                != Some(pin.checkout.destination.as_str())
                || bound.container_binding_id.as_str() != pin.container_binding_id)
        {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "consultation.native_binding_mismatch",
            ));
        }
        pin.validate()
            .map_err(|CheckoutError(rule)| self.deny(ApiErrorCode::PlacementBlocked, rule))?;
        if stored.as_ref().is_some_and(|p| p.readback.is_some()) {
            pin.verify()
                .map_err(|e| self.deny(ApiErrorCode::PlacementBlocked, e.0))?;
        }
        let state = self.state()?;
        for (_, _, claim) in state
            .with_store(|s| s.checkout_claims())
            .map_err(|e| self.refuse(&e))?
        {
            if checkout::destination(claim.as_str())
                .map_err(|e| self.deny(ApiErrorCode::PlacementBlocked, e.0))?
                == pin.checkout.destination
            {
                return Err(self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "consultation.destination_ownership_conflict",
                ));
            }
        }
        state
            .with_store(|s| {
                s.guard_consultation_destination(project.id, node.id, &pin.checkout.destination)
            })
            .map_err(|e| self.refuse(&e))
    }

    pub(super) fn consultation_scope(
        &self,
        project: ProjectId,
        node: TopologyNodeId,
        scope: ExecutionScope,
    ) -> Result<ExecutionScope, ApiError> {
        Ok(match self.consultation_checkout(project, node)? {
            Some(pin) => scope.with_consultation_checkout(pin),
            None => scope,
        })
    }

    pub(super) async fn prepare_consultation_checkout(
        &self,
        project: ProjectId,
        node: TopologyNodeId,
    ) -> Result<(), ApiError> {
        let Some(pin) = self.consultation_checkout(project, node)? else {
            return Ok(());
        };
        self.consultation_root(project, node)?;
        let state = self.state()?;
        let stored = state
            .with_store(|s| s.consultation_placement(project, node))
            .map_err(|e| self.refuse(&e))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "consultation.intent_missing",
                )
            })?;
        state
            .with_store(|s| s.guard_consultation_placement(project, node))
            .map_err(|e| self.refuse(&e))?;
        let checkout = pin.checkout.clone();
        let confirmed = stored.readback.is_some();
        let readback = tokio::task::spawn_blocking(move || {
            if confirmed {
                checkout::verify(&checkout)?;
            } else {
                checkout::prepare(&checkout)?;
            }
            checkout::readback(&checkout)
        })
        .await
        .map_err(|_| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "consultation.checkout_incomplete",
            )
        })?
        .map_err(|e| self.deny(ApiErrorCode::PlacementBlocked, e.0))?;
        let mut readback = readback;
        readback["schema_version"] = 1.into();
        readback["intent_hash"] = serde_json::json!(stored.intent.hash());
        let readback = self.intent(&readback)?;
        state
            .with_store(|s| {
                s.confirm_consultation_checkout(project, node, &stored.intent, &readback)
            })
            .map_err(|e| self.refuse(&e))
    }
}
