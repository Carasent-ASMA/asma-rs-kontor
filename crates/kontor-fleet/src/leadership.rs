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
        let pinned: PinnedRevision = revision.deserialize().map_err(|_| invalid(L01))?;
        let mut matching = pinned
            .value
            .seats
            .iter()
            .filter(|seat| &seat.role_slot_id == role_slot_id);
        let (Some(seat), None) = (matching.next(), matching.next()) else {
            return Err(invalid(L02));
        };
        if &seat.role != role {
            return Err(invalid(L03));
        }
        let core_team_revision_hash = revision.hash().clone();
        Ok(Self {
            text: format!("leadership/{core_team_revision_hash}/{role_slot_id}"),
            core_team_revision_hash,
            core_team_version: pinned.value.version,
            role_slot_id: role_slot_id.clone(),
        })
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

impl fmt::Display for LeadershipKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
