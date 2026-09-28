//! The schema_version 2 leadership seat key.
//!
//! Epic leadership seats have no team template to key on. They have a pinned
//! Core Team revision and a stable slot in it, so the key names exactly those:
//! `leadership/<core-team-revision-hash>/<role-slot-id>`. The hash is the
//! canonical content hash of the complete immutable revision — its version,
//! catalog hash and ordered seat snapshots — so a later roster edit is a
//! different key rather than a silent reinterpretation of an old one.
//!
//! Current Core Team slot ids are the lowercased role code, which is why no
//! shorter spelling is accepted: `leadership/lsa` would be `core/<role_code>`
//! renamed. The only constructor takes the pinned revision and its seat and
//! proves the seat is in it; there is no parser, `FromStr` or `Deserialize`.

use std::fmt;

use kontor_core::id::{CanonicalDocument, ContentHash, RoleSlotId, SpecVersion};
use kontor_core::spec::CatalogRoleRef;
use serde::Deserialize;

use crate::rule::{L01, L02, L03};
use crate::{FleetError, invalid};

/// A proved leadership seat key: one slot of one pinned Core Team revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeadershipKey {
    core_team_revision_hash: ContentHash,
    core_team_version: SpecVersion,
    role_slot_id: RoleSlotId,
    text: String,
}

/// The part of a canonical Core Team revision this key must prove.
///
/// `kontor-teams` canonicalizes a revision inside its one hashing envelope,
/// `{"schema_version": …, "value": <revision>}`. Unread revision fields are
/// still covered: the key's hash is of the whole document, not of this view.
#[derive(Deserialize)]
struct PinnedRevision {
    value: PinnedRoster,
}

#[derive(Deserialize)]
struct PinnedRoster {
    version: SpecVersion,
    seats: Vec<PinnedSeat>,
}

#[derive(Deserialize)]
struct PinnedSeat {
    role_slot_id: RoleSlotId,
    role: CatalogRoleRef,
}

impl LeadershipKey {
    /// Build the key for one seat of one pinned Core Team revision.
    ///
    /// `revision` is the canonical document of the complete revision
    /// (`CoreTeamRevision::canonicalize` in `kontor-teams`); `role_slot_id` and
    /// `role` are that seat's stable slot and frozen role snapshot.
    ///
    /// # Errors
    /// Refuses with L-01 when the document is not a Core Team revision, L-02
    /// when the slot is absent or occurs more than once, and L-03 when the
    /// revision pins a different role snapshot for that slot.
    pub fn for_pinned_seat(
        revision: &CanonicalDocument,
        role_slot_id: &RoleSlotId,
        role: &CatalogRoleRef,
    ) -> Result<Self, FleetError> {
        let (pinned, seat) = pinned_seat(revision, role_slot_id)?;
        if &seat.role != role {
            return Err(invalid(L03));
        }
        Ok(Self::of(revision, &pinned, role_slot_id))
    }

    /// Build the key for one stable slot of one verified Core Team revision,
    /// taking the seat's frozen role from the revision itself.
    ///
    /// For a reader that holds the selected roster artifact and a slot id but
    /// no separate seat snapshot — the direct-mode reader. The revision is
    /// still the only source of the seat: the slot must occur exactly once in
    /// it, so a slot the roster does not pin, or pins twice, builds nothing.
    ///
    /// # Errors
    /// L-01 when the document is not a Core Team revision, L-02 when the slot
    /// is absent or occurs more than once.
    pub fn for_pinned_slot(
        revision: &CanonicalDocument,
        role_slot_id: &RoleSlotId,
    ) -> Result<Self, FleetError> {
        let (pinned, _) = pinned_seat(revision, role_slot_id)?;
        Ok(Self::of(revision, &pinned, role_slot_id))
    }

    /// The stable slots a canonical Core Team revision pins, in its order.
    ///
    /// # Errors
    /// L-01 when the document is not a Core Team revision, L-02 when a slot
    /// occurs more than once.
    pub fn pinned_slots(revision: &CanonicalDocument) -> Result<Vec<RoleSlotId>, FleetError> {
        let pinned: PinnedRevision = revision.deserialize().map_err(|_| invalid(L01))?;
        let slots: Vec<RoleSlotId> = pinned
            .value
            .seats
            .into_iter()
            .map(|seat| seat.role_slot_id)
            .collect();
        let unique: std::collections::BTreeSet<&RoleSlotId> = slots.iter().collect();
        if unique.len() != slots.len() {
            return Err(invalid(L02));
        }
        Ok(slots)
    }

    fn of(
        revision: &CanonicalDocument,
        pinned: &PinnedRevision,
        role_slot_id: &RoleSlotId,
    ) -> Self {
        let core_team_revision_hash = revision.hash().clone();
        Self {
            text: format!("leadership/{core_team_revision_hash}/{role_slot_id}"),
            core_team_revision_hash,
            core_team_version: pinned.value.version,
            role_slot_id: role_slot_id.clone(),
        }
    }

    /// The canonical content hash of the pinned Core Team revision.
    #[must_use]
    pub fn core_team_revision_hash(&self) -> &ContentHash {
        &self.core_team_revision_hash
    }

    /// The pinned revision's version, for evidence; identity is the hash.
    #[must_use]
    pub fn core_team_version(&self) -> SpecVersion {
        self.core_team_version
    }

    /// The seat's stable slot.
    #[must_use]
    pub fn role_slot_id(&self) -> &RoleSlotId {
        &self.role_slot_id
    }

    /// The binding key text a schema_version 2 policy binds.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

/// The pinned revision's view and its one seat for `role_slot_id`.
fn pinned_seat(
    revision: &CanonicalDocument,
    role_slot_id: &RoleSlotId,
) -> Result<(PinnedRevision, PinnedSeat), FleetError> {
    let mut pinned: PinnedRevision = revision.deserialize().map_err(|_| invalid(L01))?;
    let mut positions = pinned
        .value
        .seats
        .iter()
        .enumerate()
        .filter(|(_, seat)| &seat.role_slot_id == role_slot_id)
        .map(|(position, _)| position);
    let (Some(position), None) = (positions.next(), positions.next()) else {
        return Err(invalid(L02));
    };
    let seat = pinned.value.seats.swap_remove(position);
    Ok((pinned, seat))
}

impl fmt::Display for LeadershipKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
