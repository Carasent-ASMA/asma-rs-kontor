//! One deterministic joint allocation of a Committee's slots (ASMA-8280 G-5).
//!
//! Direct-mode resolution and governed Committee admission both call
//! [`allocate`]; neither keeps an allocator of its own. The input is pure: each
//! slot's ordered, flattened candidates — the policy's own routes, or a legacy
//! caller's template routes — each carrying the policy vendor when a policy
//! names one and the independence key reviewers must not share, and each
//! slot's stated [`Eligibility`]. Nothing else is read.
//!
//! The order is the whole tie-break: slots in input order, then each slot's
//! candidates in the order given, which is the policy's chain and rung order.
//! The first complete assignment that order reaches is the allocation. There is
//! no partial answer: either every slot has a route, or none does and the
//! receipt says why.

use std::collections::BTreeMap;

use kontor_core::spec::ModelRung;
use serde::{Deserialize, Serialize};

use crate::Eligibility;

/// The independence a joint allocation holds its reviewers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationDiversity {
    /// Reviewers may share an independence key. Only a legacy Committee
    /// template that declares no diversity asks for this.
    None,
    /// Every reviewer's independence key is known, and no two reviewers share
    /// one. A reviewer candidate with no key cannot fill the slot.
    DistinctVendorPerReviewer,
}

/// What one slot does in the Committee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationRole {
    /// Held to the diversity rule.
    Reviewer,
    /// Held only to its own eligibility.
    Judge,
}

/// One route a slot may take, flattened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AllocationCandidate {
    /// The exact account alias, model and effort.
    pub rung: ModelRung,
    /// The chain step (or legacy rank), counted from one.
    pub step: u16,
    /// The route's position inside its step, counted from one.
    pub sub_step: u16,
    /// The model's maker, when a policy names it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    /// The key no two reviewers may share: the policy vendor, or a legacy
    /// caller's provider family. `None` is an unknown vendor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub independence: Option<String>,
}

/// One slot to allocate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllocationSlot {
    /// The slot's stable id, unique within the allocation.
    pub slot_id: String,
    /// What the slot does.
    pub role: AllocationRole,
    /// The runtime facts this slot is allocated under.
    pub eligibility: Eligibility,
    /// Every route the slot may take, in preference order.
    pub candidates: Vec<AllocationCandidate>,
}

/// Why a candidate was passed over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationExclusion {
    /// The slot's eligibility names the account unavailable now.
    AccountUnavailableNow,
    /// The slot's eligibility excludes the model's vendor.
    VendorExcluded,
    /// A reviewer under a diversity rule cannot take a route whose vendor is
    /// unknown.
    VendorUnknown,
    /// An earlier reviewer in this allocation already holds its vendor; the
    /// candidate names that slot in `conflicts_with`.
    VendorHeld,
    /// Taking it leaves a later slot with no route the rules admit.
    NoCompleteAllocation,
}

/// One candidate a slot considered, and why it was passed over if it was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConsideredCandidate {
    /// The candidate, in the slot's order.
    pub candidate: AllocationCandidate,
    /// `None` for the candidate selected or any candidate after it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excluded: Option<AllocationExclusion>,
    /// The slot holding the vendor, for [`AllocationExclusion::VendorHeld`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflicts_with: Option<String>,
}

/// Why no complete allocation exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationFailure {
    /// A slot has no candidate its eligibility and the diversity rule admit.
    NoEligibleCandidate,
    /// Every slot has candidates, but no assignment gives each reviewer its
    /// own vendor.
    NoDistinctReviewerVendors,
}

/// One slot's part of a joint allocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SlotAllocation {
    /// The slot's id.
    pub slot_id: String,
    /// What the slot does.
    pub role: AllocationRole,
    /// The eligibility the slot was allocated under.
    pub eligibility: Eligibility,
    /// The route chosen; `None` for every slot when the allocation is blocked.
    pub selected: Option<AllocationCandidate>,
    /// Where `selected` sits in the slot's candidates.
    #[serde(skip)]
    pub selected_index: Option<usize>,
    /// Every candidate, in order, with its verdict.
    pub considered: Vec<ConsideredCandidate>,
    /// Why this slot blocks the allocation, when it does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<AllocationFailure>,
}

/// A whole Committee's allocation: complete, or blocked with every reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JointAllocation {
    /// The rule the reviewers were held to.
    pub diversity: AllocationDiversity,
    /// Every slot, in input order.
    pub slots: Vec<SlotAllocation>,
    /// `Some` exactly when no complete allocation exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked: Option<AllocationFailure>,
}

impl JointAllocation {
    /// Whether every slot has a route.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.blocked.is_none()
    }

    /// The index of the chosen candidate of every slot, in slot order, when
    /// the allocation is complete.
    #[must_use]
    pub fn chosen(&self) -> Option<Vec<usize>> {
        if !self.is_complete() {
            return None;
        }
        self.slots.iter().map(|slot| slot.selected_index).collect()
    }
}

/// Allocate every slot together.
///
/// Each candidate is first judged by its slot's eligibility — the account
/// unavailable now, the vendor excluded — and, for a reviewer under
/// [`AllocationDiversity::DistinctVendorPerReviewer`], by whether its vendor is
/// known. The search then walks slots in order and each slot's remaining
/// candidates in order, holding each reviewer's vendor for the slots after it,
/// and returns the first complete assignment. A judge is never held to a
/// reviewer's vendor.
#[must_use]
pub fn allocate(diversity: AllocationDiversity, slots: &[AllocationSlot]) -> JointAllocation {
    let constrained: Vec<bool> = slots
        .iter()
        .map(|slot| {
            diversity == AllocationDiversity::DistinctVendorPerReviewer
                && slot.role == AllocationRole::Reviewer
        })
        .collect();
    let verdicts: Vec<Vec<Option<AllocationExclusion>>> = slots
        .iter()
        .zip(&constrained)
        .map(|(slot, constrained)| {
            slot.candidates
                .iter()
                .map(|candidate| static_exclusion(slot, candidate, *constrained))
                .collect()
        })
        .collect();

    let mut chosen = Vec::with_capacity(slots.len());
    let mut held = BTreeMap::new();
    if walk(slots, &verdicts, &constrained, &mut held, &mut chosen) {
        return JointAllocation {
            diversity,
            slots: receipts(slots, &verdicts, &constrained, &chosen),
            blocked: None,
        };
    }

    let unfillable: Vec<bool> = verdicts
        .iter()
        .map(|slot| slot.iter().all(Option::is_some))
        .collect();
    let blocked = if unfillable.iter().any(|unfillable| *unfillable) {
        AllocationFailure::NoEligibleCandidate
    } else {
        AllocationFailure::NoDistinctReviewerVendors
    };
    let slots = slots
        .iter()
        .zip(&verdicts)
        .enumerate()
        .map(|(index, (slot, verdicts))| SlotAllocation {
            slot_id: slot.slot_id.clone(),
            role: slot.role,
            eligibility: slot.eligibility.clone(),
            selected: None,
            selected_index: None,
            considered: slot
                .candidates
                .iter()
                .zip(verdicts)
                .map(|(candidate, verdict)| ConsideredCandidate {
                    candidate: candidate.clone(),
                    excluded: Some(verdict.unwrap_or(AllocationExclusion::NoCompleteAllocation)),
                    conflicts_with: None,
                })
                .collect(),
            failure: match blocked {
                AllocationFailure::NoEligibleCandidate => {
                    unfillable[index].then_some(AllocationFailure::NoEligibleCandidate)
                }
                AllocationFailure::NoDistinctReviewerVendors => {
                    constrained[index].then_some(AllocationFailure::NoDistinctReviewerVendors)
                }
            },
        })
        .collect();
    JointAllocation {
        diversity,
        slots,
        blocked: Some(blocked),
    }
}

/// What the slot's own eligibility and the diversity rule say about one
/// candidate before any other slot is considered.
fn static_exclusion(
    slot: &AllocationSlot,
    candidate: &AllocationCandidate,
    constrained: bool,
) -> Option<AllocationExclusion> {
    if slot
        .eligibility
        .unavailable_accounts
        .contains(&candidate.rung.provider.0)
    {
        return Some(AllocationExclusion::AccountUnavailableNow);
    }
    if candidate
        .vendor
        .as_ref()
        .is_some_and(|vendor| slot.eligibility.excluded_vendors.contains(vendor))
    {
        return Some(AllocationExclusion::VendorExcluded);
    }
    if constrained && candidate.independence.is_none() {
        return Some(AllocationExclusion::VendorUnknown);
    }
    None
}

/// Depth-first over slots in order and candidates in order; the first complete
/// assignment wins. `held` maps each reviewer vendor to the slot holding it.
fn walk(
    slots: &[AllocationSlot],
    verdicts: &[Vec<Option<AllocationExclusion>>],
    constrained: &[bool],
    held: &mut BTreeMap<String, usize>,
    chosen: &mut Vec<usize>,
) -> bool {
    let index = chosen.len();
    let Some(slot) = slots.get(index) else {
        return true;
    };
    for (position, candidate) in slot.candidates.iter().enumerate() {
        if verdicts[index][position].is_some() {
            continue;
        }
        let key = if constrained[index] {
            let Some(key) = candidate.independence.as_ref() else {
                continue;
            };
            if held.contains_key(key) {
                continue;
            }
            held.insert(key.clone(), index);
            Some(key)
        } else {
            None
        };
        chosen.push(position);
        if walk(slots, verdicts, constrained, held, chosen) {
            return true;
        }
        chosen.pop();
        if let Some(key) = key {
            held.remove(key);
        }
    }
    false
}

/// Every slot's receipt for a complete assignment.
fn receipts(
    slots: &[AllocationSlot],
    verdicts: &[Vec<Option<AllocationExclusion>>],
    constrained: &[bool],
    chosen: &[usize],
) -> Vec<SlotAllocation> {
    slots
        .iter()
        .enumerate()
        .map(|(index, slot)| {
            let selected = chosen[index];
            // The vendors earlier reviewers hold in the final assignment: the
            // only holders a candidate before the selected one can have met.
            let earlier: BTreeMap<&str, &str> = (0..index)
                .filter(|earlier| constrained[*earlier])
                .filter_map(|earlier| {
                    slots[earlier].candidates[chosen[earlier]]
                        .independence
                        .as_deref()
                        .map(|key| (key, slots[earlier].slot_id.as_str()))
                })
                .collect();
            let considered = slot
                .candidates
                .iter()
                .enumerate()
                .map(|(position, candidate)| {
                    let (excluded, conflicts_with) = match verdicts[index][position] {
                        Some(verdict) => (Some(verdict), None),
                        None if position >= selected => (None, None),
                        None => match candidate
                            .independence
                            .as_deref()
                            .filter(|_| constrained[index])
                            .and_then(|key| earlier.get(key))
                        {
                            Some(holder) => (
                                Some(AllocationExclusion::VendorHeld),
                                Some((*holder).to_owned()),
                            ),
                            None => (Some(AllocationExclusion::NoCompleteAllocation), None),
                        },
                    };
                    ConsideredCandidate {
                        candidate: candidate.clone(),
                        excluded,
                        conflicts_with,
                    }
                })
                .collect();
            SlotAllocation {
                slot_id: slot.slot_id.clone(),
                role: slot.role,
                eligibility: slot.eligibility.clone(),
                selected: Some(slot.candidates[selected].clone()),
                selected_index: Some(selected),
                considered,
                failure: None,
            }
        })
        .collect()
}
