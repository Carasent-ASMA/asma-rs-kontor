//! `kontor-mcp` — the capability-gated tool surface of one Kontor Realm, and the
//! narrow loopback client both it and the CLI are built on.
//!
//! # What this crate is allowed to touch
//!
//! One loopback HTTP endpoint, over `/v1`, with one bearer credential read from one
//! Realm's `0600` file. That is the whole outward surface. There is no SQLite
//! connection here, no repository, no scheduler, no runtime adapter and no `asma`
//! subprocess — every one of those lives behind the daemon, which is the process
//! that owns the decisions about them. The dependency list is the enforcement: the
//! only Kontor crate below this one is `kontor-core`, which is pure domain
//! vocabulary.
//!
//! # Why the CLI depends on this crate and not the other way round
//!
//! `kontor mcp` starts this server, so `kontor-cli` must be above `kontor-mcp` in
//! the graph. The narrow client, the credential and endpoint resolution and the
//! operation catalogue are needed by *both*, so they live here — at the bottom —
//! rather than being written twice. A third crate would be the tidier home, but the
//! workspace member list is owned by KON-MVP-02 (CON-007) and a ticket does not
//! edit it.
//!
//! # The one thing to understand before changing anything here
//!
//! [`execute`] is the single path from a named operation to an effect, and the order
//! inside it is the contract:
//!
//! ```text
//! 1. is this operation served at all?          → no  ⇒ refuse (fail closed)
//! 2. does the configured authority reach it?    → no  ⇒ refuse (nothing dispatched)
//! 3. do the arguments match the declared schema?→ no  ⇒ refuse (nothing dispatched)
//! 4. is this a dry run?                         → yes ⇒ describe, dispatch nothing
//! 5. only now: call the daemon
//! ```
//!
//! Steps 1–3 are why an observer-configured server cannot mutate anything: the
//! refusal happens before a request exists, so there is no dispatch to intercept.
//! A test proves it by counting what a recording transport received — which is only
//! meaningful because of this order.

pub mod capability;
pub mod client;
pub mod fake;
pub mod server;
pub mod tools;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::capability::{Denied, Gate};
use crate::client::{CallFailure, RealmClient, Refusal};
use crate::tools::{Effect, ToolSpec};

/// The envelope generation every answer declares.
pub const SCHEMA_VERSION: u32 = 1;

/// One successful answer, in the shape every caller of this control plane gets.
///
/// The field set is fixed and the daemon's own documents are nested inside it
/// unchanged. That is deliberate: a caller that wants a receipt's `state` reads
/// `receipt.value.state`, which is where `kontor-api` put it, and no field on the
/// way through is renamed, flattened or promoted. A CLI that rewrote server fields
/// would be a second contract nobody versioned.
#[derive(Debug, Clone, Serialize)]
pub struct Envelope {
    /// This envelope's generation.
    pub schema_version: u32,
    /// The Realm the answer came from. Every answer names one.
    pub realm_id: String,
    /// The operation that was performed.
    pub command: String,
    /// The daemon's own document, for a read. `null` for a mutation.
    pub data: Value,
    /// The daemon's own receipt envelope, for a mutation. `null` for a read.
    pub receipt: Value,
}

/// Everything a call can fail as.
///
/// The three variants are three different things a caller should do, which is why
/// they are not collapsed: a [`Failure::Denied`] means fix the call, a
/// [`Failure::Refused`] means the Realm considered it and said no, and a
/// [`Failure::Call`] transport failure means the Realm never answered.
#[derive(Debug, thiserror::Error)]
pub enum Failure {
    /// Refused on this machine, before anything was dispatched.
    #[error(transparent)]
    Denied(#[from] Denied),
    /// Refused by the Realm, relayed unchanged.
    #[error(transparent)]
    Refused(Refusal),
    /// The Realm could not be reached, or did not answer this contract.
    #[error(transparent)]
    Call(#[from] CallFailure),
}

impl From<Refusal> for Failure {
    fn from(refusal: Refusal) -> Self {
        Self::Refused(refusal)
    }
}

impl Failure {
    /// The stable machine code this failure reports.
    ///
    /// A Realm's refusal reports the Realm's own code, untranslated. A local
    /// refusal reports `invalid_request`, which is what the contract already calls
    /// "the request itself is malformed" — and an authority refusal reports
    /// `forbidden`, because that is what it is, whichever side noticed.
    #[must_use]
    pub fn code(&self) -> &str {
        match self {
            Self::Denied(Denied::Authority { .. }) => "forbidden",
            Self::Denied(Denied::NoSuchTool { .. }) => "not_found",
            Self::Denied(_) => "invalid_request",
            Self::Refused(refusal) => &refusal.code,
            Self::Call(CallFailure::Refused(refusal)) => &refusal.code,
            Self::Call(CallFailure::Local(_)) => "invalid_request",
            Self::Call(CallFailure::Transport(_)) => "unavailable",
        }
    }

    /// The document a caller should be shown.
    ///
    /// For a Realm refusal it is the `ApiErrorBody` the daemon sent, byte for byte.
    /// For a local refusal it is a document of the same shape, so a caller
    /// branching on `code` does not have to know which side refused.
    #[must_use]
    pub fn body(&self, realm_id: Option<&str>) -> Value {
        match self {
            Self::Refused(refusal) | Self::Call(CallFailure::Refused(refusal)) => {
                refusal.body.clone()
            }
            other => serde_json::json!({
                "realm_id": realm_id,
                "code": other.code(),
                "rule": other.to_string(),
                "current_revision": Value::Null,
                "oldest_retained_cursor": Value::Null,
                "newest_cursor": Value::Null,
            }),
        }
    }
}

/// Run one named operation end to end.
///
/// # Errors
/// Returns [`Failure::Denied`] when the operation is not served here, when the
/// configured authority does not reach it, or when its arguments do not match its
/// declared schema — in every one of those cases *nothing has been dispatched*.
/// Returns [`Failure::Refused`] when the Realm refused, and [`Failure::Call`] when
/// it could not be reached.
pub async fn execute(
    client: &RealmClient,
    gate: Gate,
    name: &str,
    arguments: &Map<String, Value>,
) -> Result<Envelope, Failure> {
    // 1. Served at all? A staged surface is absent rather than failing, so an
    //    unknown name is the honest answer for it too.
    let tool = tools::find(name).ok_or_else(|| Denied::NoSuchTool {
        tool: name.to_owned(),
        configured: gate.configured(),
    })?;
    // 2. Authority, before a single argument is read.
    let _admitted = gate.admit(tool.name, tool.tier)?;
    // 3. Arguments, against the declaration the schema was generated from.
    let plan = tool.plan(arguments)?;

    // 4. A dry run answers with the request it would have made. The realm is asked
    //    to identify itself so the answer is still realm-qualified — a read, and
    //    never the mutation being described.
    if plan.dry_run {
        let realm_id = client.establish_realm().await?;
        return Ok(Envelope {
            schema_version: SCHEMA_VERSION,
            realm_id,
            command: tool.name.to_owned(),
            data: serde_json::json!({ "dry_run": true, "request": plan.describe() }),
            receipt: Value::Null,
        });
    }

    // 5. Only now.
    let reply = match plan.budget {
        Some(budget) => client.stream(&plan.request, budget).await?,
        None => client.send(&plan.request).await?,
    };
    let realm_id = reply
        .body
        .get("realm_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| client.expected_realm())
        .unwrap_or_default();
    Ok(envelope(&tool, realm_id, reply.body))
}

/// Place one answer in the field its effect belongs in.
fn envelope(tool: &ToolSpec, realm_id: String, body: Value) -> Envelope {
    let (data, receipt) = match tool.effect {
        // A mutation's answer *is* a receipt, and it goes in the receipt field
        // whole. Splitting it would mean deciding which of the daemon's fields
        // matter, and that is not this crate's decision to make.
        Effect::Mutation => (Value::Null, body),
        Effect::Query | Effect::Stream => (body, Value::Null),
    };
    Envelope {
        schema_version: SCHEMA_VERSION,
        realm_id,
        command: tool.name.to_owned(),
        data,
        receipt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::CallerTier;

    #[test]
    fn a_local_refusal_wears_the_same_shape_as_the_daemons_own() {
        let denied = Failure::Denied(Denied::Authority {
            tool: "run_launch".to_owned(),
            required: CallerTier::Operator,
            configured: CallerTier::Observer,
        });
        assert_eq!(denied.code(), "forbidden");
        let body = denied.body(Some("0192f0c0-0000-7000-8000-000000000001"));
        for field in [
            "realm_id",
            "code",
            "rule",
            "current_revision",
            "oldest_retained_cursor",
            "newest_cursor",
        ] {
            assert!(
                body.get(field).is_some(),
                "a local refusal must carry {field}, so a caller branching on code needs no \
                 knowledge of which side refused"
            );
        }
    }

    #[test]
    fn a_realms_refusal_is_relayed_and_never_rewritten() {
        let sent = serde_json::json!({
            "realm_id": "0192f0c0-0000-7000-8000-000000000001",
            "code": "revision_conflict",
            "rule": "the aggregate moved since the caller read it",
            "current_revision": 12,
            "oldest_retained_cursor": Value::Null,
            "newest_cursor": Value::Null,
        });
        let failure = Failure::Refused(Refusal {
            status: 409,
            code: "revision_conflict".to_owned(),
            body: sent.clone(),
        });
        assert_eq!(failure.code(), "revision_conflict");
        assert_eq!(
            failure.body(None),
            sent,
            "the daemon's own body is passed through byte for byte, including the revision the \
             caller is owed"
        );
    }

    #[test]
    fn an_unknown_tool_is_not_found_rather_than_forbidden() {
        // A staged surface is absent, and "absent" must not read as "you lack the
        // authority", or an operator would go looking for a credential that would
        // not have helped.
        let denied = Failure::Denied(Denied::NoSuchTool {
            tool: "ticket_apply".to_owned(),
            configured: CallerTier::Admin,
        });
        assert_eq!(denied.code(), "not_found");
    }
}
