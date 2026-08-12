//! The catalogue: every operation this control plane exposes, declared once.
//!
//! # One declaration, two callers
//!
//! A [`ToolSpec`] is the single place that knows an operation's name, the
//! authority it requires, the properties it accepts and the `/v1` request it
//! becomes. The MCP server turns that declaration into a JSON schema and
//! validates arguments against it; the CLI turns clap arguments into the same
//! operand map and calls the same builder. Neither of them has its own idea of
//! what a route is, which is the point: two copies of a route table drift, and the
//! copy that drifts is always the one nobody tested.
//!
//! # The schema is the declaration
//!
//! [`ToolSpec::input_schema`] is *derived* from [`ToolSpec::properties`]. That is
//! what makes the disclosure rule checkable rather than aspirational: a property
//! that does not exist in the declaration cannot appear in the schema, so a test
//! that scans every schema for a runtime endpoint, a credential, an outbound
//! comment or an arbitrary external status is scanning the whole truth.
//! `additionalProperties` is `false`, and [`ToolSpec::validate`] refuses an
//! unknown property rather than dropping it — silently dropping one makes a
//! caller's mistaken belief invisible and makes smuggling worth trying.
//!
//! # What is absent
//!
//! Ticket writes, calendar and override commands, intake approval and the
//! scheduling plan are not here. Every one of them is either unreadable in this
//! build — `TicketRepository` exposes no reads, so nothing can discover the link
//! or the revision a ticket command needs — or owned by a ticket that has not
//! merged. `kontor_api::query::STAGED` is the list, and `tests/capability_matrix.rs`
//! asserts none of those names is served here.

pub mod admin;
pub mod observer;
pub mod operator;

use kontor_core::id::{
    AgentRunId, AggregateRevision, CommandReceiptId, ExternalId, GateKey, ProjectId, SpecVersion,
    TaskId, TeamRunId, WorkProfileKey,
};
use kontor_core::receipt::{AggregateRef, CommandKind, DesiredStateRule};
use serde_json::{Map, Value};

use crate::capability::Denied;
use crate::client::{CallerTier, FrameBudget, Request};

/// What one property accepts.
///
/// Deliberately small. Every operand this control plane takes is an identifier, a
/// revision, an open key, a bounded line of text, a flag or one of a closed set of
/// spellings — so a richer type system here would only be describing shapes no
/// route has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyKind {
    /// A single line of text: an identifier, an open key, a cursor or an anchor.
    Text,
    /// A non-negative whole number: a revision, a version, a page bound.
    Integer,
    /// A flag.
    Boolean,
    /// A list of text values.
    TextArray,
    /// One of a closed set of spellings.
    Choice(&'static [&'static str]),
}

impl PropertyKind {
    /// The JSON Schema fragment describing this kind.
    fn schema(self) -> Value {
        match self {
            Self::Text => serde_json::json!({ "type": "string", "minLength": 1 }),
            Self::Integer => serde_json::json!({ "type": "integer", "minimum": 0 }),
            Self::Boolean => serde_json::json!({ "type": "boolean" }),
            Self::TextArray => serde_json::json!({
                "type": "array",
                "items": { "type": "string", "minLength": 1 }
            }),
            Self::Choice(values) => serde_json::json!({ "type": "string", "enum": values }),
        }
    }

    /// What a refusal should say this kind is.
    const fn expected(self) -> &'static str {
        match self {
            Self::Text | Self::Choice(_) => "a string",
            Self::Integer => "a non-negative integer",
            Self::Boolean => "a boolean",
            Self::TextArray => "an array of strings",
        }
    }
}

/// One operand an operation accepts.
#[derive(Debug, Clone, Copy)]
pub struct Property {
    /// Its name, in the arguments object and on the command line.
    pub name: &'static str,
    /// What it accepts.
    pub kind: PropertyKind,
    /// Whether the operation refuses without it.
    pub required: bool,
    /// What it means. This reaches a language model, so it says what the value
    /// *is* rather than how to format it.
    pub description: &'static str,
}

impl Property {
    /// A required single-line operand.
    #[must_use]
    pub const fn required(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            kind: PropertyKind::Text,
            required: true,
            description,
        }
    }

    /// An optional single-line operand.
    #[must_use]
    pub const fn optional(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            kind: PropertyKind::Text,
            required: false,
            description,
        }
    }

    /// A required whole-number operand.
    #[must_use]
    pub const fn number(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            kind: PropertyKind::Integer,
            required: true,
            description,
        }
    }

    /// An optional whole-number operand.
    #[must_use]
    pub const fn optional_number(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            kind: PropertyKind::Integer,
            required: false,
            description,
        }
    }

    /// An optional flag.
    #[must_use]
    pub const fn flag(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            kind: PropertyKind::Boolean,
            required: false,
            description,
        }
    }

    /// An operand that must be one of a closed set of spellings.
    #[must_use]
    pub const fn choice(
        name: &'static str,
        values: &'static [&'static str],
        description: &'static str,
    ) -> Self {
        Self {
            name,
            kind: PropertyKind::Choice(values),
            required: true,
            description,
        }
    }

    /// An optional list of text values.
    #[must_use]
    pub const fn optional_list(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            kind: PropertyKind::TextArray,
            required: false,
            description,
        }
    }
}

/// The operand every mutation may carry to make a retry a replay.
///
/// One spelling, shared by every mutating operation, so a caller learns it once.
pub const IDEMPOTENCY_KEY: Property = Property::optional(
    "idempotency_key",
    "The stable key this mutation is committed under. Repeat it to replay the \
     original receipt instead of recording a second command. Generated when absent.",
);

/// The operand that stops a mutation before it is dispatched.
pub const DRY_RUN: Property = Property::flag(
    "dry_run",
    "Validate and return the request that would be sent, without sending it.",
);

/// Whether an operation reads, follows or writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// A side-effect-free read.
    Query,
    /// A bounded read of an event stream.
    Stream,
    /// A write, committed under an idempotency key.
    Mutation,
}

/// What one admitted call resolved to.
#[derive(Debug, Clone)]
pub struct Plan {
    /// The request to make.
    pub request: Request,
    /// The stream budget, for a [`Effect::Stream`] operation.
    pub budget: Option<FrameBudget>,
    /// Whether the caller asked for the request without the effect.
    pub dry_run: bool,
}

impl Plan {
    /// A plan that will be dispatched.
    #[must_use]
    pub const fn of(request: Request) -> Self {
        Self {
            request,
            budget: None,
            dry_run: false,
        }
    }

    /// A plan that reads a bounded prefix of a stream.
    #[must_use]
    pub const fn streaming(request: Request, budget: FrameBudget) -> Self {
        Self {
            request,
            budget: Some(budget),
            dry_run: false,
        }
    }

    /// Mark this plan as validated but not to be dispatched.
    #[must_use]
    pub const fn dry(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    /// The request as a document, for a dry run's answer.
    #[must_use]
    pub fn describe(&self) -> Value {
        serde_json::json!({
            "method": self.request.method.as_str(),
            "path": self.request.path,
            "query": self.request.query
                .iter()
                .map(|(name, value)| serde_json::json!([name, value]))
                .collect::<Vec<_>>(),
            "idempotency_key": self.request.idempotency_key,
            "body": self.request.body,
        })
    }
}

/// One operation: its name, its authority, its operands and the request it is.
#[derive(Debug, Clone, Copy)]
pub struct ToolSpec {
    /// The tool name, and the CLI's own name for the operation.
    pub name: &'static str,
    /// The authority a caller must reach. Fixed, never computed from arguments.
    pub tier: CallerTier,
    /// Whether it reads, follows or writes.
    pub effect: Effect,
    /// What it does.
    pub description: &'static str,
    /// The operands it accepts.
    pub properties: &'static [Property],
    /// How it becomes a request.
    pub build: fn(&Operands<'_>) -> Result<Plan, Denied>,
}

impl ToolSpec {
    /// The JSON Schema for this tool's arguments.
    ///
    /// Derived from [`ToolSpec::properties`], so the schema and the validator
    /// cannot disagree about what is accepted.
    #[must_use]
    pub fn input_schema(&self) -> Map<String, Value> {
        let mut properties = Map::new();
        let mut required = Vec::new();
        for property in self.properties {
            let mut fragment = property.kind.schema();
            if let Some(object) = fragment.as_object_mut() {
                object.insert(
                    "description".to_owned(),
                    Value::String(property.description.to_owned()),
                );
            }
            properties.insert(property.name.to_owned(), fragment);
            if property.required {
                required.push(Value::String(property.name.to_owned()));
            }
        }
        let mut schema = Map::new();
        schema.insert("type".to_owned(), Value::String("object".to_owned()));
        schema.insert("properties".to_owned(), Value::Object(properties));
        schema.insert("required".to_owned(), Value::Array(required));
        // Closed on purpose: an argument this tool does not declare is refused
        // rather than ignored.
        schema.insert("additionalProperties".to_owned(), Value::Bool(false));
        schema
    }

    /// The property named, if this tool has one.
    fn property(&self, name: &str) -> Option<&Property> {
        self.properties
            .iter()
            .find(|property| property.name == name)
    }

    /// Refuse arguments this tool does not accept.
    ///
    /// # Errors
    /// Returns [`Denied::ForbiddenProperty`] for an undeclared property,
    /// [`Denied::MissingProperty`] for an absent required one, and
    /// [`Denied::WrongType`] for one of the wrong shape.
    pub fn validate(&self, arguments: &Map<String, Value>) -> Result<(), Denied> {
        for name in arguments.keys() {
            if self.property(name).is_none() {
                return Err(Denied::ForbiddenProperty {
                    tool: self.name.to_owned(),
                    property: name.clone(),
                });
            }
        }
        for property in self.properties {
            let Some(value) = arguments.get(property.name) else {
                if property.required {
                    return Err(Denied::MissingProperty {
                        tool: self.name.to_owned(),
                        property: property.name.to_owned(),
                    });
                }
                continue;
            };
            let matches = match property.kind {
                PropertyKind::Text => value.as_str().is_some_and(|text| !text.is_empty()),
                PropertyKind::Choice(values) => {
                    value.as_str().is_some_and(|text| values.contains(&text))
                }
                PropertyKind::Integer => value.as_u64().is_some(),
                PropertyKind::Boolean => value.is_boolean(),
                PropertyKind::TextArray => value
                    .as_array()
                    .is_some_and(|items| items.iter().all(|item| item.is_string())),
            };
            if !matches {
                return Err(Denied::WrongType {
                    tool: self.name.to_owned(),
                    property: property.name.to_owned(),
                    expected: property.kind.expected(),
                });
            }
        }
        Ok(())
    }

    /// Validate arguments and resolve them into a request.
    ///
    /// This does **not** check authority: that is [`crate::capability::Gate`]'s
    /// job and it runs first. Splitting them keeps one obligation in one place —
    /// and keeps the authority check ahead of every line of argument handling.
    ///
    /// # Errors
    /// As [`ToolSpec::validate`], plus whatever the operation's own builder
    /// refuses.
    pub fn plan(&self, arguments: &Map<String, Value>) -> Result<Plan, Denied> {
        self.validate(arguments)?;
        let operands = Operands {
            tool: self.name,
            values: arguments,
        };
        (self.build)(&operands)
    }
}

/// Every operation this build serves, in name order.
#[must_use]
pub fn catalogue() -> Vec<ToolSpec> {
    let mut every: Vec<ToolSpec> = observer::tools()
        .iter()
        .chain(operator::tools())
        .chain(admin::tools())
        .copied()
        .collect();
    every.sort_by_key(|tool| tool.name);
    every
}

/// The operation with this name, if it is served.
#[must_use]
pub fn find(name: &str) -> Option<ToolSpec> {
    catalogue().into_iter().find(|tool| tool.name == name)
}

/// Validated arguments, with the domain parsers an operation needs.
///
/// Every getter refuses in the domain's own words rather than silently
/// substituting a default: an unparseable identifier is a caller error, and a
/// control plane that guessed which task was meant would be worse than one that
/// asked again.
#[derive(Debug)]
pub struct Operands<'a> {
    tool: &'static str,
    values: &'a Map<String, Value>,
}

impl Operands<'_> {
    /// The operation these operands belong to. Named in every refusal.
    #[must_use]
    pub const fn tool_name(&self) -> &'static str {
        self.tool
    }

    /// Refuse one operand in the domain's words.
    fn invalid(&self, property: &'static str, error: &kontor_core::DomainError) -> Denied {
        Denied::WrongTypeDetail {
            tool: self.tool.to_owned(),
            property: property.to_owned(),
            rule: error.to_string(),
        }
    }

    /// One required text operand. Validation already proved it is present.
    fn text(&self, name: &'static str) -> &str {
        self.values
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    /// One optional text operand.
    #[must_use]
    pub fn optional_text(&self, name: &str) -> Option<&str> {
        self.values.get(name).and_then(Value::as_str)
    }

    /// One text operand relayed exactly as the caller wrote it.
    ///
    /// A runtime continuation cursor and a live anchor are opaque by design — the
    /// runtime issued them and only the runtime can resolve them — so this surface
    /// carries the text rather than parsing a structure it has no business
    /// understanding.
    #[must_use]
    pub fn opaque(&self, name: &str) -> &str {
        self.optional_text(name).unwrap_or_default()
    }

    /// One optional whole-number operand.
    #[must_use]
    pub fn optional_number(&self, name: &str) -> Option<u64> {
        self.values.get(name).and_then(Value::as_u64)
    }

    /// One optional flag, absent meaning false.
    #[must_use]
    pub fn flag(&self, name: &str) -> bool {
        self.values
            .get(name)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// One optional list of text values.
    #[must_use]
    pub fn list(&self, name: &str) -> Vec<String> {
        self.values
            .get(name)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether this call is a dry run.
    #[must_use]
    pub fn dry_run(&self) -> bool {
        self.flag(DRY_RUN.name)
    }

    /// The project a call acts in.
    ///
    /// # Errors
    /// Refuses an identifier that is not a canonical UUID v7.
    pub fn project_id(&self) -> Result<ProjectId, Denied> {
        ProjectId::parse(self.text("project_id"))
            .map_err(|error| self.invalid("project_id", &error))
    }

    /// A task.
    ///
    /// # Errors
    /// As [`Operands::project_id`].
    pub fn task_id(&self) -> Result<TaskId, Denied> {
        TaskId::parse(self.text("task_id")).map_err(|error| self.invalid("task_id", &error))
    }

    /// An agent run.
    ///
    /// # Errors
    /// As [`Operands::project_id`].
    pub fn agent_run_id(&self) -> Result<AgentRunId, Denied> {
        AgentRunId::parse(self.text("agent_run_id"))
            .map_err(|error| self.invalid("agent_run_id", &error))
    }

    /// A team run.
    ///
    /// # Errors
    /// As [`Operands::project_id`].
    pub fn team_run_id(&self) -> Result<TeamRunId, Denied> {
        TeamRunId::parse(self.text("team_run_id"))
            .map_err(|error| self.invalid("team_run_id", &error))
    }

    /// A command receipt.
    ///
    /// # Errors
    /// As [`Operands::project_id`].
    pub fn receipt_id(&self) -> Result<CommandReceiptId, Denied> {
        CommandReceiptId::parse(self.text("receipt_id"))
            .map_err(|error| self.invalid("receipt_id", &error))
    }

    /// The revision the caller computed its intent against.
    ///
    /// # Errors
    /// Refuses zero: a revision counts from one, and zero is what an uninitialized
    /// field looks like.
    pub fn expected_revision(&self) -> Result<AggregateRevision, Denied> {
        let value = self
            .optional_number("expected_revision")
            .unwrap_or_default();
        AggregateRevision::parse(value).map_err(|error| self.invalid("expected_revision", &error))
    }

    /// An open work-profile key. Deployment data; never enumerated here.
    ///
    /// # Errors
    /// Refuses a key outside the lexical rule for an open key.
    pub fn profile_key(&self) -> Result<WorkProfileKey, Denied> {
        WorkProfileKey::parse(self.text("profile_key"))
            .map_err(|error| self.invalid("profile_key", &error))
    }

    /// An open gate key.
    ///
    /// # Errors
    /// As [`Operands::profile_key`].
    pub fn gate_key(&self) -> Result<GateKey, Denied> {
        GateKey::parse(self.text("gate")).map_err(|error| self.invalid("gate", &error))
    }

    /// A pinned specification revision.
    ///
    /// # Errors
    /// Refuses zero and anything wider than a `u32`.
    pub fn spec_version(&self) -> Result<SpecVersion, Denied> {
        let value = self.optional_number("version").unwrap_or_default();
        let narrowed = u32::try_from(value).unwrap_or_default();
        SpecVersion::parse(narrowed).map_err(|error| self.invalid("version", &error))
    }

    /// A runtime's own identifier — a permission request id, for instance.
    ///
    /// # Errors
    /// Refuses whitespace, control characters and anything that reads as secret
    /// material.
    pub fn external_id(&self, name: &'static str) -> Result<ExternalId, Denied> {
        ExternalId::parse(self.text(name)).map_err(|error| self.invalid(name, &error))
    }

    /// An external-ticket link.
    ///
    /// # Errors
    /// Refuses an identifier that is not a canonical UUID v7.
    pub fn ticket_link_id(&self) -> Result<kontor_core::id::TicketLinkId, Denied> {
        kontor_core::id::TicketLinkId::parse(self.text("link_id"))
            .map_err(|error| self.invalid("link_id", &error))
    }

    /// A runtime family.
    ///
    /// # Errors
    /// Refuses a key outside the lexical rule for an open key.
    pub fn runtime_kind(&self) -> Result<kontor_core::id::RuntimeKindKey, Denied> {
        kontor_core::id::RuntimeKindKey::parse(self.text("runtime_kind"))
            .map_err(|error| self.invalid("runtime_kind", &error))
    }

    /// A detected status conflict.
    ///
    /// # Errors
    /// Refuses an identifier that is not a canonical UUID v7.
    pub fn conflict_id(&self) -> Result<kontor_core::id::StatusConflictId, Denied> {
        kontor_core::id::StatusConflictId::parse(self.text("conflict_id"))
            .map_err(|error| self.invalid("conflict_id", &error))
    }

    /// The idempotency key this mutation commits under, generating one when the
    /// caller named none.
    ///
    /// A generated key is a UUID v7, which is what the session routes require:
    /// they read the `Idempotency-Key` *as* the client's stable message id, so one
    /// spelling has to satisfy both.
    #[must_use]
    pub fn idempotency_key(&self) -> String {
        self.optional_text(IDEMPOTENCY_KEY.name)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| {
                kontor_core::id::generate_uuid_v7()
                    .as_hyphenated()
                    .to_string()
            })
    }

    /// The idempotency key a *session* write commits under.
    ///
    /// The session routes read the key as the client's stable message id, and that
    /// id is a canonical UUID v7 — the runtime's own ledger keys the effect on it.
    /// So a caller's key is held to that rule here, where the refusal names the
    /// property, rather than being built into a request the daemon then rejects.
    ///
    /// # Errors
    /// Refuses a key that is not a canonical lowercase hyphenated UUID v7.
    pub fn session_key(&self) -> Result<String, Denied> {
        let key = self.idempotency_key();
        let canonical = uuid::Uuid::parse_str(&key).ok().is_some_and(|parsed| {
            parsed.get_version_num() == 7 && parsed.as_hyphenated().to_string() == key
        });
        if canonical {
            return Ok(key);
        }
        Err(Denied::WrongTypeDetail {
            tool: self.tool.to_owned(),
            property: IDEMPOTENCY_KEY.name.to_owned(),
            rule: "a session write is committed under the client's stable message id, \
                   which is a canonical lowercase hyphenated UUID v7"
                .to_owned(),
        })
    }
}

/// Build one control-plane command request.
///
/// The desired run state is read out of the domain's own compatibility matrix
/// rather than written down here, and the pair is checked through
/// [`CommandKind::ensure_compatible`] before a body exists. So a command that may
/// not target that aggregate — or that would carry the wrong desired state — is
/// refused on this machine, and the daemon's identical check is a second line of
/// defence rather than the only one.
///
/// # Errors
/// Refuses an incompatible command and target pair.
pub fn command_request(
    operands: &Operands<'_>,
    kind: CommandKind,
    project_id: ProjectId,
    target: AggregateRef,
    revision: AggregateRevision,
    intent: Value,
) -> Result<Plan, Denied> {
    let desired = match kind.rule_for(target.kind()) {
        Some(rule) => match rule.desired {
            DesiredStateRule::Requires(state) => Some(state),
            DesiredStateRule::Forbidden => None,
        },
        None => None,
    };
    kind.ensure_compatible(&target, desired)
        .map_err(|error| Denied::WrongTypeDetail {
            tool: operands.tool.to_owned(),
            property: "target".to_owned(),
            rule: error.to_string(),
        })?;
    let body = serde_json::json!({
        "project_id": project_id,
        "target": target,
        "expected_revision": revision,
        "desired_state": desired,
        "intent": intent,
        // Every command carries a canonical dispatch payload, and this surface has
        // nothing to put in one: what a command means is in the intent, and the
        // payload is the dispatcher's to fill. An empty generation-stamped
        // document is the honest minimum the contract accepts.
        "payload": { "schema_version": 1 },
    });
    Ok(Plan::of(
        Request::post(format!("/v1/commands/{kind}"), body).with_key(operands.idempotency_key()),
    )
    .dry(operands.dry_run()))
}

/// The canonical intent document a command records.
///
/// A caller's free-text reason is carried as `reason`, and nothing else is. The
/// key names here matter: a canonical document is refused outright if it carries a
/// key that reads as secret material — `token`, `secret`, `authorization` — so an
/// intent builder is not a place to be inventive with names.
#[must_use]
pub fn intent(reason: Option<&str>, extra: &[(&str, Value)]) -> Value {
    let mut document = Map::new();
    document.insert("schema_version".to_owned(), Value::from(1));
    if let Some(reason) = reason {
        document.insert("reason".to_owned(), Value::String(reason.to_owned()));
    }
    for (name, value) in extra {
        document.insert((*name).to_owned(), value.clone());
    }
    Value::Object(document)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(json: Value) -> Map<String, Value> {
        json.as_object().expect("an arguments object").clone()
    }

    #[test]
    fn every_tool_name_is_unique_and_lexically_stable() {
        let catalogue = catalogue();
        let mut names: Vec<&str> = catalogue.iter().map(|tool| tool.name).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(count, names.len(), "two tools share a name");
        assert!(
            catalogue.windows(2).all(|pair| pair[0].name < pair[1].name),
            "the catalogue is served in name order so a listing is stable"
        );
    }

    #[test]
    fn a_schema_declares_exactly_the_properties_the_validator_accepts() {
        for tool in catalogue() {
            let schema = tool.input_schema();
            let declared = schema["properties"]
                .as_object()
                .expect("a properties object");
            assert_eq!(
                declared.len(),
                tool.properties.len(),
                "{} declares a different number of properties than it accepts",
                tool.name
            );
            assert_eq!(
                schema["additionalProperties"],
                Value::Bool(false),
                "{} must not accept an undeclared property",
                tool.name
            );
            for property in tool.properties {
                assert!(
                    declared.contains_key(property.name),
                    "{} accepts {} but does not declare it",
                    tool.name,
                    property.name
                );
            }
        }
    }

    #[test]
    fn an_undeclared_property_is_refused_rather_than_dropped() {
        let tool = find("run_show").expect("the run_show tool");
        let refusal = tool
            .validate(&arguments(serde_json::json!({
                "agent_run_id": "0192f0c0-0000-7000-8000-000000000001",
                "runtime_endpoint": "http://10.0.0.4:9000"
            })))
            .expect_err("an undeclared property is refused");
        assert_eq!(
            refusal,
            Denied::ForbiddenProperty {
                tool: "run_show".to_owned(),
                property: "runtime_endpoint".to_owned(),
            }
        );
    }

    #[test]
    fn a_missing_or_mistyped_operand_is_named() {
        let tool = find("task_list").expect("the task_list tool");
        assert!(matches!(
            tool.validate(&arguments(serde_json::json!({}))),
            Err(Denied::MissingProperty { .. })
        ));
        assert!(matches!(
            tool.validate(&arguments(serde_json::json!({ "project_id": 7 }))),
            Err(Denied::WrongType { .. })
        ));
        assert!(
            matches!(
                tool.validate(&arguments(serde_json::json!({ "project_id": "" }))),
                Err(Denied::WrongType { .. })
            ),
            "an empty identifier is not a value"
        );
    }

    #[test]
    fn an_unparseable_identifier_is_refused_before_a_request_exists() {
        let tool = find("run_show").expect("the run_show tool");
        let refusal = tool
            .plan(&arguments(serde_json::json!({
                "agent_run_id": "not-a-uuid"
            })))
            .expect_err("a malformed identifier never becomes a route");
        assert!(matches!(refusal, Denied::WrongTypeDetail { .. }));
    }

    #[test]
    fn a_choice_property_refuses_a_spelling_outside_its_set() {
        let tool = find("session_permission").expect("the session_permission tool");
        assert!(matches!(
            tool.validate(&arguments(serde_json::json!({
                "agent_run_id": "0192f0c0-0000-7000-8000-000000000001",
                "permission_request_id": "perm-1",
                "decision": "maybe"
            }))),
            Err(Denied::WrongType { .. })
        ));
    }

    #[test]
    fn a_command_takes_its_desired_state_from_the_domain_matrix() {
        let tool = find("run_launch").expect("the run_launch tool");
        let plan = tool
            .plan(&arguments(serde_json::json!({
                "project_id": "0192f0c0-0000-7000-8000-000000000001",
                "agent_run_id": "0192f0c0-0000-7000-8000-000000000002",
                "expected_revision": 3
            })))
            .expect("a launch plans");
        let body = plan.request.body.as_ref().expect("a command body");
        assert_eq!(
            body["desired_state"],
            serde_json::json!("run_requested"),
            "the desired state comes from CommandKind's own rule, not from this module"
        );
        assert_eq!(plan.request.path, "/v1/commands/launch_run");
        assert!(
            plan.request.idempotency_key.is_some(),
            "a mutation is always committed under a key, generated when none was given"
        );
        assert_eq!(body["payload"]["schema_version"], serde_json::json!(1));
    }

    #[test]
    fn a_generated_idempotency_key_differs_between_calls_and_a_given_one_is_kept() {
        let tool = find("task_resume").expect("the task_resume tool");
        let mut base = arguments(serde_json::json!({
            "project_id": "0192f0c0-0000-7000-8000-000000000001",
            "task_id": "0192f0c0-0000-7000-8000-000000000002",
            "expected_revision": 1
        }));
        let first = tool.plan(&base).expect("a resume plans");
        let second = tool.plan(&base).expect("a resume plans");
        assert_ne!(
            first.request.idempotency_key, second.request.idempotency_key,
            "two fresh mutations must not share a key, or one would replay the other"
        );

        base.insert("idempotency_key".to_owned(), Value::from("replay-me"));
        let named = tool.plan(&base).expect("a resume plans");
        assert_eq!(
            named.request.idempotency_key.as_deref(),
            Some("replay-me"),
            "a caller's key is what makes a retry a replay, so it is never regenerated"
        );
    }

    #[test]
    fn a_dry_run_is_planned_and_marked_and_never_loses_its_flag() {
        let tool = find("run_cancel").expect("the run_cancel tool");
        let plan = tool
            .plan(&arguments(serde_json::json!({
                "project_id": "0192f0c0-0000-7000-8000-000000000001",
                "agent_run_id": "0192f0c0-0000-7000-8000-000000000002",
                "expected_revision": 2,
                "dry_run": true
            })))
            .expect("a dry run plans");
        assert!(plan.dry_run, "the flag survives into the plan");
        let described = plan.describe();
        assert_eq!(described["method"], serde_json::json!("POST"));
        assert_eq!(
            described["path"],
            serde_json::json!("/v1/commands/cancel_run")
        );
    }

    #[test]
    fn every_mutation_accepts_an_idempotency_key_and_a_dry_run() {
        for tool in catalogue()
            .into_iter()
            .filter(|tool| tool.effect == Effect::Mutation)
        {
            assert!(
                tool.property(IDEMPOTENCY_KEY.name).is_some(),
                "{} is a mutation and must be replayable",
                tool.name
            );
            assert!(
                tool.property(DRY_RUN.name).is_some(),
                "{} is a mutation and must be inspectable without being performed",
                tool.name
            );
        }
    }

    #[test]
    fn every_query_is_a_get_and_every_mutation_is_a_post() {
        // The operand values here are throwaway: what is asserted is that the
        // *shape* of an operation matches its declared effect, so a read can never
        // be built as a write.
        for tool in catalogue() {
            let mut arguments = Map::new();
            // One identifier for every id-shaped operand of a tool. Distinct ids
            // would be more lifelike, but `authorize_execution` legitimately
            // demands that a project-scoped grant name its own project — and this
            // test is about method and path, not about scope rules.
            let identifier = kontor_core::id::generate_uuid_v7()
                .as_hyphenated()
                .to_string();
            for property in tool.properties {
                if !property.required {
                    continue;
                }
                let value = match property.kind {
                    PropertyKind::Integer => Value::from(1),
                    PropertyKind::Boolean => Value::from(false),
                    PropertyKind::TextArray => Value::Array(Vec::new()),
                    PropertyKind::Choice(values) => Value::from(values[0]),
                    PropertyKind::Text => Value::from(sample(property.name, &identifier)),
                };
                arguments.insert(property.name.to_owned(), value);
            }
            let plan = tool.plan(&arguments).unwrap_or_else(|error| {
                panic!("{} plans from its own operands: {error}", tool.name)
            });
            let expected = match tool.effect {
                Effect::Query | Effect::Stream => crate::client::Method::Get,
                Effect::Mutation => crate::client::Method::Post,
            };
            assert_eq!(
                plan.request.method,
                expected,
                "{} declares {:?} and must be built as {}",
                tool.name,
                tool.effect,
                expected.as_str()
            );
            assert!(
                plan.request.path.starts_with("/v1/"),
                "{} addresses something outside the versioned contract",
                tool.name
            );
        }
    }

    /// A plausible value for one text operand, so the shape tests can build every
    /// operation without knowing what each one means.
    fn sample(name: &str, identifier: &str) -> String {
        match name {
            "profile_key" => "delivery".to_owned(),
            "gate" => "review".to_owned(),
            "body" => "do the work".to_owned(),
            "permission_request_id" => "perm-1".to_owned(),
            "after" => "1:1".to_owned(),
            _ => identifier.to_owned(),
        }
    }
}
