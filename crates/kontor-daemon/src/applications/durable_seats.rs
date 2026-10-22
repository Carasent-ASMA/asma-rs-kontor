//! Durable desk seats (ASMA-8451): dated declarations, an accurate report, and
//! a reconciliation that restarts only a positively missing daemon-owned seat.
//!
//! Nothing here is a new launch path. A daemon-owned desk seat is a persistent
//! hosted seat: its declaration plans the SeatBinding whose id is the runtime
//! correlation label, a launch prepares that occupancy's intent before the
//! effect, and the native that comes back is bound, installed and observed
//! exactly as an epic's leadership seat is. What is new is the decision in
//! front of it. [`classify`] reads facts already gathered and [`decide`] turns
//! the classification into typed state; only `launch` and `recover` reach the
//! runtime, and only for a seat the caller selected.
//!
//! A Paseo-direct seat is reported from its declaration alone. No census, no
//! adoption, no restart: it is never Kontor's to touch (CON-007).
use super::*;
use kontor_api::durable_seats::{
    DeclareDurableSeatRequest, DurableSeatDeclarationOutcomeDto, DurableSeatDto,
    DurableSeatReconcileOutcomeDto, DurableSeatReportDto, ReconcileDurableSeatsRequest,
};
use kontor_core::durable_seat::{
    DurableSeatBindingFact, DurableSeatCensus, DurableSeatDecision, DurableSeatDeclaration,
    DurableSeatFacts, DurableSeatObservation, DurableSeatReason, DurableSeatRecoveryOwner,
    DurableSeatStatus, classify, decide, render_durable_seat_title,
};
use kontor_core::id::DurableSeatKey;
use kontor_runtime::durable_seat::{HostedSeatCensusRequest, HostedSeatScope};

/// One declared seat with everything read about it before any decision.
struct ObservedSeat {
    declaration: DurableSeatDeclaration,
    facts: DurableSeatFacts,
    observation: DurableSeatObservation,
    /// The current occupancy row, when there is one.
    occupancy: Option<StoredHostedTopologySeat>,
    /// Natives already in the seat's append-only route history, oldest first.
    history: Vec<ExternalId>,
}

impl Services {
    /// The desk row, or the refusal a seat operation is owed without one.
    fn durable_seat_desk(
        &self,
        project_id: ProjectId,
        desk_key: &DeskKey,
    ) -> Result<StoredDesk, ApiError> {
        self.state()?
            .with_store(|store| store.get_desk(project_id, desk_key))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no desk is ensured under this key; ensure the desk before declaring its seats",
                )
            })
    }

    pub(super) fn write_durable_seat_declaration(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        desk_key: &DeskKey,
        request: &DeclareDurableSeatRequest,
    ) -> Result<DurableSeatDeclarationOutcomeDto, ApiError> {
        let state = self.state()?;
        let project = self.project_at(project_id, request.expected_revision)?;
        let desk = self.durable_seat_desk(project_id, desk_key)?;
        let (definition, _) = self.desk_definition(&desk)?;
        let catalog = self.domain.role_catalogs.first().ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "this build ships no role catalog",
            )
        })?;
        if catalog.role(&request.role_code).is_none() {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "the role catalog declares no such role code",
            ));
        }
        let model_rung = request
            .model_route
            .as_ref()
            .map(|route| parse_runtime_model_route(route, self.fleet.current().as_deref()))
            .transpose()
            .map_err(|error| self.refuse_domain(&error))?;
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "durable_seat_declare",
            "project": project_id.to_string(),
            "desk": desk_key.as_str(),
            "seat": request.seat_key.as_str(),
            "role_code": request.role_code.as_str(),
            "seat_label": request.seat_label.as_str(),
            "model_route": request.model_route,
            "procedure": request.procedure.as_ref().map(ExternalName::as_str),
            "recovery_owner": request.recovery_owner.as_str(),
            "decided_by": request.decided_by.as_str(),
            "decided_on": request.decided_on.to_string(),
        }))?;
        let replayed = self
            .replayed(key, &intent, Some(&AggregateRef::Project { project_id }))?
            .is_some();
        let declaration = DurableSeatDeclaration {
            project_id,
            desk_key: desk_key.clone(),
            seat_key: request.seat_key.clone(),
            title: render_durable_seat_title(
                &definition.separator,
                &request.role_code,
                &request.seat_label,
            )
            .map_err(|error| self.refuse_domain(&error))?,
            role_code: request.role_code.clone(),
            seat_label: request.seat_label.clone(),
            model_rung,
            procedure: request.procedure.clone(),
            recovery_owner: request.recovery_owner,
            decided_by: request.decided_by.clone(),
            decided_on: request.decided_on,
            // Planned now, before anything is launched, so every later
            // attempt correlates by the same label. A retry whose answer was
            // lost plans another, and the store keeps the first.
            seat_binding_id: (request.recovery_owner == DurableSeatRecoveryOwner::Daemon)
                .then(SeatBindingId::generate),
            declared_at: kontor_api::now(),
        };
        declaration
            .validate(&definition.separator)
            .map_err(|error| self.refuse_domain(&error))?;
        let (stored, applied) = state
            .with_store(|store| store.declare_durable_seat(&declaration))
            .map_err(|error| self.refuse(&error))?;
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::DeclareDurableSeat,
            AggregateRef::Project { project_id },
            project.revision,
            &intent,
        )?;
        Ok(DurableSeatDeclarationOutcomeDto {
            seat: durable_seat_dto(&stored, None, None, None),
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: if replayed || applied == kontor_store::Applied::Unchanged {
                    AppliedDto::Unchanged
                } else {
                    AppliedDto::Created
                },
                revision: project.revision,
                snapshot_cursor: self.cursor()?,
            },
        })
    }

    pub(super) async fn read_durable_seats(
        &self,
        project_id: ProjectId,
        desk_key: &DeskKey,
    ) -> Result<DurableSeatReportDto, ApiError> {
        self.project_row(project_id)?;
        let desk = self.durable_seat_desk(project_id, desk_key)?;
        let declarations = self
            .state()?
            .with_store(|store| store.list_durable_seat_declarations(project_id, desk_key))
            .map_err(|error| self.refuse(&error))?;
        let mut seats = Vec::with_capacity(declarations.len());
        for declaration in declarations {
            let observed = self.observe_durable_seat(&desk, declaration).await?;
            seats.push(self.observed_seat_dto(&observed, DurableSeatDecision::None)?);
        }
        Ok(DurableSeatReportDto {
            desk_key: desk_key.as_str().to_owned(),
            seats,
        })
    }

    pub(super) async fn run_durable_seat_reconciliation(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        desk_key: &DeskKey,
        request: &ReconcileDurableSeatsRequest,
    ) -> Result<DurableSeatReconcileOutcomeDto, ApiError> {
        let _native_activity = self.native_activity()?;
        let state = self.state()?;
        let project = self.project_at(project_id, request.expected_revision)?;
        let desk = self.durable_seat_desk(project_id, desk_key)?;
        let selected: BTreeSet<DurableSeatKey> = request.seat_keys.iter().cloned().collect();
        if selected.is_empty() || selected.len() != request.seat_keys.len() {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "select at least one declared seat, each exactly once",
            ));
        }
        let mut declarations = Vec::with_capacity(selected.len());
        for seat_key in &selected {
            let declaration = state
                .with_store(|store| {
                    store.get_durable_seat_declaration(project_id, desk_key, seat_key)
                })
                .map_err(|error| self.refuse(&error))?
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::NotFound,
                        "a selected seat is not declared in this desk",
                    )
                })?;
            declarations.push(declaration);
        }
        // Refused before the runtime is consulted about anything: selecting a
        // seat Kontor does not own for a restart is a request to adopt it.
        if request.restart_missing
            && declarations
                .iter()
                .any(|declaration| declaration.recovery_owner != DurableSeatRecoveryOwner::Daemon)
        {
            return Err(self.deny(
                ApiErrorCode::Forbidden,
                "a Paseo-direct seat is recovered outside Kontor and cannot be selected for a restart",
            ));
        }
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "durable_seats_reconcile",
            "project": project_id.to_string(),
            "desk": desk_key.as_str(),
            "seats": selected.iter().map(DurableSeatKey::as_str).collect::<Vec<_>>(),
            "restart_missing": request.restart_missing,
        }))?;
        // One reconciliation at a time, and the receipt is read under the
        // guard. A second call with the same key waits here and then finds the
        // first one's receipt; a second key waits and then observes the seat
        // the first one bound. Neither reaches the runtime a second time.
        let _guard = self.durable_seat_guard.lock().await;
        let replayed = self
            .replayed(key, &intent, Some(&AggregateRef::Project { project_id }))?
            .is_some();
        let mut seats = Vec::with_capacity(declarations.len());
        for declaration in declarations {
            let observed = self.observe_durable_seat(&desk, declaration).await?;
            // The replay fence. A recorded reconciliation already acted, and
            // whatever has happened to the seat since is a new question for a
            // new key, never a reason to act again under this one.
            let decision = if replayed {
                DurableSeatDecision::None
            } else {
                decide(
                    observed.declaration.recovery_owner,
                    observed.observation,
                    request.restart_missing,
                )
            };
            if decision != DurableSeatDecision::None {
                self.launch_durable_seat(&desk, &observed, decision).await?;
            }
            seats.push(self.observed_seat_dto(&observed, decision)?);
        }
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::ReconcileDurableSeats,
            AggregateRef::Project { project_id },
            project.revision,
            &intent,
        )?;
        Ok(DurableSeatReconcileOutcomeDto {
            desk_key: desk_key.as_str().to_owned(),
            seats,
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: if replayed {
                    AppliedDto::Unchanged
                } else {
                    AppliedDto::Created
                },
                revision: project.revision,
                snapshot_cursor: self.cursor()?,
            },
        })
    }

    /// Read every fact [`classify`] needs about one seat, then classify it.
    ///
    /// Read-only. A Paseo-direct seat is classified from its declaration
    /// alone; a daemon-owned one is censused on the runtime that holds its
    /// occupancy, or the configured one when nothing was ever bound.
    async fn observe_durable_seat(
        &self,
        desk: &StoredDesk,
        declaration: DurableSeatDeclaration,
    ) -> Result<ObservedSeat, ApiError> {
        let state = self.state()?;
        let project_id = desk.project_id;
        let Some(seat_binding_id) = declaration.seat_binding_id else {
            let facts = DurableSeatFacts {
                owner: declaration.recovery_owner,
                binding: DurableSeatBindingFact::Absent,
                occupancy: None,
                fenced: BTreeSet::new(),
                launch_prepared: false,
                census: DurableSeatCensus::NotTaken,
            };
            return Ok(ObservedSeat {
                observation: classify(&facts),
                declaration,
                facts,
                occupancy: None,
                history: Vec::new(),
            });
        };
        let binding = match state
            .with_store(|store| store.get_seat_binding(project_id, seat_binding_id))
            .map_err(|error| self.refuse(&error))?
        {
            None => DurableSeatBindingFact::Absent,
            Some(seat) if seat.topology_node_id != desk.workspace_node_id => {
                return Err(self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "a declared seat's binding is hosted outside its desk's workspace",
                ));
            }
            Some(seat) if seat.lifecycle == TopologyLifecycle::Active => {
                DurableSeatBindingFact::Active
            }
            Some(_) => DurableSeatBindingFact::Retired,
        };
        let occupancy = state
            .with_store(|store| store.get_hosted_topology_seat(project_id, seat_binding_id))
            .map_err(|error| self.refuse(&error))?;
        let history = state
            .with_store(|store| {
                store.list_hosted_topology_seat_history_native_ids(project_id, seat_binding_id)
            })
            .map_err(|error| self.refuse(&error))?;
        let next_generation = u64::try_from(history.len())
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        let launch_prepared = occupancy.is_none()
            && state
                .with_store(|store| {
                    store.get_hosted_seat_launch_intent(
                        project_id,
                        seat_binding_id,
                        next_generation,
                    )
                })
                .map_err(|error| self.refuse(&error))?
                .is_some_and(|intent| intent.state == HostedSeatLaunchIntentState::Prepared);
        let census = if binding == DurableSeatBindingFact::Retired {
            // Classified before the census is read; asking would be noise.
            DurableSeatCensus::NotTaken
        } else {
            self.census_durable_seat(seat_binding_id, occupancy.as_ref())
                .await?
        };
        let facts = DurableSeatFacts {
            owner: declaration.recovery_owner,
            binding,
            occupancy: occupancy
                .as_ref()
                .map(|seat| seat.native_identity.native_id.clone()),
            fenced: history.iter().cloned().collect(),
            launch_prepared,
            census,
        };
        Ok(ObservedSeat {
            observation: classify(&facts),
            declaration,
            facts,
            occupancy,
            history,
        })
    }

    /// Ask the runtime which natives carry one seat's binding.
    ///
    /// Every way of not getting an authoritative answer is kept distinct from
    /// an empty one, because only an empty one may lead to a launch.
    async fn census_durable_seat(
        &self,
        seat_binding_id: SeatBindingId,
        occupancy: Option<&StoredHostedTopologySeat>,
    ) -> Result<DurableSeatCensus, ApiError> {
        let state = self.state()?;
        let runtime_kind = match occupancy {
            Some(seat) => seat.native_identity.runtime_kind.clone(),
            None => match self.node_runtime_kind() {
                Ok(kind) => kind,
                Err(_) => return Ok(DurableSeatCensus::Unavailable),
            },
        };
        let Some(adapter) = state.runtimes().get(&runtime_kind) else {
            return Ok(DurableSeatCensus::Unavailable);
        };
        Ok(
            match adapter
                .census_hosted_seat(&HostedSeatCensusRequest {
                    seat_binding_id,
                    requested_at: kontor_api::now(),
                })
                .await
            {
                Ok(census) => DurableSeatCensus::Taken(census.carriers),
                Err(RuntimeError::UnsupportedCapability { .. }) => DurableSeatCensus::Unavailable,
                Err(RuntimeError::CorrelationFailed) => DurableSeatCensus::Uncorrelated,
                Err(_) => DurableSeatCensus::Failed,
            },
        )
    }

    /// Launch, or recover, one daemon-owned seat's next occupancy.
    ///
    /// Everything below is the hosted-seat launch an epic's leadership seat
    /// takes, addressed to the desk instead of an epic: the desk's proved
    /// workspace container, its canonical directory, and its own native
    /// project as the parent.
    async fn launch_durable_seat(
        &self,
        desk: &StoredDesk,
        observed: &ObservedSeat,
        decision: DurableSeatDecision,
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        let project_id = desk.project_id;
        let declaration = &observed.declaration;
        let (Some(seat_binding_id), Some(model_rung)) =
            (declaration.seat_binding_id, declaration.model_rung.clone())
        else {
            return Err(self.deny(
                ApiErrorCode::Forbidden,
                "only a daemon-owned seat with a declared route is launched by Kontor",
            ));
        };
        let unmaterialized = || {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "the desk is not materialized; materialize it before reconciling its seats",
            )
        };
        let workspace = state
            .with_store(|store| store.get_topology_node(project_id, desk.workspace_node_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(unmaterialized)?;
        let container = self
            .prove_existing_bound_container(project_id, &workspace)
            .await?
            .ok_or_else(unmaterialized)?;
        let cwd = self.desk_root(desk)?;
        if container.root() != Some(&cwd) {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "the desk workspace is not rooted at the desk's canonical directory",
            ));
        }
        let parent = state
            .with_store(|store| {
                store.get_topology_node_container(project_id, desk.topology_node_id)
            })
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(unmaterialized)?;
        let adapter = state
            .runtimes()
            .get(&container.binding.identity.runtime_kind)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the runtime holding the desk's workspace is not configured",
                )
            })?;
        if !adapter.provider_available(model_rung.provider.0.as_str()) {
            return Err(ApiError::from_runtime(
                state.realm_id(),
                &RuntimeError::ProviderUnavailable {
                    provider: model_rung.provider.0.clone(),
                },
            ));
        }
        let now = kontor_api::now();
        if observed.facts.binding == DurableSeatBindingFact::Absent {
            let catalog = self.domain.role_catalogs.first().ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "this build ships no role catalog",
                )
            })?;
            let entry = catalog.role(&declaration.role_code).ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the declared role code is no longer in the role catalog",
                )
            })?;
            let role_slot_id = RoleSlotId::parse(declaration.seat_key.as_str())
                .map_err(|error| self.refuse_domain(&error))?;
            state
                .with_store(|store| {
                    store.create_seat_binding(&NewSeatBinding {
                        id: seat_binding_id,
                        project_id,
                        topology_node_id: desk.workspace_node_id,
                        role_slot_id,
                        role: CatalogRoleRef {
                            catalog_id: catalog.catalog_id,
                            catalog_revision: catalog.version,
                            role_code: entry.role_code.clone(),
                            standard_title: entry.standard_title.clone(),
                            custom_display_name: None,
                        },
                        // A desk seat is not delivery work: no task, no TeamRun.
                        task_id: None,
                        team_run_id: None,
                        attach_deadline: now
                            .checked_add(jiff::SignedDuration::from_secs(SEAT_ATTACH_SECONDS))
                            .unwrap_or(now),
                        parent_seat_binding_id: None,
                        created_at: now,
                    })
                })
                .map_err(|error| self.refuse(&error))?;
        }
        // A recorded occupant the census proved absent moves into the seat's
        // append-only history first. That is what fences it: its generation's
        // credential stops authorizing anything, and the launch below names it
        // so the runtime cannot mistake it for the new occupant.
        let mut history = observed.history.clone();
        if let Some(predecessor) = observed.occupancy.as_ref() {
            state
                .with_store(|store| {
                    store.archive_hosted_topology_seat_route(
                        predecessor,
                        now,
                        "durable desk seat native absent from an authoritative runtime census",
                    )
                })
                .map_err(|error| self.refuse(&error))?;
            history.push(predecessor.native_identity.native_id.clone());
        }
        let generation = u64::try_from(history.len())
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        // Launch intent, not live configuration, survives a lost answer and a
        // restart: a prepared intent for this generation already resolved its
        // authority, and a replay asks for exactly what that attempt did.
        let autonomy = match state
            .with_store(|store| {
                store.get_hosted_seat_launch_intent(project_id, seat_binding_id, generation)
            })
            .map_err(|error| self.refuse(&error))?
        {
            Some(intent) => intent.autonomy,
            None => freeze_hosted_seat_autonomy(adapter.declared_autonomy()),
        };
        state
            .with_store(|store| {
                store.prepare_hosted_seat_launch_intent(&StoredHostedSeatLaunchIntent {
                    project_id,
                    seat_binding_id,
                    occupancy_generation: generation,
                    autonomy,
                    model_rung: (model_rung.clone()).into(),
                    state: HostedSeatLaunchIntentState::Prepared,
                    observed_native_id: None,
                    prepared_at: now,
                    installed_at: None,
                })
            })
            .map_err(|error| self.refuse(&error))?;
        let capabilities = adapter
            .discover_capabilities()
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        let context_policy = ContextPolicySnapshot::standard(
            &capabilities.limits.context_window,
            capabilities.supports(RuntimeCapability::ContextPolicy),
            SCHEMA_VERSION,
            now,
        )
        .map_err(|error| self.refuse_domain(&error))?;
        let procedure = declaration
            .procedure
            .as_ref()
            .map(|procedure| format!(" Follow {} when work arrives.", procedure.as_str()))
            .unwrap_or_default();
        let prompt = BoundedText::parse(&format!(
            "Durable {} seat of desk {}. It belongs to no epic.{procedure} Remain reusable under SeatBinding {}. Use the generation-fenced KONTOR_AUTH value only for this seat's supported identity-bound routes and never disclose it.",
            declaration.title.as_str(),
            desk.desk_key.as_str(),
            seat_binding_id,
        ))
        .map_err(|error| self.refuse_domain(&error))?;
        let outcome = adapter
            .launch_hosted_seat(&HostedSeatLaunchRequest {
                resolved_route: None,
                seat_binding_id,
                role_slot_id: RoleSlotId::parse(declaration.seat_key.as_str())
                    .map_err(|error| self.refuse_domain(&error))?,
                display_name: declaration.title.clone(),
                container,
                cwd,
                scope: HostedSeatScope::Desk {
                    project: parent.identity,
                },
                prompt,
                role_prompt: None,
                credential: ConsultationCredential::new(
                    state
                        .credentials()
                        .seat_credential_for_generation(seat_binding_id, generation),
                ),
                fenced_predecessor_native_ids: history,
                model_rung: model_rung.clone(),
                fleet_provenance: None,
                autonomy,
                context_policy,
                requested_at: now,
            })
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        // A recovery binds the native the census already saw, or nothing.
        if decision == DurableSeatDecision::Recover
            && !matches!(
                &observed.facts.census,
                DurableSeatCensus::Taken(carriers)
                    if carriers.iter().any(|carrier| !carrier.archived
                        && carrier.native_id == outcome.identity.native_id)
            )
        {
            return Err(self.deny(
                ApiErrorCode::StaleBinding,
                "the recovered native is not the one the census found carrying the seat",
            ));
        }
        let hosted = StoredHostedTopologySeat {
            project_id,
            seat_binding_id,
            model_rung,
            native_identity: outcome.identity,
            autonomy,
            provider_session_id: outcome.provider_session_id,
            observed_at: outcome.observed_at,
        };
        state
            .with_store(|store| store.bind_hosted_topology_seat(&hosted))
            .map_err(|error| self.refuse(&error))?;
        state
            .with_store(|store| {
                store.install_hosted_seat_launch_intent(
                    project_id,
                    seat_binding_id,
                    generation,
                    &hosted.native_identity.native_id,
                    hosted.observed_at,
                    &hosted.model_rung.clone().into(),
                )
            })
            .map_err(|error| self.refuse(&error))?;
        state
            .with_store(|store| {
                store.observe_seat_binding(
                    project_id,
                    seat_binding_id,
                    &SeatLivenessObservation {
                        attached_at: Some(hosted.observed_at),
                        runtime_reported: Some(kontor_core::state::ObservedRunState::Running),
                        ..SeatLivenessObservation::default()
                    },
                    hosted.observed_at,
                )
            })
            .map_err(|error| self.refuse(&error))?;
        Ok(())
    }

    /// One observed seat as the wire reports it, with its occupancy read
    /// after any effect this call had.
    fn observed_seat_dto(
        &self,
        observed: &ObservedSeat,
        decision: DurableSeatDecision,
    ) -> Result<DurableSeatDto, ApiError> {
        let state = self.state()?;
        let declaration = &observed.declaration;
        let occupancy = match declaration.seat_binding_id {
            Some(seat_binding_id) => {
                let project_id = declaration.project_id;
                let seat = state
                    .with_store(|store| store.get_hosted_topology_seat(project_id, seat_binding_id))
                    .map_err(|error| self.refuse(&error))?;
                let generation = state
                    .with_store(|store| {
                        store.hosted_topology_seat_occupancy_generation(project_id, seat_binding_id)
                    })
                    .map_err(|error| self.refuse(&error))?;
                seat.map(|seat| (seat.native_identity.native_id, generation))
            }
            None => None,
        };
        let mut dto = durable_seat_dto(
            declaration,
            Some(observed.observation.status),
            Some(observed.observation.reason),
            Some(decision),
        );
        if let Some((native_id, generation)) = occupancy {
            dto.native_id = Some(native_id.as_str().to_owned());
            dto.occupancy_generation = generation;
        }
        Ok(dto)
    }
}

fn durable_seat_dto(
    declaration: &DurableSeatDeclaration,
    status: Option<DurableSeatStatus>,
    reason: Option<DurableSeatReason>,
    decision: Option<DurableSeatDecision>,
) -> DurableSeatDto {
    DurableSeatDto {
        seat_key: declaration.seat_key.as_str().to_owned(),
        title: declaration.title.as_str().to_owned(),
        role_code: declaration.role_code.as_str().to_owned(),
        seat_label: declaration.seat_label.as_str().to_owned(),
        recovery_owner: declaration.recovery_owner,
        model_route: declaration
            .model_rung
            .as_ref()
            .map(|rung| RuntimeModelRouteRequest {
                provider: rung.provider.0.clone(),
                model: rung.model.0.clone(),
                effort: rung.effort.map(|effort| effort.as_str().to_owned()),
            }),
        procedure: declaration
            .procedure
            .as_ref()
            .map(|procedure| procedure.as_str().to_owned()),
        decided_by: declaration.decided_by.as_str().to_owned(),
        decided_on: declaration.decided_on.to_string(),
        declared_at: kontor_core::id::format_utc_timestamp(declaration.declared_at),
        seat_binding_id: declaration.seat_binding_id.map(|id| id.to_string()),
        native_id: None,
        occupancy_generation: None,
        status,
        reason,
        decision,
    }
}
