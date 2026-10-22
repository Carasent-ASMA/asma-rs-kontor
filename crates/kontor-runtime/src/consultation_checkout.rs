//! Exact consultation placement, frozen before Git or native runtime effects.

use kontor_core::branch::TrackerKey;
use kontor_core::id::{AggregateRevision, ContentHash, MiniProjectId, ProjectId, TopologyNodeId};
use kontor_core::placement::ConsultationLayout;
use serde::{Deserialize, Serialize};

use crate::checkout::{self, CheckoutError, TaskCheckout};

/// Existing checkout machinery narrowed to one epic's durable consultation node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsultationCheckout {
    /// Canonical intent format.
    pub schema_version: u32,
    /// Owning project; never inferred from the filesystem.
    pub project_id: ProjectId,
    /// Owning epic.
    pub epic_id: MiniProjectId,
    /// Exact consultation node.
    pub node_id: TopologyNodeId,
    /// Node revision witnessed before the first native effect.
    pub node_revision: AggregateRevision,
    /// Exact topology snapshot, unaffected by later project selection.
    pub topology: kontor_core::spec::TopologySnapshot,
    /// Stable native correlation identity written before dispatch.
    pub container_binding_id: String,
    /// Epic's immutable selected bundle, including the layout.
    pub bundle_hash: ContentHash,
    /// Exact policy from that verified bundle.
    pub policy: ConsultationLayout,
    /// Confirmed canonical epic key.
    pub key: String,
    /// Explicit root source, resolved commit, destination and branch.
    pub checkout: TaskCheckout,
}

impl ConsultationCheckout {
    /// Prove exact rendering as well as Git-source and containment invariants.
    ///
    /// # Errors
    /// Refuses altered policy, source, branch or destination.
    pub fn validate(&self) -> Result<(), CheckoutError> {
        crate::container::ContainerBindingId::parse(&self.container_binding_id)
            .map_err(|_| CheckoutError("consultation.container_binding_invalid"))?;
        let key = TrackerKey::parse(&self.key)
            .map_err(|_| CheckoutError("consultation.binding_unconfirmed"))?;
        let (relative, managed, branch) = self
            .policy
            .render(&key, self.node_id)
            .map_err(CheckoutError)?;
        let root = &self.checkout.project_root;
        if self.schema_version != 1
            || self.checkout.destination != format!("{root}/{relative}")
            || self.checkout.claim != self.checkout.destination
            || self.checkout.managed_root.as_deref() != Some(format!("{root}/{managed}").as_str())
            || self.checkout.branch != branch.as_str()
            || self.checkout.confirmed_keys != [self.key.to_string()]
            || self.checkout.module.is_some()
            || self.checkout.source.declaration.path != *root
        {
            return Err(CheckoutError("consultation.placement_binding_mismatch"));
        }
        self.checkout.validate()
    }

    /// Verify the exact already-prepared checkout; never create in an adapter.
    ///
    /// # Errors
    /// Refuses any source, rendering, root, common-directory or branch drift.
    pub fn verify(&self) -> Result<(), CheckoutError> {
        self.validate()?;
        checkout::verify(&self.checkout)
    }
}
