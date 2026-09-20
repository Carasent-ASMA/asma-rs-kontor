# ASMA-8239 — withdrawn generic applicability abstraction (preserved for release-LSA review)

Withdrawn by the implementer on 2026-09-20 to conform to the frozen high-scope
record `docs/evidence/ASMA-8239/HIGH-SCOPE-RECORD.md`
(SHA-256 `8ad432f88cfbd8c18c8e42748cf2a38ea9c217f1512d321754ff5d5822a85e33`),
which forbids a generic kontor-core applicability predicate or operation registry.

Self-assessment against the record's own conformance test: **nonconforming**.
`decide(operation, subject)` classified arbitrary operation/subject pairs and
enumerated gate-rejection, evaluator-attestation, adoption and retitle operations
in a reusable registry. That is the abstraction the record excludes, so it is
withdrawn rather than defended. Its nine passing cases are superseded by the
record's direct API/daemon/runtime applicability matrix.

Nothing here is lost: the full withdrawn content follows verbatim so the LSA can
inspect what was built before narrowing.

## Withdrawn file: `crates/kontor-core/src/container_recreation.rs`

```rust
//! Which admin recovery operation may make a native container exist again.
//!
//! Recreating a native container is the only container operation that *builds*
//! something. Every other one — adopt, retitle, archive, inspect — addresses a
//! native that already exists, so the worst a mistake can do is touch the wrong
//! one. A recreation mistake adds a second native beside a live one, and the
//! runtime has no way to tell the two apart afterwards.
//!
//! That is why the authority is keyed by *operation* and not only by subject
//! shape. A subject-shaped check alone would say yes to any caller holding a
//! stale native child, and the set of callers holding one grows every time a
//! recovery surface is added. The question this module answers is the narrower
//! one: is *this operation* one that Kontor has decided may create a native at
//! all.
//!
//! The set is closed and fail-closed. A new recovery surface is inapplicable
//! until someone adds it here and writes the case that proves it, which is the
//! opposite of the default that let a generic capability leak into callers it
//! was never scoped for.
//!
//! # Why evaluator recovery is not in the set
//!
//! [`crate::retired_evaluator`] attests that an exact retired seat *already*
//! rendered a verdict. It is evidence-only by construction: it holds no store,
//! no adapter and no clock, and it advances no workflow. An evaluator recovery
//! that could reach container recreation would be able to produce a native as a
//! side effect of reading history, which would make the attestation a mutation
//! of the very past it claims to be describing.

use crate::spec::NodeProjectionCapability;
use crate::{DomainError, DomainResult};

crate::closed_enum! {
    /// An admin recovery operation that may ask about container recreation.
    ///
    /// Membership here is not permission: it only means the operation is part
    /// of the vocabulary this predicate decides over. Permission is
    /// [`ContainerRecreationOperation::may_recreate`].
    ContainerRecreationOperation, "ContainerRecreationOperation" {
        /// Recovery of a role slot whose gate rejection retired its seat, and
        /// whose node lost the native child the replacement seat must work in.
        ///
        /// The one operation permitted to recreate. A rejection settlement
        /// retires the seat that held the container, so a node in this state
        /// legitimately has no native and no candidate at its canonical path —
        /// which is exactly the census result recreation requires, and exactly
        /// the state no adoption can repair.
        GateRejectionRoleSlotRecovery => "gate_rejection_role_slot_recovery",
        /// Attestation that an exact retired evaluator seat already rendered
        /// its verdict.
        ///
        /// Never permitted. See the module note: this operation reads history
        /// and must not be able to write a native while doing so.
        RetiredEvaluatorAttestation => "retired_evaluator_attestation",
        /// Adoption of the sole live native that replaces a stale binding.
        ///
        /// Never permitted, and for a reason that is easy to get backwards: it
        /// is the operation *closest* to recreation, and it already has its own
        /// census. Letting it fall through to creation when its census found
        /// nothing would silently convert "adopt what is there" into "make one",
        /// which is a different authority and a different receipt.
        StaleContainerAdoption => "stale_container_adoption",
        /// Correction of a bound container's visible title.
        ///
        /// Never permitted. A retitle whose target is missing is a stale
        /// binding to be reported, not a container to be rebuilt.
        ContainerRetitle => "container_retitle",
    }
}

impl ContainerRecreationOperation {
    /// Whether this operation may create a native container.
    ///
    /// Written as an exhaustive match rather than a set lookup so that adding a
    /// variant fails to compile until its answer is stated.
    #[must_use]
    pub const fn may_recreate(self) -> bool {
        match self {
            Self::GateRejectionRoleSlotRecovery => true,
            Self::RetiredEvaluatorAttestation
            | Self::StaleContainerAdoption
            | Self::ContainerRetitle => false,
        }
    }
}

/// The durable shape of the thing an operation proposes to recreate.
///
/// Every field is read from persisted Kontor state by the caller, never from a
/// runtime. A subject assembled from what a runtime reported would let the
/// runtime decide whether it is allowed to be written to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecreationSubject {
    /// The projection capabilities the pinned specification revision assigns to
    /// the node's kind.
    pub capabilities: Vec<NodeProjectionCapability>,
    /// Whether the persisted binding retains a canonical working directory.
    pub has_canonical_cwd: bool,
    /// Whether the persisted binding retains its exact native project ancestor.
    pub has_native_parent: bool,
}

/// Why a recreation subject or operation was refused.
///
/// Typed rather than free text because these are the refusals an operator sees,
/// and a caller that wants to distinguish "wrong operation" from "wrong shape"
/// should not have to match on a sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecreationInapplicable {
    /// The operation is never permitted to create a native container.
    OperationNotPermitted,
    /// The node's pinned kind does not materialize as a native child.
    NotNativeChild,
    /// The persisted binding retains no canonical working directory.
    NoCanonicalCwd,
    /// The persisted binding retains no exact native project ancestor.
    NoNativeParent,
}

impl RecreationInapplicable {
    /// The stable rule text reported with this refusal.
    #[must_use]
    pub const fn rule(self) -> &'static str {
        match self {
            Self::OperationNotPermitted => {
                "operation_inapplicable: this recovery operation may not create a native container"
            }
            Self::NotNativeChild => {
                "operation_inapplicable: only a native_child node's container may be recreated"
            }
            Self::NoCanonicalCwd => {
                "operation_inapplicable: the persisted binding has no canonical working directory to recreate at"
            }
            Self::NoNativeParent => {
                "operation_inapplicable: the persisted binding has no exact native project ancestor to recreate below"
            }
        }
    }
}

impl From<RecreationInapplicable> for DomainError {
    fn from(value: RecreationInapplicable) -> Self {
        Self::invalid("ContainerRecreation", value.rule())
    }
}

/// Decide whether one operation may recreate one subject's native container.
///
/// The operation is checked before the subject, deliberately. A caller that is
/// not permitted to create a native should be told that, rather than being told
/// its subject is the wrong shape — which would invite it to go and find a
/// subject of the right shape.
///
/// # Errors
/// Returns [`RecreationInapplicable`] for an operation outside the permitted
/// set, a node whose pinned kind is not a native child, and a persisted binding
/// missing either identity that recreation must preserve.
pub fn decide(
    operation: ContainerRecreationOperation,
    subject: &RecreationSubject,
) -> Result<(), RecreationInapplicable> {
    if !operation.may_recreate() {
        return Err(RecreationInapplicable::OperationNotPermitted);
    }
    if !is_native_child(subject) {
        return Err(RecreationInapplicable::NotNativeChild);
    }
    if !subject.has_canonical_cwd {
        return Err(RecreationInapplicable::NoCanonicalCwd);
    }
    if !subject.has_native_parent {
        return Err(RecreationInapplicable::NoNativeParent);
    }
    Ok(())
}

/// Whether the pinned capability set makes this node a native child.
///
/// A set carrying both `native_child` and `native_root` is not a child that
/// also happens to be a root; it is an unresolvable declaration, and the
/// runtime refuses it for the same reason. Answering "yes, child" here would
/// let recreation act on a node whose shape the runtime cannot resolve.
fn is_native_child(subject: &RecreationSubject) -> bool {
    subject
        .capabilities
        .contains(&NodeProjectionCapability::NativeChild)
        && !subject
            .capabilities
            .contains(&NodeProjectionCapability::NativeRoot)
}

/// Parse an operation supplied on the wire.
///
/// # Errors
/// Returns [`DomainError::Invalid`] for any text outside the closed set, which
/// is what makes an unrecognised operation a refusal rather than a default.
pub fn parse_operation(text: &str) -> DomainResult<ContainerRecreationOperation> {
    ContainerRecreationOperation::parse(text)
        .map_err(|_| DomainError::invalid("ContainerRecreationOperation", "is not a known value"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn child_subject() -> RecreationSubject {
        RecreationSubject {
            capabilities: vec![NodeProjectionCapability::NativeChild],
            has_canonical_cwd: true,
            has_native_parent: true,
        }
    }

    #[test]
    fn gate_rejection_role_slot_recovery_may_recreate_a_native_child() {
        assert_eq!(
            decide(
                ContainerRecreationOperation::GateRejectionRoleSlotRecovery,
                &child_subject()
            ),
            Ok(())
        );
    }

    #[test]
    fn evaluator_attestation_may_never_recreate_even_a_valid_subject() {
        assert_eq!(
            decide(
                ContainerRecreationOperation::RetiredEvaluatorAttestation,
                &child_subject()
            ),
            Err(RecreationInapplicable::OperationNotPermitted),
        );
    }

    #[test]
    fn adoption_and_retitle_may_never_recreate() {
        for operation in [
            ContainerRecreationOperation::StaleContainerAdoption,
            ContainerRecreationOperation::ContainerRetitle,
        ] {
            assert_eq!(
                decide(operation, &child_subject()),
                Err(RecreationInapplicable::OperationNotPermitted),
                "{operation} must not reach creation",
            );
        }
    }

    #[test]
    fn exactly_one_operation_is_permitted() {
        let permitted = ContainerRecreationOperation::ALL
            .iter()
            .filter(|operation| operation.may_recreate())
            .count();
        assert_eq!(
            permitted, 1,
            "widening the permitted set is a decision that must be made here, with a case",
        );
    }

    #[test]
    fn the_permitted_operation_still_refuses_a_non_child_subject() {
        for capabilities in [
            vec![NodeProjectionCapability::NativeRoot],
            vec![NodeProjectionCapability::LogicalOnly],
            // Unresolvable: child and root at once.
            vec![
                NodeProjectionCapability::NativeChild,
                NodeProjectionCapability::NativeRoot,
            ],
        ] {
            let subject = RecreationSubject {
                capabilities,
                ..child_subject()
            };
            assert_eq!(
                decide(
                    ContainerRecreationOperation::GateRejectionRoleSlotRecovery,
                    &subject
                ),
                Err(RecreationInapplicable::NotNativeChild),
            );
        }
    }

    #[test]
    fn a_session_host_native_child_is_still_a_native_child() {
        let subject = RecreationSubject {
            capabilities: vec![
                NodeProjectionCapability::NativeChild,
                NodeProjectionCapability::SessionHost,
            ],
            ..child_subject()
        };
        assert_eq!(
            decide(
                ContainerRecreationOperation::GateRejectionRoleSlotRecovery,
                &subject
            ),
            Ok(())
        );
    }

    #[test]
    fn a_child_missing_either_preserved_identity_is_refused() {
        let no_cwd = RecreationSubject {
            has_canonical_cwd: false,
            ..child_subject()
        };
        assert_eq!(
            decide(
                ContainerRecreationOperation::GateRejectionRoleSlotRecovery,
                &no_cwd
            ),
            Err(RecreationInapplicable::NoCanonicalCwd),
        );
        let no_parent = RecreationSubject {
            has_native_parent: false,
            ..child_subject()
        };
        assert_eq!(
            decide(
                ContainerRecreationOperation::GateRejectionRoleSlotRecovery,
                &no_parent
            ),
            Err(RecreationInapplicable::NoNativeParent),
        );
    }

    #[test]
    fn an_unpermitted_operation_is_refused_before_its_subject_is_judged() {
        // A malformed subject *and* a forbidden operation must report the
        // operation: telling this caller to go and fix its subject would be
        // telling it how to get a native created.
        let broken = RecreationSubject {
            capabilities: vec![NodeProjectionCapability::LogicalOnly],
            has_canonical_cwd: false,
            has_native_parent: false,
        };
        assert_eq!(
            decide(
                ContainerRecreationOperation::RetiredEvaluatorAttestation,
                &broken
            ),
            Err(RecreationInapplicable::OperationNotPermitted),
        );
    }

    #[test]
    fn operation_spellings_round_trip_and_reject_unknown_text() {
        for operation in ContainerRecreationOperation::ALL {
            assert_eq!(parse_operation(operation.as_str()).ok(), Some(*operation));
        }
        assert!(parse_operation("container_recreation").is_err());
        assert!(parse_operation("").is_err());
    }
}
```
