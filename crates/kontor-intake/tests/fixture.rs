//! Builders shared by the intake suites.
//!
//! Every source kind here is a *string a deployment chose*, and that is the
//! point: `manual`, `pull_request`, `ci`, `monitoring` and `bug_report` are
//! spelled out in the tests so a reader can see that no code path anywhere
//! distinguishes them.

#![allow(dead_code)]

use std::collections::BTreeMap;

use kontor_core::id::{
    AccountProfileId, EventSchemaKey, ExternalId, ExternalName, SourceConnectionKey, SourceEventId,
    SourceKindKey, SpecVersion, TeamTemplateId, Timestamp, WorkProfileKey, parse_utc_timestamp,
};
use kontor_core::spec::{
    AutoArmPolicy, BudgetBounds, CanonicalSourceEvent, ContextTemplateRef, DedupExpression,
    JsonPointer, TeamTemplateRef, TriggerFilterClause, TriggerLimits, TriggerSpec,
};
use kontor_intake::{InboundEvent, canonicalize};

pub(crate) const OBSERVED_AT: &str = "2026-08-12T09:00:00Z";
pub(crate) const INGESTED_AT: &str = "2026-08-12T09:00:01Z";

pub(crate) fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical UTC fixture timestamp")
}

pub(crate) fn money() -> kontor_core::id::Money {
    kontor_core::id::Money {
        minor_units: 1_500,
        currency: kontor_core::id::CurrencyCode::parse("NOK").expect("a legal currency"),
    }
}

pub(crate) fn budget() -> BudgetBounds {
    BudgetBounds {
        max_tokens: 100_000,
        max_commands: 40,
        max_duration_seconds: 1_800,
        max_cost: money(),
    }
}

pub(crate) fn pointer(text: &str) -> JsonPointer {
    JsonPointer::parse(text).expect("a legal JSON pointer")
}

/// One inbound event, as any adapter hands it over.
pub(crate) fn inbound(
    source_kind: &str,
    connection: &str,
    external_event_id: &str,
    attributes: &[(&str, serde_json::Value)],
) -> InboundEvent {
    InboundEvent {
        source_kind: SourceKindKey::parse(source_kind).expect("a legal source kind"),
        source_connection: SourceConnectionKey::parse(connection).expect("a legal connection"),
        authenticated_as: AccountProfileId::generate(),
        external_event_id: ExternalId::parse(external_event_id).expect("a legal external id"),
        event_schema: EventSchemaKey::parse("schema.work-requested").expect("a legal schema key"),
        event_schema_version: SpecVersion::parse(2).expect("a legal revision"),
        observed_at: at(OBSERVED_AT),
        subject: ExternalName::parse("A unit of work someone asked for").expect("a legal name"),
        attributes: attributes
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect::<BTreeMap<_, _>>(),
    }
}

/// The same inbound event, canonicalized with a fixed id and instant.
pub(crate) fn event(
    source_kind: &str,
    connection: &str,
    external_event_id: &str,
    attributes: &[(&str, serde_json::Value)],
) -> CanonicalSourceEvent {
    canonicalize(
        &inbound(source_kind, connection, external_event_id, attributes),
        SourceEventId::generate(),
        at(INGESTED_AT),
    )
    .expect("the fixture envelope canonicalizes")
}

/// A trigger listening to one connection, filtering on one attribute.
pub(crate) fn trigger(
    id: &str,
    source_kind: &str,
    connection: &str,
    filter: &[(&str, &str)],
) -> TriggerSpec {
    TriggerSpec {
        schema_version: kontor_core::id::SCHEMA_VERSION,
        id: kontor_core::id::TriggerKey::parse(id).expect("a legal trigger key"),
        version: SpecVersion::FIRST,
        source_kind: SourceKindKey::parse(source_kind).expect("a legal source kind"),
        source_connection: SourceConnectionKey::parse(connection).expect("a legal connection"),
        event_schema: EventSchemaKey::parse("schema.work-requested").expect("a legal schema key"),
        event_schema_version: SpecVersion::parse(2).expect("a legal revision"),
        filter: filter
            .iter()
            .map(|(path, equals)| TriggerFilterClause {
                pointer: pointer(path),
                equals: ExternalName::parse(equals).expect("a legal literal"),
            })
            .collect(),
        dedup: DedupExpression {
            pointers: vec![pointer("/attributes/kind"), pointer("/external_event_id")],
        },
        work_profile: WorkProfileKey::parse("q7.delivery").expect("a legal profile key"),
        work_profile_version: SpecVersion::parse(3).expect("a legal revision"),
        team_template: TeamTemplateRef {
            template_id: TeamTemplateId::parse("0193f000-0000-7000-8000-00000000b001")
                .expect("a legal template id"),
            version: SpecVersion::FIRST,
        },
        context_template: ContextTemplateRef {
            template: kontor_core::id::ArtifactKey::parse("context.default")
                .expect("a legal artifact key"),
            version: SpecVersion::FIRST,
        },
        approval: AutoArmPolicy::ApprovalRequired,
        limits: TriggerLimits {
            priority: 50,
            max_concurrency: 2,
            budget: budget(),
        },
        calendar_policy: None,
    }
}
