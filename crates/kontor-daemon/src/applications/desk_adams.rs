//! Desk Adams: epic and ad-hoc leadership placed on a durable desk (ASMA-8452).
//!
//! An epic pinned to a Team Definition that declares `desk_leadership` has no
//! control plane of its own. Its leadership subject is a node directly below
//! the declared desk's workspace: the subject owns no native container, its
//! seats run in the workspace's, and it is the subject, never the shared
//! workspace, that says which epic a seat acts for. Sharing the workspace
//! confers no authority on anything. An epic pinned to any other definition
//! keeps the historical ECP, byte for byte.
use super::*;
use kontor_api::applications::{
    AdHocDeskAdamDto, AdHocDeskAdamOpenRequest, CoreTeamMaterializeRequest,
    CoreTeamSeatRouteRequest, DeskAdamHandoffDto, DeskAdamHandoffRequest, DeskAdamOriginRequest,
    DeskAdamSubjectDto, TeamDefinitionRefDto,
};
use kontor_core::repository::StoredDeskAdamSubject;
use kontor_core::spec::TeamDeskLeadership;
use kontor_runtime::adapter::HostedSeatRetirePlacement;
use kontor_runtime::durable_seat::HostedSeatCensusRequest;

/// The desk leadership that governs one epic.
pub(super) struct DeskLed {
    /// The exact definition revision that declares it.
    pub(super) definition: TeamDefinitionSnapshot,
    /// What that revision declares.
    pub(super) leadership: TeamDeskLeadership,
}

impl Services {
    /// The desk leadership that governs one epic, or `None` when the epic
    /// keeps the historical control plane.
    ///
    /// Read from the epic's pinned Team Definition. An epic with neither a pin
    /// nor a node is about to pin the project's stored selection, so that
    /// selection answers for it; an epic that already owns nodes without a
    /// pin is a legacy epic and stays on its ECP. Read-only: nothing is
    /// published, selected or pinned here.
    pub(super) fn epic_desk_leadership(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<Option<DeskLed>, ApiError> {
        let state = self.state()?;
        let snapshot = match state
            .with_store(|store| store.get_mini_project_team_definition(project_id, epic_id))
            .map_err(|error| self.refuse(&error))?
        {
            Some(pinned) => pinned.definition,
            None => {
                let placed = !state
                    .with_store(|store| store.list_topology_nodes(project_id, Some(epic_id)))
                    .map_err(|error| self.refuse(&error))?
                    .is_empty();
                let selected = state
                    .with_store(|store| store.get_project_team_definition_default(project_id))
                    .map_err(|error| self.refuse(&error))?;
                match selected {
                    Some(selected) if !placed => selected.definition,
                    _ => return Ok(None),
                }
            }
        };
        let definition = self.published_definition(project_id, &snapshot)?;
        Ok(definition.desk_leadership.map(|leadership| DeskLed {
            definition: snapshot,
            leadership,
        }))
    }

    /// The kind of one epic's control subject: its desk Adam subject when a
    /// desk leads it, the historical ECP otherwise.
    pub(super) fn epic_control_kind(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<TopologyKindKey, ApiError> {
        let context = self.runtime_domain(project_id, Some(epic_id))?;
        Ok(match self.epic_desk_leadership(project_id, epic_id)? {
            Some(led) => led.leadership.epic_kind,
            None => context.domain.delivery.control_kind,
        })
    }

    /// The one subject that leads an epic from a desk, if it has been placed.
    pub(super) fn epic_subject(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<Option<StoredDeskAdamSubject>, ApiError> {
        self.state()?
            .with_store(|store| store.get_epic_desk_adam_subject(project_id, epic_id))
            .map_err(|error| self.refuse(&error))
    }

    /// Ensure one epic's leadership subject below its desk's workspace, and
    /// return the subject node.
    ///
    /// A fresh subject row and its node are one atomic write: a refused node
    /// leaves no subject behind for a later ensure to adopt, while a retry that
    /// lost its answer still reconciles the same planned ids. The short name
    /// its seats render is frozen here from the epic's name; nothing later
    /// re-reads it, so no rename can retitle a running Adam.
    pub(super) fn ensure_epic_subject(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        led: &DeskLed,
        topology: &TopologySnapshot,
    ) -> Result<SessionTopologyNode, ApiError> {
        let subject = match self.epic_subject(project_id, epic_id)? {
            Some(subject) => subject,
            None => {
                let epic = self.epic_row(project_id, epic_id)?;
                let topic = self.desk_adam_topic(&epic.name)?;
                let workspace = self.ensure_desk(project_id, &led.leadership.desk_key)?;
                let now = kontor_api::now();
                let planned = StoredDeskAdamSubject {
                    project_id,
                    topology_node_id: TopologyNodeId::generate(),
                    desk_key: led.leadership.desk_key.clone(),
                    mini_project_id: Some(epic_id),
                    topic,
                    origin_topology_node_id: None,
                    team_definition: led.definition.clone(),
                    created_at: now,
                };
                let planned_node = NewSessionTopologyNode {
                    id: planned.topology_node_id,
                    project_id,
                    mini_project_id: Some(epic_id),
                    topology: topology.clone(),
                    kind: led.leadership.epic_kind.clone(),
                    parent_id: Some(workspace.id),
                    task_id: None,
                    created_at: now,
                };
                match self.state()?.with_store(|store| {
                    store.create_desk_adam_subject_with_node(&planned, &planned_node)
                }) {
                    Ok(_) => planned,
                    Err(RepositoryError::Conflict { .. }) => {
                        self.epic_subject(project_id, epic_id)?.ok_or_else(|| {
                            self.deny(
                                ApiErrorCode::PlacementBlocked,
                                "a desk Adam subject conflicts with another subject's epic, topic or binding",
                            )
                        })?
                    }
                    Err(error) => return Err(self.refuse(&error)),
                }
            }
        };
        if subject.desk_key != led.leadership.desk_key {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "the epic's desk Adam subject is on another desk than its definition declares",
            ));
        }
        self.ensure_subject_node(&subject, &led.leadership.epic_kind, topology)
    }

    /// The frozen short name a subject's seat titles render.
    ///
    /// An external name is already non-empty and at most 512 characters, so
    /// what is left to refuse is a separator glyph: one would let the name
    /// forge a title segment.
    pub(super) fn desk_adam_topic(&self, name: &ExternalName) -> Result<ExternalName, ApiError> {
        if name.as_str().contains(['\u{2022}', '\u{00b7}']) {
            return Err(self
                .deny(
                    ApiErrorCode::InvalidRequest,
                    "a desk Adam title cannot carry a separator glyph",
                )
                .about("desk Adam subject")
                .advising(
                    "name the epic or topic without \u{2022} or \u{00b7}; nothing was placed",
                ));
        }
        Ok(name.clone())
    }

    /// Write one subject row, or read the winner of a race that wrote it.
    fn record_subject(
        &self,
        planned: StoredDeskAdamSubject,
        reread: impl FnOnce() -> Result<Option<StoredDeskAdamSubject>, ApiError>,
    ) -> Result<StoredDeskAdamSubject, ApiError> {
        match self
            .state()?
            .with_store(|store| store.create_desk_adam_subject(&planned))
        {
            Ok(()) => Ok(planned),
            // Another ensure of the same subject won, and this one has written
            // nothing; reconcile against the winner's node id.
            Err(RepositoryError::Conflict { .. }) => reread()?.ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "a desk Adam subject conflicts with another subject's epic, topic or binding",
                )
            }),
            Err(error) => Err(self.refuse(&error)),
        }
    }

    /// The node one subject row planned, created if it is not there yet.
    ///
    /// A node already there must be exactly the one the row planned: the
    /// subject kind, directly below its desk's workspace, scoped to the row's
    /// epic or to none. Anything else is refused rather than adopted.
    fn ensure_subject_node(
        &self,
        subject: &StoredDeskAdamSubject,
        kind: &TopologyKindKey,
        topology: &TopologySnapshot,
    ) -> Result<SessionTopologyNode, ApiError> {
        let state = self.state()?;
        let project_id = subject.project_id;
        let workspace = self.ensure_desk(project_id, &subject.desk_key)?;
        if let Some(node) = state
            .with_store(|store| store.get_topology_node(project_id, subject.topology_node_id))
            .map_err(|error| self.refuse(&error))?
        {
            if &node.kind != kind
                || node.parent_id != Some(workspace.id)
                || node.mini_project_id != subject.mini_project_id
                || node.task_id.is_some()
                || &node.topology != topology
            {
                return Err(self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "a desk Adam subject node no longer matches the placement its row planned",
                ));
            }
            return Ok(node);
        }
        state
            .with_store(|store| {
                store.create_topology_node(&NewSessionTopologyNode {
                    id: subject.topology_node_id,
                    project_id,
                    mini_project_id: subject.mini_project_id,
                    topology: topology.clone(),
                    kind: kind.clone(),
                    parent_id: Some(workspace.id),
                    task_id: None,
                    created_at: kontor_api::now(),
                })
            })
            .map_err(|error| self.refuse(&error))
    }

    /// The desk workspace whose native container holds a subject's sessions,
    /// with its desk; `None` for a node that hosts its own sessions.
    pub(super) fn subject_host(
        &self,
        node: &SessionTopologyNode,
    ) -> Result<Option<(SessionTopologyNode, StoredDesk)>, ApiError> {
        let state = self.state()?;
        let project_id = node.project_id;
        let Some(subject) = state
            .with_store(|store| store.get_desk_adam_subject(project_id, node.id))
            .map_err(|error| self.refuse(&error))?
        else {
            // A PR review Adam borrows its desk's workspace the same way.
            return self.pr_review_adam_host(node);
        };
        let desk = state
            .with_store(|store| store.get_desk(project_id, &subject.desk_key))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the desk a desk Adam subject names is missing",
                )
            })?;
        let host = state
            .with_store(|store| store.get_topology_node(project_id, desk.workspace_node_id))
            .map_err(|error| self.refuse(&error))?
            .filter(|host| node.parent_id == Some(host.id))
            .filter(|_| node.mini_project_id == subject.mini_project_id)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "a desk Adam subject is not below the workspace its row registered",
                )
            })?;
        Ok(Some((host, desk)))
    }

    /// The node whose native container holds one node's sessions: its desk's
    /// workspace for a desk Adam subject, the node itself otherwise.
    pub(super) fn session_container_node(
        &self,
        node: &SessionTopologyNode,
    ) -> Result<SessionTopologyNode, ApiError> {
        Ok(self
            .subject_host(node)?
            .map_or_else(|| node.clone(), |(host, _)| host))
    }

    /// The directory a seat on `node` is launched in: its desk's directory for
    /// a desk Adam subject, the epic's registration directory otherwise.
    pub(super) fn seat_launch_root(
        &self,
        node: &SessionTopologyNode,
    ) -> Result<WorkspaceRoot, ApiError> {
        match self.subject_host(node)? {
            Some((_, desk)) => self.desk_root(&desk),
            None => self.runtime_root(node.project_id, node.mini_project_id),
        }
    }

    /// Where a hosted seat on `node` is placed natively: its desk's own
    /// project for a desk Adam subject, the epic's project otherwise.
    ///
    /// The placement is only where the native lives. A desk Adam's native
    /// carries no epic or mini-project label and nothing is filed under its
    /// epic; the epic it acts for is its subject's, read from the subject row
    /// on every path that asks.
    pub(super) fn hosted_seat_scope(
        &self,
        node: &SessionTopologyNode,
        scope: &ExecutionScope,
    ) -> Result<HostedSeatScope, ApiError> {
        match self.subject_host(node)? {
            Some((_, desk)) => self.desk_seat_scope(&desk),
            None => Ok(HostedSeatScope::Epic(scope.clone())),
        }
    }

    /// The desk placement of a desk Adam's native: the desk workspace's exact
    /// persisted parent, read by id, as for every durable desk seat.
    fn desk_seat_scope(&self, desk: &StoredDesk) -> Result<HostedSeatScope, ApiError> {
        let parent = self
            .state()?
            .with_store(|store| {
                store.get_topology_node_container(desk.project_id, desk.topology_node_id)
            })
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the desk is not materialized; materialize it before launching a desk Adam",
                )
            })?;
        Ok(HostedSeatScope::Desk {
            project: parent.identity,
        })
    }

    /// The exact definition revision a subject's seat titles render from.
    pub(super) fn subject_definition(
        &self,
        node: &SessionTopologyNode,
    ) -> Result<Option<TeamDefinitionSpec>, ApiError> {
        let Some(subject) = self
            .state()?
            .with_store(|store| store.get_desk_adam_subject(node.project_id, node.id))
            .map_err(|error| self.refuse(&error))?
        else {
            // A PR review's Adam and room render from the revision that
            // placed the review.
            return self.pr_review_definition_of(node);
        };
        self.published_definition(node.project_id, &subject.team_definition)
            .map(Some)
    }

    /// The topic and epic key a subject's seat template asks for.
    ///
    /// Both come from the subject row and the epic's confirmed binding, never
    /// from a caller: the topic is the value frozen when the subject was
    /// placed, and the epic is the subject's own, not the workspace's.
    pub(super) fn desk_adam_title_values(
        &self,
        node: &SessionTopologyNode,
        template: &NativeNameTemplate,
        mut values: NativeNameValues,
    ) -> Result<NativeNameValues, ApiError> {
        let subject = self
            .state()?
            .with_store(|store| store.get_desk_adam_subject(node.project_id, node.id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "only a desk Adam subject renders a topic or epic key in a seat title",
                )
            })?;
        if template_uses_token(template, NativeNameToken::Topic) {
            values = values.with_topic(subject.topic.as_str());
        }
        if template_uses_token(template, NativeNameToken::EpicJiraKey) {
            let epic_id = subject.mini_project_id.ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "an ad-hoc desk Adam has no epic key to render",
                )
            })?;
            let key = self.jira_key_for_subject(node.project_id, epic_id, None)?;
            values = values.with_epic_jira_key(&key);
        }
        Ok(values)
    }

    /// Refuse, before an epic's graph is written, a desk-led epic that could
    /// never be governed: a name that would forge a title segment, or a roster
    /// that persists a seat its desk Adam subject cannot host.
    ///
    /// Governance runs after the graph is written, and an epic's name is its
    /// identity, so a refusal there would leave an epic no retry can govern.
    pub(super) fn refuse_ungovernable_desk_led_epic(
        &self,
        project_id: ProjectId,
        name: &ExternalName,
        origin: Option<&DeskAdamOriginRequest>,
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        let existing = state
            .with_store(|store| store.list_mini_projects(project_id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .find(|epic| &epic.name == name);
        let leadership = match &existing {
            Some(epic) => self
                .epic_desk_leadership(project_id, epic.id)?
                .map(|led| led.leadership),
            None => match state
                .with_store(|store| store.get_project_team_definition_default(project_id))
                .map_err(|error| self.refuse(&error))?
            {
                Some(selected) => {
                    self.published_definition(project_id, &selected.definition)?
                        .desk_leadership
                }
                None => None,
            },
        };
        let Some(leadership) = leadership else {
            return match origin {
                Some(_) => Err(self.not_desk_led()),
                None => Ok(()),
            };
        };
        self.desk_adam_topic(name)?;
        if let Some(origin) = origin {
            self.checked_desk_adam_origin(
                project_id,
                existing.as_ref().map(|epic| epic.id),
                &leadership.desk_key,
                origin,
            )?;
        }
        let frozen = match &existing {
            Some(epic) => self.optional_frozen_roster(project_id, epic.id)?,
            None => None,
        };
        let roster = match frozen {
            Some(frozen) => frozen,
            None => self.epic_bootstrap_roster(project_id)?,
        };
        self.refuse_desk_led_roster(&roster)
    }

    /// Refuse a roster for a desk-led epic unless it persists only the LSA and
    /// TPM its desk Adam subject hosts; every other role must be on demand.
    pub(super) fn refuse_desk_led_roster_for(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        roster: &FrozenRoster,
    ) -> Result<(), ApiError> {
        if self.epic_desk_leadership(project_id, epic_id)?.is_none() {
            return Ok(());
        }
        self.refuse_desk_led_roster(roster)
    }

    fn refuse_desk_led_roster(&self, roster: &FrozenRoster) -> Result<(), ApiError> {
        let architecture =
            self.frozen_purpose(&roster.revision, CoreTeamSeatPurpose::ArchitectureCloser)?;
        let process = self.frozen_purpose(&roster.revision, CoreTeamSeatPurpose::ProcessCloser)?;
        if roster.revision.seats.iter().any(|seat| {
            seat.presence != EpicPresence::OnDemand
                && seat.role_slot_id != architecture.role_slot_id
                && seat.role_slot_id != process.role_slot_id
        }) {
            return Err(self
                .deny(
                    ApiErrorCode::PlacementBlocked,
                    "a desk-led epic persists only its LSA and TPM seats",
                )
                .about("desk Adam roster")
                .advising(
                    "publish a Core Team whose other roles are on demand, then apply again; nothing was placed",
                ));
        }
        Ok(())
    }

    /// Refuse, before any effect, a route that would give a desk Adam subject
    /// a second live session.
    ///
    /// A subject has one phase owner at a time: its LSA while planning, its
    /// TPM while coordinating. The schema refuses a second hosted occupancy
    /// whatever path asks for one; this answers the same question first, with
    /// the step that resolves it, and before a native is created that would
    /// then have nowhere to be recorded.
    pub(super) fn refuse_second_desk_adam(
        &self,
        project_id: ProjectId,
        control: &SessionTopologyNode,
        routes: &BTreeMap<String, ModelRung>,
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        if state
            .with_store(|store| store.get_desk_adam_subject(project_id, control.id))
            .map_err(|error| self.refuse(&error))?
            .is_none()
        {
            return Ok(());
        }
        let mut routed = routes.keys();
        let (Some(role_code), None) = (routed.next(), routed.next()) else {
            return if routes.is_empty() {
                Ok(())
            } else {
                Err(self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "a desk Adam subject hosts one live session at a time; route only its current phase owner",
                ))
            };
        };
        for seat in state
            .with_store(|store| store.list_seat_bindings(project_id, control.id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .filter(|seat| seat.is_non_terminal() && seat.role.role_code.as_str() != role_code)
        {
            if state
                .with_store(|store| store.get_hosted_topology_seat(project_id, seat.id))
                .map_err(|error| self.refuse(&error))?
                .is_some()
            {
                return Err(self
                    .deny(
                        ApiErrorCode::PlacementBlocked,
                        "a desk Adam subject already hosts its live phase owner",
                    )
                    .about("desk Adam subject")
                    .advising(
                        "hand the subject off to the next phase owner instead; nothing was changed",
                    ));
            }
        }
        Ok(())
    }

    /// Hand one desk-led epic's Adam subject from its planning LSA to its
    /// coordinating TPM.
    ///
    /// The subject has one phase owner at a time, so the LSA's native is
    /// fenced first: retired in the runtime and moved into the seat's route
    /// history. Its generation-scoped credential then names no live occupancy,
    /// so nothing it held can still act. Only then does the TPM launch, as a
    /// fresh native with its own credential, through the materialization every
    /// Core Team seat uses. A crash between the two leaves no owner rather
    /// than two, and the same request resumes from whichever step it reached.
    ///
    /// Across vendors only. A handoff within one vendor could keep the LSA's
    /// session and provider conversation, which would carry the LSA's
    /// credential into TPM authority. Whether that is acceptable is an open
    /// decision, so it refuses before any effect and nothing here claims it.
    pub(super) async fn desk_adam_handoff(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &DeskAdamHandoffRequest,
    ) -> Result<DeskAdamHandoffDto, ApiError> {
        let state = self.state()?;
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "desk_adam_handoff",
            "project": project_id.to_string(),
            "epic": epic_id.to_string(),
            "from": request.from_seat_binding_id.to_string(),
            "predecessor": request.expected_native_id.as_str(),
            "generation": request.expected_generation,
            "to": request.to_seat_binding_id.to_string(),
            "model_route": request.model_route,
        }))?;
        let target = AggregateRef::MiniProject {
            mini_project_id: epic_id,
        };
        let replayed = self.replayed(key, &intent, Some(&target))?.is_some();
        let epic = self.epic_row(project_id, epic_id)?;
        if !replayed && epic.revision != request.expected_revision {
            return Err(self
                .deny(
                    ApiErrorCode::RevisionConflict,
                    "the epic moved since the caller read it",
                )
                .with_revision(Some(epic.revision)));
        }
        let subject = match (
            self.epic_desk_leadership(project_id, epic_id)?,
            self.epic_subject(project_id, epic_id)?,
        ) {
            (Some(_), Some(subject)) => subject,
            _ => {
                return Err(self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "only a placed desk-led epic hands its Adam subject off",
                ));
            }
        };
        let from = self.subject_seat(
            project_id,
            &subject,
            request.from_seat_binding_id,
            CoreTeamSeatPurpose::ArchitectureCloser,
        )?;
        let to = self.subject_seat(
            project_id,
            &subject,
            request.to_seat_binding_id,
            CoreTeamSeatPurpose::ProcessCloser,
        )?;
        let fleet = self.fleet.current();
        let desired = parse_runtime_model_route(&request.model_route, fleet.as_deref())
            .map_err(|error| self.refuse_domain(&error))?;

        // The exact predecessor the caller saw: still live, or already in the
        // seat's history because an earlier attempt fenced it and lost its
        // answer. Anything else moved since the caller read it.
        let live = state
            .with_store(|store| store.get_hosted_topology_seat(project_id, from.id))
            .map_err(|error| self.refuse(&error))?;
        let (predecessor, fenced) = match live {
            Some(live)
                if live.native_identity.native_id == request.expected_native_id
                    && live.native_identity.generation == request.expected_generation =>
            {
                (live, false)
            }
            Some(_) => return Err(self.planning_session_moved()),
            None => {
                let archived = state
                    .with_store(|store| {
                        store.get_hosted_topology_seat_history(
                            project_id,
                            from.id,
                            &request.expected_native_id,
                        )
                    })
                    .map_err(|error| self.refuse(&error))?
                    .filter(|archived| {
                        archived.native_identity.generation == request.expected_generation
                    })
                    .ok_or_else(|| self.planning_session_moved())?;
                (archived, true)
            }
        };
        let vendors = (
            crate::fleet::independence_key(&predecessor.model_rung, fleet.as_deref()),
            crate::fleet::independence_key(&desired, fleet.as_deref()),
        );
        if !matches!(&vendors, (Some(lsa), Some(tpm)) if lsa != tpm) {
            return Err(self
                .deny(
                    ApiErrorCode::UnsupportedCapability,
                    "a desk Adam hands off only across vendors until the same-vendor transfer is decided",
                )
                .about("desk Adam handoff")
                .advising(
                    "route the TPM on another vendor; a same-vendor handoff waits on Igor's pending decision on seat-credential continuity (ASMA-8452); nothing was changed",
                ));
        }

        if !fenced {
            self.fence_subject_session(
                project_id,
                &subject,
                &predecessor,
                (
                    "desk Adam handoff to the coordinating TPM",
                    "desk Adam handoff; the planning session was already gone",
                ),
            )
            .await?;
        }

        // The TPM launches through the one materialization every Core Team
        // seat uses, under a key derived from this one, so a retry of the same
        // handoff converges on the same launch.
        let current = self.epic_row(project_id, epic_id)?;
        let launched = ApplicationOperations::materialize_core_team(
            self,
            &self.epic_apply_child_key(key, "desk-adam-handoff")?,
            project_id,
            epic_id,
            &CoreTeamMaterializeRequest {
                expected_revision: current.revision,
                routes: vec![CoreTeamSeatRouteRequest {
                    role_code: to.role.role_code.as_str().to_owned(),
                    model_route: Some(request.model_route.clone()),
                    eligibility: None,
                }],
            },
        )
        .await?;
        let (receipt_id, created) = self.record_classified(
            key,
            project_id,
            CommandKind::HandOffDeskAdam,
            target,
            current.revision,
            &intent,
        )?;
        Ok(DeskAdamHandoffDto {
            from_seat_binding_id: from.id,
            fenced_native_id: predecessor.native_identity.native_id.clone(),
            to_seat_binding_id: to.id,
            core_team: launched.core_team,
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: if created {
                    AppliedDto::Created
                } else {
                    AppliedDto::Unchanged
                },
                revision: current.revision,
                snapshot_cursor: self.cursor()?,
            },
        })
    }

    fn planning_session_moved(&self) -> ApiError {
        self.deny(
            ApiErrorCode::RevisionConflict,
            "the planning session moved since the caller read it",
        )
    }

    /// One active LSA or TPM seat of exactly this subject.
    fn subject_seat(
        &self,
        project_id: ProjectId,
        subject: &StoredDeskAdamSubject,
        seat_binding_id: SeatBindingId,
        purpose: CoreTeamSeatPurpose,
    ) -> Result<SeatBinding, ApiError> {
        let epic = subject.mini_project_id.ok_or_else(|| {
            self.deny(
                ApiErrorCode::RoleSlotUnbound,
                "role_purpose_unpinned: subject has no epic roster",
            )
        })?;
        let expected = self.epic_purpose(project_id, epic, purpose)?;
        self.state()?
            .with_store(|store| store.get_seat_binding(project_id, seat_binding_id))
            .map_err(|error| self.refuse(&error))?
            .filter(|seat| {
                seat.topology_node_id == subject.topology_node_id
                    && seat.role_slot_id == expected.role_slot_id
                    && seat.role.catalog_id == expected.role.catalog_id
                    && seat.role.catalog_revision == expected.role.catalog_revision
                    && seat.role.role_code == expected.role.role_code
                    && seat.is_non_terminal()
            })
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the handoff names a seat that is not this epic's desk Adam LSA or TPM",
                )
            })
    }

    /// Retire one session of a subject and move it into its seat's route
    /// history, recording the first reason when the runtime retired it and
    /// the second when it proved the session already gone. The readback of
    /// the desk workspace is the fence: a workspace the runtime no longer
    /// holds refuses here, before anything is retired, and only a session the
    /// runtime proves absent is treated as gone.
    async fn fence_subject_session(
        &self,
        project_id: ProjectId,
        subject: &StoredDeskAdamSubject,
        predecessor: &StoredHostedTopologySeat,
        (retired_reason, absent_reason): (&'static str, &'static str),
    ) -> Result<(), ApiError> {
        let node = self
            .state()?
            .with_store(|store| store.get_topology_node(project_id, subject.topology_node_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the desk Adam subject node no longer exists",
                )
            })?;
        self.fence_node_session(
            project_id,
            &node,
            predecessor,
            (retired_reason, absent_reason),
        )
        .await
    }

    /// Retire one hosted session of `node` in the native container its
    /// sessions run in, and move it into its seat's route history.
    ///
    /// The container's readback is the fence: one the runtime no longer holds
    /// refuses before anything is retired, and only a session the runtime
    /// proves absent is treated as gone.
    pub(super) async fn fence_node_session(
        &self,
        project_id: ProjectId,
        node: &SessionTopologyNode,
        predecessor: &StoredHostedTopologySeat,
        reasons: (&'static str, &'static str),
    ) -> Result<(), ApiError> {
        self.fence_node_session_with(project_id, node, predecessor, reasons, false)
            .await
    }

    /// Fence one PR review's session for a close.
    ///
    /// A retirement whose transport failed after the write may already have
    /// taken the session, so it is reported as unconfirmed rather than as a
    /// channel refusal that says nothing changed.
    pub(super) async fn fence_pr_review_node_session(
        &self,
        project_id: ProjectId,
        node: &SessionTopologyNode,
        predecessor: &StoredHostedTopologySeat,
        reasons: (&'static str, &'static str),
    ) -> Result<(), ApiError> {
        self.fence_node_session_with(project_id, node, predecessor, reasons, true)
            .await
    }

    async fn fence_node_session_with(
        &self,
        project_id: ProjectId,
        node: &SessionTopologyNode,
        predecessor: &StoredHostedTopologySeat,
        (retired_reason, absent_reason): (&'static str, &'static str),
        unconfirmed_transport: bool,
    ) -> Result<(), ApiError> {
        let _native_activity = self.native_activity()?;
        let state = self.state()?;
        let adapter = state
            .runtimes()
            .get(&predecessor.native_identity.runtime_kind)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the hosted seat runtime is not configured in this daemon",
                )
            })?;
        let proved = self
            .prove_existing_bound_container(project_id, node)
            .await?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::StaleBinding,
                    "the desk workspace has no proved native container to fence the session in",
                )
            })?;
        let placement = HostedSeatRetirePlacement {
            workspace_native_id: proved.binding.identity.native_id.clone(),
            canonical_cwd: proved.binding.root.clone().ok_or_else(|| {
                self.deny(
                    ApiErrorCode::StaleBinding,
                    "the proved desk workspace records no canonical directory",
                )
            })?,
            provider_session_id: predecessor.provider_session_id.clone(),
        };
        let (retired_at, reason) = match adapter
            .retire_hosted_seat(&HostedSeatRetireRequest {
                resolved_route: None,
                placement: Some(placement),
                seat_binding_id: predecessor.seat_binding_id,
                identity: predecessor.native_identity.clone(),
                model_rung: predecessor.model_rung.clone(),
                autonomy: predecessor.autonomy,
                requested_at: kontor_api::now(),
            })
            .await
        {
            Ok(retired) => (retired.archived_at, retired_reason),
            Err(error) if error.proves_hosted_predecessor_absent() => {
                (kontor_api::now(), absent_reason)
            }
            // A retirement is a write. A transport failure after it was sent
            // cannot prove the session survived, so saying "the runtime could
            // not be reached; nothing was changed" would be false and would
            // invite a caller to act as though the seat were still live.
            Err(RuntimeError::Transport { .. }) if unconfirmed_transport => {
                return Err(ApiError::new(
                    state.realm_id(),
                    ApiErrorCode::DeliveryUnconfirmed,
                    "the session's retirement was put on the wire and the runtime did not confirm its outcome",
                )
                .advising(
                    "retry the same operation with the same idempotency key; a retirement that already landed is replay-safe",
                ));
            }
            Err(error) => return Err(ApiError::from_runtime(state.realm_id(), &error)),
        };
        state
            .with_store(|store| {
                store.archive_hosted_topology_seat_route(predecessor, retired_at, reason)
            })
            .map_err(|error| self.refuse(&error))?;
        Ok(())
    }

    /// Open one ad-hoc desk Adam on the desk the project's selected Team
    /// Definition leads from, or return the subject its topic already opened.
    ///
    /// The subject, its node and its LSA seat, and with a `model_route` the
    /// LSA's native session, launched in the desk workspace on the epic-free
    /// hosted-seat contract (ASMA-8451). The route is parsed before anything
    /// is written.
    pub(super) async fn open_ad_hoc_subject(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &AdHocDeskAdamOpenRequest,
    ) -> Result<AdHocDeskAdamDto, ApiError> {
        let state = self.state()?;
        let fleet = self.fleet.current();
        let model_rung = request
            .model_route
            .as_ref()
            .map(|route| parse_runtime_model_route(route, fleet.as_deref()))
            .transpose()
            .map_err(|error| self.refuse_domain(&error))?;
        let mut intent_document = serde_json::json!({
            "schema_version": 1,
            "operation": "open_ad_hoc_desk_adam",
            "project": project_id.to_string(),
            "topic": request.topic.as_str(),
        });
        // Widened only when named, so a logical-only open keeps its intent
        // bytes and its receipt stays replayable.
        if let Some(route) = &request.model_route {
            intent_document["model_route"] = serde_json::json!(route);
        }
        let intent = self.intent(&intent_document)?;
        let target = AggregateRef::Project { project_id };
        self.replayed(key, &intent, Some(&target))?;
        let project = state
            .with_store(|store| store.get_project(project_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such project exists in this realm",
                )
            })?;
        let selected = state
            .with_store(|store| store.get_project_team_definition_default(project_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| self.no_desk_leadership())?;
        let leadership = self
            .published_definition(project_id, &selected.definition)?
            .desk_leadership
            .ok_or_else(|| self.no_desk_leadership())?;
        let topic = self.desk_adam_topic(&request.topic)?;
        let reread = || {
            state
                .with_store(|store| {
                    store.get_ad_hoc_desk_adam_subject(project_id, &leadership.desk_key, &topic)
                })
                .map_err(|error| self.refuse(&error))
        };
        let subject = match reread()? {
            Some(subject) => subject,
            None => {
                self.ensure_desk(project_id, &leadership.desk_key)?;
                self.record_subject(
                    StoredDeskAdamSubject {
                        project_id,
                        topology_node_id: TopologyNodeId::generate(),
                        desk_key: leadership.desk_key.clone(),
                        mini_project_id: None,
                        topic: topic.clone(),
                        origin_topology_node_id: None,
                        team_definition: selected.definition.clone(),
                        created_at: kontor_api::now(),
                    },
                    reread,
                )?
            }
        };
        if state
            .with_store(|store| {
                store.get_desk_adam_subject_bound_from(project_id, subject.topology_node_id)
            })
            .map_err(|error| self.refuse(&error))?
            .is_some()
        {
            return Err(self
                .deny(
                    ApiErrorCode::PlacementBlocked,
                    "this ad-hoc desk Adam topic has already moved to an epic",
                )
                .about("ad-hoc desk Adam")
                .advising("open another topic, or work in the epic it moved to"));
        }
        // An existing subject keeps the revision that placed it, whatever the
        // project selects today.
        let definition = self.published_definition(project_id, &subject.team_definition)?;
        let placed = definition.desk_leadership.as_ref().ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "the revision that placed this ad-hoc desk Adam declares no desk leadership",
            )
        })?;
        let node = self.ensure_subject_node(&subject, &placed.ad_hoc_kind, &definition.topology)?;
        let lsa = self.ensure_ad_hoc_lsa(project_id, &node, &definition, placed)?;
        let lsa_title = self.seat_name_from_definition(&definition, &node, &lsa.role_slot_id)?;
        if let Some(model_rung) = &model_rung {
            self.launch_ad_hoc_lsa(project_id, &node, &subject, &lsa, &lsa_title, model_rung)
                .await?;
        }
        let lsa_native_id = state
            .with_store(|store| store.get_hosted_topology_seat(project_id, lsa.id))
            .map_err(|error| self.refuse(&error))?
            .map(|hosted| hosted.native_identity.native_id);
        let (receipt_id, created) = self.record_classified(
            key,
            project_id,
            CommandKind::OpenAdHocDeskAdam,
            target,
            project.revision,
            &intent,
        )?;
        Ok(AdHocDeskAdamDto {
            subject: desk_adam_subject_dto(&subject),
            lsa_seat_binding_id: lsa.id,
            lsa_title,
            lsa_native_id,
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: if created {
                    AppliedDto::Created
                } else {
                    AppliedDto::Unchanged
                },
                revision: project.revision,
                snapshot_cursor: self.cursor()?,
            },
        })
    }

    fn no_desk_leadership(&self) -> ApiError {
        self.deny(
            ApiErrorCode::PlacementBlocked,
            "the project's selected Team Definition declares no desk leadership",
        )
        .about("ad-hoc desk Adam")
        .advising("select a Team Definition that declares desk_leadership; nothing was placed")
    }

    fn not_desk_led(&self) -> ApiError {
        self.deny(
            ApiErrorCode::PlacementBlocked,
            "desk_adam_origin applies only to a desk-led epic",
        )
        .about("desk Adam binding")
        .advising("omit desk_adam_origin, or select a desk-led Team Definition; nothing was placed")
    }

    /// The ad-hoc subject's one LSA seat, created if it is not there yet.
    ///
    /// Its slot is the one the placing revision declares for an ad-hoc LSA,
    /// so its title renders `LSA • ADHOC • <topic>` from that revision alone.
    fn ensure_ad_hoc_lsa(
        &self,
        project_id: ProjectId,
        node: &SessionTopologyNode,
        definition: &TeamDefinitionSpec,
        leadership: &TeamDeskLeadership,
    ) -> Result<SeatBinding, ApiError> {
        let state = self.state()?;
        // Unscoped resume retains its durable role/slot; it grants no epic purpose.
        let held: Vec<_> = state
            .with_store(|store| store.list_seat_bindings(project_id, node.id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .filter(SeatBinding::is_non_terminal)
            .collect();
        if !held.is_empty() {
            let [binding] = held.as_slice() else {
                return Err(self.deny(ApiErrorCode::RoleSlotUnbound, "role_slot_binding_ambiguous"));
            };
            let declared = definition
                .container(&leadership.ad_hoc_kind)
                .is_some_and(|container| {
                    container.slots.iter().any(|slot| {
                        slot.slot_id == binding.role_slot_id
                            && slot.role_code.as_ref() == Some(&binding.role.role_code)
                    })
                });
            if !declared {
                return Err(self.deny(ApiErrorCode::RoleSlotUnbound, "role_slot_binding_mismatch"));
            }
            return Ok(binding.clone());
        }
        let roster = self.epic_bootstrap_roster(project_id)?;
        let role = self
            .frozen_purpose(&roster.revision, CoreTeamSeatPurpose::ArchitectureCloser)?
            .role;
        let lead = &role.role_code;
        let slot = definition
            .container(&leadership.ad_hoc_kind)
            .and_then(|container| {
                container
                    .slots
                    .iter()
                    .find(|slot| slot.role_code.as_ref() == Some(lead))
            })
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the leadership definition declares no LSA seat for an ad-hoc subject",
                )
            })?;
        let held = || {
            state
                .with_store(|store| store.list_seat_bindings(project_id, node.id))
                .map_err(|error| self.refuse(&error))
                .and_then(|seats| {
                    let held = seats
                        .into_iter()
                        .find(|seat| seat.role_slot_id == slot.slot_id && seat.is_non_terminal());
                    if held.as_ref().is_some_and(|seat| {
                        seat.role.catalog_id != role.catalog_id
                            || seat.role.catalog_revision != role.catalog_revision
                            || seat.role.role_code != role.role_code
                    }) {
                        return Err(
                            self.deny(ApiErrorCode::RoleSlotUnbound, "role_slot_binding_mismatch")
                        );
                    }
                    Ok(held)
                })
        };
        if let Some(seat) = held()? {
            return Ok(seat);
        }
        let now = kontor_api::now();
        let created = state.with_store(|store| {
            store.create_seat_binding(&NewSeatBinding {
                id: SeatBindingId::generate(),
                project_id,
                topology_node_id: node.id,
                role_slot_id: slot.slot_id.clone(),
                role: role.clone(),
                // Leadership presence, not delivery work: no task, no TeamRun.
                task_id: None,
                team_run_id: None,
                attach_deadline: now
                    .checked_add(jiff::SignedDuration::from_secs(SEAT_ATTACH_SECONDS))
                    .unwrap_or(now),
                parent_seat_binding_id: None,
                created_at: now,
            })
        });
        match created {
            Ok(seat) => Ok(seat),
            // A concurrent open of the same topic created it first.
            Err(RepositoryError::Conflict { .. }) => held()?.ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the ad-hoc desk Adam's LSA seat could not be created or found",
                )
            }),
            Err(error) => Err(self.refuse(&error)),
        }
    }

    /// Launch the ad-hoc LSA's first native, or prove the one it already has.
    ///
    /// This is the generation-one launch of a Core Team leadership seat,
    /// addressed to the desk instead of an epic: the desk workspace's proved
    /// container and directory, and the desk's own native project, with no
    /// epic label and nothing filed under an epic. An ad-hoc native is never
    /// re-routed or relaunched; its topic moves to an epic, whose LSA launches
    /// fresh, or a new topic is opened.
    async fn launch_ad_hoc_lsa(
        &self,
        project_id: ProjectId,
        node: &SessionTopologyNode,
        subject: &StoredDeskAdamSubject,
        seat: &SeatBinding,
        title: &ExternalName,
        model_rung: &ModelRung,
    ) -> Result<(), ApiError> {
        let held = self.desk_adam_guard.lock().await;
        self.launch_ad_hoc_lsa_held(project_id, node, subject, seat, title, model_rung, &held)
            .await
    }

    /// [`Self::launch_ad_hoc_lsa`], for a caller that already holds the desk
    /// Adam guard: an open, or a binding recovering a launch that lost its
    /// answer.
    ///
    /// Under the guard the seat and its topic are read again. A binding that
    /// finished while this launch waited has released the seat and moved the
    /// topic, and nothing may reach the runtime for either after that.
    #[allow(clippy::too_many_arguments)]
    async fn launch_ad_hoc_lsa_held(
        &self,
        project_id: ProjectId,
        node: &SessionTopologyNode,
        subject: &StoredDeskAdamSubject,
        seat: &SeatBinding,
        title: &ExternalName,
        model_rung: &ModelRung,
        _held: &tokio::sync::MutexGuard<'_, ()>,
    ) -> Result<(), ApiError> {
        let _native_activity = self.native_activity()?;
        let state = self.state()?;
        let seated = state
            .with_store(|store| store.get_seat_binding(project_id, seat.id))
            .map_err(|error| self.refuse(&error))?
            .is_some_and(|seat| seat.is_non_terminal());
        let moved = state
            .with_store(|store| {
                store.get_desk_adam_subject_bound_from(project_id, subject.topology_node_id)
            })
            .map_err(|error| self.refuse(&error))?
            .is_some();
        if !seated || moved {
            return Err(self
                .deny(
                    ApiErrorCode::PlacementBlocked,
                    "this ad-hoc desk Adam topic has already moved to an epic",
                )
                .about("ad-hoc desk Adam")
                .advising("open another topic, or work in the epic it moved to"));
        }
        let runtime_kind = self.node_runtime_kind()?;
        let adapter = state.runtimes().get(&runtime_kind).ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "the runtime selected for desk Adam placement is not configured",
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
        if let Some(existing) = state
            .with_store(|store| store.get_hosted_topology_seat(project_id, seat.id))
            .map_err(|error| self.refuse(&error))?
        {
            if &existing.model_rung != model_rung {
                return Err(self
                    .deny(
                        ApiErrorCode::PlacementBlocked,
                        "this ad-hoc desk Adam is already live under a different route",
                    )
                    .about("ad-hoc desk Adam")
                    .advising(
                        "open it with the route it runs under, or bind its topic to an epic, whose LSA launches fresh",
                    ));
            }
            return self
                .reuse_bound_core_team_seat(
                    project_id,
                    seat.id,
                    &existing,
                    model_rung,
                    adapter.as_ref(),
                )
                .await
                .map_err(|error| {
                    error.about("ad-hoc desk Adam").advising(
                        "an ad-hoc desk Adam is not relaunched; bind its topic to an epic, whose LSA launches fresh, or open another topic",
                    )
                });
        }
        let cwd = self.seat_launch_root(node)?;
        let container = self
            .ensure_container(project_id, node, &cwd, adapter.as_ref())
            .await?;
        let (_, desk) = self.subject_host(node)?.ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "an ad-hoc desk Adam subject is not below its desk's workspace",
            )
        })?;
        let scope = self.desk_seat_scope(&desk)?;
        let capabilities = adapter
            .discover_capabilities()
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        let context_policy = ContextPolicySnapshot::standard(
            &capabilities.limits.context_window,
            capabilities.supports(RuntimeCapability::ContextPolicy),
            SCHEMA_VERSION,
            kontor_api::now(),
        )
        .map_err(|error| self.refuse_domain(&error))?;
        let prompt = BoundedText::parse(&format!(
            "Ad-hoc {} seat on desk {} for the topic {}. It belongs to no epic until one adopts the topic. Continue only work authorized through Kontor, then remain reusable under SeatBinding {}. Use the generation-fenced KONTOR_AUTH value only for this seat's supported identity-bound routes and never disclose it.",
            seat.role.role_code.as_str(),
            subject.desk_key.as_str(),
            subject.topic.as_str(),
            seat.id,
        ))
        .map_err(|error| self.refuse_domain(&error))?;
        // A prepared intent means an earlier attempt resolved this
        // generation's authority and may have created the native before its
        // answer was lost: the replay asks for what that attempt did.
        let autonomy = match state
            .with_store(|store| {
                store.get_hosted_seat_launch_intent(project_id, seat.id, FIRST_HOSTED_OCCUPANCY)
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
                    seat_binding_id: seat.id,
                    occupancy_generation: FIRST_HOSTED_OCCUPANCY,
                    autonomy,
                    model_rung: (model_rung.clone()).into(),
                    state: HostedSeatLaunchIntentState::Prepared,
                    observed_native_id: None,
                    prepared_at: kontor_api::now(),
                    installed_at: None,
                })
            })
            .map_err(|error| self.refuse(&error))?;
        let role_persona = self.freeze_role_persona(&seat.role.role_code)?;
        if let Some(persona) = role_persona.as_ref() {
            state
                .with_store(|store| {
                    store.record_hosted_seat_role_persona(
                        project_id,
                        seat.id,
                        FIRST_HOSTED_OCCUPANCY,
                        persona,
                    )
                })
                .map_err(|error| self.refuse(&error))?;
        }
        let outcome = adapter
            .launch_hosted_seat(&HostedSeatLaunchRequest {
                resolved_route: None,
                seat_binding_id: seat.id,
                role_slot_id: seat.role_slot_id.clone(),
                display_name: title.clone(),
                container,
                cwd,
                scope,
                prompt,
                role_prompt: role_persona.as_ref().map(|persona| persona.prompt.clone()),
                credential: ConsultationCredential::new(
                    state
                        .credentials()
                        .seat_credential_for_generation(seat.id, FIRST_HOSTED_OCCUPANCY),
                ),
                fenced_predecessor_native_ids: Vec::new(),
                model_rung: model_rung.clone(),
                fleet_provenance: None,
                autonomy,
                context_policy,
                requested_at: kontor_api::now(),
            })
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        let hosted = StoredHostedTopologySeat {
            project_id,
            seat_binding_id: seat.id,
            model_rung: model_rung.clone(),
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
                    seat.id,
                    FIRST_HOSTED_OCCUPANCY,
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
                    seat.id,
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

    /// Check one `desk_adam_origin` against the epic it would bind, before
    /// anything is written, and return the ad-hoc subject it names.
    ///
    /// The origin must be an ad-hoc subject of the epic's own desk, bound to
    /// no other epic; an epic already placed must have been placed from this
    /// same origin, which is what makes a replay converge. Keeping the ad-hoc
    /// native refuses: whether a seat credential may follow its session across
    /// that boundary is the same open decision as a same-vendor handoff.
    pub(super) fn checked_desk_adam_origin(
        &self,
        project_id: ProjectId,
        epic_id: Option<MiniProjectId>,
        desk_key: &DeskKey,
        origin: &DeskAdamOriginRequest,
    ) -> Result<StoredDeskAdamSubject, ApiError> {
        if origin.keep_native {
            return Err(self
                .deny(
                    ApiErrorCode::UnsupportedCapability,
                    "a desk Adam binding never keeps the ad-hoc native session",
                )
                .about("desk Adam binding")
                .advising(
                    "omit keep_native: the epic's LSA launches fresh and the ad-hoc session is fenced; keeping the session waits on Igor's pending decision on seat-credential continuity (ASMA-8452); nothing was placed",
                ));
        }
        let state = self.state()?;
        let source = state
            .with_store(|store| store.get_desk_adam_subject(project_id, origin.subject))
            .map_err(|error| self.refuse(&error))?
            .filter(|source| source.mini_project_id.is_none() && &source.desk_key == desk_key)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "desk_adam_origin names no ad-hoc desk Adam subject on this epic's desk",
                )
                .about("desk Adam binding")
                .advising("name an ad-hoc subject opened on the epic's desk; nothing was placed")
            })?;
        if let Some(bound) = state
            .with_store(|store| store.get_desk_adam_subject_bound_from(project_id, origin.subject))
            .map_err(|error| self.refuse(&error))?
            && (epic_id.is_none() || bound.mini_project_id != epic_id)
        {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "the ad-hoc desk Adam subject is already bound to another epic",
            ));
        }
        if let Some(epic_id) = epic_id
            && let Some(placed) = self.epic_subject(project_id, epic_id)?
            && placed.origin_topology_node_id != Some(origin.subject)
        {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "the epic's desk Adam is already placed from another origin",
            ));
        }
        // A live ad-hoc session is fenced after the epic's graph is written and
        // before its subject row, and a launch that lost its answer is
        // censused first. The runtime holding, or launching, the session must
        // at least be configured, or that fence could never run.
        for seat in state
            .with_store(|store| store.list_seat_bindings(project_id, origin.subject))
            .map_err(|error| self.refuse(&error))?
        {
            let runtime_kind = match state
                .with_store(|store| store.get_hosted_topology_seat(project_id, seat.id))
                .map_err(|error| self.refuse(&error))?
            {
                Some(hosted) => Some(hosted.native_identity.runtime_kind),
                None if self.unconfirmed_launch(project_id, &seat)?.is_some() => {
                    self.node_runtime_kind().ok()
                }
                None => continue,
            };
            if runtime_kind.is_none_or(|kind| state.runtimes().get(&kind).is_none()) {
                return Err(self
                    .deny(
                        ApiErrorCode::Unavailable,
                        "the runtime holding the ad-hoc desk Adam's session is not configured",
                    )
                    .about("desk Adam binding"));
            }
        }
        Ok(source)
    }

    /// The prepared launch intent of an origin seat that records no
    /// occupancy: a launch that may have created its native and then lost the
    /// answer, so the empty occupancy proves nothing.
    fn unconfirmed_launch(
        &self,
        project_id: ProjectId,
        seat: &SeatBinding,
    ) -> Result<Option<StoredHostedSeatLaunchIntent>, ApiError> {
        let state = self.state()?;
        if !seat.is_non_terminal()
            || state
                .with_store(|store| store.get_hosted_topology_seat(project_id, seat.id))
                .map_err(|error| self.refuse(&error))?
                .is_some()
        {
            return Ok(None);
        }
        let generation = state
            .with_store(|store| {
                store.list_hosted_topology_seat_history_native_ids(project_id, seat.id)
            })
            .map_err(|error| self.refuse(&error))?
            .len();
        let generation = u64::try_from(generation)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        Ok(state
            .with_store(|store| {
                store.get_hosted_seat_launch_intent(project_id, seat.id, generation)
            })
            .map_err(|error| self.refuse(&error))?
            .filter(|intent| intent.state == HostedSeatLaunchIntentState::Prepared))
    }

    /// Settle an origin seat whose launch prepared its intent and recorded no
    /// native, before the binding fences the origin.
    ///
    /// The runtime is censused as a durable desk seat's recovery censuses it
    /// (ASMA-8451). No live carrier proves the launch never took effect, or
    /// that its native is already archived: there is nothing to fence. One
    /// live carrier is recovered by replaying the launch under its prepared
    /// intent, which binds that native rather than creating another, and the
    /// fence then retires it. Anything else, a census included that does not
    /// answer, refuses before anything is released, and a retry with the same
    /// key resumes here.
    async fn settle_unconfirmed_origin_launch(
        &self,
        project_id: ProjectId,
        source: &StoredDeskAdamSubject,
        seat: &SeatBinding,
        held: &tokio::sync::MutexGuard<'_, ()>,
    ) -> Result<(), ApiError> {
        let Some(intent) = self.unconfirmed_launch(project_id, seat)? else {
            return Ok(());
        };
        let state = self.state()?;
        let runtime_kind = self.node_runtime_kind()?;
        let adapter = state.runtimes().get(&runtime_kind).ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "the runtime holding the ad-hoc desk Adam's session is not configured",
            )
            .about("desk Adam binding")
        })?;
        let census = adapter
            .census_hosted_seat(&HostedSeatCensusRequest {
                seat_binding_id: seat.id,
                requested_at: kontor_api::now(),
            })
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        match census
            .carriers
            .iter()
            .filter(|carrier| !carrier.archived)
            .count()
        {
            0 => Ok(()),
            1 => {
                let node = state
                    .with_store(|store| store.get_topology_node(project_id, source.topology_node_id))
                    .map_err(|error| self.refuse(&error))?
                    .ok_or_else(|| {
                        self.deny(
                            ApiErrorCode::PlacementBlocked,
                            "the ad-hoc desk Adam's node is missing",
                        )
                    })?;
                let definition = self.published_definition(project_id, &source.team_definition)?;
                let title =
                    self.seat_name_from_definition(&definition, &node, &seat.role_slot_id)?;
                self.launch_ad_hoc_lsa_held(
                    project_id,
                    &node,
                    source,
                    seat,
                    &title,
                    &intent.model_rung.canonical_rung(),
                    held,
                )
                .await
            }
            _ => Err(self
                .deny(
                    ApiErrorCode::PlacementBlocked,
                    "several live sessions carry the ad-hoc desk Adam's seat",
                )
                .about("desk Adam binding")
                .advising(
                    "retire the sessions Kontor did not launch, then retry with the same key; nothing was released",
                )),
        }
    }

    /// Bind one desk-led epic's leadership to an ad-hoc subject.
    ///
    /// The ad-hoc seats are fenced first: a launch that lost its answer is
    /// settled against the runtime, each live session is retired in the desk
    /// workspace and archived, then every seat is released. Only then is
    /// the epic's subject row written naming its origin; the schema refuses
    /// the row while any ad-hoc seat is still active or occupied. The ad-hoc
    /// row and its seats' history are never rewritten, and the epic's LSA
    /// later launches as a fresh native with its own credential and the
    /// handoff capsule. Runs on every apply that names the origin, replays
    /// included, and converges: a fence that could not finish is finished by
    /// the retry.
    pub(super) async fn bind_desk_adam_origin(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        origin: &DeskAdamOriginRequest,
    ) -> Result<(), ApiError> {
        // Held from the census to the subject row, so no ad-hoc launch can
        // reach the runtime for the origin between them.
        let held = self.desk_adam_guard.lock().await;
        let led = self
            .epic_desk_leadership(project_id, epic_id)?
            .ok_or_else(|| self.not_desk_led())?;
        let source = self.checked_desk_adam_origin(
            project_id,
            Some(epic_id),
            &led.leadership.desk_key,
            origin,
        )?;
        if self.epic_subject(project_id, epic_id)?.is_some() {
            return Ok(());
        }
        let state = self.state()?;
        let seats = state
            .with_store(|store| store.list_seat_bindings(project_id, source.topology_node_id))
            .map_err(|error| self.refuse(&error))?;
        for seat in &seats {
            self.settle_unconfirmed_origin_launch(project_id, &source, seat, &held)
                .await?;
        }
        for seat in &seats {
            if let Some(hosted) = state
                .with_store(|store| store.get_hosted_topology_seat(project_id, seat.id))
                .map_err(|error| self.refuse(&error))?
            {
                self.fence_subject_session(
                    project_id,
                    &source,
                    &hosted,
                    (
                        "desk Adam binding of an ad-hoc topic to an epic",
                        "desk Adam binding; the ad-hoc session was already gone",
                    ),
                )
                .await?;
            }
        }
        let now = kontor_api::now();
        for seat in seats.into_iter().filter(SeatBinding::is_non_terminal) {
            state
                .with_store(|store| {
                    store.observe_seat_binding(
                        project_id,
                        seat.id,
                        &SeatLivenessObservation {
                            released_at: Some(now),
                            ..SeatLivenessObservation::default()
                        },
                        now,
                    )
                })
                .map_err(|error| self.refuse(&error))?;
        }
        let epic = self.epic_row(project_id, epic_id)?;
        let topic = self.desk_adam_topic(&epic.name)?;
        self.ensure_desk(project_id, &led.leadership.desk_key)?;
        let subject = self.record_subject(
            StoredDeskAdamSubject {
                project_id,
                topology_node_id: TopologyNodeId::generate(),
                desk_key: led.leadership.desk_key.clone(),
                mini_project_id: Some(epic_id),
                topic,
                origin_topology_node_id: Some(source.topology_node_id),
                team_definition: led.definition.clone(),
                created_at: now,
            },
            || self.epic_subject(project_id, epic_id),
        )?;
        if subject.origin_topology_node_id != Some(source.topology_node_id) {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "the epic's desk Adam is already placed from another origin",
            ));
        }
        Ok(())
    }

    /// The handoff capsule a desk-led epic's LSA launches with when its
    /// subject was bound from an ad-hoc one.
    ///
    /// Server-owned and read only from the two subject rows and the fenced
    /// seats, so the fresh native learns where its work came from without
    /// inheriting the ad-hoc session, its conversation or its credential.
    pub(super) fn desk_adam_launch_capsule(
        &self,
        control: &SessionTopologyNode,
        role_code: &RoleCode,
    ) -> Result<Option<String>, ApiError> {
        let state = self.state()?;
        let project_id = control.project_id;
        let Some(subject) = state
            .with_store(|store| store.get_desk_adam_subject(project_id, control.id))
            .map_err(|error| self.refuse(&error))?
        else {
            return Ok(None);
        };
        let Some(epic) = subject.mini_project_id else {
            return Ok(None);
        };
        let expected =
            self.epic_purpose(project_id, epic, CoreTeamSeatPurpose::ArchitectureCloser)?;
        if role_code != &expected.role.role_code {
            return Ok(None);
        }
        let Some(origin) = subject.origin_topology_node_id else {
            return Ok(None);
        };
        let source = state
            .with_store(|store| store.get_desk_adam_subject(project_id, origin))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the ad-hoc desk Adam an epic was bound from is missing",
                )
            })?;
        let fenced = state
            .with_store(|store| store.list_seat_bindings(project_id, origin))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .map(|seat| seat.id.to_string())
            .collect::<Vec<_>>();
        let capsule = self.intent(&serde_json::json!({
            "schema_version": 1,
            "continuation": "desk_adam_binding",
            "source": {
                "topology_node_id": origin.to_string(),
                "desk_key": source.desk_key.as_str(),
                "topic": source.topic.as_str(),
                "opened_at": source.created_at.to_string(),
                "fenced_seat_binding_ids": fenced,
            },
            "target": {
                "epic_id": subject.mini_project_id.map(|id| id.to_string()),
                "topology_node_id": subject.topology_node_id.to_string(),
            },
        }))?;
        Ok(Some(capsule.json().to_owned()))
    }

    /// One published Team Definition revision, refused unless its bytes still
    /// hash to the snapshot that names it.
    pub(super) fn published_definition(
        &self,
        project_id: ProjectId,
        snapshot: &TeamDefinitionSnapshot,
    ) -> Result<TeamDefinitionSpec, ApiError> {
        let definition = self
            .state()?
            .with_store(|store| {
                store.get_team_definition(project_id, snapshot.definition_id, snapshot.version)
            })
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the leadership Team Definition revision is unavailable",
                )
            })?;
        let observed = TeamDefinitionSnapshot::from_revision(&definition)
            .map_err(|error| self.refuse_domain(&error))?;
        if &observed != snapshot {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "the leadership Team Definition hash does not match its published bytes",
            ));
        }
        Ok(definition)
    }
}

/// The wire shape of one desk Adam subject.
pub(super) fn desk_adam_subject_dto(subject: &StoredDeskAdamSubject) -> DeskAdamSubjectDto {
    DeskAdamSubjectDto {
        topology_node_id: subject.topology_node_id,
        desk_key: subject.desk_key.clone(),
        epic_id: subject.mini_project_id,
        topic: subject.topic.clone(),
        origin_topology_node_id: subject.origin_topology_node_id,
        team_definition: TeamDefinitionRefDto {
            id: subject.team_definition.definition_id,
            version: subject.team_definition.version,
        },
        created_at: subject.created_at,
    }
}
