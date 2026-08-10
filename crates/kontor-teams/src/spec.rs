//! The typed team template: logical role requirements, concrete role slots, the
//! handoff DAG and the conversion into the KON-MVP-03 envelope.
//!
//! Two identifiers live here and are never conflated:
//!
//! * a **logical role** ([`kontor_core::spec::RoleRef`]) is a reusable
//!   definition — `researcher` — that a template refers to and a work profile
//!   hands work to;
//! * a **role slot** ([`RoleSlotId`]) is one concrete seat in one team —
//!   `researcher-a` — and is the only thing that ever becomes
//!   [`kontor_core::repository::NewAgentRun::role`].
//!
//! Declaring the same logical role twice is therefore legal and is spelled with
//! two slots; it is never spelled by launching a slot twice.
//!
//! Nothing in this module interprets a slot id, a role name or a gate name. A
//! template whose slots are `researcher-a`/`researcher-b` and one whose slots are
//! `q7`/`q8` validate through exactly the same code.

use std::collections::{BTreeMap, BTreeSet};

use kontor_core::id::{
    ArtifactKey, CanonicalDocument, ExternalName, GateKey, PhaseKey, RoleKey, SchemaVersion,
    SpecVersion, TeamTemplateId,
};
use kontor_core::spec::{
    ContextTemplateRef, RoleAuthority, RoleRef, SkillRef, TeamRunSnapshot, TeamTemplateRevision,
};
use kontor_core::{DomainError, DomainResult};
use serde::{Deserialize, Serialize};

/// Upper bound on the number of concrete role slots in one template.
pub const MAX_ROLE_SLOTS: usize = 64;
/// Upper bound on the successor depth a template may declare for one slot.
pub const MAX_SUCCESSOR_DEPTH: u32 = 16;
/// Upper bound on the longest handoff path a template may declare.
pub const MAX_HANDOFF_DEPTH: u32 = 64;

// ---------------------------------------------------------------------------
// Role slot identity
// ---------------------------------------------------------------------------

/// The stable address of one concrete seat in one team run.
///
/// Defined in [`kontor_core::id`] and re-exported here, because a slot is half
/// of the key a runtime admits a launch on. Team code keeps naming it through
/// this module.
pub use kontor_core::id::RoleSlotId;

// ---------------------------------------------------------------------------
// Template documents
// ---------------------------------------------------------------------------

/// How many concrete slots one logical role must be represented by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleRequirement {
    /// The pinned logical role revision.
    pub role: RoleRef,
    /// Fewest slots that must carry this role. At least one.
    pub min_slots: u32,
    /// Most slots that may carry this role. At least `min_slots`.
    pub max_slots: u32,
}

/// Who may waive one role slot's obligation at team closure.
///
/// A waiver is authority plus evidence, never a flag: a slot is only excused by
/// a role the template already authorized, and only with every reference the
/// template demands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleSlotWaiverPolicy {
    /// Roles allowed to excuse this slot. Never empty, never the slot's own
    /// logical role.
    pub authorized_roles: Vec<RoleKey>,
    /// Evidence every waiver of this slot must cite. Never empty.
    pub required_evidence: Vec<ArtifactKey>,
}

/// One concrete seat in a team.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleSlotSpec {
    /// The stable slot address.
    pub id: RoleSlotId,
    /// The pinned logical role this seat fills.
    pub role: RoleRef,
    /// Pinned skill revisions this seat requires.
    pub skills: Vec<SkillRef>,
    /// Pinned context-pack template, if the seat prescribes one.
    pub context: Option<ContextTemplateRef>,
    /// Gates the seat's role may pass or reject.
    pub may_evaluate: Vec<GateKey>,
    /// Gates the seat's role may waive. Disjoint from `may_evaluate`.
    pub may_waive: Vec<GateKey>,
    /// How this seat may be excused at closure, if it may be at all.
    pub waiver_policy: Option<RoleSlotWaiverPolicy>,
}

/// One declared handoff between two slots of the same team.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleHandoff {
    /// The slot that hands work over.
    pub from_slot: RoleSlotId,
    /// The slot that receives it.
    pub to_slot: RoleSlotId,
    /// The profile phase after which the handoff becomes available.
    ///
    /// Optional because one template serves several work profiles, whose phase
    /// vocabularies differ. When it is present the composing pack proves the
    /// phase exists in the profile that pinned this template.
    pub after_phase: Option<PhaseKey>,
    /// Artifacts that must exist for the handoff to be legal.
    pub required_artifacts: Vec<ArtifactKey>,
}

/// A complete, versioned team template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamTemplateSpec {
    /// Schema generation of this document.
    pub schema_version: SchemaVersion,
    /// The template id shared by every revision.
    pub template_id: TeamTemplateId,
    /// This immutable revision.
    pub version: SpecVersion,
    /// Human name.
    pub name: ExternalName,
    /// Logical roles the template requires, with their cardinality.
    pub roles: Vec<RoleRequirement>,
    /// The concrete seats. Cardinality is counted from these, never from
    /// however many sessions happen to be alive.
    pub slots: Vec<RoleSlotSpec>,
    /// The handoff DAG over `slots`.
    pub handoffs: Vec<RoleHandoff>,
    /// How many times one slot may be replaced by a successor attempt.
    pub max_successor_depth: u32,
    /// The longest handoff path this template allows.
    pub max_handoff_depth: u32,
}

impl TeamTemplateSpec {
    /// Validate the whole template.
    ///
    /// Rejects: an empty or oversized slot set, duplicate slot ids, duplicate
    /// role requirements, invalid cardinality, a slot whose logical role is not
    /// required at that exact revision, a role represented by too few or too
    /// many slots, duplicate skill or context pins, gate authority that overlaps
    /// between evaluating and waiving the same gate, a waiver policy without
    /// authority or evidence, a waiver authorized by the slot's own role, and
    /// handoffs that are self, duplicate, dangling, cyclic or deeper than the
    /// declared bound.
    ///
    /// # Errors
    /// Returns the first [`DomainError`]. Errors name the rule, never the
    /// offending document content.
    pub fn validate(&self) -> DomainResult<()> {
        self.validate_bounds()?;
        let required = self.validate_role_requirements()?;
        self.validate_slots(&required)?;
        self.validate_cardinality(&required)?;
        self.validate_authority()?;
        self.validate_handoffs()?;
        Ok(())
    }

    /// Validate, canonicalize and hash in one step.
    ///
    /// # Errors
    /// As [`TeamTemplateSpec::validate`], plus canonicalization failures.
    pub fn canonicalize(&self) -> DomainResult<CanonicalDocument> {
        self.validate()?;
        CanonicalDocument::from_serializable(self)
    }

    fn validate_bounds(&self) -> DomainResult<()> {
        if self.slots.is_empty() {
            return Err(DomainError::invalid(
                "TeamTemplateSpec",
                "must declare at least one role slot",
            ));
        }
        if self.slots.len() > MAX_ROLE_SLOTS {
            return Err(DomainError::invalid(
                "TeamTemplateSpec",
                "declares more role slots than the bound allows",
            ));
        }
        if self.max_successor_depth > MAX_SUCCESSOR_DEPTH {
            return Err(DomainError::invalid(
                "TeamTemplateSpec",
                "declares a successor depth beyond the global bound",
            ));
        }
        if self.max_handoff_depth == 0 || self.max_handoff_depth > MAX_HANDOFF_DEPTH {
            return Err(DomainError::invalid(
                "TeamTemplateSpec",
                "declares a handoff depth outside the global bound",
            ));
        }
        Ok(())
    }

    fn validate_role_requirements(&self) -> DomainResult<BTreeMap<&RoleKey, &RoleRequirement>> {
        if self.roles.is_empty() {
            return Err(DomainError::invalid(
                "TeamTemplateSpec",
                "must declare at least one logical role requirement",
            ));
        }
        let mut required: BTreeMap<&RoleKey, &RoleRequirement> = BTreeMap::new();
        for requirement in &self.roles {
            if requirement.min_slots == 0
                || requirement.min_slots > requirement.max_slots
                || requirement.max_slots as usize > MAX_ROLE_SLOTS
            {
                return Err(DomainError::invalid(
                    "TeamTemplateSpec",
                    "declares role cardinality outside 1 <= min <= max <= bound",
                ));
            }
            if required
                .insert(&requirement.role.role, requirement)
                .is_some()
            {
                return Err(DomainError::invalid(
                    "TeamTemplateSpec",
                    "declares a duplicate logical role requirement",
                ));
            }
        }
        Ok(required)
    }

    fn validate_slots(&self, required: &BTreeMap<&RoleKey, &RoleRequirement>) -> DomainResult<()> {
        let mut slot_ids: BTreeSet<&RoleSlotId> = BTreeSet::new();
        for slot in &self.slots {
            if !slot_ids.insert(&slot.id) {
                return Err(DomainError::invalid(
                    "TeamTemplateSpec",
                    "declares a duplicate role slot id",
                ));
            }
            let requirement = required.get(&slot.role.role).ok_or(DomainError::Invalid {
                subject: "TeamTemplateSpec",
                rule: "a slot fills a logical role the template does not require",
            })?;
            if requirement.role.version != slot.role.version {
                return Err(DomainError::invalid(
                    "TeamTemplateSpec",
                    "a slot pins a different revision of its logical role",
                ));
            }
            let mut skills: BTreeSet<&kontor_core::id::SkillKey> = BTreeSet::new();
            for skill in &slot.skills {
                if !skills.insert(&skill.skill) {
                    return Err(DomainError::invalid(
                        "TeamTemplateSpec",
                        "a slot pins the same skill twice",
                    ));
                }
            }
            if let Some(policy) = &slot.waiver_policy {
                Self::validate_waiver_policy(policy, &slot.role.role)?;
            }
        }
        Ok(())
    }

    fn validate_waiver_policy(policy: &RoleSlotWaiverPolicy, own: &RoleKey) -> DomainResult<()> {
        if policy.authorized_roles.is_empty() {
            return Err(DomainError::MissingAuthority {
                subject: "role slot waiver",
                rule: "a waivable slot must declare waiver authority",
            });
        }
        if policy.required_evidence.is_empty() {
            return Err(DomainError::MissingEvidence {
                subject: "role slot waiver",
                rule: "a waivable slot must declare the evidence a waiver cites",
            });
        }
        let distinct: BTreeSet<&RoleKey> = policy.authorized_roles.iter().collect();
        if distinct.len() != policy.authorized_roles.len() {
            return Err(DomainError::invalid(
                "TeamTemplateSpec",
                "a waiver policy lists a duplicate authorized role",
            ));
        }
        if distinct.contains(own) {
            return Err(DomainError::MissingAuthority {
                subject: "role slot waiver",
                rule: "a slot's own role must not excuse the slot",
            });
        }
        let evidence: BTreeSet<&ArtifactKey> = policy.required_evidence.iter().collect();
        if evidence.len() != policy.required_evidence.len() {
            return Err(DomainError::invalid(
                "TeamTemplateSpec",
                "a waiver policy lists a duplicate evidence reference",
            ));
        }
        Ok(())
    }

    fn validate_cardinality(
        &self,
        required: &BTreeMap<&RoleKey, &RoleRequirement>,
    ) -> DomainResult<()> {
        for (role, requirement) in required {
            let declared = u32::try_from(self.cardinality_of(role)).unwrap_or(u32::MAX);
            if declared < requirement.min_slots {
                return Err(DomainError::invalid(
                    "TeamTemplateSpec",
                    "a logical role is represented by fewer slots than it requires",
                ));
            }
            if declared > requirement.max_slots {
                return Err(DomainError::invalid(
                    "TeamTemplateSpec",
                    "a logical role is represented by more slots than it allows",
                ));
            }
        }
        Ok(())
    }

    /// Prove no role can both decide and forgive the same gate.
    ///
    /// The check runs on the *merged* authority, because two slots of one
    /// logical role produce one [`RoleAuthority`] entry: letting `researcher-a`
    /// evaluate a gate while `researcher-b` waives it would hand the role both
    /// powers through the back door.
    fn validate_authority(&self) -> DomainResult<()> {
        for slot in &self.slots {
            let evaluate: BTreeSet<&GateKey> = slot.may_evaluate.iter().collect();
            if evaluate.len() != slot.may_evaluate.len() {
                return Err(DomainError::invalid(
                    "TeamTemplateSpec",
                    "a slot lists a duplicate evaluated gate",
                ));
            }
            let waive: BTreeSet<&GateKey> = slot.may_waive.iter().collect();
            if waive.len() != slot.may_waive.len() {
                return Err(DomainError::invalid(
                    "TeamTemplateSpec",
                    "a slot lists a duplicate waived gate",
                ));
            }
        }
        for authority in self.role_authority() {
            let evaluate: BTreeSet<&GateKey> = authority.may_evaluate.iter().collect();
            if authority
                .may_waive
                .iter()
                .any(|gate| evaluate.contains(gate))
            {
                return Err(DomainError::MissingAuthority {
                    subject: "team role authority",
                    rule: "waiver authority must be distinct from evaluator authority",
                });
            }
        }
        Ok(())
    }

    fn validate_handoffs(&self) -> DomainResult<()> {
        let declared: BTreeSet<&RoleSlotId> = self.slots.iter().map(|slot| &slot.id).collect();
        let mut successors: BTreeMap<&RoleSlotId, BTreeSet<&RoleSlotId>> = self
            .slots
            .iter()
            .map(|slot| (&slot.id, BTreeSet::new()))
            .collect();
        let mut indegree: BTreeMap<&RoleSlotId, usize> =
            self.slots.iter().map(|slot| (&slot.id, 0)).collect();

        for handoff in &self.handoffs {
            if !declared.contains(&handoff.from_slot) || !declared.contains(&handoff.to_slot) {
                return Err(DomainError::invalid(
                    "TeamTemplateSpec",
                    "a handoff references an undeclared role slot",
                ));
            }
            if handoff.from_slot == handoff.to_slot {
                return Err(DomainError::invalid(
                    "TeamTemplateSpec",
                    "a handoff connects a slot to itself",
                ));
            }
            let inserted = successors
                .get_mut(&handoff.from_slot)
                .is_some_and(|set| set.insert(&handoff.to_slot));
            if !inserted {
                return Err(DomainError::invalid(
                    "TeamTemplateSpec",
                    "declares a duplicate handoff",
                ));
            }
            if let Some(degree) = indegree.get_mut(&handoff.to_slot) {
                *degree += 1;
            }
            let artifacts: BTreeSet<&ArtifactKey> = handoff.required_artifacts.iter().collect();
            if artifacts.len() != handoff.required_artifacts.len() {
                return Err(DomainError::invalid(
                    "TeamTemplateSpec",
                    "a handoff lists a duplicate required artifact",
                ));
            }
        }

        // Kahn's algorithm, carrying the longest path so a cycle and a too-deep
        // chain are decided by the same traversal. Multiple roots and joins are
        // legal: independent lanes are exactly what a research team looks like.
        let mut depth: BTreeMap<&RoleSlotId, u32> =
            self.slots.iter().map(|slot| (&slot.id, 0)).collect();
        let mut queue: Vec<&RoleSlotId> = indegree
            .iter()
            .filter(|(_, degree)| **degree == 0)
            .map(|(slot, _)| *slot)
            .collect();
        let mut visited = 0usize;
        let mut longest = 0u32;
        while let Some(slot) = queue.pop() {
            visited += 1;
            let here = depth.get(slot).copied().unwrap_or(0);
            longest = longest.max(here);
            let onward: Vec<&RoleSlotId> = successors
                .get(slot)
                .map(|set| set.iter().copied().collect())
                .unwrap_or_default();
            for next in onward {
                if let Some(reached) = depth.get_mut(next) {
                    *reached = (*reached).max(here.saturating_add(1));
                }
                if let Some(degree) = indegree.get_mut(next) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push(next);
                    }
                }
            }
        }
        if visited != self.slots.len() {
            return Err(DomainError::invalid(
                "TeamTemplateSpec",
                "the handoff graph contains a cycle",
            ));
        }
        if longest > self.max_handoff_depth {
            return Err(DomainError::invalid(
                "TeamTemplateSpec",
                "the longest handoff path exceeds the declared bound",
            ));
        }
        Ok(())
    }

    /// The authority this template carries, merged per logical role.
    ///
    /// This is what [`TeamTemplateRevision::role_authority`] and
    /// [`TeamRunSnapshot::role_authority`] are derived from, so a run judges
    /// authority against exactly what the template declared.
    #[must_use]
    pub fn role_authority(&self) -> Vec<RoleAuthority> {
        let mut merged: BTreeMap<&RoleKey, (BTreeSet<&GateKey>, BTreeSet<&GateKey>)> =
            BTreeMap::new();
        for slot in &self.slots {
            let entry = merged
                .entry(&slot.role.role)
                .or_insert_with(|| (BTreeSet::new(), BTreeSet::new()));
            entry.0.extend(slot.may_evaluate.iter());
            entry.1.extend(slot.may_waive.iter());
        }
        merged
            .into_iter()
            .map(|(role, (evaluate, waive))| RoleAuthority {
                role: role.clone(),
                may_evaluate: evaluate.into_iter().cloned().collect(),
                may_waive: waive.into_iter().cloned().collect(),
            })
            .collect()
    }

    /// Look up one declared slot.
    #[must_use]
    pub fn slot(&self, id: &RoleSlotId) -> Option<&RoleSlotSpec> {
        self.slots.iter().find(|slot| &slot.id == id)
    }

    /// Every slot that carries one logical role, in declaration order.
    #[must_use]
    pub fn slots_of(&self, role: &RoleKey) -> Vec<&RoleSlotSpec> {
        self.slots
            .iter()
            .filter(|slot| &slot.role.role == role)
            .collect()
    }

    /// How many slots carry one logical role.
    #[must_use]
    pub fn cardinality_of(&self, role: &RoleKey) -> usize {
        self.slots_of(role).len()
    }

    /// Canonicalize into the KON-MVP-03 envelope.
    ///
    /// The envelope duplicates the id, version, name and authority that also
    /// live inside the canonical definition. [`TeamTemplateSpec::from_revision`]
    /// proves the two agree, so a hand-edited envelope cannot claim a template
    /// the bytes do not describe.
    ///
    /// # Errors
    /// As [`TeamTemplateSpec::canonicalize`].
    pub fn to_revision(&self) -> DomainResult<TeamTemplateRevision> {
        Ok(TeamTemplateRevision {
            template_id: self.template_id,
            version: self.version,
            name: self.name.clone(),
            definition: self.canonicalize()?,
            role_authority: self.role_authority(),
        })
    }

    /// Read a template back out of its envelope, proving both halves agree.
    ///
    /// # Errors
    /// Returns [`DomainError`] when the definition does not parse, does not
    /// validate, does not hash to the stored digest, or disagrees with the
    /// envelope's id, version, name or authority.
    pub fn from_revision(revision: &TeamTemplateRevision) -> DomainResult<Self> {
        let spec: Self = revision.definition.deserialize()?;
        spec.verify_envelope(
            revision.template_id,
            revision.version,
            Some(&revision.name),
            &revision.role_authority,
            revision.definition.hash(),
        )?;
        Ok(spec)
    }

    /// Read a template back out of the copy frozen into a team run.
    ///
    /// A run snapshot carries no separate name — the definition owns it — so
    /// the name is the one field this path has nothing to cross-check.
    ///
    /// # Errors
    /// As [`TeamTemplateSpec::from_revision`].
    pub fn from_snapshot(snapshot: &TeamRunSnapshot) -> DomainResult<Self> {
        let spec: Self = snapshot.definition.deserialize()?;
        spec.verify_envelope(
            snapshot.template_id,
            snapshot.template_version,
            None,
            &snapshot.role_authority,
            snapshot.definition.hash(),
        )?;
        Ok(spec)
    }

    fn verify_envelope(
        &self,
        template_id: TeamTemplateId,
        version: SpecVersion,
        name: Option<&ExternalName>,
        role_authority: &[RoleAuthority],
        digest: &kontor_core::id::ContentHash,
    ) -> DomainResult<()> {
        let document = self.canonicalize()?;
        if document.hash() != digest {
            return Err(DomainError::invalid(
                "TeamTemplateSpec",
                "the stored definition does not match its recorded digest",
            ));
        }
        if self.template_id != template_id
            || self.version != version
            || name.is_some_and(|declared| declared != &self.name)
        {
            return Err(DomainError::invalid(
                "TeamTemplateSpec",
                "the envelope names a different template revision than the definition",
            ));
        }
        if self.role_authority() != role_authority {
            return Err(DomainError::MissingAuthority {
                subject: "team template",
                rule: "the envelope carries authority the definition does not derive",
            });
        }
        Ok(())
    }
}

/// Publish the next revision of a template without touching the previous one.
///
/// The clone is versioned *before* the edit runs, and the result is checked
/// afterwards, so an edit can change the team but cannot rename the template or
/// choose its own version number.
///
/// # Errors
/// * [`DomainError::Invalid`] when the edit changed the template id or the
///   version, or when the revised template does not validate.
/// * Version overflow, from [`SpecVersion::next`].
pub fn revise_team_template<F>(
    previous: &TeamTemplateSpec,
    edit: F,
) -> DomainResult<TeamTemplateSpec>
where
    F: FnOnce(&mut TeamTemplateSpec),
{
    let expected = previous.version.next()?;
    let mut revised = previous.clone();
    revised.version = expected;
    edit(&mut revised);
    if revised.template_id != previous.template_id {
        return Err(DomainError::invalid(
            "TeamTemplateSpec",
            "a revision must preserve the template's logical id",
        ));
    }
    if revised.version != expected {
        return Err(DomainError::invalid(
            "TeamTemplateSpec",
            "a revision must publish exactly the next version",
        ));
    }
    revised.validate()?;
    Ok(revised)
}

/// A bundle of team templates, as a data file carries them.
///
/// The loader is the same one a deployment uses for its own templates; the file
/// this crate ships is data, not a code path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamPackSpec {
    /// Schema generation of this document.
    pub schema_version: SchemaVersion,
    /// The templates it carries.
    pub teams: Vec<TeamTemplateSpec>,
}

impl TeamPackSpec {
    /// Validate every template and prove their `(id, version)` identities are
    /// unique.
    ///
    /// # Errors
    /// As [`TeamTemplateSpec::validate`], plus a duplicate identity.
    pub fn validate(&self) -> DomainResult<()> {
        let mut seen: BTreeSet<(TeamTemplateId, SpecVersion)> = BTreeSet::new();
        for team in &self.teams {
            team.validate()?;
            if !seen.insert((team.template_id, team.version)) {
                return Err(DomainError::invalid(
                    "TeamPackSpec",
                    "declares a duplicate team template revision",
                ));
            }
        }
        Ok(())
    }
}

/// The team templates bundled with this build, as data.
const BUNDLED_TEAMS: &str = include_str!("../fixtures/mvp-team-pack.json");

/// Parse and validate a team pack from its data form.
///
/// # Errors
/// Returns [`DomainError`] when the text is not a valid pack document or a
/// template does not validate.
pub fn parse_team_pack(json: &str) -> DomainResult<TeamPackSpec> {
    let pack: TeamPackSpec = serde_json::from_str(json)
        .map_err(|_| DomainError::invalid("TeamPackSpec", "is not a valid team pack document"))?;
    pack.validate()?;
    Ok(pack)
}

/// The bundled team templates, parsed through the same loader a deployment uses
/// for its own file.
///
/// # Errors
/// As [`parse_team_pack`].
pub fn bundled_teams() -> DomainResult<TeamPackSpec> {
    parse_team_pack(BUNDLED_TEAMS)
}
