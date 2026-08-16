//! The one path every tool call takes: resolve, authorize, validate, call once.
//!
//! # The four steps, and why there is only one copy of them
//!
//! 1. **Resolve** the name against [`REGISTRY`]. A name that is not in the closed
//!    vocabulary never becomes a request.
//! 2. **Authorize** against the server's single configured tier, before a
//!    [`Request`] exists.
//! 3. **Validate** the arguments against the tool's declared schema: no unknown
//!    property, no missing required one, no value the domain rejects.
//! 4. **Call once**, and return the daemon's status and body unchanged.
//!
//! Writing this once rather than per tool is what makes the cardinality claim
//! checkable: there is exactly one `transport.call` and one `transport.frames` in
//! this crate, both below, and neither is in a loop or a retry. A tool that
//! dispatched twice, retried a timeout, polled after a start or reconnected a
//! stream would have to add a second call site here, which the mutant suite
//! watches by counting requests per invocation.
//!
//! # What this path never does
//!
//! It does not generate an idempotency key, retry a write, cache an answer,
//! synthesize a receipt, rewrite a status, rename a code, or compose two
//! operations. `epics:apply`, scheduler start, lifecycle, settlement and ticket
//! apply may each do a great deal — inside `kontord`, under one HTTP call.

use serde::Serialize;

use crate::capability::{Denied, Gate};
use crate::client::{CallerTier, FrameBudget, Method, Reply, Request, Transport, TransportFailure};
use crate::registry::{ArgSpec, ArgType, OpKind, Place, REGISTRY, ToolSpec};

/// One tool's whole answer.
///
/// The daemon's `status` and `body` are carried verbatim. The tool name is MCP
/// framing around them, not a reinterpretation of them: a caller that wants the
/// receipt, the revision or the refusal code reads the body it would have read
/// from the daemon directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Envelope {
    /// The tool that was called.
    pub tool: String,
    /// The daemon's HTTP status, unchanged.
    pub status: u16,
    /// The daemon's JSON body, unchanged.
    pub body: serde_json::Value,
}

impl Envelope {
    /// Whether the daemon answered with a success status.
    #[must_use]
    pub const fn is_success(&self) -> bool {
        self.status >= 200 && self.status < 300
    }

    /// The daemon's own stable machine code, when it sent one.
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        self.body.get("code").and_then(serde_json::Value::as_str)
    }
}

/// Why a call produced no answer from the Realm.
///
/// A refusal *by* the Realm is not here: that is an [`Envelope`] carrying the
/// daemon's own status and body. These are the two cases where the daemon never
/// spoke — the call was refused locally, or there was nothing on the other end.
#[derive(Debug, thiserror::Error)]
pub enum Failure {
    /// Refused here, before anything was dispatched.
    #[error(transparent)]
    Denied(#[from] Denied),
    /// There was no usable answer.
    #[error(transparent)]
    Transport(#[from] TransportFailure),
}

impl Failure {
    /// The stable machine code a caller branches on.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Denied(denied) => denied.code(),
            // Nothing answered, so there is no realm verdict to relay. `unavailable`
            // is the contract's own spelling for "try again unchanged".
            Self::Transport(_) => "unavailable",
        }
    }
}

/// One realm, one credential tier, one closed tool vocabulary.
pub struct Dispatcher {
    gate: Gate,
    transport: Box<dyn Transport>,
}

impl std::fmt::Debug for Dispatcher {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Dispatcher")
            .field("tier", &self.gate.configured())
            .field("transport", &self.transport)
            .finish_non_exhaustive()
    }
}

impl Dispatcher {
    /// Build a dispatcher whose authority is the tier its transport carries.
    ///
    /// The gate cannot be configured separately from the credential. If it could,
    /// a server could be told it is an observer while holding the admin secret,
    /// and the gate would be documentation.
    #[must_use]
    pub fn new(transport: Box<dyn Transport>) -> Self {
        Self {
            gate: Gate::new(transport.tier()),
            transport,
        }
    }

    /// The authority every call is made at.
    #[must_use]
    pub const fn tier(&self) -> CallerTier {
        self.gate.configured()
    }

    /// Where this dispatcher calls.
    #[must_use]
    pub fn base_url(&self) -> String {
        self.transport.base_url()
    }

    /// The tools this server serves, which is every tool its tier reaches.
    pub fn tools(&self) -> impl Iterator<Item = &'static ToolSpec> {
        let configured = self.gate.configured();
        REGISTRY
            .iter()
            .filter(move |tool| configured.at_least(tool.tier))
    }

    /// Resolve, authorize, validate and make exactly one request.
    ///
    /// # Errors
    /// Returns [`Failure::Denied`] when the call is refused here — with **no**
    /// request made — and [`Failure::Transport`] when there was no usable answer.
    /// A refusal by the Realm is an `Ok` [`Envelope`] carrying its status and body.
    pub async fn call(
        &self,
        tool: &str,
        arguments: &serde_json::Value,
    ) -> Result<Envelope, Failure> {
        // 1. Resolve. A staged or misspelled name is absent rather than present
        //    and failing: advertising a tool that cannot work teaches a caller to
        //    trust an answer nobody computed.
        let spec = ToolSpec::find(tool).ok_or_else(|| Denied::NoSuchTool {
            tool: tool.to_owned(),
            configured: self.gate.configured(),
        })?;

        // 2. Authorize, before a request exists. This is the *only* authority
        //    decision in the crate. `required_tier` is the declared tier for every
        //    tool but the gate recording, where a waiver is authority-changing, and
        //    it is never below the declared tier — so a second check against
        //    `spec.tier` here would be unreachable. It was written once and
        //    removed: two checks that mask each other mean neither is proven, and
        //    the mutant that disables either one has to fail a test.
        let admitted = self.gate.admit(tool, spec.required_tier(arguments))?;

        // 3. Validate against the declared schema.
        let request = build(spec, arguments)?;
        debug_assert_eq!(admitted.tier(), self.gate.configured());

        // 4. Exactly one request. The two call sites below are the only ones in
        //    this crate, and neither retries.
        let reply = match spec.kind {
            OpKind::Stream => {
                let budget = budget_from(arguments);
                self.transport.frames(&request, budget).await?
            }
            OpKind::Read | OpKind::Write => self.transport.call(&request).await?,
        };
        let Reply { status, body } = reply;
        Ok(Envelope {
            tool: spec.name.to_owned(),
            status,
            body,
        })
    }
}

/// How much of one streamed response to read.
///
/// Both bounds are caller-supplied and both are bounded by the schema, so a read
/// ends on this side rather than waiting on a stream that never closes.
fn budget_from(arguments: &serde_json::Value) -> FrameBudget {
    let default = FrameBudget::default();
    let number = |name: &str| arguments.get(name).and_then(serde_json::Value::as_u64);
    FrameBudget {
        max_frames: number("max_frames").map_or(default.max_frames, |value| {
            usize::try_from(value).unwrap_or(default.max_frames)
        }),
        idle: number("idle_ms").map_or(default.idle, std::time::Duration::from_millis),
    }
}

/// Turn validated arguments into the one request they describe.
///
/// Every property is accounted for: an argument the schema does not declare is
/// refused rather than dropped, which is what stops a caller smuggling a field
/// past a tool and into a body the daemon might one day read.
fn build(spec: &ToolSpec, arguments: &serde_json::Value) -> Result<Request, Denied> {
    let object = match arguments {
        serde_json::Value::Object(object) => object,
        // A tool with no arguments may be called with nothing at all.
        serde_json::Value::Null => &serde_json::Map::new().clone(),
        _ => {
            return Err(Denied::NotAnObject {
                tool: spec.name.to_owned(),
            });
        }
    };

    for name in object.keys() {
        if !spec.args.iter().any(|arg| arg.name == name) {
            return Err(Denied::ForbiddenProperty {
                tool: spec.name.to_owned(),
                property: name.clone(),
            });
        }
    }

    let mut path = spec.path.to_owned();
    let mut query = Vec::new();
    let mut idempotency_key = None;
    let mut body = serde_json::Map::new();
    let mut has_body_arg = false;

    for arg in spec.args {
        if matches!(arg.place, Place::Body) {
            has_body_arg = true;
        }
        let Some(value) = object.get(arg.name) else {
            if arg.required {
                return Err(Denied::MissingProperty {
                    tool: spec.name.to_owned(),
                    property: arg.name.to_owned(),
                });
            }
            continue;
        };
        check(spec.name, arg, value)?;
        match arg.place {
            Place::Path => {
                let encoded = encode_segment(&scalar_text(value));
                // `.` and `..` are relative-path spellings, not names: the encoder
                // resolves them away and leaves nothing, and an empty segment
                // addresses a different route than the tool declares.
                if encoded.is_empty() {
                    return Err(Denied::InvalidValue {
                        tool: spec.name.to_owned(),
                        property: arg.name.to_owned(),
                        rule: "must name one path segment".to_owned(),
                    });
                }
                path = path.replace(&format!("{{{}}}", arg.name), &encoded);
            }
            Place::Query => query.push((arg.name.to_owned(), scalar_text(value))),
            Place::Header => idempotency_key = Some(scalar_text(value)),
            Place::Body => {
                body.insert(arg.name.to_owned(), value.clone());
            }
            // Bounds never reach the wire.
            Place::Bound => {}
        }
    }

    Ok(Request {
        method: spec.method,
        path,
        query,
        idempotency_key,
        // A route whose DTO has no properties takes no body at all — settlement is
        // the one that matters, because a body there would be a client naming an
        // outcome. A route that has properties always sends an object, even an
        // empty one, because its handler expects a document.
        body: (spec.method == Method::Post && has_body_arg)
            .then_some(serde_json::Value::Object(body)),
    })
}

/// Percent-encode one value so it is exactly one path segment.
///
/// This is the guard that makes route substitution safe *whatever* an argument's
/// declared type is. Most path arguments are identifiers or open keys, which cannot
/// carry a separator — but one of them is a runtime's own opaque request id, and a
/// value holding `/`, `..`, `?` or `#` would otherwise silently address a different
/// route than the tool declares.
///
/// The encoding is `url`'s own, not a hand-rolled table: `path_segments_mut`
/// encodes exactly what a segment must encode and leaves what it must not, which is
/// a judgement this crate has no business re-deriving.
fn encode_segment(value: &str) -> String {
    // ponytail: one throwaway parse per segment. A control-plane call makes a
    // handful; if this ever sat in a hot loop, hoist the base URL.
    let mut url = match url::Url::parse("http://encode.invalid/") {
        Ok(url) => url,
        // A constant that does not parse is unreachable; refusing the value is the
        // safe reading if it ever were.
        Err(_) => return String::new(),
    };
    match url.path_segments_mut() {
        Ok(mut segments) => {
            segments.clear().push(value);
        }
        Err(()) => return String::new(),
    }
    url.path().trim_start_matches('/').to_owned()
}

/// The text form of a scalar, for a path segment, a query value or a header.
fn scalar_text(value: &serde_json::Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), ToOwned::to_owned)
}

/// Refuse one argument the schema or the domain does not accept.
fn check(tool: &'static str, arg: &ArgSpec, value: &serde_json::Value) -> Result<(), Denied> {
    let wrong = |expected: &'static str| Denied::WrongType {
        tool: tool.to_owned(),
        property: arg.name.to_owned(),
        expected,
    };
    let invalid = |error: &kontor_core::DomainError| Denied::InvalidValue {
        tool: tool.to_owned(),
        property: arg.name.to_owned(),
        rule: error.to_string(),
    };

    match arg.ty {
        ArgType::ProjectId
        | ArgType::MiniProjectId
        | ArgType::TaskId
        | ArgType::TeamRunId
        | ArgType::AgentRunId
        | ArgType::AccountProfileId
        | ArgType::IntakeReceiptId
        | ArgType::TopologySpecId
        | ArgType::TopologyNodeId
        | ArgType::SeatBindingId
        | ArgType::RoleCatalogId
        | ArgType::OpenKey
        | ArgType::ExternalName
        | ArgType::ExternalId
        | ArgType::IdempotencyKey
        | ArgType::Timestamp => {
            let text = value.as_str().ok_or_else(|| wrong("a string"))?;
            parse_domain(arg.ty, text).map_err(|error| invalid(&error))?;
        }
        ArgType::Text => {
            let text = value.as_str().ok_or_else(|| wrong("a string"))?;
            if text.is_empty() {
                return Err(Denied::WrongType {
                    tool: tool.to_owned(),
                    property: arg.name.to_owned(),
                    expected: "a non-empty string",
                });
            }
        }
        ArgType::Enum(allowed) => {
            let text = value.as_str().ok_or_else(|| wrong("a string"))?;
            if !allowed.contains(&text) {
                return Err(Denied::InvalidValue {
                    tool: tool.to_owned(),
                    property: arg.name.to_owned(),
                    rule: format!("one of {}", allowed.join(", ")),
                });
            }
        }
        ArgType::Revision => {
            let number = value.as_u64().ok_or_else(|| wrong("a positive integer"))?;
            kontor_core::id::AggregateRevision::parse(number).map_err(|error| invalid(&error))?;
        }
        ArgType::SpecVersion => {
            let number = value.as_u64().ok_or_else(|| wrong("a positive integer"))?;
            let version = u32::try_from(number).map_err(|_| wrong("a 32-bit positive integer"))?;
            kontor_core::id::SpecVersion::parse(version).map_err(|error| invalid(&error))?;
        }
        ArgType::U32 => {
            let number = value
                .as_u64()
                .ok_or_else(|| wrong("a non-negative integer"))?;
            u32::try_from(number).map_err(|_| wrong("a 32-bit non-negative integer"))?;
        }
        ArgType::U64 => {
            value
                .as_u64()
                .ok_or_else(|| wrong("a non-negative integer"))?;
        }
        ArgType::I64 => {
            value.as_i64().ok_or_else(|| wrong("an integer"))?;
        }
        ArgType::Bool => {
            value.as_bool().ok_or_else(|| wrong("a boolean"))?;
        }
        ArgType::TextArray => {
            let items = value.as_array().ok_or_else(|| wrong("an array of text"))?;
            if items.iter().any(|item| !item.is_string()) {
                return Err(wrong("an array of text"));
            }
        }
        // A nested document the daemon validates. Its *shape* is still checked, so
        // a caller cannot pass a bare string where the DTO declares an object.
        ArgType::Json => {
            if !(value.is_object() || value.is_array()) {
                return Err(wrong("an object or an array"));
            }
        }
    }
    Ok(())
}

/// Parse one string-shaped argument with the domain's own parser.
fn parse_domain(ty: ArgType, text: &str) -> Result<(), kontor_core::DomainError> {
    use kontor_core::id;
    match ty {
        ArgType::ProjectId => id::ProjectId::parse(text).map(drop),
        ArgType::MiniProjectId => id::MiniProjectId::parse(text).map(drop),
        ArgType::TaskId => id::TaskId::parse(text).map(drop),
        ArgType::TeamRunId => id::TeamRunId::parse(text).map(drop),
        ArgType::AgentRunId => id::AgentRunId::parse(text).map(drop),
        ArgType::AccountProfileId => id::AccountProfileId::parse(text).map(drop),
        ArgType::IntakeReceiptId => id::IntakeReceiptId::parse(text).map(drop),
        ArgType::TopologySpecId => id::TopologySpecId::parse(text).map(drop),
        ArgType::TopologyNodeId => id::TopologyNodeId::parse(text).map(drop),
        ArgType::SeatBindingId => id::SeatBindingId::parse(text).map(drop),
        ArgType::RoleCatalogId => id::RoleCatalogId::parse(text).map(drop),
        ArgType::OpenKey => id::validate_open_key("OpenKey", text),
        ArgType::ExternalName => id::ExternalName::parse(text).map(drop),
        ArgType::ExternalId => id::ExternalId::parse(text).map(drop),
        ArgType::IdempotencyKey => id::IdempotencyKey::parse(text).map(drop),
        ArgType::Timestamp => id::parse_utc_timestamp(text).map(drop),
        // Every other type is checked by shape, above. Spelled out rather than
        // left to a wildcard: a new identifier kind added to the string group
        // and forgotten here would be accepted as any text at all, which is the
        // one failure this function exists to prevent. Exhaustiveness makes that
        // a compile error instead of a silently open argument.
        ArgType::Text
        | ArgType::Enum(_)
        | ArgType::Revision
        | ArgType::SpecVersion
        | ArgType::U32
        | ArgType::U64
        | ArgType::I64
        | ArgType::Bool
        | ArgType::TextArray
        | ArgType::Json => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ToolSpec;

    /// A canonical v7 UUID, so the domain parsers accept it.
    const UUID: &str = "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b70";

    fn spec(name: &str) -> &'static ToolSpec {
        ToolSpec::find(name).expect("a declared tool")
    }

    #[test]
    fn a_path_template_is_filled_from_validated_arguments() {
        let request = build(
            spec("kontor_task_get"),
            &serde_json::json!({ "project_id": UUID, "task_id": UUID }),
        )
        .expect("a well-formed call");
        assert_eq!(request.method, Method::Get);
        assert_eq!(request.path, format!("/v1/projects/{UUID}/tasks/{UUID}"));
        assert!(request.body.is_none(), "a read carries no body");
        assert!(request.idempotency_key.is_none());
    }

    #[test]
    fn an_unknown_property_is_refused_rather_than_dropped() {
        let denied = build(
            spec("kontor_realm_get"),
            &serde_json::json!({ "database_path": "/tmp/kontor.db" }),
        )
        .expect_err("a property this tool does not have");
        assert!(matches!(denied, Denied::ForbiddenProperty { .. }));
    }

    #[test]
    fn a_missing_required_property_is_refused_before_a_request_exists() {
        let denied = build(spec("kontor_run_get"), &serde_json::json!({}))
            .expect_err("the run id is required");
        assert!(matches!(
            denied,
            Denied::MissingProperty { ref property, .. } if property == "agent_run_id"
        ));
    }

    #[test]
    fn a_write_without_its_idempotency_key_never_becomes_a_request() {
        let denied = build(
            spec("kontor_project_ensure"),
            &serde_json::json!({ "name": "Pilot", "root_path": "/tmp/pilot" }),
        )
        .expect_err("a write must be committed under a caller's key");
        assert!(matches!(
            denied,
            Denied::MissingProperty { ref property, .. } if property == "idempotency_key"
        ));
    }

    #[test]
    fn the_idempotency_key_reaches_the_header_and_never_the_body() {
        let request = build(
            spec("kontor_project_ensure"),
            &serde_json::json!({
                "idempotency_key": "pilot-ensure-1",
                "name": "Pilot",
                "root_path": "/tmp/pilot",
            }),
        )
        .expect("a well-formed write");
        assert_eq!(request.idempotency_key.as_deref(), Some("pilot-ensure-1"));
        assert_eq!(
            request.body,
            Some(serde_json::json!({ "name": "Pilot", "root_path": "/tmp/pilot" })),
            "the key is a header, not a document property"
        );
    }

    #[test]
    fn a_malformed_identifier_is_refused_by_the_domains_own_parser() {
        let denied = build(
            spec("kontor_run_get"),
            &serde_json::json!({ "agent_run_id": "not-a-uuid" }),
        )
        .expect_err("the domain rejects it");
        assert!(matches!(denied, Denied::InvalidValue { .. }));

        // A v4 UUID is well formed and still not a Kontor identifier.
        assert!(
            build(
                spec("kontor_run_get"),
                &serde_json::json!({ "agent_run_id": "9f1b6c1e-4d3a-4e2b-8c5f-1a2b3c4d5e6f" }),
            )
            .is_err(),
            "only canonical v7 identifiers are accepted"
        );
    }

    #[test]
    fn a_value_of_the_wrong_shape_is_refused() {
        assert!(matches!(
            build(
                spec("kontor_run_get"),
                &serde_json::json!({ "agent_run_id": 12 })
            )
            .expect_err("a number is not an identifier"),
            Denied::WrongType { .. }
        ));
        assert!(matches!(
            build(
                spec("kontor_lifecycle_transition"),
                &serde_json::json!({
                    "project_id": UUID, "epic_id": UUID,
                    "idempotency_key": "k", "action": "delete_everything",
                    "expected_revision": 3, "reason": "because",
                }),
            )
            .expect_err("an action outside the closed set"),
            Denied::InvalidValue { .. }
        ));
        assert!(
            build(
                spec("kontor_lifecycle_transition"),
                &serde_json::json!({
                    "project_id": UUID, "epic_id": UUID,
                    "idempotency_key": "k", "action": "block",
                    "expected_revision": 0, "reason": "because",
                }),
            )
            .is_err(),
            "a revision starts at 1"
        );
    }

    #[test]
    fn settlement_builds_a_request_with_no_body_at_all() {
        let request = build(
            spec("kontor_runtime_settle"),
            &serde_json::json!({
                "project_id": UUID,
                "agent_run_id": UUID,
                "idempotency_key": "settle-1",
            }),
        )
        .expect("a well-formed settlement");
        assert_eq!(request.method, Method::Post);
        assert!(
            request.body.is_none(),
            "a settlement body would be a client naming how a run ended"
        );
        assert_eq!(request.idempotency_key.as_deref(), Some("settle-1"));
    }

    #[test]
    fn a_streamed_read_bounds_itself_from_its_arguments_and_defaults_otherwise() {
        assert_eq!(budget_from(&serde_json::json!({})), FrameBudget::default());
        let bounded = budget_from(&serde_json::json!({ "max_frames": 5, "idle_ms": 250 }));
        assert_eq!(bounded.max_frames, 5);
        assert_eq!(bounded.idle, std::time::Duration::from_millis(250));
    }

    #[test]
    fn frame_bounds_are_local_and_never_reach_the_wire() {
        let request = build(
            spec("kontor_events_list"),
            &serde_json::json!({ "after": 41, "max_frames": 3, "idle_ms": 100 }),
        )
        .expect("a well-formed stream read");
        assert_eq!(request.path, "/v1/events");
        assert_eq!(request.query, vec![("after".to_owned(), "41".to_owned())]);
        assert!(request.body.is_none());
    }

    #[test]
    fn an_optional_query_argument_is_absent_rather_than_empty_when_it_is_not_given() {
        let request = build(
            spec("kontor_session_timeline_get"),
            &serde_json::json!({ "agent_run_id": UUID }),
        )
        .expect("a well-formed timeline read");
        assert!(
            request.query.is_empty(),
            "an omitted cursor must not become an empty one"
        );
    }
}
