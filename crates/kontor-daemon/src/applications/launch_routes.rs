//! Resolve only new placements. Runtime re-entry reads the owning private intent.

use super::*;
use kontor_core::launch_route::{ResolvedRoute, StoredLaunchRoute};

type ReplacementLaunchIntent = (
    StoredLaunchRoute,
    Option<AccountProfileId>,
    Option<crate::fleet::FleetDecision>,
);

fn public_route_evidence(
    route: &ResolvedRoute,
    document: &CanonicalDocument,
) -> Result<CanonicalDocument, &'static str> {
    let mut evidence: serde_json::Value = document
        .deserialize()
        .map_err(|_| "host_route_readback_invalid")?;
    if let Some(reason) = evidence["read"]["attention_reason"].as_str() {
        evidence["read"]["attention_reason"] = serde_json::json!(
            reason
                .replace(&route.native_alias, &route.slot)
                .replace(&route.route_model_id, &route.canonical_model)
        );
    }
    if evidence["read"]["provider"].as_str().is_none()
        && evidence["read"]["model"].as_str().is_none()
    {
        return CanonicalDocument::from_value(&evidence).map_err(|_| "host_route_readback_invalid");
    }
    if evidence["read"]["provider"]
        .as_str()
        .is_some_and(|value| value != route.native_alias)
        || evidence["read"]["model"]
            .as_str()
            .is_some_and(|value| value != route.route_model_id)
        || evidence["read"]["effective_thinking"]
            .as_str()
            .is_some_and(|value| {
                route
                    .canonical_effort
                    .is_some_and(|effort| value != effort.as_str())
            })
    {
        return Err("host_route_readback_mismatch");
    }
    evidence["projection"] = serde_json::json!("canonical");
    if evidence["read"]["provider"].is_string() {
        evidence["read"]["provider"] = serde_json::json!(route.slot);
    }
    if evidence["read"]["model"].is_string() {
        evidence["read"]["model"] = serde_json::json!(route.canonical_model);
    }
    CanonicalDocument::from_value(&evidence).map_err(|_| "host_route_readback_invalid")
}

impl Services {
    pub(super) fn consultation_effect_route<'a>(
        &self,
        route: &'a StoredLaunchRoute,
    ) -> Result<Option<&'a ResolvedRoute>, ApiError> {
        route
            .validate()
            .map_err(|error| self.refuse_domain(&error))?;
        if self.selected_config.is_some() && route.frozen().is_none() {
            return Err(self.deny(ApiErrorCode::PlacementBlocked, "host_route_legacy_unproven"));
        }
        Ok(route.frozen())
    }

    pub(super) fn frozen_leadership_route(
        &self,
        roster: &CoreTeamRevision,
        seat: &CoreTeamSeat,
        binding_id: SeatBindingId,
        generation: u64,
        rung: &ModelRung,
    ) -> Result<Option<LeadershipRoute>, ApiError> {
        let Some(row) = self
            .fleet
            .leadership_decision(&binding_id.to_string(), generation)
            .map_err(|_| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "host_route_decision_unreadable",
                )
            })?
        else {
            return Ok(None);
        };
        if row.provider != rung.provider.0
            || row.model != rung.model.0
            || row.effort.as_deref() != rung.effort.map(|effort| effort.as_str())
        {
            return Err(self.deny(ApiErrorCode::StaleBinding, "host_route_binding_mismatch"));
        }
        let pinned = roster
            .canonicalize()
            .map_err(|error| self.refuse_domain(&error))?;
        let key =
            kontor_fleet::LeadershipKey::for_pinned_seat(&pinned, &seat.role_slot_id, &seat.role)
                .map_err(|error| self.refuse_fleet_policy(&error))?;
        if row.core_team_revision_hash != key.core_team_revision_hash().as_str()
            || row.role_slot_id != seat.role_slot_id.as_str()
        {
            return Err(self.deny(ApiErrorCode::StaleBinding, "host_route_binding_mismatch"));
        }
        Ok(Some(LeadershipRoute {
            key,
            provenance: kontor_fleet::FleetProvenance {
                policy_hash: ContentHash::parse(&row.fleet_hash)
                    .map_err(|error| self.refuse_domain(&error))?,
                schema_version: row.policy_schema_version,
                binding_key: row.binding_key,
                chain: row.chain,
            },
            route: kontor_fleet::FleetRoute {
                rung: rung.clone(),
                step: row.step,
                sub_step: row.sub_step,
                vendor: row.vendor,
            },
            source_bundle_hash: row
                .source_bundle_hash
                .as_deref()
                .map(ContentHash::parse)
                .transpose()
                .map_err(|error| self.refuse_domain(&error))?,
            eligibility: row.eligibility,
        }))
    }

    pub(super) fn frozen_materialized_route(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        roster: &CoreTeamRevision,
        seat: &CoreTeamSeat,
    ) -> Result<Option<(ModelRung, Option<LeadershipRoute>)>, ApiError> {
        let state = self.state()?;
        let kind = self.epic_control_kind(project_id, epic_id)?;
        let nodes = state
            .with_store(|store| store.list_topology_nodes(project_id, Some(epic_id)))
            .map_err(|error| self.refuse(&error))?;
        let Some(node) = nodes
            .iter()
            .find(|node| node.kind == kind && node.task_id.is_none())
        else {
            return Ok(None);
        };
        let bindings = state
            .with_store(|store| store.list_seat_bindings(project_id, node.id))
            .map_err(|error| self.refuse(&error))?;
        let Some(binding) = bindings
            .iter()
            .find(|binding| binding.is_non_terminal() && binding.role_slot_id == seat.role_slot_id)
        else {
            return Ok(None);
        };
        let generation = state
            .with_store(|store| {
                store.hosted_topology_seat_occupancy_generation(project_id, binding.id)
            })
            .map_err(|error| self.refuse(&error))?
            .unwrap_or(FIRST_HOSTED_OCCUPANCY);
        let Some(intent) = state
            .with_store(|store| {
                store.get_hosted_seat_launch_intent(project_id, binding.id, generation)
            })
            .map_err(|error| self.refuse(&error))?
        else {
            return Ok(None);
        };
        let rung = intent.model_rung.canonical_rung();
        let leadership =
            self.frozen_leadership_route(roster, seat, binding.id, generation, &rung)?;
        Ok(Some((rung, leadership)))
    }

    pub(super) fn replacement_launch_intent(
        &self,
        project_id: ProjectId,
        key: &IdempotencyKey,
    ) -> Result<Option<ReplacementLaunchIntent>, ApiError> {
        let Some(receipt) = self
            .state()?
            .with_store(|store| store.get_receipt_by_key(key))
            .map_err(|error| self.refuse(&error))?
        else {
            return Ok(None);
        };
        if receipt.project_id != project_id || receipt.kind != CommandKind::ReplaceSeat {
            return Err(self.deny(
                ApiErrorCode::IdempotencyConflict,
                "the key belongs to another command",
            ));
        }
        let value: serde_json::Value = receipt
            .intent
            .deserialize()
            .map_err(|error| self.refuse_domain(&error))?;
        let Some(route) = value.get("launch_route") else {
            if self.selected_config.is_some() {
                return Err(self.deny(ApiErrorCode::PlacementBlocked, "host_route_legacy_unproven"));
            }
            return Ok(None);
        };
        let route: StoredLaunchRoute = serde_json::from_value(route.clone()).map_err(|_| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "host_route_snapshot_invalid",
            )
        })?;
        let account = value
            .get("successor_account_profile_id")
            .and_then(serde_json::Value::as_str)
            .map(AccountProfileId::parse)
            .transpose()
            .map_err(|error| self.refuse_domain(&error))?;
        let decision = value
            .get("fleet_decision")
            .map(|value| serde_json::from_value(value.clone()))
            .transpose()
            .map_err(|_| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "host_route_snapshot_invalid",
                )
            })?;
        Ok(Some((route, account, decision)))
    }

    pub(crate) fn delivery_private_route(
        &self,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
    ) -> Result<Option<ResolvedRoute>, ApiError> {
        let run = self
            .state()?
            .with_store(|store| store.get_agent_run(project_id, agent_run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| self.deny(ApiErrorCode::NotFound, "the agent run no longer exists"))?;
        Ok(self
            .frozen_delivery_decision(run.team_run_id, agent_run_id)?
            .and_then(|decision| decision.launch_route)
            .and_then(|route| route.frozen().cloned()))
    }

    pub(super) fn canonical_delivery_observation(
        &self,
        project_id: ProjectId,
        observation: &ControlPlaneObservation,
    ) -> Result<ControlPlaneObservation, ApiError> {
        let mut result = observation.clone();
        if let Some(route) = self.delivery_private_route(project_id, observation.agent_run_id)? {
            result.evidence = public_route_evidence(&route, &result.evidence)
                .map_err(|rule| self.deny(ApiErrorCode::StaleBinding, rule))?;
        }
        Ok(result)
    }

    pub(super) fn frozen_delivery_choice(
        &self,
        team_run_id: TeamRunId,
        agent_run_id: AgentRunId,
    ) -> Result<Option<(ModelRung, Option<AccountProfileId>)>, ApiError> {
        self.frozen_delivery_decision(team_run_id, agent_run_id)?
            .map(|decision| {
                let route = decision.launch_route.ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "host_route_snapshot_missing",
                    )
                })?;
                if self.selected_config.is_some() && route.frozen().is_none() {
                    return Err(
                        self.deny(ApiErrorCode::PlacementBlocked, "host_route_legacy_unproven")
                    );
                }
                let account = decision
                    .account_profile_id
                    .as_deref()
                    .map(AccountProfileId::parse)
                    .transpose()
                    .map_err(|error| self.refuse_domain(&error))?;
                Ok((route.canonical_rung(), account))
            })
            .transpose()
    }

    pub(super) fn frozen_delivery_decision(
        &self,
        team_run_id: TeamRunId,
        agent_run_id: AgentRunId,
    ) -> Result<Option<crate::fleet::FleetDecision>, ApiError> {
        self.fleet
            .private_launch_decision(&team_run_id.to_string(), &agent_run_id.to_string())
            .map_err(|_| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "host_route_decision_unreadable",
                )
            })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn delivery_launch_route(
        &self,
        team_run_id: TeamRunId,
        agent_run_id: AgentRunId,
        slot: &RoleSlotId,
        adapter: &dyn RuntimeAdapter,
        rung: &ModelRung,
        account: Option<AccountProfileId>,
        now: Timestamp,
        preplanned: Option<&StoredLaunchRoute>,
        mut decision: Option<crate::fleet::FleetDecision>,
    ) -> Result<StoredLaunchRoute, ApiError> {
        if let Some(decision) = self.frozen_delivery_decision(team_run_id, agent_run_id)? {
            let route = decision.launch_route.ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "host_route_snapshot_missing",
                )
            })?;
            if self.selected_config.is_some() && route.frozen().is_none() {
                return Err(self.deny(ApiErrorCode::PlacementBlocked, "host_route_legacy_unproven"));
            }
            if route.canonical_rung() != *rung
                || preplanned.is_some_and(|requested| requested != &route)
                || decision.account_profile_id != account.map(|id| id.to_string())
            {
                return Err(self.deny(
                    ApiErrorCode::StaleBinding,
                    "host_route_substitution_refused",
                ));
            }
            return Ok(route);
        }
        if self.selected_config.is_some()
            && preplanned.is_none()
            && self
                .fleet
                .decision_for(&team_run_id.to_string(), &agent_run_id.to_string())
                .is_some()
        {
            return Err(self.deny(ApiErrorCode::PlacementBlocked, "host_route_legacy_unproven"));
        }
        let mut frozen_bundle = decision
            .as_ref()
            .and_then(|row| row.source_bundle_hash.clone());
        let route = if let Some(route) = preplanned {
            if route.canonical_rung() != *rung {
                return Err(self.deny(ApiErrorCode::StaleBinding, "host_route_binding_mismatch"));
            }
            route.clone()
        } else {
            let placement = if decision.is_none() && self.selected_config.is_some() {
                self.fleet_placement()
                    .map_err(|error| self.refuse_domain(&error))?
            } else {
                None
            };
            let hash = decision
                .as_ref()
                .map(|row| ContentHash::parse(&row.fleet_hash))
                .transpose()
                .map_err(|error| self.refuse_domain(&error))?;
            let bundle = decision
                .as_ref()
                .and_then(|row| row.source_bundle_hash.as_deref())
                .map(ContentHash::parse)
                .transpose()
                .map_err(|error| self.refuse_domain(&error))?;
            if frozen_bundle.is_none() {
                frozen_bundle = placement
                    .as_ref()
                    .and_then(|source| source.source_bundle_hash.as_ref().map(ToString::to_string));
            }
            self.freeze_launch_route(
                adapter,
                rung,
                hash.as_ref()
                    .or_else(|| placement.as_ref().map(|source| source.policy.hash())),
                bundle.as_ref().or_else(|| {
                    placement
                        .as_ref()
                        .and_then(|source| source.source_bundle_hash.as_ref())
                }),
                now,
            )?
        };
        if decision.is_none()
            && let Some(resolved) = route.frozen()
        {
            decision = Some(crate::fleet::FleetDecision {
                launch_route: None,
                team_run_id: team_run_id.to_string(),
                agent_run_id: agent_run_id.to_string(),
                binding_key: format!("team/{team_run_id}/{}", slot.as_str()),
                role_slot: slot.as_str().to_owned(),
                fleet_hash: resolved.policy_hash.to_string(),
                chain: None,
                source_bundle_hash: frozen_bundle.clone(),
                step: 0,
                sub_step: 0,
                provider: rung.provider.0.clone(),
                model: rung.model.0.clone(),
                effort: rung.effort.map(|effort| effort.as_str().to_owned()),
                vendor: resolved.canonical_vendor.clone(),
                account_profile_id: account.map(|id| id.to_string()),
                decided_at: now.to_string(),
                eligibility: None,
            });
        }
        if self.selected_config.is_some() && route.frozen().is_none() {
            return Err(self.deny(ApiErrorCode::PlacementBlocked, "host_route_legacy_unproven"));
        }
        if let Some(mut decision) = decision {
            if decision.provider != rung.provider.0
                || decision.model != rung.model.0
                || decision.effort.as_deref() != rung.effort.map(|effort| effort.as_str())
                || route
                    .frozen()
                    .is_some_and(|route| route.policy_hash.as_str() != decision.fleet_hash)
            {
                return Err(self.deny(ApiErrorCode::StaleBinding, "host_route_binding_mismatch"));
            }
            if decision.source_bundle_hash.is_none() {
                decision.source_bundle_hash = frozen_bundle;
            }
            decision.team_run_id = team_run_id.to_string();
            decision.agent_run_id = agent_run_id.to_string();
            decision.account_profile_id = account.map(|id| id.to_string());
            if route.frozen().is_some() {
                decision.launch_route = Some(route.clone());
            }
            self.fleet.record_decision(&decision).map_err(|_| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "host_route_substitution_refused",
                )
            })?;
        }
        Ok(route)
    }

    pub(super) fn host_ordered_resolution(
        &self,
        policy: &kontor_fleet::FleetSnapshot,
        resolution: &kontor_fleet::FleetResolution,
        source_bundle_hash: Option<&ContentHash>,
        now: Timestamp,
    ) -> kontor_core::DomainResult<kontor_fleet::FleetResolution> {
        let mut result = resolution.clone();
        if let Some(config) = &self.selected_config {
            let host =
                config
                    .host_routes
                    .as_ref()
                    .ok_or(kontor_core::DomainError::MissingEvidence {
                        subject: "ResolvedRoute",
                        rule: "host_route_unavailable",
                    })?;
            let candidates = self
                .fleet
                .host_candidates(host, policy, resolution, source_bundle_hash, now)
                .map_err(|error| kontor_core::DomainError::MissingEvidence {
                    subject: "ResolvedRoute",
                    rule: match error {
                        crate::fleet::FleetError::Invalid { rule } => rule,
                        _ => "host_route_source_unavailable",
                    },
                })?;
            let state = self
                .state()
                .map_err(|_| kontor_core::DomainError::MissingEvidence {
                    subject: "ResolvedRoute",
                    rule: "host_route_runtime_unavailable",
                })?;
            let kind = self.node_runtime_kind().map_err(|_| {
                kontor_core::DomainError::MissingEvidence {
                    subject: "ResolvedRoute",
                    rule: "host_route_runtime_unavailable",
                }
            })?;
            let adapter =
                state
                    .runtimes()
                    .get(&kind)
                    .ok_or(kontor_core::DomainError::MissingEvidence {
                        subject: "ResolvedRoute",
                        rule: "host_route_runtime_unavailable",
                    })?;
            result.routes = candidates
                .into_iter()
                .filter_map(|(route, frozen)| {
                    adapter
                        .validate_launch_route(&frozen)
                        .is_ok()
                        .then_some(route)
                })
                .collect();
            if result.routes.is_empty() {
                return Err(kontor_core::DomainError::MissingEvidence {
                    subject: "ResolvedRoute",
                    rule: "host_route_runtime_unavailable",
                });
            }
        }
        Ok(result)
    }

    pub(super) fn hosted_message_route(
        &self,
        project_id: ProjectId,
        seat_binding_id: SeatBindingId,
        identity: &kontor_core::state::NativeRuntimeIdentity,
    ) -> Result<Option<ResolvedRoute>, ApiError> {
        let route = self.hosted_private_route_for_native(project_id, seat_binding_id, identity)?;
        if self.selected_config.is_some() && route.is_none() {
            return Err(self.deny(ApiErrorCode::PlacementBlocked, "host_route_legacy_unproven"));
        }
        Ok(route)
    }

    pub(super) fn hosted_private_route_for_native(
        &self,
        project_id: ProjectId,
        seat_binding_id: SeatBindingId,
        identity: &kontor_core::state::NativeRuntimeIdentity,
    ) -> Result<Option<ResolvedRoute>, ApiError> {
        let state = self.state()?;
        let intents = state
            .with_store(|store| store.list_hosted_seat_launch_intents(project_id, seat_binding_id))
            .map_err(|error| self.refuse(&error))?;
        let matches: Vec<_> = intents
            .iter()
            .filter(|intent| intent.observed_native_id.as_ref() == Some(&identity.native_id))
            .collect();
        match matches.as_slice() {
            [intent] => return Ok(intent.model_rung.frozen().cloned()),
            [] => {}
            _ => return Err(self.deny(ApiErrorCode::StaleBinding, "host_route_binding_mismatch")),
        }
        let current = state
            .with_store(|store| store.get_hosted_topology_seat(project_id, seat_binding_id))
            .map_err(|error| self.refuse(&error))?;
        if current
            .as_ref()
            .is_some_and(|row| &row.native_identity == identity)
        {
            let generation = state
                .with_store(|store| {
                    store.hosted_topology_seat_occupancy_generation(project_id, seat_binding_id)
                })
                .map_err(|error| self.refuse(&error))?;
            if let Some(generation) = generation {
                return self.hosted_private_route(project_id, seat_binding_id, generation);
            }
        }
        // An adopted historical occupancy has no manufactured launch snapshot.
        if intents.is_empty() {
            return Ok(None);
        }
        Err(self.deny(ApiErrorCode::StaleBinding, "host_route_binding_mismatch"))
    }

    pub(super) fn freeze_launch_route(
        &self,
        adapter: &dyn RuntimeAdapter,
        rung: &ModelRung,
        policy_hash: Option<&ContentHash>,
        source_bundle_hash: Option<&ContentHash>,
        now: Timestamp,
    ) -> Result<StoredLaunchRoute, ApiError> {
        let Some(config) = &self.selected_config else {
            return Ok(rung.clone().into());
        };
        let host = config
            .host_routes
            .as_ref()
            .ok_or_else(|| self.deny(ApiErrorCode::PlacementBlocked, "host_route_unavailable"))?;
        let route = self
            .fleet
            .freeze_host_route(host, rung, policy_hash, source_bundle_hash, now)
            .map_err(|error| {
                self.refuse_domain(&kontor_core::DomainError::MissingEvidence {
                    subject: "ResolvedRoute",
                    rule: match error {
                        crate::fleet::FleetError::Invalid { rule } => rule,
                        _ => "host_route_source_unavailable",
                    },
                })
            })?;
        adapter
            .validate_launch_route(&route)
            .map_err(|error| match error {
                RuntimeError::ProviderUnavailable { .. }
                | RuntimeError::UnsupportedCapability { .. }
                | RuntimeError::PermissionModeUnsupported { .. } => self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "host_route_runtime_unavailable",
                ),
                error => ApiError::from_runtime(self.realm_id, &error),
            })?;
        StoredLaunchRoute::resolved(route).map_err(|error| self.refuse_domain(&error))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn hosted_launch_route(
        &self,
        project_id: ProjectId,
        seat_binding_id: SeatBindingId,
        generation: u64,
        adapter: &dyn RuntimeAdapter,
        rung: &ModelRung,
        policy_hash: Option<&ContentHash>,
        source_bundle_hash: Option<&ContentHash>,
        now: Timestamp,
    ) -> Result<StoredLaunchRoute, ApiError> {
        if let Some(intent) = self
            .state()?
            .with_store(|store| {
                store.get_hosted_seat_launch_intent(project_id, seat_binding_id, generation)
            })
            .map_err(|error| self.refuse(&error))?
        {
            if intent.model_rung.canonical_rung() != *rung {
                return Err(self.deny(ApiErrorCode::StaleBinding, "host_route_binding_mismatch"));
            }
            if self.selected_config.is_some() && intent.model_rung.frozen().is_none() {
                return Err(self.deny(ApiErrorCode::PlacementBlocked, "host_route_legacy_unproven"));
            }
            intent
                .model_rung
                .validate()
                .map_err(|error| self.refuse_domain(&error))?;
            return Ok(intent.model_rung);
        }
        self.freeze_launch_route(adapter, rung, policy_hash, source_bundle_hash, now)
    }

    pub(super) fn hosted_private_route(
        &self,
        project_id: ProjectId,
        seat_binding_id: SeatBindingId,
        occupancy_generation: u64,
    ) -> Result<Option<ResolvedRoute>, ApiError> {
        let intent = self
            .state()?
            .with_store(|store| {
                store.get_hosted_seat_launch_intent(
                    project_id,
                    seat_binding_id,
                    occupancy_generation,
                )
            })
            .map_err(|error| self.refuse(&error))?;
        Ok(intent.and_then(|intent| intent.model_rung.frozen().cloned()))
    }
}

#[cfg(test)]
mod tests {
    use super::super::domain_bundle_tests::{resolve, start};
    use super::*;
    use kontor_runtime::fake::ScriptedFakeRuntime;

    #[test]
    fn private_delivery_snapshot_replays_after_host_and_activation_changes() {
        let directory = tempfile::tempdir().unwrap();
        let folder = super::super::config_fixture::write_config_folder(directory.path());
        let mut host: serde_yaml_ng::Value = serde_yaml_ng::from_str(
            &std::fs::read_to_string(folder.join("host.example.yml")).unwrap(),
        )
        .unwrap();
        host.as_mapping_mut()
            .unwrap()
            .remove(serde_yaml_ng::Value::String("checkouts".into()));
        std::fs::write(
            folder.join("host.yml"),
            serde_yaml_ng::to_string(&host).unwrap(),
        )
        .unwrap();
        let root = directory.path().join("state");
        let daemon = start(&root, &folder);
        let services = daemon.jira_reconciler();
        let (bundle, hash, _) = resolve(&services, &folder);
        services.fleet.publish_bundle(&bundle).unwrap();
        services
            .fleet
            .activate_bundle(&hash, &crate::fleet::ActivationFence::default())
            .unwrap();
        let adapter = ScriptedFakeRuntime::new(kontor_runtime::capability::RuntimeCapabilities {
            trust_grade: kontor_runtime::capability::TrustGrade::A,
            supported: BTreeSet::new(),
            account_env: false,
            limits: kontor_runtime::capability::RuntimeLimits {
                max_message_bytes: 1024,
                max_history_page: 100,
                max_concurrent_sessions: 8,
                context_window: kontor_core::spec::ContextWindowBounds::unknown(),
            },
        });
        let rung = ModelRung {
            provider: ProviderRef("codex-primary".into()),
            model: ModelRef("gpt-example".into()),
            effort: Some(kontor_core::spec::EffortLevel::Xhigh),
        };
        let team = TeamRunId::generate();
        let run = AgentRunId::generate();
        let slot = RoleSlotId::parse("synthetic-worker").unwrap();
        let route = services
            .delivery_launch_route(
                team,
                run,
                &slot,
                &adapter,
                &rung,
                None,
                kontor_api::now(),
                None,
                None,
            )
            .unwrap();
        assert!(route.frozen().is_some());
        let bytes = serde_json::to_vec(&route).unwrap();
        let log = root.join("fleet-decisions").join(format!("{team}.jsonl"));
        let log_before = std::fs::read(&log).unwrap();
        let original = route.frozen().unwrap().native_alias.clone();
        let host = std::fs::read_to_string(folder.join("host.yml"))
            .unwrap()
            .replace(&original, "opaque-edited-selector");
        std::fs::write(folder.join("host.yml"), host).unwrap();
        // No current source is available at all: an accidental re-resolution fails.
        std::fs::remove_file(root.join(kontor_fleet_activation::FLEET_ACTIVATION_FILE)).unwrap();
        std::fs::write(
            folder.join("orchestration/fleet.yml"),
            "schema_version: 2\n",
        )
        .unwrap();
        drop(services);
        drop(daemon);
        let restarted = start(&root, &folder);
        let services = restarted.jira_reconciler();
        assert_eq!(
            services.frozen_delivery_choice(team, run).unwrap(),
            Some((rung.clone(), None))
        );
        let replay = services
            .delivery_launch_route(
                team,
                run,
                &slot,
                &adapter,
                &rung,
                None,
                kontor_api::now(),
                None,
                None,
            )
            .unwrap();
        assert_eq!(serde_json::to_vec(&replay).unwrap(), bytes);
        let mut substitute = replay.frozen().unwrap().clone();
        substitute.native_alias = "opaque-edited-selector".into();
        let substitute = StoredLaunchRoute::resolved(substitute).unwrap();
        let error = services
            .delivery_launch_route(
                team,
                run,
                &slot,
                &adapter,
                &rung,
                None,
                kontor_api::now(),
                Some(&substitute),
                None,
            )
            .unwrap_err();
        assert_eq!(error.code, ApiErrorCode::StaleBinding);
        assert!(!format!("{error:?}").contains(&original));
        assert_eq!(std::fs::read(log).unwrap(), log_before);
    }
    #[test]
    fn equal_policy_hashes_cannot_join_different_activated_bundle_sources() {
        let directory = tempfile::tempdir().unwrap();
        let folder = super::super::config_fixture::write_config_folder(directory.path());
        let mut host: serde_yaml_ng::Value = serde_yaml_ng::from_str(
            &std::fs::read_to_string(folder.join("host.example.yml")).unwrap(),
        )
        .unwrap();
        host.as_mapping_mut()
            .unwrap()
            .remove(serde_yaml_ng::Value::String("checkouts".into()));
        std::fs::write(
            folder.join("host.yml"),
            serde_yaml_ng::to_string(&host).unwrap(),
        )
        .unwrap();
        let root = directory.path().join("state");
        let daemon = start(&root, &folder);
        let services = daemon.jira_reconciler();
        let (first, first_hash, _) = resolve(&services, &folder);
        services.fleet.publish_bundle(&first).unwrap();
        services
            .fleet
            .activate_bundle(&first_hash, &crate::fleet::ActivationFence::default())
            .unwrap();
        let policy_hash = services.fleet.policy().unwrap().unwrap().hash().clone();
        let entry = folder.join(crate::config_folder::ENTRY);
        let changed = format!(
            "{}\n# a distinct retained source with the same policy\n",
            std::fs::read_to_string(&entry).unwrap()
        );
        std::fs::write(entry, changed).unwrap();
        let (second, second_hash, _) = resolve(&services, &folder);
        assert_ne!(first_hash, second_hash);
        assert_eq!(second.manifest.policy_hash, policy_hash);
        services.fleet.publish_bundle(&second).unwrap();
        services
            .fleet
            .activate_bundle(
                &second_hash,
                &crate::fleet::ActivationFence {
                    policy_hash: Some(policy_hash.clone()),
                    source_bundle_hash: Some(first_hash.clone()),
                },
            )
            .unwrap();
        let rung = ModelRung {
            provider: ProviderRef("codex-primary".into()),
            model: ModelRef("gpt-example".into()),
            effort: Some(kontor_core::spec::EffortLevel::Xhigh),
        };
        let host = services
            .selected_config
            .as_ref()
            .unwrap()
            .host_routes
            .as_ref()
            .unwrap();
        assert!(matches!(
            services.fleet.freeze_host_route(
                host,
                &rung,
                Some(&policy_hash),
                None,
                kontor_api::now()
            ),
            Err(crate::fleet::FleetError::Invalid {
                rule: "host_route_source_unpinned"
            })
        ));
        assert!(
            services
                .fleet
                .freeze_host_route(host, &rung, None, None, kontor_api::now())
                .is_ok()
        );
        assert!(matches!(
            services.fleet.freeze_host_route(
                host,
                &rung,
                Some(&policy_hash),
                Some(&first_hash),
                kontor_api::now()
            ),
            Err(crate::fleet::FleetError::Invalid {
                rule: "host_route_policy_mismatch"
            })
        ));
        assert!(
            services
                .fleet
                .freeze_host_route(
                    host,
                    &rung,
                    Some(&policy_hash),
                    Some(&second_hash),
                    kontor_api::now()
                )
                .is_ok()
        );
    }
    #[test]
    fn public_evidence_projects_selectors_and_preserves_uncertain_observations() {
        let route = ResolvedRoute {
            schema_version: 1,
            slot: "codex-primary".into(),
            route_provider: "codex".into(),
            route_harness: "paseo".into(),
            native_alias: "opaque-selected".into(),
            qualification: None,
            route_model_id: "opaque-model".into(),
            canonical_model: "gpt-example".into(),
            canonical_effort: None,
            canonical_vendor: "openai".into(),
            policy_hash: ContentHash::of(b"policy"),
            host_route_hash: ContentHash::of(b"host"),
            resolved_at: kontor_api::now(),
        };
        let digest = ContentHash::of(b"native readback");
        let input = CanonicalDocument::from_value(&serde_json::json!({"schema_version":1,"raw_digest":digest,"read":{"provider":route.native_alias,"model":route.route_model_id,"attention_reason":"opaque-selected could not serve opaque-model"}})).unwrap();
        let public = public_route_evidence(&route, &input).unwrap();
        assert!(!public.json().contains("opaque-"));
        let value: serde_json::Value = public.deserialize().unwrap();
        assert_eq!(value["read"]["provider"], route.slot);
        assert_eq!(value["read"]["model"], route.canonical_model);
        assert_eq!(value["raw_digest"], digest.as_str());
        assert_eq!(value["projection"], "canonical");
        for read in [
            serde_json::Value::Null,
            serde_json::json!({"provider":null,"model":null}),
        ] {
            let unknown = CanonicalDocument::from_value(&serde_json::json!({"schema_version":1,"read":read,"normalized":{"run_state":"unknown"}})).unwrap();
            assert_eq!(public_route_evidence(&route, &unknown).unwrap(), unknown);
        }
        let substituted = CanonicalDocument::from_value(&serde_json::json!({"schema_version":1,"read":{"provider":"opaque-other","model":route.route_model_id}})).unwrap();
        assert_eq!(
            public_route_evidence(&route, &substituted),
            Err("host_route_readback_mismatch")
        );
    }
}
