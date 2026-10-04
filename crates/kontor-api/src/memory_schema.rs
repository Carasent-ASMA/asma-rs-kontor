//! OpenAPI adapters for domain types without adding HTTP dependencies to core/store.
use serde_json::{Value, json};
use utoipa::openapi::{OpenApi, Ref, RefOr, schema::Schema};

use kontor_core::memory::{
    DegradedReason, EvidenceConfidence, ExperienceKind, OutcomeKind, ProjectionPolicy,
    RetrievalMode,
};

fn reference(name: &str) -> Value {
    json!({"$ref":format!("#/components/schemas/{name}")})
}
fn nullable(schema: Value) -> Value {
    json!({"oneOf":[schema,{"type":"null"}]})
}
fn object(properties: Value, optional: &[&str]) -> Value {
    let required: Vec<_> = properties
        .as_object()
        .expect("schema properties")
        .keys()
        .filter(|key| !optional.contains(&key.as_str()))
        .cloned()
        .collect();
    json!({"type":"object","additionalProperties":false,"properties":properties,"required":required})
}
fn array(items: Value) -> Value {
    json!({"type":"array","items":items})
}
fn bounded_text(max: usize) -> Value {
    json!({"type":"string","minLength":1,"maxLength":max,"description":format!("Nonblank; at most {max} UTF-8 bytes; canonical sensitive-material validation applies.")})
}
fn bounded_list(min: usize) -> Value {
    json!({"type":"array","minItems":min,"maxItems":16,"items":bounded_text(2048)})
}

pub(super) struct MemorySchemas;
impl utoipa::Modify for MemorySchemas {
    fn modify(&self, document: &mut OpenApi) {
        let string = json!({"type":"string"});
        let uuid = json!({"type":"string","format":"uuid"});
        let hash = json!({"type":"string","pattern":"^[0-9a-f]{64}$"});
        let number = json!({"type":"integer","format":"int64","minimum":0});
        let boolean = json!({"type":"boolean"});
        let timestamp = json!({"type":"string","format":"date-time"});
        let schemas = [
            (
                "ExperienceKind",
                json!({"type":"string","enum":ExperienceKind::ALL}),
            ),
            (
                "EvidenceConfidence",
                json!({"type":"string","enum":EvidenceConfidence::ALL}),
            ),
            (
                "ProjectionPolicy",
                json!({"type":"string","enum":ProjectionPolicy::ALL,"default":"local_only"}),
            ),
            (
                "OutcomeKind",
                json!({"type":"string","enum":OutcomeKind::ALL}),
            ),
            (
                "RetrievalMode",
                json!({"type":"string","enum":RetrievalMode::ALL}),
            ),
            (
                "DegradedReason",
                json!({"type":"string","enum":DegradedReason::ALL}),
            ),
            (
                "ExperienceOutcome",
                object(
                    json!({"kind":reference("OutcomeKind"),"summary":bounded_text(4096)}),
                    &[],
                ),
            ),
            (
                "EvidenceRef",
                json!({"oneOf":[
                    object(json!({"type":{"type":"string","enum":["receipt"]},"receipt_id":uuid,"content_hash":hash}), &[]),
                    object(json!({"type":{"type":"string","enum":["memory_revision"]},"project_id":uuid,"item_id":bounded_text(128),"revision_id":uuid,"content_hash":hash}), &[]),
                    object(json!({"type":{"type":"string","enum":["artifact"]},"locator":bounded_text(1024),"content_hash":hash}), &[])
                ]}),
            ),
            (
                "ExperienceMemoryV1",
                object(
                    json!({
                        "schema_version":{"type":"integer","enum":[1]},
                        "document_type":{"type":"string","enum":["experience_memory"]},
                        "kind":reference("ExperienceKind"),"situation":bounded_text(4096),"intent":bounded_text(4096),
                        "actions":bounded_list(1),"outcome":reference("ExperienceOutcome"),"went_well":bounded_list(0),"went_wrong":bounded_list(0),
                        "lesson":bounded_text(4096),"future_cues":bounded_list(1),"avoid":bounded_list(0),"domains":bounded_list(1),
                        "occurred_at":timestamp,"confidence":reference("EvidenceConfidence"),"projection_policy":reference("ProjectionPolicy"),
                        "evidence_refs":{"type":"array","minItems":1,"maxItems":16,"items":reference("EvidenceRef")}
                    }),
                    &["projection_policy"],
                ),
            ),
            (
                "MemoryIdentity",
                object(
                    json!({"project_id":uuid,"item_id":string,"revision_id":uuid,"content_hash":hash}),
                    &[],
                ),
            ),
            (
                "RecallExclusions",
                object(
                    json!({"invalid":number,"duplicate":number,"budget":number,"item_limit":number}),
                    &[],
                ),
            ),
            (
                "RecallMetadata",
                object(
                    json!({
                        "schema_version":{"type":"integer","enum":[1]},"intent_hash":hash,"mode":reference("RetrievalMode"),"reason":nullable(reference("DegradedReason")),
                        "memory_cursor":number,"projection_cursor":nullable(number.clone()),"projection_digest":nullable(hash.clone()),
                        "identities":{"type":"array","maxItems":8,"items":reference("MemoryIdentity")},"exclusions":reference("RecallExclusions"),
                        "block_hash":hash,"block_bytes":{"type":"integer","minimum":2,"maximum":32768}
                    }),
                    &[],
                ),
            ),
            (
                "FrozenRevision",
                object(json!({"revision_id":uuid,"content_hash":hash}), &[]),
            ),
            (
                "ContextMemoryBinding",
                object(
                    json!({
                        "project_id":uuid,"run_id":string,"selection_cursor":number,
                        "selection_spec":{"type":"object","additionalProperties":true},"ordered_revisions":array(reference("FrozenRevision")),"result_hash":hash,"bound_at":timestamp
                    }),
                    &[],
                ),
            ),
            (
                "RecalledMemory",
                object(
                    json!({"binding":reference("ContextMemoryBinding"),"metadata":reference("RecallMetadata"),"canonical_block":string,"replayed":boolean}),
                    &[],
                ),
            ),
            (
                "ProjectionEntry",
                object(
                    json!({"identity":reference("MemoryIdentity"),"cues":bounded_list(1),"lesson":bounded_text(4096)}),
                    &[],
                ),
            ),
            (
                "ProjectionSnapshot",
                object(
                    json!({"project_id":uuid,"memory_cursor":number,"dataset":string,"digest":hash,"identities":array(reference("MemoryIdentity"))}),
                    &[],
                ),
            ),
            (
                "ProjectionPreview",
                object(
                    json!({"snapshot":reference("ProjectionSnapshot"),"entries":array(reference("ProjectionEntry")),"active_generation":number}),
                    &[],
                ),
            ),
            (
                "ProjectionReadback",
                object(
                    json!({"active":nullable(reference("ProjectionSnapshot")),"generation":number,"stale":boolean,"adapter_available":boolean}),
                    &[],
                ),
            ),
            (
                "ExperienceClassification",
                object(
                    json!({"identity":reference("MemoryIdentity"),"recall_eligible":boolean,"projection_policy":nullable(reference("ProjectionPolicy"))}),
                    &[],
                ),
            ),
            (
                "MemoryProvenance",
                object(
                    json!({"source":string,"source_id":nullable(string.clone()),"legacy_last_write_wins":boolean,"history_unavailable":boolean}),
                    &["source_id"],
                ),
            ),
            (
                "ExperienceRevision",
                object(
                    json!({
                        "project_id":uuid,"item_id":string,"revision_id":uuid,"revision":number,"document":reference("ExperienceMemoryV1"),"provenance":reference("MemoryProvenance"),
                        "proposed_by":string,"proposed_at":timestamp,"supersedes_id":nullable(uuid.clone()),"approved":boolean,"current":boolean,"tombstoned":boolean
                    }),
                    &[],
                ),
            ),
            (
                "MemoryReceipt",
                object(
                    json!({
                        "receipt_id":uuid,"project_id":uuid,"operation":string,"item_id":nullable(string.clone()),"revision_id":nullable(uuid.clone()),
                        "aggregate_revision":nullable(number.clone()),"result_hash":hash,"recorded_at":timestamp
                    }),
                    &[],
                ),
            ),
        ];
        let components = document.components.get_or_insert_with(Default::default);
        for (name, value) in schemas {
            let schema: Schema = serde_json::from_value(value).expect("memory OpenAPI schema");
            components.schemas.insert(name.to_owned(), RefOr::T(schema));
        }
    }
}

macro_rules! schema_ref {
    ($function:ident, $name:literal) => {
        pub(super) fn $function() -> RefOr<Schema> {
            Ref::from_schema_name($name).into()
        }
    };
}
schema_ref!(experience, "ExperienceMemoryV1");
schema_ref!(recall, "RecalledMemory");
schema_ref!(projection, "ProjectionReadback");
schema_ref!(preview, "ProjectionPreview");
schema_ref!(revision, "ExperienceRevision");
schema_ref!(receipt, "MemoryReceipt");
schema_ref!(provenance, "MemoryProvenance");
pub(super) fn classifications() -> RefOr<Schema> {
    serde_json::from_value(array(reference("ExperienceClassification")))
        .expect("classification schema")
}
