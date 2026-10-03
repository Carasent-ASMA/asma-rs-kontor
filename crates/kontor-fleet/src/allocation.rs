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
//!
//! A selected protocol may hold some slots to more than reviewer diversity
//! ([`AllocationConstraints`]): a cap on the slots one vendor may take,
//! counted across reviewers and judge together, and the deepest rung a verdict
//! slot may take. The formal Independent Review is the one protocol that does
//! ([`committee_constraints`]); every other allocation holds nothing and is
//! exactly what it was.

use std::collections::{BTreeMap, BTreeSet};

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

/// A cap on how many governed slots, reviewers and judge together, may take
/// routes one vendor makes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VendorCap {
    /// The maker the cap counts, as the policy names it.
    pub vendor: String,
    /// The most governed slots that may take a route it makes.
    pub max: usize,
}

/// What a selected protocol holds some slots to, beyond reviewer diversity.
///
/// The default holds nothing: [`allocate`] is [`allocate_constrained`] under
/// it, so a generic allocation is exactly what it was.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AllocationConstraints {
    /// The slots the protocol governs, by `slot_id`; no other slot is held.
    pub slots: BTreeSet<String>,
    /// At most `max` governed slots may take routes `vendor` makes. A governed
    /// candidate whose maker is not named cannot fill its slot, so an unknown
    /// vendor never evades the cap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vendor_cap: Option<VendorCap>,
    /// The deepest rung (chain step) a governed slot may take. A governed
    /// candidate that states no rung cannot be shown to be within it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_rung: Option<u16>,
}

impl AllocationConstraints {
    /// Whether these constraints hold no slot to anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty() || (self.vendor_cap.is_none() && self.max_rung.is_none())
    }
}

/// One route a slot may take, flattened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllocationCandidate {
    /// The exact account alias, model and effort.
    pub rung: ModelRung,
    /// The chain step (or legacy rank), counted from one. Zero states no
    /// rung: a slot held to a rung limit cannot take it.
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
    /// A reviewer under a diversity rule, or any slot under a vendor cap,
    /// cannot take a route whose vendor is unknown.
    VendorUnknown,
    /// An earlier reviewer in this allocation already holds its vendor; the
    /// candidate names that slot in `conflicts_with`.
    VendorHeld,
    /// Taking it leaves a later slot with no route the rules admit.
    NoCompleteAllocation,
    /// The selected protocol seats this slot only on its first rungs — below
    /// rung 2 a seat may work but may not pass — and this route is deeper.
    RungBeyondVerdict,
    /// The capped vendor already holds every governed slot the selected
    /// protocol allows it; `conflicts_with` names one slot holding it.
    VendorCapReached,
    /// The selected protocol holds this slot to a rung limit and no
    /// authoritative source states this route's rung (step `0`).
    RungUnknown,
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
    /// Every slot has candidates and the reviewers could be diverse, but no
    /// assignment keeps the capped vendor within the selected protocol's cap.
    VendorCapExceeded,
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
    /// The selected protocol's constraints, when they held any slot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constraints: Option<AllocationConstraints>,
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
    allocate_constrained(diversity, slots, &AllocationConstraints::default())
}

/// Allocate every slot together under a selected protocol's constraints.
///
/// As [`allocate`], and in addition a governed slot cannot take a route deeper
/// than [`AllocationConstraints::max_rung`] or whose rung is unstated, nor,
/// under a vendor cap, a route whose maker is unknown. The search is the same exhaustive walk in the same
/// order: it counts every governed slot — reviewers and judge together —
/// holding the capped vendor, and backtracks rather than refuse when an
/// earlier slot's choice would leave a later one only the capped vendor. The
/// cap is independent of reviewer diversity: a judge may share a reviewer's
/// vendor. Under empty constraints this is exactly [`allocate`].
#[must_use]
pub fn allocate_constrained(
    diversity: AllocationDiversity,
    slots: &[AllocationSlot],
    constraints: &AllocationConstraints,
) -> JointAllocation {
    let constrained: Vec<bool> = slots
        .iter()
        .map(|slot| {
            diversity == AllocationDiversity::DistinctVendorPerReviewer
                && slot.role == AllocationRole::Reviewer
        })
        .collect();
    let governed: Vec<bool> = slots
        .iter()
        .map(|slot| !constraints.is_empty() && constraints.slots.contains(&slot.slot_id))
        .collect();
    let verdicts: Vec<Vec<Option<AllocationExclusion>>> = slots
        .iter()
        .zip(constrained.iter().zip(&governed))
        .map(|(slot, (constrained, governed))| {
            slot.candidates
                .iter()
                .map(|candidate| {
                    static_exclusion(slot, candidate, *constrained, *governed, constraints)
                })
                .collect()
        })
        .collect();
    let recorded = (!constraints.is_empty()).then(|| constraints.clone());
    let cap = constraints
        .vendor_cap
        .as_ref()
        .filter(|_| !constraints.is_empty());

    let mut search = Search::new(slots, &verdicts, &constrained, &governed, cap);
    if search.walk() {
        return JointAllocation {
            diversity,
            slots: search.receipts(),
            blocked: None,
            constraints: recorded,
        };
    }

    let unfillable: Vec<bool> = verdicts
        .iter()
        .map(|slot| slot.iter().all(Option::is_some))
        .collect();
    let blocked = if unfillable.iter().any(|unfillable| *unfillable) {
        AllocationFailure::NoEligibleCandidate
    } else if cap.is_some() && Search::new(slots, &verdicts, &constrained, &governed, None).walk() {
        AllocationFailure::VendorCapExceeded
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
                AllocationFailure::VendorCapExceeded => {
                    governed[index].then_some(AllocationFailure::VendorCapExceeded)
                }
            },
        })
        .collect();
    JointAllocation {
        diversity,
        slots,
        blocked: Some(blocked),
        constraints: recorded,
    }
}

/// The maker a candidate names, if any: `unknown` names none.
fn maker(candidate: &AllocationCandidate) -> Option<&str> {
    candidate
        .vendor
        .as_deref()
        .filter(|vendor| *vendor != "unknown")
}

/// What the slot's own eligibility, the diversity rule and the selected
/// protocol say about one candidate before any other slot is considered.
fn static_exclusion(
    slot: &AllocationSlot,
    candidate: &AllocationCandidate,
    constrained: bool,
    governed: bool,
    constraints: &AllocationConstraints,
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
    if governed {
        if let Some(deepest) = constraints.max_rung {
            if candidate.step == 0 {
                return Some(AllocationExclusion::RungUnknown);
            }
            if candidate.step > deepest {
                return Some(AllocationExclusion::RungBeyondVerdict);
            }
        }
        if constraints.vendor_cap.is_some() && maker(candidate).is_none() {
            return Some(AllocationExclusion::VendorUnknown);
        }
    }
    None
}

/// One exhaustive depth-first search over slots in order and candidates in
/// order; the first complete assignment wins.
struct Search<'a> {
    slots: &'a [AllocationSlot],
    verdicts: &'a [Vec<Option<AllocationExclusion>>],
    constrained: &'a [bool],
    governed: &'a [bool],
    cap: Option<&'a VendorCap>,
    /// Each reviewer vendor held, and the slot holding it.
    held: BTreeMap<String, usize>,
    /// How many governed slots hold the capped vendor.
    capped: usize,
    chosen: Vec<usize>,
}

impl<'a> Search<'a> {
    fn new(
        slots: &'a [AllocationSlot],
        verdicts: &'a [Vec<Option<AllocationExclusion>>],
        constrained: &'a [bool],
        governed: &'a [bool],
        cap: Option<&'a VendorCap>,
    ) -> Self {
        Self {
            slots,
            verdicts,
            constrained,
            governed,
            cap,
            held: BTreeMap::new(),
            capped: 0,
            chosen: Vec::with_capacity(slots.len()),
        }
    }

    /// Whether taking `candidate` for slot `index` counts toward the cap.
    fn counts(&self, index: usize, candidate: &AllocationCandidate) -> bool {
        self.governed[index]
            && self
                .cap
                .is_some_and(|cap| maker(candidate) == Some(cap.vendor.as_str()))
    }

    fn walk(&mut self) -> bool {
        let slots = self.slots;
        let index = self.chosen.len();
        let Some(slot) = slots.get(index) else {
            return true;
        };
        for (position, candidate) in slot.candidates.iter().enumerate() {
            if self.verdicts[index][position].is_some() {
                continue;
            }
            let key = if self.constrained[index] {
                let Some(key) = candidate.independence.as_ref() else {
                    continue;
                };
                if self.held.contains_key(key) {
                    continue;
                }
                Some(key)
            } else {
                None
            };
            let counts = self.counts(index, candidate);
            if counts && self.cap.is_some_and(|cap| self.capped >= cap.max) {
                continue;
            }
            if let Some(key) = key {
                self.held.insert(key.clone(), index);
            }
            if counts {
                self.capped += 1;
            }
            self.chosen.push(position);
            if self.walk() {
                return true;
            }
            self.chosen.pop();
            if counts {
                self.capped -= 1;
            }
            if let Some(key) = key {
                self.held.remove(key);
            }
        }
        false
    }

    /// Every slot's receipt for the complete assignment the walk found.
    fn receipts(&self) -> Vec<SlotAllocation> {
        let (slots, chosen) = (self.slots, &self.chosen);
        slots
            .iter()
            .enumerate()
            .map(|(index, slot)| {
                let selected = chosen[index];
                // The vendors earlier reviewers hold in the final assignment: the
                // only holders a candidate before the selected one can have met.
                let earlier: BTreeMap<&str, &str> = (0..index)
                    .filter(|earlier| self.constrained[*earlier])
                    .filter_map(|earlier| {
                        slots[earlier].candidates[chosen[earlier]]
                            .independence
                            .as_deref()
                            .map(|key| (key, slots[earlier].slot_id.as_str()))
                    })
                    .collect();
                // The other governed slots holding the capped vendor in the
                // final assignment, in slot order.
                let holders: Vec<&str> = (0..slots.len())
                    .filter(|other| *other != index)
                    .filter(|other| self.counts(*other, &slots[*other].candidates[chosen[*other]]))
                    .map(|other| slots[other].slot_id.as_str())
                    .collect();
                let considered = slot
                    .candidates
                    .iter()
                    .enumerate()
                    .map(|(position, candidate)| {
                        let (excluded, conflicts_with) = match self.verdicts[index][position] {
                            Some(verdict) => (Some(verdict), None),
                            None if position >= selected => (None, None),
                            None => match candidate
                                .independence
                                .as_deref()
                                .filter(|_| self.constrained[index])
                                .and_then(|key| earlier.get(key))
                            {
                                Some(holder) => (
                                    Some(AllocationExclusion::VendorHeld),
                                    Some((*holder).to_owned()),
                                ),
                                None if self.counts(index, candidate)
                                    && self.cap.is_some_and(|cap| holders.len() >= cap.max) =>
                                {
                                    (
                                        Some(AllocationExclusion::VendorCapReached),
                                        holders.first().map(|holder| (*holder).to_owned()),
                                    )
                                }
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
}

/// The bundled `Independent review` Committee template: the formal
/// Independent Review a completion verdict convenes.
pub const INDEPENDENT_REVIEW_TEMPLATE_ID: &str = "01991c00-0000-7000-8000-000000000001";

/// The vendor the formal Independent Review seats at most
/// [`INDEPENDENT_REVIEW_CAPPED_SEATS`] times, reviewers and judge together.
pub const INDEPENDENT_REVIEW_CAPPED_VENDOR: &str = "anthropic";

/// At most one Claude seat per formal Independent Review.
pub const INDEPENDENT_REVIEW_CAPPED_SEATS: usize = 1;

/// The deepest rung a verdict slot may take: below rung 2 a seat may work but
/// may not pass or merge.
pub const VERDICT_RUNG_LIMIT: u16 = 2;

/// The constraints a Committee template's protocol holds its slots to.
///
/// The formal Independent Review ([`INDEPENDENT_REVIEW_TEMPLATE_ID`]) holds
/// the given slots to at most [`INDEPENDENT_REVIEW_CAPPED_SEATS`]
/// [`INDEPENDENT_REVIEW_CAPPED_VENDOR`] seat and to rungs up to
/// [`VERDICT_RUNG_LIMIT`]. Every other template is generic and holds nothing.
/// The constraints come from the selected protocol's identity alone: no caller
/// field or default can relax them.
#[must_use]
pub fn committee_constraints<I, S>(template_id: &str, slot_ids: I) -> AllocationConstraints
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    if template_id != INDEPENDENT_REVIEW_TEMPLATE_ID {
        return AllocationConstraints::default();
    }
    AllocationConstraints {
        slots: slot_ids.into_iter().map(Into::into).collect(),
        vendor_cap: Some(VendorCap {
            vendor: INDEPENDENT_REVIEW_CAPPED_VENDOR.to_owned(),
            max: INDEPENDENT_REVIEW_CAPPED_SEATS,
        }),
        max_rung: Some(VERDICT_RUNG_LIMIT),
    }
}

/// The Committee template a `committee/<template>/<slot>` binding key names.
#[must_use]
pub fn committee_template_of(binding_key: &str) -> Option<&str> {
    let (template, slot) = binding_key.strip_prefix("committee/")?.split_once('/')?;
    (!template.is_empty() && !slot.is_empty() && !slot.contains('/')).then_some(template)
}
