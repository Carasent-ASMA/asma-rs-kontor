//! Project Core Team, Quick-session and promotion application behavior.
//!
//! This module deliberately depends on semantic effects rather than a native
//! runtime adapter. The composition root maps those effects to the topology
//! commands that own placement; this layer never receives a native parent,
//! project id or reparent operation from a model-facing request.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use kontor_context::{ContinuationMode, HandoffCapsule, TestAttempt, WorkspaceRef};
use kontor_core::id::{
    AgentRunId, AggregateRevision, BoundedText, CanonicalDocument, ContentHash, ContextPackId,
    ExternalId, ExternalName, HandoffId, IdempotencyKey, MiniProjectId, ProjectId, QuickSessionId,
    RealmId, RoleCode, RoleSlotId, SCHEMA_VERSION, SchemaVersion, SeatBindingId, SpecVersion,
    Timestamp, TopologyKindKey, TopologyNodeId,
};
use kontor_core::spec::{CatalogRoleRef, CodeLifecycle, RoleCatalogRevision, TopologySnapshot};

// The presence policy is catalog vocabulary, not application state: the wire
// contract and this layer resolve the same spelling. Re-exported so existing
// `kontor_teams::EpicPresence` callers keep one path to it.
pub use kontor_core::spec::EpicPresence;
use kontor_core::{DomainError, DomainResult};
use serde::{Deserialize, Serialize};

kontor_core::closed_enum! {
    /// What promotion does with the source Quick session.
    #[derive(Default)]
    SourceDisposition, "SourceDisposition" {
        /// Keep the source durable and idle.
        #[default]
        Idle => "idle",
        /// Archive the source after the handoff is delivered.
        Archive => "archive",
    }
}

/// One requested Core Team role before the server resolves its catalog facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreTeamSeatSelection {
    /// Stable role code from the selected catalog revision.
    pub role_code: RoleCode,
    /// Presentation-only label.
    pub custom_display_name: Option<ExternalName>,
    /// Epic materialization policy.
    pub presence: EpicPresence,
    /// Whether this role may start a Quick session.
    pub ad_hoc_allowed: bool,
}

/// One resolved Core Team seat policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreTeamSeat {
    /// Stable slot identity. It is derived from the role code, never a label.
    pub role_slot_id: RoleSlotId,
    /// Exact role-catalog snapshot.
    pub role: CatalogRoleRef,
    /// Epic materialization policy.
    pub presence: EpicPresence,
    /// Whether this role may start a Quick session.
    pub ad_hoc_allowed: bool,
}

/// One immutable Project Core Team revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreTeamRevision {
    /// Revision of this project configuration.
    pub version: SpecVersion,
    /// Hash of the exact role catalog this revision resolved against.
    pub catalog_hash: ContentHash,
    /// Seats in deterministic declared order.
    pub seats: Vec<CoreTeamSeat>,
}

impl CoreTeamRevision {
    /// Resolve a new revision against one immutable role catalog.
    ///
    /// Missing mandatory `LSA` and `TPM` entries are inserted as required
    /// seats. Supplying either mandatory role as anything but required is
    /// rejected instead of silently weakening it.
    ///
    /// # Errors
    /// Rejects an invalid catalog, unknown/non-current/duplicate role, or a
    /// weakened mandatory role.
    pub fn resolve(
        version: SpecVersion,
        catalog: &RoleCatalogRevision,
        selections: &[CoreTeamSeatSelection],
    ) -> DomainResult<Self> {
        let catalog_hash = catalog.canonicalize()?.hash().clone();
        let mut codes = BTreeSet::new();
        let mut seats = Vec::with_capacity(selections.len() + 2);
        for selection in selections {
            if !codes.insert(selection.role_code.clone()) {
                return Err(DomainError::invalid(
                    "CoreTeamRevision",
                    "declares a duplicate role code",
                ));
            }
            seats.push(resolve_seat(catalog, selection)?);
        }

        for (code, quick) in [("TPM", false), ("LSA", true)] {
            let role_code = RoleCode::parse(code)?;
            match seats.iter().find(|seat| seat.role.role_code == role_code) {
                Some(seat) if seat.presence != EpicPresence::Required => {
                    return Err(DomainError::invalid(
                        "CoreTeamRevision",
                        "declares a mandatory epic role as optional",
                    ));
                }
                Some(_) => {}
                None => seats.insert(
                    0,
                    resolve_seat(
                        catalog,
                        &CoreTeamSeatSelection {
                            role_code,
                            custom_display_name: None,
                            presence: EpicPresence::Required,
                            ad_hoc_allowed: quick,
                        },
                    )?,
                ),
            }
        }

        Ok(Self {
            version,
            catalog_hash,
            seats,
        })
    }

    /// Roles visible in the Quick-session picker.
    #[must_use]
    pub fn quick_roles(&self) -> Vec<CatalogRoleRef> {
        self.seats
            .iter()
            .filter(|seat| seat.ad_hoc_allowed)
            .map(|seat| seat.role.clone())
            .collect()
    }

    /// Seats a new epic materializes immediately.
    #[must_use]
    pub fn initial_epic_seats(&self) -> Vec<CoreTeamSeat> {
        self.seats
            .iter()
            .filter(|seat| seat.presence != EpicPresence::OnDemand)
            .cloned()
            .collect()
    }

    fn quick_role(&self, code: &RoleCode) -> Option<&CatalogRoleRef> {
        self.seats
            .iter()
            .find(|seat| seat.ad_hoc_allowed && &seat.role.role_code == code)
            .map(|seat| &seat.role)
    }
}

/// The data-defined topology kinds this workflow projects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationalKinds {
    /// Quick Session Workspace kind.
    pub quick: TopologyKindKey,
    /// Epic Session Workspace kind.
    pub epic: TopologyKindKey,
    /// Epic Control Plane kind.
    pub control: TopologyKindKey,
}

/// Exact configured and observed PSW base identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSessionBaseBinding {
    /// Logical PSW node.
    pub topology_node_id: TopologyNodeId,
    /// Native project selected by configuration.
    pub configured_native_project_id: ExternalId,
    /// Native project read back from the runtime.
    pub observed_native_project_id: ExternalId,
}

impl ProjectSessionBaseBinding {
    fn validate(&self) -> DomainResult<()> {
        if self.configured_native_project_id != self.observed_native_project_id {
            return Err(DomainError::MissingAuthority {
                subject: "Quick session placement",
                rule: "the configured PSW base does not match runtime readback",
            });
        }
        Ok(())
    }
}

/// Immutable evidence captured from the source Quick session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuickSourceEvidence {
    /// Realm shared by the source and target.
    pub realm_id: RealmId,
    /// Native run that produced the work.
    pub source_run_id: AgentRunId,
    /// Context Pack frozen for that run.
    pub context_pack_id: ContextPackId,
    /// Hash of that exact pack.
    pub context_pack_hash: ContentHash,
    /// Source workspace identity.
    pub workspace: WorkspaceRef,
    /// Work attempted in the source.
    pub attempted_work: Vec<BoundedText>,
    /// Files touched in the source.
    pub touched_files: Vec<BoundedText>,
    /// Commits produced in the source.
    pub commits: Vec<ExternalId>,
    /// Tests attempted in the source.
    pub tests: Vec<TestAttempt>,
    /// Decisions already taken.
    pub decisions: Vec<BoundedText>,
    /// Durable evidence references.
    pub evidence: Vec<ExternalId>,
    /// Remaining work.
    pub remaining_work: Vec<BoundedText>,
    /// Known risks.
    pub risks: Vec<BoundedText>,
    /// The next action delivered to the epic LSA.
    pub recommended_next_action: BoundedText,
}

/// Open one Quick session under a bound PSW.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuickSessionRequest {
    /// Owning project.
    pub project_id: ProjectId,
    /// Eligible Core Team role.
    pub role_code: RoleCode,
    /// Optional presentation-only label for this seat.
    pub custom_display_name: Option<ExternalName>,
    /// Recorded purpose; never interpreted as policy.
    pub purpose: BoundedText,
    /// Portable source evidence.
    pub source: QuickSourceEvidence,
    /// Request instant.
    pub requested_at: Timestamp,
}

/// One durable Quick session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuickSession {
    /// Owning project.
    pub project_id: ProjectId,
    /// Quick-session identity.
    pub id: QuickSessionId,
    /// Exact role snapshot.
    pub role: CatalogRoleRef,
    /// Hosting QSW node.
    pub topology_node_id: TopologyNodeId,
    /// Data-defined QSW kind pinned by this workflow.
    pub kind: TopologyKindKey,
    /// QSW seat binding.
    pub seat_binding_id: SeatBindingId,
    /// Logical PSW parent.
    pub psw_topology_node_id: TopologyNodeId,
    /// Recorded purpose.
    pub purpose: BoundedText,
    /// Source evidence frozen at creation.
    pub source: QuickSourceEvidence,
    /// Current source disposition.
    pub disposition: SourceDisposition,
    /// Mutable aggregate revision.
    pub revision: AggregateRevision,
    /// Whether the semantic topology effect completed.
    pub materialized: bool,
    /// Creation instant.
    pub created_at: Timestamp,
    intent_hash: ContentHash,
}

/// Another immutable project configuration frozen during promotion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinnedConfiguration {
    /// Configuration identity.
    pub id: ExternalName,
    /// Exact revision.
    pub version: SpecVersion,
    /// Canonical hash of that revision.
    pub hash: ContentHash,
}

/// The requested promoted MiniProject policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromotionTarget {
    /// Tracker-neutral MiniProject name.
    pub name: ExternalName,
    /// Whether the ASMA Epic policy should activate.
    pub activate_asma_epic: bool,
    /// Confirmed Jira Epic binding, required for ASMA activation.
    pub confirmed_jira_epic_id: Option<ExternalId>,
}

impl PromotionTarget {
    fn validate(&self) -> DomainResult<()> {
        if self.activate_asma_epic && self.confirmed_jira_epic_id.is_none() {
            return Err(DomainError::MissingAuthority {
                subject: "ASMA Epic activation",
                rule: "a confirmed Jira Epic binding is required",
            });
        }
        Ok(())
    }
}

/// One logical node planned by promotion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromotionNode {
    /// Durable node id.
    pub id: TopologyNodeId,
    /// Data-defined kind.
    pub kind: TopologyKindKey,
    /// Exact logical parent.
    pub parent_id: TopologyNodeId,
}

/// One epic seat planned by promotion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromotionSeat {
    /// Durable binding id.
    pub id: SeatBindingId,
    /// Stable slot id.
    pub role_slot_id: RoleSlotId,
    /// ECP node hosting this seat.
    pub topology_node_id: TopologyNodeId,
    /// Exact role snapshot.
    pub role: CatalogRoleRef,
}

/// Immutable effects authorized by one promotion preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromotionPlan {
    /// Source Quick session.
    pub quick_session_id: QuickSessionId,
    /// New tracker-neutral MiniProject.
    pub mini_project_id: MiniProjectId,
    /// Requested target policy.
    pub target: PromotionTarget,
    /// Core Team revision frozen for the epic.
    pub core_team: CoreTeamRevision,
    /// Topology revision frozen for the epic.
    pub topology: TopologySnapshot,
    /// Other profile/configuration revisions frozen for the epic.
    pub configurations: Vec<PinnedConfiguration>,
    /// ESW followed by its ECP.
    pub nodes: Vec<PromotionNode>,
    /// Required/default ECP seats. On-demand roles are absent.
    pub seats: Vec<PromotionSeat>,
    /// Immutable portable handoff.
    pub handoff: HandoffCapsule,
    /// What happens to the source after delivery.
    pub source_disposition: SourceDisposition,
}

/// A promotion plan and the hash an apply must name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromotionPreview {
    /// Immutable plan.
    pub plan: PromotionPlan,
    /// Canonical plan hash.
    pub preview_hash: ContentHash,
}

/// Result of one applied promotion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromotionOutcome {
    /// Created MiniProject.
    pub mini_project_id: MiniProjectId,
    /// Source Quick session.
    pub quick_session_id: QuickSessionId,
    /// Exact handoff delivered to the LSA.
    pub handoff_hash: ContentHash,
    /// LSA seat that received it.
    pub lsa_seat_binding_id: SeatBindingId,
}

/// Immutable effects authorized by one explicit epic-roster upgrade preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RosterUpgradePlan {
    /// Promoted source identifying the concrete epic.
    pub quick_session_id: QuickSessionId,
    /// Roster revision currently pinned by the epic.
    pub from_version: SpecVersion,
    /// Published project roster revision to pin.
    pub target: CoreTeamRevision,
    /// New required/default seats to materialize. Existing seats are reused.
    pub new_seats: Vec<PromotionSeat>,
}

/// One roster-upgrade plan and the hash an apply must name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RosterUpgradePreview {
    /// Immutable plan.
    pub plan: RosterUpgradePlan,
    /// Canonical plan hash.
    pub preview_hash: ContentHash,
}

/// Result of one explicit roster upgrade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RosterUpgradeOutcome {
    /// Roster now pinned by the epic.
    pub core_team: CoreTeamRevision,
    /// New seats created by this upgrade.
    pub materialized_seats: Vec<PromotionSeat>,
}

/// Semantic side effects owned by topology, runtime and handoff adapters.
///
/// There is intentionally no reparent operation. Promotion moves work through
/// the handoff and leaves the native source where it is.
#[async_trait]
pub trait OperationalEffects: Send {
    /// Materialize/reconcile one QSW and its seat.
    async fn materialize_quick(
        &mut self,
        base: &ProjectSessionBaseBinding,
        session: &QuickSession,
    ) -> DomainResult<()>;

    /// Create/reconcile the MiniProject, ESW, ECP and initial seats.
    async fn materialize_epic(&mut self, plan: &PromotionPlan) -> DomainResult<()>;

    /// Materialize only the additional required/default seats in a roster move.
    async fn materialize_roster(
        &mut self,
        mini_project_id: MiniProjectId,
        seats: &[PromotionSeat],
    ) -> DomainResult<()>;

    /// Deliver one exact immutable handoff to the target LSA seat.
    async fn deliver_handoff(
        &mut self,
        lsa_seat_binding_id: SeatBindingId,
        handoff: &CanonicalDocument,
    ) -> DomainResult<()>;

    /// Archive the source only after explicit archive disposition.
    async fn archive_quick(&mut self, session: &QuickSession) -> DomainResult<()>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CoreTeamKeyBinding {
    intent_hash: ContentHash,
    project_id: ProjectId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PromotionDraft {
    intent_hash: ContentHash,
    preview: PromotionPreview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PromotionApplication {
    intent_hash: ContentHash,
    outcome: Option<PromotionOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RosterUpgradeApplication {
    intent_hash: ContentHash,
    outcome: Option<RosterUpgradeOutcome>,
}

/// Durable, serializable OP-04 application state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationalWorkflow {
    realm_id: RealmId,
    kinds: OperationalKinds,
    core_teams: BTreeMap<String, CoreTeamRevision>,
    core_team_keys: BTreeMap<String, CoreTeamKeyBinding>,
    bases: BTreeMap<String, ProjectSessionBaseBinding>,
    quick_sessions: BTreeMap<String, QuickSession>,
    quick_keys: BTreeMap<String, String>,
    promotion_drafts: BTreeMap<String, PromotionDraft>,
    promotions: BTreeMap<String, PromotionApplication>,
    promotion_keys: BTreeMap<String, String>,
    epic_rosters: BTreeMap<String, CoreTeamRevision>,
    epic_seats: BTreeMap<String, Vec<PromotionSeat>>,
    roster_drafts: BTreeMap<String, RosterUpgradePreview>,
    roster_upgrades: BTreeMap<String, RosterUpgradeApplication>,
    roster_upgrade_keys: BTreeMap<String, String>,
}

impl OperationalWorkflow {
    /// Start an empty workflow using data-defined topology kinds.
    #[must_use]
    pub fn new(realm_id: RealmId, kinds: OperationalKinds) -> Self {
        Self {
            realm_id,
            kinds,
            core_teams: BTreeMap::new(),
            core_team_keys: BTreeMap::new(),
            bases: BTreeMap::new(),
            quick_sessions: BTreeMap::new(),
            quick_keys: BTreeMap::new(),
            promotion_drafts: BTreeMap::new(),
            promotions: BTreeMap::new(),
            promotion_keys: BTreeMap::new(),
            epic_rosters: BTreeMap::new(),
            epic_seats: BTreeMap::new(),
            roster_drafts: BTreeMap::new(),
            roster_upgrades: BTreeMap::new(),
            roster_upgrade_keys: BTreeMap::new(),
        }
    }

    /// Preview one Core Team revision without changing state.
    ///
    /// # Errors
    /// Returns canonical-document validation errors.
    pub fn preview_core_team(
        &self,
        project_id: ProjectId,
        revision: CoreTeamRevision,
    ) -> DomainResult<(CoreTeamRevision, ContentHash)> {
        let hash = canonical_hash(&(&project_id, &revision))?;
        Ok((revision, hash))
    }

    /// Apply the exact Core Team preview once.
    ///
    /// # Errors
    /// Rejects a changed preview, reused key, or non-sequential revision.
    pub fn apply_core_team(
        &mut self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        revision: CoreTeamRevision,
        preview_hash: &ContentHash,
    ) -> DomainResult<CoreTeamRevision> {
        let intent_hash = canonical_hash(&(&project_id, &revision, preview_hash))?;
        let key_text = key.to_string();
        if let Some(bound) = self.core_team_keys.get(&key_text) {
            if bound.intent_hash != intent_hash || bound.project_id != project_id {
                return Err(reused_key());
            }
            return self
                .core_team(project_id)
                .cloned()
                .ok_or_else(|| DomainError::invalid("CoreTeamRevision", "replay lost its result"));
        }
        if canonical_hash(&(&project_id, &revision))? != *preview_hash {
            return Err(DomainError::invalid(
                "CoreTeamRevision",
                "apply does not match the named preview",
            ));
        }
        match self.core_team(project_id) {
            None if revision.version != SpecVersion::FIRST => {
                return Err(DomainError::invalid(
                    "CoreTeamRevision",
                    "the first project revision must be version one",
                ));
            }
            Some(current) if current.version.next()? != revision.version => {
                return Err(DomainError::invalid(
                    "CoreTeamRevision",
                    "the new revision is not the next project revision",
                ));
            }
            _ => {}
        }
        self.core_teams
            .insert(project_id.to_string(), revision.clone());
        self.core_team_keys.insert(
            key_text,
            CoreTeamKeyBinding {
                intent_hash,
                project_id,
            },
        );
        Ok(revision)
    }

    /// Read the current Project Core Team.
    #[must_use]
    pub fn core_team(&self, project_id: ProjectId) -> Option<&CoreTeamRevision> {
        self.core_teams.get(&project_id.to_string())
    }

    /// Record the exact configured/read-back PSW base.
    ///
    /// # Errors
    /// Refuses a mismatched readback.
    pub fn bind_project_base(
        &mut self,
        project_id: ProjectId,
        binding: ProjectSessionBaseBinding,
    ) -> DomainResult<()> {
        binding.validate()?;
        self.bases.insert(project_id.to_string(), binding);
        Ok(())
    }

    /// Open or reconcile one Quick session under the bound PSW.
    ///
    /// The command record is written before the semantic effect. A retry after
    /// a lost/failed response therefore uses the same node and seat ids.
    ///
    /// # Errors
    /// Rejects a missing/mismatched base, unknown or ineligible role, reused
    /// key, or effect refusal.
    pub async fn ensure_quick_session<E: OperationalEffects>(
        &mut self,
        key: &IdempotencyKey,
        request: &QuickSessionRequest,
        effects: &mut E,
    ) -> DomainResult<QuickSession> {
        let intent_hash = canonical_hash(request)?;
        if request.source.realm_id != self.realm_id {
            return Err(DomainError::RealmMismatch {
                expected: self.realm_id,
                found: request.source.realm_id,
            });
        }
        let key_text = key.to_string();
        let session_id = if let Some(id) = self.quick_keys.get(&key_text) {
            let session = self.quick_sessions.get(id).ok_or_else(|| {
                DomainError::invalid("QuickSession", "idempotency binding lost its session")
            })?;
            if session.intent_hash != intent_hash {
                return Err(reused_key());
            }
            id.clone()
        } else {
            let base = self.base(request.project_id)?.clone();
            let roster =
                self.core_team(request.project_id)
                    .ok_or(DomainError::MissingAuthority {
                        subject: "Quick session",
                        rule: "the project has no Core Team revision",
                    })?;
            let mut role = roster
                .quick_role(&request.role_code)
                .cloned()
                .ok_or_else(|| {
                    DomainError::invalid(
                        "QuickSession",
                        "the requested role is not eligible for ad-hoc work",
                    )
                })?;
            role.custom_display_name
                .clone_from(&request.custom_display_name);
            let id = QuickSessionId::generate();
            let session = QuickSession {
                project_id: request.project_id,
                id,
                role,
                topology_node_id: TopologyNodeId::generate(),
                kind: self.kinds.quick.clone(),
                seat_binding_id: SeatBindingId::generate(),
                psw_topology_node_id: base.topology_node_id,
                purpose: request.purpose.clone(),
                source: request.source.clone(),
                disposition: SourceDisposition::Idle,
                revision: AggregateRevision::INITIAL,
                materialized: false,
                created_at: request.requested_at,
                intent_hash,
            };
            self.quick_sessions.insert(id.to_string(), session);
            self.quick_keys.insert(key_text, id.to_string());
            id.to_string()
        };

        let session = self.quick_sessions[&session_id].clone();
        if !session.materialized {
            let base = self.base(session.project_id)?.clone();
            effects.materialize_quick(&base, &session).await?;
            self.quick_sessions
                .get_mut(&session_id)
                .expect("the planned Quick session is still present")
                .materialized = true;
        }
        Ok(self.quick_sessions[&session_id].clone())
    }

    /// Preview one immutable QSW-to-ESW promotion.
    ///
    /// # Errors
    /// Rejects an unknown/unmaterialized/already-concluded source, ASMA
    /// activation without a confirmed Jira Epic binding, or changed repeat
    /// preview.
    pub fn preview_promotion(
        &mut self,
        quick_session_id: QuickSessionId,
        target: PromotionTarget,
        topology: TopologySnapshot,
        configurations: Vec<PinnedConfiguration>,
        source_disposition: SourceDisposition,
    ) -> DomainResult<PromotionPreview> {
        target.validate()?;
        let quick = self.quick(quick_session_id)?.clone();
        if !quick.materialized || quick.disposition != SourceDisposition::Idle {
            return Err(DomainError::invalid(
                "Promotion",
                "the source Quick session is not an active materialized source",
            ));
        }
        let intent_hash = canonical_hash(&(
            quick_session_id,
            &target,
            &topology,
            &configurations,
            source_disposition,
        ))?;
        if let Some(draft) = self.promotion_drafts.get(&quick_session_id.to_string()) {
            if draft.intent_hash != intent_hash {
                return Err(DomainError::invalid(
                    "Promotion",
                    "the source already has a different promotion preview",
                ));
            }
            return Ok(draft.preview.clone());
        }

        let roster =
            self.core_team(quick.project_id)
                .cloned()
                .ok_or(DomainError::MissingAuthority {
                    subject: "Promotion",
                    rule: "the project has no Core Team revision",
                })?;
        let mini_project_id = MiniProjectId::generate();
        let epic_node_id = TopologyNodeId::generate();
        let control_node_id = TopologyNodeId::generate();
        let seats = roster
            .initial_epic_seats()
            .into_iter()
            .map(|seat| PromotionSeat {
                id: SeatBindingId::generate(),
                role_slot_id: seat.role_slot_id,
                topology_node_id: control_node_id,
                role: seat.role,
            })
            .collect();
        let handoff = handoff_for(&quick)?;
        let plan = PromotionPlan {
            quick_session_id,
            mini_project_id,
            target,
            core_team: roster,
            topology,
            configurations,
            nodes: vec![
                PromotionNode {
                    id: epic_node_id,
                    kind: self.kinds.epic.clone(),
                    parent_id: quick.psw_topology_node_id,
                },
                PromotionNode {
                    id: control_node_id,
                    kind: self.kinds.control.clone(),
                    parent_id: epic_node_id,
                },
            ],
            seats,
            handoff,
            source_disposition,
        };
        let preview = PromotionPreview {
            preview_hash: canonical_hash(&plan)?,
            plan,
        };
        self.promotion_drafts.insert(
            quick_session_id.to_string(),
            PromotionDraft {
                intent_hash,
                preview: preview.clone(),
            },
        );
        Ok(preview)
    }

    /// Apply a named promotion preview exactly once.
    ///
    /// Effects are individually idempotent against the durable ids in the
    /// preview. The application record is written before the first effect, so a
    /// retry after a partial or lost response continues the same promotion.
    ///
    /// # Errors
    /// Rejects a stale source revision, changed/reused key, unknown preview,
    /// missing LSA, or semantic effect refusal.
    pub async fn apply_promotion<E: OperationalEffects>(
        &mut self,
        key: &IdempotencyKey,
        quick_session_id: QuickSessionId,
        preview_hash: &ContentHash,
        expected_revision: AggregateRevision,
        effects: &mut E,
    ) -> DomainResult<PromotionOutcome> {
        let draft = self
            .promotion_drafts
            .get(&quick_session_id.to_string())
            .cloned()
            .ok_or_else(|| DomainError::invalid("Promotion", "has no preview"))?;
        if draft.preview.preview_hash != *preview_hash {
            return Err(DomainError::invalid(
                "Promotion",
                "apply does not match the named preview",
            ));
        }
        let intent_hash = canonical_hash(&(quick_session_id, preview_hash, expected_revision))?;
        let key_text = key.to_string();
        let quick_text = quick_session_id.to_string();
        if let Some(bound_quick) = self.promotion_keys.get(&key_text) {
            if bound_quick != &quick_text || self.promotions[bound_quick].intent_hash != intent_hash
            {
                return Err(reused_key());
            }
            if let Some(outcome) = &self.promotions[bound_quick].outcome {
                return Ok(outcome.clone());
            }
        } else {
            if self.promotions.contains_key(&quick_text) {
                return Err(DomainError::invalid(
                    "Promotion",
                    "the source is already bound to another apply command",
                ));
            }
            if self.quick(quick_session_id)?.revision != expected_revision {
                return Err(DomainError::invalid(
                    "Promotion",
                    "the source revision moved since preview",
                ));
            }
            self.promotions.insert(
                quick_text.clone(),
                PromotionApplication {
                    intent_hash,
                    outcome: None,
                },
            );
            self.promotion_keys.insert(key_text, quick_text.clone());
        }

        effects.materialize_epic(&draft.preview.plan).await?;
        let lsa = draft
            .preview
            .plan
            .seats
            .iter()
            .find(|seat| seat.role.role_code.as_str() == "LSA")
            .ok_or_else(|| {
                DomainError::invalid("Promotion", "the frozen roster has no LSA seat")
            })?;
        let handoff = draft
            .preview
            .plan
            .handoff
            .canonical(draft.preview.plan.handoff.realm_id)?;
        effects.deliver_handoff(lsa.id, &handoff).await?;
        if draft.preview.plan.source_disposition == SourceDisposition::Archive {
            let quick = self.quick(quick_session_id)?.clone();
            effects.archive_quick(&quick).await?;
        }

        let quick = self
            .quick_sessions
            .get_mut(&quick_text)
            .expect("the promotion source is still present");
        quick.disposition = draft.preview.plan.source_disposition;
        quick.revision = quick.revision.next()?;
        let outcome = PromotionOutcome {
            mini_project_id: draft.preview.plan.mini_project_id,
            quick_session_id,
            handoff_hash: handoff.hash().clone(),
            lsa_seat_binding_id: lsa.id,
        };
        self.promotions
            .get_mut(&quick_text)
            .expect("the promotion application is still present")
            .outcome = Some(outcome.clone());
        self.epic_rosters
            .insert(quick_text.clone(), draft.preview.plan.core_team.clone());
        self.epic_seats
            .insert(quick_text, draft.preview.plan.seats.clone());
        Ok(outcome)
    }

    /// Preview an explicit move to the project's current published roster.
    ///
    /// Existing seat identities are preserved. Only newly required/default
    /// roles are planned; removed or newly on-demand roles are not silently
    /// retired or created.
    ///
    /// # Errors
    /// Rejects an unpromoted source or a target that is not the next published
    /// project roster revision.
    pub fn preview_roster_upgrade(
        &mut self,
        quick_session_id: QuickSessionId,
    ) -> DomainResult<RosterUpgradePreview> {
        let quick_text = quick_session_id.to_string();
        let current = self.epic_rosters.get(&quick_text).cloned().ok_or_else(|| {
            DomainError::invalid("RosterUpgrade", "the source has no promoted epic")
        })?;
        let project_id = self.quick(quick_session_id)?.project_id;
        let target = self
            .core_team(project_id)
            .cloned()
            .ok_or(DomainError::MissingAuthority {
                subject: "Roster upgrade",
                rule: "the project has no Core Team revision",
            })?;
        if current.version.next()? != target.version {
            return Err(DomainError::invalid(
                "RosterUpgrade",
                "the project roster is not the next epic revision",
            ));
        }
        let draft_key = roster_draft_key(quick_session_id, target.version);
        if let Some(preview) = self.roster_drafts.get(&draft_key) {
            return Ok(preview.clone());
        }
        let existing: BTreeSet<RoleSlotId> = self.epic_seats[&quick_text]
            .iter()
            .map(|seat| seat.role_slot_id.clone())
            .collect();
        let control_node_id = self.promotion_drafts[&quick_text]
            .preview
            .plan
            .nodes
            .get(1)
            .ok_or_else(|| DomainError::invalid("RosterUpgrade", "the epic has no ECP node"))?
            .id;
        let new_seats = target
            .initial_epic_seats()
            .into_iter()
            .filter(|seat| !existing.contains(&seat.role_slot_id))
            .map(|seat| PromotionSeat {
                id: SeatBindingId::generate(),
                role_slot_id: seat.role_slot_id,
                topology_node_id: control_node_id,
                role: seat.role,
            })
            .collect();
        let plan = RosterUpgradePlan {
            quick_session_id,
            from_version: current.version,
            target,
            new_seats,
        };
        let preview = RosterUpgradePreview {
            preview_hash: canonical_hash(&plan)?,
            plan,
        };
        self.roster_drafts.insert(draft_key, preview.clone());
        Ok(preview)
    }

    /// Apply one named roster upgrade exactly once.
    ///
    /// # Errors
    /// Rejects a changed/reused key, unknown preview or semantic effect refusal.
    pub async fn apply_roster_upgrade<E: OperationalEffects>(
        &mut self,
        key: &IdempotencyKey,
        preview: &RosterUpgradePreview,
        effects: &mut E,
    ) -> DomainResult<RosterUpgradeOutcome> {
        let draft_key =
            roster_draft_key(preview.plan.quick_session_id, preview.plan.target.version);
        if self.roster_drafts.get(&draft_key) != Some(preview) {
            return Err(DomainError::invalid(
                "RosterUpgrade",
                "apply does not match the named preview",
            ));
        }
        let intent_hash = canonical_hash(&(&draft_key, &preview.preview_hash))?;
        let key_text = key.to_string();
        if let Some(bound) = self.roster_upgrade_keys.get(&key_text) {
            if bound != &draft_key || self.roster_upgrades[bound].intent_hash != intent_hash {
                return Err(reused_key());
            }
            if let Some(outcome) = &self.roster_upgrades[bound].outcome {
                return Ok(outcome.clone());
            }
        } else {
            if self.roster_upgrades.contains_key(&draft_key) {
                return Err(DomainError::invalid(
                    "RosterUpgrade",
                    "the preview is already bound to another apply command",
                ));
            }
            self.roster_upgrades.insert(
                draft_key.clone(),
                RosterUpgradeApplication {
                    intent_hash,
                    outcome: None,
                },
            );
            self.roster_upgrade_keys.insert(key_text, draft_key.clone());
        }

        let quick_text = preview.plan.quick_session_id.to_string();
        let mini_project_id = self.promotions[&quick_text]
            .outcome
            .as_ref()
            .expect("a roster preview requires an applied promotion")
            .mini_project_id;
        effects
            .materialize_roster(mini_project_id, &preview.plan.new_seats)
            .await?;
        self.epic_rosters
            .insert(quick_text.clone(), preview.plan.target.clone());
        self.epic_seats
            .get_mut(&quick_text)
            .expect("the promoted epic seats exist")
            .extend(preview.plan.new_seats.clone());
        let outcome = RosterUpgradeOutcome {
            core_team: preview.plan.target.clone(),
            materialized_seats: preview.plan.new_seats.clone(),
        };
        self.roster_upgrades
            .get_mut(&draft_key)
            .expect("the roster application exists")
            .outcome = Some(outcome.clone());
        Ok(outcome)
    }

    /// Read the roster currently pinned by one promoted epic.
    #[must_use]
    pub fn epic_roster(&self, quick_session_id: QuickSessionId) -> Option<&CoreTeamRevision> {
        self.epic_rosters.get(&quick_session_id.to_string())
    }

    fn base(&self, project_id: ProjectId) -> DomainResult<&ProjectSessionBaseBinding> {
        let base =
            self.bases
                .get(&project_id.to_string())
                .ok_or(DomainError::MissingAuthority {
                    subject: "Quick session placement",
                    rule: "the project has no bound PSW base",
                })?;
        base.validate()?;
        Ok(base)
    }

    fn quick(&self, id: QuickSessionId) -> DomainResult<&QuickSession> {
        self.quick_sessions
            .get(&id.to_string())
            .ok_or_else(|| DomainError::invalid("QuickSession", "does not exist"))
    }
}

fn resolve_seat(
    catalog: &RoleCatalogRevision,
    selection: &CoreTeamSeatSelection,
) -> DomainResult<CoreTeamSeat> {
    let entry = catalog
        .role(&selection.role_code)
        .ok_or_else(|| DomainError::invalid("CoreTeamRevision", "names an unknown role code"))?;
    if entry.lifecycle != CodeLifecycle::Current {
        return Err(DomainError::invalid(
            "CoreTeamRevision",
            "names a role that cannot create new seats",
        ));
    }
    Ok(CoreTeamSeat {
        role_slot_id: RoleSlotId::parse(&selection.role_code.as_str().to_ascii_lowercase())?,
        role: CatalogRoleRef {
            catalog_id: catalog.catalog_id,
            catalog_revision: catalog.version,
            role_code: entry.role_code.clone(),
            standard_title: entry.standard_title.clone(),
            custom_display_name: selection.custom_display_name.clone(),
        },
        presence: selection.presence,
        ad_hoc_allowed: selection.ad_hoc_allowed,
    })
}

fn handoff_for(quick: &QuickSession) -> DomainResult<HandoffCapsule> {
    let mut attempted_work = quick.source.attempted_work.clone();
    if !attempted_work.contains(&quick.purpose) {
        attempted_work.push(quick.purpose.clone());
    }
    let handoff = HandoffCapsule {
        schema_version: SCHEMA_VERSION,
        realm_id: quick.source.realm_id,
        handoff_id: HandoffId::generate(),
        continuation_mode: ContinuationMode::CrossEngineHandoff,
        source_run_id: quick.source.source_run_id,
        target_run_id: None,
        context_pack_id: quick.source.context_pack_id,
        context_pack_hash: quick.source.context_pack_hash.clone(),
        workspace: quick.source.workspace.clone(),
        attempted_work,
        touched_files: quick.source.touched_files.clone(),
        commits: quick.source.commits.clone(),
        tests: quick.source.tests.clone(),
        decisions: quick.source.decisions.clone(),
        evidence: quick.source.evidence.clone(),
        remaining_work: quick.source.remaining_work.clone(),
        risks: quick.source.risks.clone(),
        recommended_next_action: quick.source.recommended_next_action.clone(),
    };
    handoff.validate(handoff.realm_id)?;
    Ok(handoff)
}

fn canonical_hash<T: Serialize>(value: &T) -> DomainResult<ContentHash> {
    #[derive(Serialize)]
    struct Envelope<'a, T> {
        schema_version: SchemaVersion,
        value: &'a T,
    }

    Ok(CanonicalDocument::from_serializable(&Envelope {
        schema_version: SCHEMA_VERSION,
        value,
    })?
    .hash()
    .clone())
}

fn reused_key() -> DomainError {
    DomainError::invalid("IdempotencyKey", "was already bound to a different command")
}

fn roster_draft_key(quick_session_id: QuickSessionId, version: SpecVersion) -> String {
    format!("{quick_session_id}:{}", version.get())
}
