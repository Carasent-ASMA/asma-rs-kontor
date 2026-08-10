//! The one Context Pack resolution pipeline.
//!
//! [`preview`] and [`start_run`] run the *same* stages in the same order:
//!
//! 1. validate every source's envelope, realm, identity and declarations — the
//!    realm of each declared restricted reference included — and reject a
//!    duplicate `(layer, source key)` rather than trusting collection order;
//! 2. admit every restricted reference through the caller's grants — missing,
//!    denied and foreign-realm grants all reject, the grant is checked against
//!    the *pack's* realm, and none of them echo a value;
//! 3. sort by `(layer rank, source key)` and merge objects recursively, recording
//!    the winning source of every leaf;
//! 4. apply declared redactions, removing the whole subtree and its provenance
//!    and recording only path, source and reason;
//! 5. scan the resolved pack with the core sensitive-material rule;
//! 6. canonicalize the pack, its provenance and its redaction report into one
//!    [`CanonicalDocument`] whose digest is the only pack hash.
//!
//! The merge contract is deliberately small: two objects merge member by member,
//! and *anything else* — a scalar, an array, a type change, an explicit `null` —
//! replaces the earlier value as a whole. Arrays never concatenate, `null` never
//! deletes, and there are no merge directives.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Map, Value};

use kontor_core::id::{
    CanonicalDocument, ContextPackId, RealmId, SCHEMA_VERSION, SchemaVersion, SpecVersion,
    reject_sensitive_material,
};
use kontor_core::realm::ensure_realm;
use kontor_core::spec::JsonPointer;
use kontor_core::{DomainError, DomainResult};

use crate::model::{
    ContextLayer, ContextPackSnapshot, ContextSource, ProvenanceEntry, RedactionRecord,
    ReferenceInputs, ResolvedContextPack, ResolvedReference, RunBinding,
};

/// Everything one resolution reads.
///
/// The request borrows its inputs and the result owns its output: a resolution
/// consumes the sources, it never keeps them.
#[derive(Debug, Clone, Copy)]
pub struct ResolutionRequest<'a> {
    /// The realm every source must belong to.
    pub realm_id: RealmId,
    /// The declared sources, in any collection order.
    pub sources: &'a [ContextSource],
    /// The caller's grants for restricted references.
    pub references: &'a ReferenceInputs,
}

/// Resolve a Context Pack without binding it to a run.
///
/// # Errors
/// Returns [`DomainError`] for a foreign realm or envelope contract, a duplicate
/// or malformed source key, an unresolved/denied/foreign restricted reference, a
/// stale redaction declaration, sensitive material surviving redaction, or a
/// document that exceeds the core's canonical bounds.
pub fn preview(request: &ResolutionRequest<'_>) -> DomainResult<ResolvedContextPack> {
    let admitted = admit_sources(request)?;

    let mut merged = Map::new();
    let mut provenance: BTreeMap<String, Origin> = BTreeMap::new();
    for (source, content) in &admitted {
        let origin = Origin {
            layer: source.layer,
            source_id: source.source_id.clone(),
            revision: source.revision,
        };
        let members = content.as_object().ok_or_else(|| {
            DomainError::invalid("ContextSource", "content must be a JSON object")
        })?;
        merge_object(
            &mut merged,
            members,
            &mut String::new(),
            &origin,
            &mut provenance,
        );
    }

    let mut resolved = Value::Object(merged);
    let redactions = apply_redactions(&admitted, &mut resolved, &mut provenance)?;

    // Explicit backstop before canonicalization, so the reported path is inside
    // the pack rather than inside the snapshot envelope.
    reject_sensitive_material(&resolved)?;

    let provenance = finish_provenance(provenance)?;
    let document = CanonicalDocument::from_serializable(&PackDocument {
        schema_version: SCHEMA_VERSION,
        realm_id: request.realm_id,
        pack: &resolved,
        provenance: &provenance,
        redactions: &redactions,
    })?;

    Ok(ResolvedContextPack::new(
        request.realm_id,
        resolved,
        document,
        provenance,
        redactions,
    ))
}

/// Resolve a Context Pack and freeze it against a run.
///
/// The snapshot owns its canonical bytes, provenance and redaction report; it
/// holds no loader and no source handle, so mutating a source afterwards can
/// only change a *future* resolution.
///
/// # Errors
/// As [`preview`].
pub fn start_run(
    request: &ResolutionRequest<'_>,
    context_pack_id: ContextPackId,
    run: RunBinding,
) -> DomainResult<ContextPackSnapshot> {
    let pack = preview(request)?;
    Ok(ContextPackSnapshot::new(pack, context_pack_id, run))
}

/// The winning source of one leaf, while the merge is still running.
#[derive(Debug, Clone)]
struct Origin {
    layer: ContextLayer,
    source_id: String,
    revision: SpecVersion,
}

/// The exact shape that is canonicalized and hashed.
#[derive(Debug, Serialize)]
struct PackDocument<'a> {
    schema_version: SchemaVersion,
    realm_id: RealmId,
    pack: &'a Value,
    provenance: &'a [ProvenanceEntry],
    redactions: &'a [RedactionRecord],
}

/// Validate, dedupe, reference-resolve and order the sources.
fn admit_sources<'a>(
    request: &ResolutionRequest<'a>,
) -> DomainResult<Vec<(&'a ContextSource, Value)>> {
    // Uniqueness is per `(layer, source key)` — which is exactly the ordering
    // key, so no two admitted sources can tie. One key legitimately contributes
    // at several ranks; two entries at the *same* rank would need collection
    // order to break the tie, so they reject.
    let mut seen: BTreeSet<(u8, &str)> = BTreeSet::new();
    let mut admitted: Vec<(&ContextSource, Value)> = Vec::with_capacity(request.sources.len());
    for source in request.sources {
        source.validate(request.realm_id)?;
        if !seen.insert(source.order_key()) {
            return Err(DomainError::invalid(
                "ContextSource",
                "declares a source key that another source in the same layer already declares",
            ));
        }
        admitted.push((
            source,
            admit_references(source, request.references, request.realm_id)?,
        ));
    }
    // Precedence rank first, stable source key second: the caller's collection
    // order is never consulted.
    admitted.sort_by(|(left, _), (right, _)| left.order_key().cmp(&right.order_key()));
    Ok(admitted)
}

/// Write every declared restricted reference into a copy of the source content.
///
/// `realm_id` is the realm the pack is being resolved for, and it is what the
/// grant is checked against — not the reference's own declaration. Checking the
/// grant against the declaration alone would let a source declare a foreign
/// realm, present a matching foreign grant, and carry the value into a local
/// pack.
fn admit_references(
    source: &ContextSource,
    inputs: &ReferenceInputs,
    realm_id: RealmId,
) -> DomainResult<Value> {
    let mut content = source.content.clone();
    for reference in &source.restricted_references {
        let path = reference.path.as_str();
        let grant = inputs.get(&reference.reference_key).ok_or_else(|| {
            DomainError::invalid_at(
                "RestrictedReference",
                path,
                "is not resolved by the supplied grants",
            )
        })?;
        let ResolvedReference::Allowed {
            realm_id: granted_in,
            value,
        } = grant
        else {
            return Err(DomainError::invalid_at(
                "RestrictedReference",
                path,
                "was denied by the supplied grants",
            ));
        };
        ensure_realm(realm_id, *granted_in)?;
        let slot = content.pointer_mut(path).ok_or_else(|| {
            DomainError::invalid_at(
                "RestrictedReference",
                path,
                "does not resolve in this source",
            )
        })?;
        *slot = value.clone();
    }
    Ok(content)
}

/// Merge `incoming` into `target`, recording the winning source of every leaf.
fn merge_object(
    target: &mut Map<String, Value>,
    incoming: &Map<String, Value>,
    path: &mut String,
    origin: &Origin,
    provenance: &mut BTreeMap<String, Origin>,
) {
    for (key, value) in incoming {
        let restore = path.len();
        push_token(path, key);
        // Two objects merge member by member; everything else replaces.
        if matches!(target.get(key), Some(Value::Object(_))) && value.is_object() {
            if let (Some(Value::Object(existing)), Value::Object(members)) =
                (target.get_mut(key), value)
            {
                merge_object(existing, members, path, origin, provenance);
            }
        } else {
            target.insert(key.clone(), value.clone());
            clear_provenance_under(provenance, path);
            record_provenance(value, path, origin, provenance);
        }
        path.truncate(restore);
    }
}

/// Record `origin` for every leaf of `value`.
///
/// A leaf is any non-object value plus an empty object: arrays are one value
/// because their order is significant and never merged.
fn record_provenance(
    value: &Value,
    path: &mut String,
    origin: &Origin,
    provenance: &mut BTreeMap<String, Origin>,
) {
    match value {
        Value::Object(members) if !members.is_empty() => {
            for (key, member) in members {
                let restore = path.len();
                push_token(path, key);
                record_provenance(member, path, origin, provenance);
                path.truncate(restore);
            }
        }
        _ => {
            provenance.insert(path.clone(), origin.clone());
        }
    }
}

/// Drop every provenance entry at or under `path`.
///
/// The byte after the prefix must be `/`, so removing `/a` cannot also remove a
/// sibling `/ab`.
fn clear_provenance_under(provenance: &mut BTreeMap<String, Origin>, path: &str) {
    provenance.retain(|recorded, _| {
        recorded != path
            && !(recorded.starts_with(path) && recorded.as_bytes().get(path.len()) == Some(&b'/'))
    });
}

/// Apply every declared redaction to the merged pack and its provenance.
fn apply_redactions(
    admitted: &[(&ContextSource, Value)],
    resolved: &mut Value,
    provenance: &mut BTreeMap<String, Origin>,
) -> DomainResult<Vec<RedactionRecord>> {
    let mut declarations: Vec<RedactionRecord> = admitted
        .iter()
        .flat_map(|(source, _)| {
            source.redactions.iter().map(move |rule| RedactionRecord {
                path: rule.path.clone(),
                source_id: source.source_id.clone(),
                reason: rule.reason,
            })
        })
        .collect();
    declarations.sort_by(|left, right| {
        (left.path.as_str(), left.source_id.as_str())
            .cmp(&(right.path.as_str(), right.source_id.as_str()))
    });
    if declarations
        .windows(2)
        .any(|pair| pair[0].path == pair[1].path && pair[0].source_id == pair[1].source_id)
    {
        return Err(DomainError::invalid(
            "RedactionRule",
            "the same source declares the same path twice",
        ));
    }

    let mut applied: BTreeSet<&str> = BTreeSet::new();
    for declaration in &declarations {
        let path = declaration.path.as_str();
        if !applied.insert(path) {
            // A second source declaring the same path is recorded, not re-applied.
            continue;
        }
        remove_member(resolved, path).ok_or_else(|| {
            DomainError::invalid_at(
                "RedactionRule",
                path,
                "does not resolve to an object member of the resolved pack",
            )
        })?;
        clear_provenance_under(provenance, path);
    }
    Ok(declarations)
}

/// Remove the object member `pointer` addresses, returning what was there.
///
/// Only object members are removable: dropping an array element would silently
/// renumber every later index, so an array-element rule is refused instead.
fn remove_member(root: &mut Value, pointer: &str) -> Option<Value> {
    let (parent, last) = pointer.rsplit_once('/')?;
    let key = last.replace("~1", "/").replace("~0", "~");
    root.pointer_mut(parent)?.as_object_mut()?.remove(&key)
}

/// Append one RFC 6901 reference token to a pointer.
fn push_token(path: &mut String, key: &str) {
    path.push('/');
    for character in key.chars() {
        match character {
            '~' => path.push_str("~0"),
            '/' => path.push_str("~1"),
            _ => path.push(character),
        }
    }
}

/// Turn the merge's provenance map into the ordered, typed report.
fn finish_provenance(provenance: BTreeMap<String, Origin>) -> DomainResult<Vec<ProvenanceEntry>> {
    provenance
        .into_iter()
        .map(|(path, origin)| {
            Ok(ProvenanceEntry {
                path: JsonPointer::parse(&path)?,
                layer: origin.layer,
                source_id: origin.source_id,
                revision: origin.revision,
            })
        })
        .collect()
}
