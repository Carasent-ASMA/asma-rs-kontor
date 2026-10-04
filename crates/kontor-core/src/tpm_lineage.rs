//! Which native authoritatively fills one hosted seat.
//!
//! A hosted seat can end up with more than one native answering to its labels:
//! a relaunch that lost its acknowledgement, a prepared occupancy that never
//! installed, an operator-started session carrying the same title. Choosing
//! between them by label is how the wrong one gets adopted, because a label is
//! something a native *wears*, not something Kontor issued to it.
//!
//! This resolves the question the other way round. It reads only the two
//! durable records Kontor itself wrote — the occupancy it bound, and the launch
//! intent it installed — and answers exactly one native, or refuses. A native
//! that appears in neither record is not a weak candidate here; it is not a
//! candidate at all, and no input to this function can make it one. That is the
//! no-label-adoption fence expressed as a shape rather than a rule.
//!
//! Pure: no store, no clock, no adapter. It cannot write, and it cannot read a
//! runtime to break a tie it was not given evidence to break.

use crate::id::ExternalId;

/// The catalog route a seat was admitted on.
///
/// Compared whole. A lineage whose route disagrees is a different admission,
/// however well its seat and generation line up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogRoute {
    /// The provider alias.
    pub provider: String,
    /// The model.
    pub model: String,
    /// The effort level, when the rung pins one.
    pub effort: Option<String>,
}

/// One durable occupancy: a native Kontor bound to this seat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedOccupancy {
    /// The occupancy generation it was bound under.
    pub generation: u64,
    /// The exact native.
    pub native_id: ExternalId,
    /// The route it was admitted on.
    pub route: CatalogRoute,
}

/// What a launch intent has actually done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentState {
    /// Recorded before its native effect, and no native observed yet.
    Prepared,
    /// Reconciled against the native it produced.
    Installed,
}

/// One durable launch intent for this seat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedLaunchIntent {
    /// The occupancy generation it was prepared for.
    pub occupancy_generation: u64,
    /// Whether it ever installed.
    pub state: IntentState,
    /// The native it reconciled against, when it installed one.
    pub observed_native_id: Option<ExternalId>,
    /// The route it was prepared on.
    pub route: CatalogRoute,
}

/// What the caller is asking about, all of it server-derived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineageQuery {
    /// The occupancy generation in question.
    pub generation: u64,
    /// The route the seat's catalog entry currently names.
    pub expected_route: CatalogRoute,
    /// The ECP workspace the seat is placed in, as the topology holds it.
    pub expected_placement: ExternalId,
    /// The ECP workspace read back from the runtime now.
    pub observed_placement: ExternalId,
}

/// Why no authoritative native could be named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineageRefusal {
    /// The seat is not where the topology says it is.
    PlacementMismatch,
    /// No durable record names a native for this seat and generation.
    ///
    /// A prepared intent that never installed lands here on purpose: it is
    /// evidence that a launch was *intended*, never that one happened.
    NoEligibleLineage,
    /// More than one durable record names a different native.
    AmbiguousLineage,
    /// A record's route disagrees with the seat's catalog route.
    RouteMismatch,
    /// The occupancy and the intent name different natives for one generation.
    ContradictoryOccupancy,
}

impl LineageRefusal {
    /// The closed refusal vocabulary.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PlacementMismatch => "placement_mismatch",
            Self::NoEligibleLineage => "no_eligible_lineage",
            Self::AmbiguousLineage => "ambiguous_lineage",
            Self::RouteMismatch => "route_mismatch",
            Self::ContradictoryOccupancy => "contradictory_occupancy",
        }
    }
}

/// The one native this seat and generation authoritatively hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoritativeNative {
    /// The exact native.
    pub native_id: ExternalId,
    /// The generation it was bound under.
    pub generation: u64,
    /// Which durable record named it.
    pub evidence: LineageEvidence,
}

/// Which durable record named the native.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineageEvidence {
    /// The bound occupancy.
    Occupancy,
    /// An installed launch intent.
    InstalledIntent,
    /// Both, in agreement.
    OccupancyAndIntent,
}

/// Resolve the authoritative native for one seat and generation.
///
/// `occupancies` and `intents` must already be the durable rows for the exact
/// logical seat. Generation filtering, route correlation and the exactly-one
/// rule are applied here.
///
/// # Errors
/// One [`LineageRefusal`]; nothing is produced and nothing is guessed.
pub fn resolve(
    query: &LineageQuery,
    occupancies: &[ObservedOccupancy],
    intents: &[ObservedLaunchIntent],
) -> Result<AuthoritativeNative, LineageRefusal> {
    if query.expected_placement != query.observed_placement {
        return Err(LineageRefusal::PlacementMismatch);
    }
    let mut from_occupancy: Option<&ObservedOccupancy> = None;
    for occupancy in occupancies
        .iter()
        .filter(|row| row.generation == query.generation)
    {
        if occupancy.route != query.expected_route {
            return Err(LineageRefusal::RouteMismatch);
        }
        match from_occupancy {
            None => from_occupancy = Some(occupancy),
            Some(first) if first.native_id == occupancy.native_id => {}
            Some(_) => return Err(LineageRefusal::AmbiguousLineage),
        }
    }
    let mut from_intent: Option<&ObservedLaunchIntent> = None;
    for intent in intents
        .iter()
        .filter(|row| row.occupancy_generation == query.generation)
    {
        if intent.route != query.expected_route {
            return Err(LineageRefusal::RouteMismatch);
        }
        // A prepared intent names no native, and must not be read as one. It
        // is evidence a launch was intended, never that one happened.
        if intent.state != IntentState::Installed || intent.observed_native_id.is_none() {
            continue;
        }
        match from_intent {
            None => from_intent = Some(intent),
            Some(first) if first.observed_native_id == intent.observed_native_id => {}
            Some(_) => return Err(LineageRefusal::AmbiguousLineage),
        }
    }
    match (from_occupancy, from_intent) {
        (None, None) => Err(LineageRefusal::NoEligibleLineage),
        (Some(occupancy), None) => Ok(AuthoritativeNative {
            native_id: occupancy.native_id.clone(),
            generation: query.generation,
            evidence: LineageEvidence::Occupancy,
        }),
        (None, Some(intent)) => Ok(AuthoritativeNative {
            native_id: intent
                .observed_native_id
                .clone()
                .expect("an installed intent kept here always names its native"),
            generation: query.generation,
            evidence: LineageEvidence::InstalledIntent,
        }),
        (Some(occupancy), Some(intent)) => {
            let installed = intent
                .observed_native_id
                .as_ref()
                .expect("an installed intent kept here always names its native");
            if &occupancy.native_id != installed {
                return Err(LineageRefusal::ContradictoryOccupancy);
            }
            Ok(AuthoritativeNative {
                native_id: occupancy.native_id.clone(),
                generation: query.generation,
                evidence: LineageEvidence::OccupancyAndIntent,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact ASMA-8098 shape: logical seat
    /// 01a070e5-7909-7c31-8843-9705948f4f9e in ECP wks_32f1f3d29ab252b3, whose
    /// generation 1 occupancy holds f8c211e4 and whose generation 2 intent was
    /// prepared on 2026-09-18 and never installed. ba0022fc wears the same
    /// labels and appears in no durable record.
    const BOUND: &str = "f8c211e4-e41e-4897-ba58-4656eb35192e";
    const SAME_LABEL_OTHER: &str = "ba0022fc-d47e-4ad6-bce4-456201543346";
    const ECP: &str = "wks_32f1f3d29ab252b3";

    fn native(id: &str) -> ExternalId {
        ExternalId::parse(id).expect("a native id")
    }

    fn route() -> CatalogRoute {
        CatalogRoute {
            provider: "codex-personal".to_owned(),
            model: "gpt-5.6-sol".to_owned(),
            effort: Some("high".to_owned()),
        }
    }

    fn query(generation: u64) -> LineageQuery {
        LineageQuery {
            generation,
            expected_route: route(),
            expected_placement: native(ECP),
            observed_placement: native(ECP),
        }
    }

    fn occupancy(generation: u64, id: &str) -> ObservedOccupancy {
        ObservedOccupancy {
            generation,
            native_id: native(id),
            route: route(),
        }
    }

    fn installed(generation: u64, id: &str) -> ObservedLaunchIntent {
        ObservedLaunchIntent {
            occupancy_generation: generation,
            state: IntentState::Installed,
            observed_native_id: Some(native(id)),
            route: route(),
        }
    }

    fn prepared(generation: u64) -> ObservedLaunchIntent {
        ObservedLaunchIntent {
            occupancy_generation: generation,
            state: IntentState::Prepared,
            observed_native_id: None,
            route: route(),
        }
    }

    #[test]
    fn the_live_seat_resolves_generation_one_to_exactly_one_native() {
        let resolved = resolve(&query(1), &[occupancy(1, BOUND)], &[prepared(2)])
            .expect("the bound occupancy names the native");
        assert_eq!(resolved.native_id.as_str(), BOUND);
        assert_eq!(resolved.generation, 1);
        assert_eq!(resolved.evidence, LineageEvidence::Occupancy);
        assert_ne!(
            resolved.native_id.as_str(),
            SAME_LABEL_OTHER,
            "the only route to the same-label native is a label lookup this \
             resolver cannot perform"
        );
    }

    #[test]
    fn a_prepared_intent_that_never_installed_names_no_native() {
        // The live generation 2. A launch was intended; none is evidenced.
        assert_eq!(
            resolve(&query(2), &[occupancy(1, BOUND)], &[prepared(2)]),
            Err(LineageRefusal::NoEligibleLineage)
        );
    }

    #[test]
    fn a_native_no_durable_record_names_is_not_a_candidate() {
        // Both durable sources are empty for this generation. The same-label
        // native exists in the realm and is still unreachable: there is no
        // input to this function that could introduce it.
        assert_eq!(
            resolve(&query(3), &[], &[]),
            Err(LineageRefusal::NoEligibleLineage)
        );
    }

    #[test]
    fn two_occupancies_naming_different_natives_are_ambiguous() {
        assert_eq!(
            resolve(
                &query(1),
                &[occupancy(1, BOUND), occupancy(1, SAME_LABEL_OTHER)],
                &[]
            ),
            Err(LineageRefusal::AmbiguousLineage)
        );
    }

    #[test]
    fn two_installed_intents_naming_different_natives_are_ambiguous() {
        assert_eq!(
            resolve(
                &query(1),
                &[],
                &[installed(1, BOUND), installed(1, SAME_LABEL_OTHER)]
            ),
            Err(LineageRefusal::AmbiguousLineage)
        );
    }

    #[test]
    fn an_occupancy_and_intent_naming_different_natives_contradict() {
        assert_eq!(
            resolve(
                &query(1),
                &[occupancy(1, BOUND)],
                &[installed(1, SAME_LABEL_OTHER)]
            ),
            Err(LineageRefusal::ContradictoryOccupancy)
        );
    }

    #[test]
    fn an_agreeing_occupancy_and_intent_are_one_lineage_evidenced_by_both() {
        let resolved = resolve(&query(1), &[occupancy(1, BOUND)], &[installed(1, BOUND)])
            .expect("both records agree");
        assert_eq!(resolved.native_id.as_str(), BOUND);
        assert_eq!(resolved.evidence, LineageEvidence::OccupancyAndIntent);
    }

    #[test]
    fn a_replayed_record_is_one_lineage_not_two() {
        // Lost-ack shape: the same durable row observed twice must not read as
        // a second native, or every replay would manufacture ambiguity.
        let resolved = resolve(
            &query(1),
            &[occupancy(1, BOUND), occupancy(1, BOUND)],
            &[installed(1, BOUND), installed(1, BOUND)],
        )
        .expect("a replay resolves exactly as the first observation did");
        assert_eq!(resolved.native_id.as_str(), BOUND);
        assert_eq!(resolved.evidence, LineageEvidence::OccupancyAndIntent);
    }

    #[test]
    fn another_generations_lineage_is_not_this_generations() {
        // Generation 1 holds a native; generation 2 must not borrow it.
        assert_eq!(
            resolve(&query(2), &[occupancy(1, BOUND)], &[]),
            Err(LineageRefusal::NoEligibleLineage)
        );
    }

    #[test]
    fn a_record_whose_route_disagrees_is_a_different_admission() {
        let mut wrong = occupancy(1, BOUND);
        wrong.route.model = "gpt-5.6-mini".to_owned();
        assert_eq!(
            resolve(&query(1), &[wrong], &[]),
            Err(LineageRefusal::RouteMismatch)
        );
    }

    #[test]
    fn a_seat_that_moved_out_of_its_ecp_refuses_before_any_lineage_is_read() {
        let mut moved = query(1);
        moved.observed_placement = native("wks_somewhere_else");
        assert_eq!(
            resolve(&moved, &[occupancy(1, BOUND)], &[]),
            Err(LineageRefusal::PlacementMismatch)
        );
    }
}
