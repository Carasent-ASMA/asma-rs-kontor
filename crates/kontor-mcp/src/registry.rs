//! The closed tool vocabulary: one row per Lead-applicable `/v1` operation.
//!
//! # Why a table and not one function per tool
//!
//! Every tool does the same four things — resolve, check authority, validate,
//! make one request — so the only thing that actually differs between them is
//! *data*: a name, a tier, a method, a path template and an argument list. Writing
//! that as data means [`crate::dispatch`] is one code path that cannot drift
//! per-tool, and it means the MCP tool list, the CLI command list and the
//! API-to-MCP parity oracle all read the same rows instead of three hand-kept
//! lists that agree until someone edits one.
//!
//! # What a row may not contain
//!
//! No base URL, no credential, no tier selection, no runtime endpoint, no store
//! handle, no external-tracker field. An argument names a path segment, a query
//! parameter, the idempotency header or a top-level property of the daemon's own
//! request DTO — nothing else. The forbidden-schema mutant suite walks every row
//! and fails on a property whose name reaches persistence, a runtime, a provider
//! or a credential, so a smuggled field is a failing test rather than a review
//! comment.
//!
//! # What the tiers are
//!
//! They mirror the `caller.require(...)` the daemon performs on the same route.
//! MCP refusing first is not a substitute for that check: it is what makes "the
//! write was never attempted" true, which is a different fact from "the write was
//! refused" and the one the capability tests assert.

use crate::client::{CallerTier, Method};

/// Where one argument goes once the request is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// A `{name}` segment of the path template.
    Path,
    /// A query parameter.
    Query,
    /// The `Idempotency-Key` header, and nothing else.
    ///
    /// It is never generated here and never retried: a key this crate invented
    /// would make a retry look like a new intent to the one component that
    /// decides what a retry means.
    Header,
    /// A top-level property of the daemon's own request DTO.
    Body,
    /// A bound on how much of one streamed response is read.
    ///
    /// Local to this crate: it never reaches the wire, which is why the parity
    /// oracle excludes it when comparing an argument list with OpenAPI.
    Bound,
}

/// What one argument must be before a request exists.
///
/// The domain types are the real ones from `kontor-core`, so a malformed
/// identifier is refused here rather than travelling to the daemon to be refused
/// there. Everything deeper than a top-level property is [`ArgType::Json`]: the
/// daemon owns validation beyond the wire schema, and a second copy of its nested
/// rules here would be a second contract that drifts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgType {
    /// A canonical v7 UUID naming a project.
    ProjectId,
    /// A canonical v7 UUID naming a goal-sized unit of work.
    MiniProjectId,
    /// A canonical v7 UUID naming a task.
    TaskId,
    /// A canonical v7 UUID naming one run of a team.
    TeamRunId,
    /// A canonical v7 UUID naming one agent run.
    AgentRunId,
    /// A canonical v7 UUID naming an account profile.
    AccountProfileId,
    /// A canonical v7 UUID naming one intake decision.
    IntakeReceiptId,
    /// A canonical v7 UUID naming one topology specification across revisions.
    TopologySpecId,
    /// A canonical v7 UUID naming one durable node in a project session topology.
    ///
    /// A node id is only ever *returned* by a projection and then addressed back:
    /// it is the one topology handle a model may hold, which is what lets it
    /// retire a node it can see without ever naming a kind, a parent or a native
    /// container.
    TopologyNodeId,
    /// A canonical v7 UUID naming one persistent seat binding.
    SeatBindingId,
    /// A canonical v7 UUID naming one server-owned role catalog across revisions.
    RoleCatalogId,
    /// A canonical v7 UUID naming one raw provider/account observation.
    CapacityObservationId,
    /// A canonical v7 UUID naming one Quick session.
    QuickSessionId,
    /// A canonical v7 UUID naming one Advisor consultation.
    AdvisorRunId,
    /// A canonical v7 UUID naming one Committee consultation.
    CommitteeRunId,
    /// An open, deployment-defined key.
    ///
    /// Its lexical rule — lowercase ASCII, digits, `.`, `_`, `-` — is also what
    /// makes it safe in a path segment: a key cannot carry a `/` or a `..` and
    /// therefore cannot move the route it is interpolated into.
    OpenKey,
    /// A human-facing name, label or reason.
    ExternalName,
    /// An identifier minted by a system outside this Realm.
    ///
    /// Unicode and may contain spaces, so it belongs in a body and never in a path
    /// segment.
    ExternalId,
    /// A caller's stable idempotency key.
    IdempotencyKey,
    /// Free text the daemon interprets.
    Text,
    /// A positive aggregate revision.
    Revision,
    /// A positive specification revision.
    SpecVersion,
    /// An RFC 3339 UTC timestamp.
    Timestamp,
    /// A boolean.
    Bool,
    /// A non-negative count.
    U32,
    /// A non-negative quantity.
    U64,
    /// A signed cursor.
    I64,
    /// One of a closed set of spellings.
    Enum(&'static [&'static str]),
    /// An array of text.
    TextArray,
    /// A nested document the daemon validates.
    ///
    /// Prefer [`ArgType::Object`] wherever the accepted shape is actually
    /// known. This variant tells a caller only that *some* object goes here,
    /// which leaves it guessing field names against a closed DTO — and a wrong
    /// guess is refused by the daemon, not by anything the caller could have
    /// read first.
    Json,
    /// A nested document whose fields are declared.
    ///
    /// The fields are emitted into the tool's own JSON Schema, so a caller sees
    /// the shape before it calls instead of discovering it from a refusal.
    Object(&'static [FieldSpec]),
}

/// One field of a declared nested object.
///
/// Deliberately not an [`ArgSpec`]: a nested field has no [`Place`] of its own —
/// it lives wherever its parent argument goes — and a type that offered one
/// would invite a field claiming to be a header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSpec {
    /// The property name a caller supplies.
    pub name: &'static str,
    /// What it must be.
    pub ty: ArgType,
    /// Whether the parent object is refused without it.
    pub required: bool,
    /// What it means, for the tool's own schema.
    pub about: &'static str,
}

/// A shorthand for one required field of a declared object.
const fn field(name: &'static str, ty: ArgType, about: &'static str) -> FieldSpec {
    FieldSpec {
        name,
        ty,
        required: true,
        about,
    }
}

impl ArgType {
    /// The JSON type a caller must supply.
    #[must_use]
    pub const fn json_type(self) -> &'static str {
        match self {
            Self::ProjectId
            | Self::MiniProjectId
            | Self::TaskId
            | Self::TeamRunId
            | Self::AgentRunId
            | Self::AccountProfileId
            | Self::IntakeReceiptId
            | Self::TopologySpecId
            | Self::TopologyNodeId
            | Self::SeatBindingId
            | Self::RoleCatalogId
            | Self::CapacityObservationId
            | Self::QuickSessionId
            | Self::AdvisorRunId
            | Self::CommitteeRunId
            | Self::OpenKey
            | Self::ExternalName
            | Self::ExternalId
            | Self::IdempotencyKey
            | Self::Text
            | Self::Timestamp
            | Self::Enum(_) => "string",
            Self::Revision | Self::SpecVersion | Self::U32 | Self::U64 | Self::I64 => "integer",
            Self::Bool => "boolean",
            Self::TextArray => "array",
            Self::Json | Self::Object(_) => "object",
        }
    }

    /// This type's own JSON Schema fragment, less its description.
    ///
    /// Recursive so that a declared object's fields are as fully described as a
    /// top-level argument: a nested `Object` inside an `Object` still shows its
    /// shape rather than bottoming out at "object".
    #[must_use]
    pub fn schema(self) -> serde_json::Map<String, serde_json::Value> {
        let mut fragment = serde_json::Map::new();
        fragment.insert("type".into(), self.json_type().into());
        match self {
            Self::Enum(allowed) => {
                fragment.insert("enum".into(), allowed.iter().copied().collect());
            }
            Self::TextArray => {
                fragment.insert("items".into(), serde_json::json!({ "type": "string" }));
            }
            Self::Object(fields) => {
                let mut properties = serde_json::Map::new();
                let mut required = Vec::new();
                for field in fields {
                    let mut property = field.ty.schema();
                    property.insert("description".into(), field.about.into());
                    properties.insert(field.name.to_owned(), property.into());
                    if field.required {
                        required.push(serde_json::Value::from(field.name));
                    }
                }
                fragment.insert("properties".into(), properties.into());
                fragment.insert("required".into(), required.into());
                // Same reasoning as the top-level schema: the daemon's DTOs are
                // closed, so advertising openness would invite a smuggled field.
                fragment.insert("additionalProperties".into(), false.into());
            }
            _ => {}
        }
        fragment
    }
}

/// One declared argument.
#[derive(Debug, Clone, Copy)]
pub struct ArgSpec {
    /// The property name a caller supplies.
    pub name: &'static str,
    /// Where it goes.
    pub place: Place,
    /// What it must be.
    pub ty: ArgType,
    /// Whether the call is refused without it.
    pub required: bool,
    /// What it means, for the tool's own schema.
    pub about: &'static str,
}

/// A shorthand for one required argument.
const fn req(name: &'static str, place: Place, ty: ArgType, about: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        place,
        ty,
        required: true,
        about,
    }
}

/// A shorthand for one optional argument.
const fn opt(name: &'static str, place: Place, ty: ArgType, about: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        place,
        ty,
        required: false,
        about,
    }
}

/// The idempotency key every daemon write is committed under.
const IDEMPOTENCY: ArgSpec = req(
    "idempotency_key",
    Place::Header,
    ArgType::IdempotencyKey,
    "The caller's stable key. Reusing it returns the original receipt.",
);

/// The bounds every budget ceiling is declared with.
///
/// This mirrors `kontor_api::applications::BudgetBoundsRequest` field for field.
/// The two are kept in step by `budget_bounds_match_the_daemons_dto` below,
/// because a schema that drifts from the DTO is worse than no schema: it is a
/// wrong answer a caller has no reason to doubt.
const BUDGET_BOUNDS: &[FieldSpec] = &[
    field(
        "max_tokens",
        ArgType::U64,
        "Maximum tokens across the bounded work.",
    ),
    field(
        "max_commands",
        ArgType::U64,
        "Maximum runtime commands across the bounded work.",
    ),
    field(
        "max_duration_seconds",
        ArgType::U64,
        "Maximum wall-clock seconds across the bounded work.",
    ),
    field(
        "max_cost_minor_units",
        ArgType::U64,
        "Maximum monetary cost, in integer minor units of `cost_currency`.",
    ),
    field(
        "cost_currency",
        ArgType::Text,
        "The ISO 4217 currency those minor units are in, such as `NOK`.",
    ),
];

/// The two bounds a streamed read is taken under.
const MAX_FRAMES: ArgSpec = opt(
    "max_frames",
    Place::Bound,
    ArgType::U32,
    "Stop after this many frames from this one response. Default 100.",
);

/// How long one streamed read waits for the next frame before returning.
const IDLE_MS: ArgSpec = opt(
    "idle_ms",
    Place::Bound,
    ArgType::U32,
    "Stop when no frame has arrived for this long. Default 2000.",
);

/// What one operation does to the Realm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    /// It reads, or computes a plan, and commits nothing.
    Read,
    /// It is committed under a caller-supplied idempotency key.
    Write,
    /// It reads a bounded prefix of one server-sent event stream.
    Stream,
}

/// One tool: one name, one authority, one `/v1` operation.
#[derive(Debug, Clone, Copy)]
pub struct ToolSpec {
    /// The MCP tool name, which is also the CLI command name.
    pub name: &'static str,
    /// The minimum authority. Higher tiers inherit it.
    pub tier: CallerTier,
    /// The method of the one request this tool makes.
    pub method: Method,
    /// The `/v1` path template. `{name}` segments are filled from path arguments.
    pub path: &'static str,
    /// What it does to the Realm.
    pub kind: OpKind,
    /// Every argument, in schema order.
    pub args: &'static [ArgSpec],
    /// One line for the tool list.
    pub about: &'static str,
}

impl ToolSpec {
    /// The tool named, if this vocabulary has it.
    #[must_use]
    pub fn find(name: &str) -> Option<&'static Self> {
        REGISTRY.iter().find(|tool| tool.name == name)
    }

    /// The arguments that go to one place.
    pub fn args_in(&self, place: Place) -> impl Iterator<Item = &'static ArgSpec> {
        self.args.iter().filter(move |arg| arg.place == place)
    }

    /// Whether this tool commits anything.
    #[must_use]
    pub const fn is_write(&self) -> bool {
        matches!(self.kind, OpKind::Write)
    }

    /// The JSON Schema an MCP client is shown, derived from the same rows the
    /// dispatch path validates against.
    ///
    /// `additionalProperties` is `false` on every tool. The daemon's DTOs are
    /// closed, and a schema that advertised otherwise would invite exactly the
    /// smuggled property the dispatch path refuses.
    #[must_use]
    pub fn input_schema(&self) -> serde_json::Value {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();
        for arg in self.args {
            let mut property = arg.ty.schema();
            property.insert("description".into(), arg.about.into());
            properties.insert(arg.name.to_owned(), property.into());
            if arg.required {
                required.push(serde_json::Value::from(arg.name));
            }
        }
        serde_json::json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        })
    }

    /// The authority this specific call needs.
    ///
    /// It is the declared tier for every tool but one. A gate *waiver* is an
    /// authority-changing decision rather than an ordinary verdict, so it demands
    /// admin — checked here, before the request exists, and enforced again by the
    /// daemon on its own terms.
    #[must_use]
    pub fn required_tier(&self, arguments: &serde_json::Value) -> CallerTier {
        if self.name == "kontor_gate_record"
            && arguments.get("verdict").and_then(serde_json::Value::as_str) == Some("waived")
        {
            return CallerTier::Admin;
        }
        self.tier
    }
}

/// One named, registry-defined subset of the tool vocabulary.
///
/// A profile narrows *presentation only*: which tools a server lists, and — so
/// the list and the callable set are the same set — which calls it admits. It
/// is always intersected with what the credential tier already allows and can
/// never widen it: authority remains the credential's. Profiles live here,
/// next to the tier declarations, because free-form tool lists in seat files
/// are deliberately not accepted — a list in configuration would be a second
/// authority model beside the credential (see `seats/README.md`).
#[derive(Debug, Clone, Copy)]
pub struct ServeProfile {
    /// The name `--serve-profile` selects.
    pub name: &'static str,
    /// The registry tool names this profile serves, within the tier.
    pub tools: &'static [&'static str],
}

impl ServeProfile {
    /// The profile named, if this registry declares it.
    #[must_use]
    pub fn find(name: &str) -> Option<&'static Self> {
        SERVE_PROFILES.iter().find(|profile| profile.name == name)
    }

    /// Whether this profile serves the named tool.
    #[must_use]
    pub fn allows(&self, tool: &str) -> bool {
        self.tools.contains(&tool)
    }

    /// Every declared profile name, so a startup refusal can name the valid ones.
    #[must_use]
    pub fn names() -> Vec<&'static str> {
        SERVE_PROFILES.iter().map(|profile| profile.name).collect()
    }
}

/// Every declared serve profile.
///
/// `worker` is the everyday working seat's surface: read the work, claim it,
/// settle a turn, record a gate verdict, talk on the session, submit intake,
/// propose memory and resolve context — 16 tools, all at or below operator
/// tier, which the drift test below pins against the registry.
pub static SERVE_PROFILES: &[ServeProfile] = &[ServeProfile {
    name: "worker",
    tools: &[
        "kontor_task_get",
        "kontor_run_get",
        "kontor_realm_get",
        "kontor_code_help_get",
        "kontor_intake_receipt_get",
        "kontor_ticket_comments_list",
        "kontor_events_list",
        "kontor_completion_get",
        "kontor_ticket_claim",
        "kontor_turn_settle",
        "kontor_gate_record",
        "kontor_session_message_send",
        "kontor_ticket_comments_pull",
        "kontor_intake_submit",
        "kontor_memory_propose",
        "kontor_context_resolve",
    ],
}];

/// Every operation a Paseo Lead Architect can reach, and nothing else.
///
/// The public `/v1` routes deliberately absent are listed in
/// [`NON_AGENT_ROUTES`]. The parity oracle proves that this table plus that list
/// covers the generated contract exactly.
pub static REGISTRY: &[ToolSpec] = &[
    // ---- Observer: projections, catalogs and session content -----------------
    ToolSpec {
        name: "kontor_realm_get",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/realm",
        kind: OpKind::Read,
        args: &[],
        about: "This realm's identity, locality and freshness.",
    },
    ToolSpec {
        name: "kontor_run_get",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/runs/{agent_run_id}",
        kind: OpKind::Read,
        args: &[req(
            "agent_run_id",
            Place::Path,
            ArgType::AgentRunId,
            "The run to read.",
        )],
        about: "One agent run's snapshot, at one control-plane position.",
    },
    ToolSpec {
        name: "kontor_task_get",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/tasks/{task_id}",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("task_id", Place::Path, ArgType::TaskId, "The task to read."),
        ],
        about: "One task's snapshot, at one control-plane position.",
    },
    ToolSpec {
        name: "kontor_events_list",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/events",
        kind: OpKind::Stream,
        args: &[
            opt(
                "after",
                Place::Query,
                ArgType::I64,
                "Resume strictly after this cursor.",
            ),
            MAX_FRAMES,
            IDLE_MS,
        ],
        about: "A bounded read of the durable control-plane event stream.",
    },
    ToolSpec {
        name: "kontor_profile_packs_list",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/catalog/packs",
        kind: OpKind::Read,
        args: &[],
        about: "Every profile pack this realm resolves categories from: the compiled seeds and \
                whatever an operator registered.",
    },
    ToolSpec {
        name: "kontor_profile_pack_register",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/catalog/packs:register",
        kind: OpKind::Write,
        // The key is bound to a fingerprint of the whole logical operation
        // rather than carried by a command receipt: a receipt is written against
        // a project and a realm-wide catalogue has none. Same key and same
        // fingerprint replays; the same key for a different pack, revision or
        // content is refused.
        args: &[
            req(
                "pack",
                Place::Body,
                ArgType::Json,
                "The whole profile-pack document: manifest, profiles, teams, roles, skills.",
            ),
            IDEMPOTENCY,
        ],
        about: "Register a work profile and its team template additively, without a rebuild.",
    },
    ToolSpec {
        name: "kontor_work_profiles_list",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/catalog/work-profiles",
        kind: OpKind::Read,
        args: &[],
        about: "The work profiles a caller may select.",
    },
    ToolSpec {
        name: "kontor_work_profile_get",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/catalog/work-profiles/{category}",
        kind: OpKind::Read,
        args: &[req(
            "category",
            Place::Path,
            ArgType::OpenKey,
            "The pack category, as the catalog reports it.",
        )],
        about: "One category's whole shape: phases, gates, artifacts, handoffs and eligible roots.",
    },
    ToolSpec {
        name: "kontor_work_profile_validate",
        tier: CallerTier::Observer,
        method: Method::Post,
        path: "/v1/catalog/work-profiles/{category}/validate",
        kind: OpKind::Read,
        // A `POST` that commits nothing and takes no key: validation *reports* a
        // finding rather than changing anything, so a caller asking "is this
        // runnable" gets an answer instead of a refusal.
        args: &[req(
            "category",
            Place::Path,
            ArgType::OpenKey,
            "The pack category to validate.",
        )],
        about: "Whether one category resolves and validates, and what is wrong when it does not.",
    },
    ToolSpec {
        name: "kontor_team_templates_list",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/catalog/team-templates",
        kind: OpKind::Read,
        args: &[],
        about: "The team template revisions a work profile may pin.",
    },
    ToolSpec {
        name: "kontor_model_catalog_get",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/catalog",
        kind: OpKind::Read,
        args: &[],
        about: "The realm-qualified provider and model catalog used by Teams.",
    },
    ToolSpec {
        name: "kontor_teams_get",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/teams",
        kind: OpKind::Read,
        args: &[],
        about: "Current Teams drafts and immutable revisions at one projection cursor.",
    },
    ToolSpec {
        name: "kontor_team_draft_save",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/teams/drafts:save",
        kind: OpKind::Write,
        args: &[
            req(
                "id",
                Place::Body,
                ArgType::OpenKey,
                "The logical team-template id.",
            ),
            req(
                "name",
                Place::Body,
                ArgType::ExternalName,
                "The draft's human label.",
            ),
            req(
                "slots",
                Place::Body,
                ArgType::Json,
                "The draft's slot declarations.",
            ),
            IDEMPOTENCY,
        ],
        about: "Create or replace one server-held Teams draft.",
    },
    ToolSpec {
        name: "kontor_team_publish",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/teams/{team_id}/publish",
        kind: OpKind::Write,
        args: &[
            req(
                "team_id",
                Place::Path,
                ArgType::OpenKey,
                "The logical team-template id.",
            ),
            IDEMPOTENCY,
        ],
        about: "Publish the next immutable revision of one Teams draft.",
    },
    ToolSpec {
        name: "kontor_runtime_capabilities_list",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/runtime-capabilities",
        kind: OpKind::Read,
        args: &[],
        about: "What every configured runtime family can currently prove.",
    },
    ToolSpec {
        name: "kontor_account_profiles_list",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/provider-account-profiles",
        kind: OpKind::Read,
        args: &[req(
            "project_id",
            Place::Path,
            ArgType::ProjectId,
            "The owning project.",
        )],
        about: "The provider-account profiles a run may be pinned to.",
    },
    ToolSpec {
        name: "kontor_epic_get",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/epics/{epic_id}",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "epic_id",
                Place::Path,
                ArgType::MiniProjectId,
                "The epic to read.",
            ),
        ],
        about: "One epic's whole graph: tasks, phases, gates and required evidence.",
    },
    ToolSpec {
        name: "kontor_session_timeline_get",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/sessions/{agent_run_id}/timeline",
        kind: OpKind::Read,
        args: &[
            req(
                "agent_run_id",
                Place::Path,
                ArgType::AgentRunId,
                "The run whose session is read.",
            ),
            opt(
                "after",
                Place::Query,
                ArgType::Text,
                "A runtime continuation cursor.",
            ),
            opt("limit", Place::Query, ArgType::U32, "Maximum items."),
        ],
        about: "One page of a session's history, read from the runtime.",
    },
    ToolSpec {
        name: "kontor_session_stream_read",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/sessions/{agent_run_id}/stream",
        kind: OpKind::Stream,
        args: &[
            req(
                "agent_run_id",
                Place::Path,
                ArgType::AgentRunId,
                "The run whose session is read.",
            ),
            req(
                "after",
                Place::Query,
                ArgType::Text,
                "The anchor a timeline read returned.",
            ),
            MAX_FRAMES,
            IDLE_MS,
        ],
        about: "A bounded read of one session's live frames, from one response.",
    },
    // ---- Admin: graph authorship, selection and arming -----------------------
    ToolSpec {
        name: "kontor_project_ensure",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects:ensure",
        kind: OpKind::Write,
        args: &[
            IDEMPOTENCY,
            req(
                "name",
                Place::Body,
                ArgType::ExternalName,
                "The project's name.",
            ),
            req(
                "root_path",
                Place::Body,
                ArgType::ExternalName,
                "The checkout root the project owns.",
            ),
        ],
        about: "Create a project, or return the existing one unchanged.",
    },
    ToolSpec {
        name: "kontor_account_profile_ensure",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/provider-account-profiles:ensure",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            IDEMPOTENCY,
            req(
                "label",
                Place::Body,
                ArgType::ExternalName,
                "The profile's label.",
            ),
            req(
                "harness",
                Place::Body,
                ArgType::OpenKey,
                "The runtime family this account drives.",
            ),
            req(
                "credential_alias",
                Place::Body,
                ArgType::Text,
                "An approved credential reference. Never a secret value.",
            ),
            req(
                "enabled",
                Place::Body,
                ArgType::Bool,
                "Whether runs may be pinned to it.",
            ),
        ],
        about: "Create a provider-account profile, or return the one with that label.",
    },
    ToolSpec {
        name: "kontor_epic_apply",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/epics:apply",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            IDEMPOTENCY,
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read the project at.",
            ),
            req(
                "name",
                Place::Body,
                ArgType::ExternalName,
                "The epic's name.",
            ),
            req(
                "work_profile_category",
                Place::Body,
                ArgType::Text,
                "The work-profile category every task starts from.",
            ),
            opt(
                "team_template",
                Place::Body,
                ArgType::Json,
                "An `{id, version}` team-template revision to pin.",
            ),
            req(
                "runtime_family",
                Place::Body,
                ArgType::OpenKey,
                "The runtime family the epic's runs use.",
            ),
            opt(
                "account_profile_id",
                Place::Body,
                ArgType::AccountProfileId,
                "The account profile runs are pinned to.",
            ),
            req(
                "tasks",
                Place::Body,
                ArgType::Json,
                "The whole task graph, applied atomically.",
            ),
        ],
        about: "Apply one whole epic graph atomically.",
    },
    ToolSpec {
        name: "kontor_execution_arm",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/epics/{epic_id}/execution:arm",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("epic_id", Place::Path, ArgType::MiniProjectId, "The epic."),
            IDEMPOTENCY,
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read the epic at.",
            ),
            opt(
                "tasks",
                Place::Body,
                ArgType::TextArray,
                "The tasks the authorization covers. Empty means the whole epic.",
            ),
            req(
                "allowed_start",
                Place::Body,
                ArgType::Timestamp,
                "When the authorization opens.",
            ),
            req(
                "allowed_end",
                Place::Body,
                ArgType::Timestamp,
                "When it expires.",
            ),
            req(
                "max_concurrency",
                Place::Body,
                ArgType::U32,
                "How many runs may be in flight.",
            ),
            req(
                "budget",
                Place::Body,
                ArgType::Object(BUDGET_BOUNDS),
                "The token, command, duration and cost bounds.",
            ),
            req(
                "granted_by",
                Place::Body,
                ArgType::AccountProfileId,
                "Who granted it.",
            ),
            req(
                "reason",
                Place::Body,
                ArgType::ExternalName,
                "Why it was granted.",
            ),
        ],
        about: "Grant a bounded execution authorization over an epic.",
    },
    ToolSpec {
        name: "kontor_execution_disarm",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/epics/{epic_id}/execution:disarm",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("epic_id", Place::Path, ArgType::MiniProjectId, "The epic."),
            IDEMPOTENCY,
            req(
                "authorization_id",
                Place::Body,
                ArgType::Text,
                "The authorization being revoked.",
            ),
            req(
                "revoked_by",
                Place::Body,
                ArgType::AccountProfileId,
                "Who revoked it.",
            ),
            req(
                "reason",
                Place::Body,
                ArgType::ExternalName,
                "Why it was revoked.",
            ),
        ],
        about: "Revoke a bounded execution authorization.",
    },
    ToolSpec {
        name: "kontor_profile_select",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/tasks/{task_id}/profile-selection",
        kind: OpKind::Write,
        args: SELECTION_ARGS,
        about: "Set or correct one task's pinned work profile before a run snapshots it.",
    },
    ToolSpec {
        name: "kontor_team_select",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/tasks/{task_id}/team-selection",
        kind: OpKind::Write,
        args: SELECTION_ARGS,
        about: "Set or correct one task's pinned team template.",
    },
    ToolSpec {
        name: "kontor_account_select",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/tasks/{task_id}/account-selection",
        kind: OpKind::Write,
        args: SELECTION_ARGS,
        about: "Set or correct the account profile one task's runs are pinned to.",
    },
    // ---- Operator: scheduling, lifecycle, settlement and sessions ------------
    ToolSpec {
        name: "kontor_scheduler_plan",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/epics/{epic_id}/scheduler:plan",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("epic_id", Place::Path, ArgType::MiniProjectId, "The epic."),
        ],
        about: "What the scheduler would start now, and what blocks the rest. Commits nothing.",
    },
    ToolSpec {
        name: "kontor_scheduler_start",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/epics/{epic_id}/scheduler:start",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("epic_id", Place::Path, ArgType::MiniProjectId, "The epic."),
            IDEMPOTENCY,
            req(
                "plan_hash",
                Place::Body,
                ArgType::Text,
                "The hash of the plan being started, so a stale plan is refused.",
            ),
        ],
        about: "Start the ready batch a plan named.",
    },
    ToolSpec {
        name: "kontor_lifecycle_transition",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/epics/{epic_id}/lifecycle",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("epic_id", Place::Path, ArgType::MiniProjectId, "The epic."),
            IDEMPOTENCY,
            req(
                "action",
                Place::Body,
                ArgType::Enum(LIFECYCLE_ACTIONS),
                "The transition to apply.",
            ),
            opt(
                "task_id",
                Place::Body,
                ArgType::TaskId,
                "The task, for a task-scoped action.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read the target at.",
            ),
            req(
                "reason",
                Place::Body,
                ArgType::ExternalName,
                "Why the transition is being made.",
            ),
            opt(
                "evidence",
                Place::Body,
                ArgType::TextArray,
                "The artifacts cited, where the transition requires them.",
            ),
        ],
        about: "Block, resume, complete, reopen or close work.",
    },
    ToolSpec {
        name: "kontor_context_resolve",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/tasks/{task_id}/context:resolve",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("task_id", Place::Path, ArgType::TaskId, "The task."),
            IDEMPOTENCY,
            opt(
                "snapshot",
                Place::Body,
                ArgType::Bool,
                "Persist the resolved pack rather than previewing it. The daemon \
                 decides what the flag may do.",
            ),
        ],
        about: "Resolve the context pack a task's next run would be handed.",
    },
    ToolSpec {
        name: "kontor_gate_record",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/tasks/{task_id}/gates/{gate_id}/record",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("task_id", Place::Path, ArgType::TaskId, "The task."),
            req(
                "gate_id",
                Place::Path,
                ArgType::OpenKey,
                "The gate the pinned profile declares.",
            ),
            IDEMPOTENCY,
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The workflow revision the epic projection reported.",
            ),
            req(
                "verdict",
                Place::Body,
                ArgType::Text,
                "The verdict. `waived` is an admin decision; the rest are operator work.",
            ),
            req(
                "evaluator_role",
                Place::Body,
                ArgType::Text,
                "The role recording it, checked against the pinned profile's authority.",
            ),
            req(
                "evaluator_account",
                Place::Body,
                ArgType::AccountProfileId,
                "The account profile recording it.",
            ),
            opt(
                "evidence",
                Place::Body,
                ArgType::TextArray,
                "The artifacts cited. A pass or a waiver requires the declared ones.",
            ),
            opt(
                "reviewer_principal",
                Place::Body,
                ArgType::Text,
                "The stable authenticated principal recording it.",
            ),
        ],
        about: "Record one gate verdict. A waiver requires admin authority.",
    },
    ToolSpec {
        name: "kontor_turn_settle",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/agent-runs/{agent_run_id}/turns:settle",
        kind: OpKind::Write,
        // A *turn* is smaller than a run: this closes Kontor's bounded piece of
        // work and leaves the seat's native session live and reusable. It takes
        // no verdict and no terminal state — whether the session ever ended is
        // `kontor_runtime_settle`'s question, and only the runtime can answer it.
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "agent_run_id",
                Place::Path,
                ArgType::AgentRunId,
                "The seat's agent run. It stays open.",
            ),
            req(
                "role_slot",
                Place::Body,
                ArgType::OpenKey,
                "The role slot whose turn this is.",
            ),
            req(
                "expected_task_revision",
                Place::Body,
                ArgType::Revision,
                "The task revision the turn was taken against.",
            ),
            opt(
                "artifacts",
                Place::Body,
                ArgType::TextArray,
                "The artifacts the turn produced.",
            ),
            IDEMPOTENCY,
        ],
        about: "Settle one bounded Kontor role turn, leaving the seat live and reusable.",
    },
    ToolSpec {
        name: "kontor_runtime_settle",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/agent-runs/{agent_run_id}/runtime:settle",
        kind: OpKind::Write,
        // No body argument exists, and that is the contract rather than an
        // omission: settlement reads the runtime's own verdict. A caller-supplied
        // outcome or evidence field here would let a client decide how a run ended.
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "agent_run_id",
                Place::Path,
                ArgType::AgentRunId,
                "The run to settle.",
            ),
            IDEMPOTENCY,
        ],
        about: "Settle one run against its runtime's own verdict.",
    },
    ToolSpec {
        name: "kontor_late_handoff_attest",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/agent-runs/{agent_run_id}/handoffs:attest-late",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "agent_run_id",
                Place::Path,
                ArgType::AgentRunId,
                "The terminal run whose durable handoff is reconciled.",
            ),
            req(
                "role_slot",
                Place::Body,
                ArgType::OpenKey,
                "The run's immutable role slot.",
            ),
            req(
                "expected_task_revision",
                Place::Body,
                ArgType::Revision,
                "The task revision the handoff was produced against.",
            ),
            req(
                "binding_generation",
                Place::Body,
                ArgType::U64,
                "The immutable native binding generation.",
            ),
            req(
                "handoff_hash",
                Place::Body,
                ArgType::Text,
                "The digest carried by the durable compaction receipt.",
            ),
            req(
                "artifacts",
                Place::Body,
                ArgType::TextArray,
                "Valid artifact keys proving the bounded handoff.",
            ),
            IDEMPOTENCY,
        ],
        about: "Attest one durable handoff after runtime cancellation without reopening the run.",
    },
    ToolSpec {
        name: "kontor_seat_replace",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/agent-runs/{agent_run_id}/successors:replace",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "agent_run_id",
                Place::Path,
                ArgType::AgentRunId,
                "The runtime-terminal predecessor run.",
            ),
            req(
                "role_slot",
                Place::Body,
                ArgType::OpenKey,
                "The predecessor's immutable role slot.",
            ),
            req(
                "expected_task_revision",
                Place::Body,
                ArgType::Revision,
                "The task revision the replacement is reconciled against.",
            ),
            req(
                "binding_generation",
                Place::Body,
                ArgType::U64,
                "The predecessor's immutable binding generation.",
            ),
            IDEMPOTENCY,
        ],
        about: "Replace one runtime-terminal unusable seat with its linked successor.",
    },
    ToolSpec {
        name: "kontor_runtime_abandon",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/agent-runs/{agent_run_id}/runtime:abandon",
        kind: OpKind::Write,
        // The sibling of settlement, for the one case settlement cannot serve: a
        // run whose launch was refused holds no session, so there is no runtime
        // verdict to read and `runtime:settle` answers 404 forever. The caller
        // supplies the outcome here — deliberately, because an operator *is* the
        // evidence — and the daemon refuses the moment a seat is actually bound,
        // so this can never be used to overrule a runtime that could speak.
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "agent_run_id",
                Place::Path,
                ArgType::AgentRunId,
                "The unbound run to abandon.",
            ),
            IDEMPOTENCY,
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The run revision the abandonment was decided against.",
            ),
            req(
                "reason",
                Place::Body,
                ArgType::ExternalName,
                "Why the operator is abandoning it. Quoted in the receipt.",
            ),
        ],
        about: "Abandon one run whose launch was refused, so its task is schedulable again.",
    },
    ToolSpec {
        name: "kontor_ticket_reconcile_plan",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/tasks/{task_id}/ticket:reconcile-plan",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("task_id", Place::Path, ArgType::TaskId, "The task."),
        ],
        about: "The deterministic external-ticket plan. Commits nothing.",
    },
    ToolSpec {
        name: "kontor_ticket_reconcile_apply",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/tasks/{task_id}/ticket:reconcile-apply",
        kind: OpKind::Write,
        // The only body property is the hash of a plan the daemon computed. There
        // is deliberately no status, transition, assignee or comment argument:
        // choosing any of them is the daemon's decision from its pinned spec.
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("task_id", Place::Path, ArgType::TaskId, "The task."),
            IDEMPOTENCY,
            req(
                "projection_hash",
                Place::Body,
                ArgType::Text,
                "The hash of the plan being applied, so a stale plan is refused.",
            ),
        ],
        about: "Apply the plan a reconcile-plan produced.",
    },
    // ---- Trigger and intake (KON-MVP-22's primitives, exposed by KON-MVP-15) ----
    ToolSpec {
        name: "kontor_trigger_get",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/triggers/{trigger}/{version}",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("trigger", Place::Path, ArgType::OpenKey, "The trigger key."),
            req(
                "version",
                Place::Path,
                ArgType::SpecVersion,
                "The pinned specification revision.",
            ),
        ],
        about: "One pinned trigger revision: its filter and dedup pointers, never an event's values.",
    },
    ToolSpec {
        name: "kontor_trigger_publish",
        // Admin, and not operator: a trigger may declare a bounded auto-arm, and
        // that is the capability to start work with no human in the loop.
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/triggers:publish",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            IDEMPOTENCY,
            req(
                "spec",
                Place::Body,
                ArgType::Json,
                "The whole trigger specification document. Required keys: schema_version, id, \
                 version, source_kind, source_connection, event_schema, event_schema_version, \
                 filter, dedup, work_profile, work_profile_version, team_template, \
                 context_template, approval, limits. `approval` is either \
                 {\"kind\":\"approval_required\"} or {\"kind\":\"bounded_auto_arm\", capability, \
                 max_concurrency, budget}.",
            ),
        ],
        about: "Install one immutable trigger revision, including a bounded auto-arm capability.",
    },
    ToolSpec {
        name: "kontor_intake_submit",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/intake:submit",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            IDEMPOTENCY,
            req(
                "trigger",
                Place::Body,
                ArgType::Enum(&["threshold", "scope_boundary", "operator"]),
                "The trigger the event is evaluated under.",
            ),
            req(
                "trigger_version",
                Place::Body,
                ArgType::SpecVersion,
                "The pinned trigger revision.",
            ),
            req(
                "external_event_id",
                Place::Body,
                ArgType::ExternalId,
                "The source system's own id for the event.",
            ),
            req(
                "external_observed_at",
                Place::Body,
                ArgType::Timestamp,
                "When the source system observed it.",
            ),
            req(
                "envelope",
                Place::Body,
                ArgType::Json,
                "The canonical event envelope the trigger is matched against.",
            ),
        ],
        // There is deliberately no `decision` or `approved` argument: a matched
        // event is decided `proposed`, and approving one is the trigger's own
        // auto-arm policy talking inside `kontord`.
        about: "Submit one canonical source event for a decision under one pinned trigger.",
    },
    ToolSpec {
        name: "kontor_intake_receipt_get",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/intake/{receipt_id}",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "receipt_id",
                Place::Path,
                ArgType::IntakeReceiptId,
                "The intake decision to read.",
            ),
        ],
        about: "One recorded intake decision, replayed rather than recomputed.",
    },
    // ---- Connector specifications and ticket conflicts (KON-MVP-14's catalogue) --
    ToolSpec {
        name: "kontor_connector_field_specs_list",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/connectors/{connector}/field-specs",
        kind: OpKind::Read,
        args: CONNECTOR_ARGS,
        about: "The ticket field-spec revisions this build ships, and whether the project pinned each.",
    },
    ToolSpec {
        name: "kontor_connector_workflow_specs_list",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/connectors/{connector}/workflow-specs",
        kind: OpKind::Read,
        args: CONNECTOR_ARGS,
        about: "The external-workflow spec revisions this build ships, and whether the project pinned each.",
    },
    ToolSpec {
        name: "kontor_ticket_conflicts_list",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/tasks/{task_id}/ticket:conflicts",
        kind: OpKind::Read,
        args: TASK_SCOPE_ARGS,
        about: "The unresolved external-status conflicts one task's links hold.",
    },
    ToolSpec {
        name: "kontor_ticket_conflict_resolve",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/tasks/{task_id}/ticket:resolve-conflict",
        kind: OpKind::Write,
        // The only body property is which conflict is being resolved. There is no
        // status, assignee or comment argument: what the resolution *does* is the
        // daemon's decision from its pinned spec.
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("task_id", Place::Path, ArgType::TaskId, "The task."),
            IDEMPOTENCY,
            req(
                "conflict_id",
                Place::Body,
                ArgType::Text,
                "The conflict being resolved.",
            ),
        ],
        about: "Resolve one detected external-status conflict.",
    },
    // ---- Inbound comment mirror -------------------------------------------------
    ToolSpec {
        name: "kontor_ticket_comments_pull",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/tasks/{task_id}/ticket:pull-comments",
        kind: OpKind::Write,
        // No body: a pull reads the external system and mirrors what it finds. A
        // caller-supplied comment here would make this a push wearing a pull's name.
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("task_id", Place::Path, ArgType::TaskId, "The task."),
            IDEMPOTENCY,
        ],
        about: "Mirror new inbound comments for one task's links. Never sends one.",
    },
    ToolSpec {
        name: "kontor_ticket_comments_list",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/tasks/{task_id}/ticket:comments",
        kind: OpKind::Read,
        args: TASK_SCOPE_ARGS,
        about: "The mirrored inbound comments for one task: authors and digests, never bodies.",
    },
    // ---- Claim-self -------------------------------------------------------------
    ToolSpec {
        name: "kontor_ticket_claim",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/tasks/{task_id}/ticket:claim",
        kind: OpKind::Write,
        // No assignee argument exists, and that is the contract rather than an
        // omission: a claim can name only the principal Kontor authenticates as.
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("task_id", Place::Path, ArgType::TaskId, "The task."),
            IDEMPOTENCY,
        ],
        about: "Record Kontor's intent to hold one task's tickets for its own principal.",
    },
    ToolSpec {
        name: "kontor_context_policy_preview",
        tier: CallerTier::Observer,
        method: Method::Post,
        path: "/v1/context-policy/preview",
        kind: OpKind::Read,
        args: &[
            opt(
                "run_override",
                Place::Body,
                ArgType::Json,
                "The policy an authorized run override would carry.",
            ),
            opt(
                "role_slot",
                Place::Body,
                ArgType::Json,
                "The policy the team template's role slot declares.",
            ),
            opt(
                "work_profile",
                Place::Body,
                ArgType::Json,
                "The work profile's declared default.",
            ),
            opt(
                "role_seed",
                Place::Body,
                ArgType::Json,
                "The deployment's seed for this role.",
            ),
            opt(
                "context_policy_capable",
                Place::Body,
                ArgType::Bool,
                "Whether the runtime being previewed can configure a context window.",
            ),
            opt(
                "safe_ceiling",
                Place::Body,
                ArgType::U64,
                "The largest trigger that runtime attests.",
            ),
            opt(
                "minimum_trigger",
                Place::Body,
                ArgType::U64,
                "The smallest trigger that runtime can take.",
            ),
        ],
        about: "Resolve a context-window policy from explicit inputs, changing nothing.",
    },
    ToolSpec {
        name: "kontor_session_compact",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/sessions/{agent_run_id}/compact",
        kind: OpKind::Write,
        args: &[
            req(
                "agent_run_id",
                Place::Path,
                ArgType::AgentRunId,
                "The run whose seat is compacted.",
            ),
            IDEMPOTENCY,
            req(
                "trigger",
                Place::Body,
                ArgType::Text,
                "threshold, scope_boundary or operator. A finished turn is not a trigger.",
            ),
            req(
                "context_pack_hash",
                Place::Body,
                ArgType::Text,
                "The immutable Context Pack the run was frozen against.",
            ),
            opt(
                "handoff_hash",
                Place::Body,
                ArgType::Text,
                "The sealed durable handoff. Required at a scope boundary or on operator request.",
            ),
            req(
                "active_tool",
                Place::Body,
                ArgType::Bool,
                "Whether a tool action is in flight right now.",
            ),
            req(
                "unresolved_permission",
                Place::Body,
                ArgType::Bool,
                "Whether a permission request is still unanswered.",
            ),
        ],
        about: "Compact one run's session context in place, at a proven safe point.",
    },
    ToolSpec {
        name: "kontor_session_message_send",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/sessions/{agent_run_id}/messages",
        kind: OpKind::Write,
        args: &[
            req(
                "agent_run_id",
                Place::Path,
                ArgType::AgentRunId,
                "The run whose session receives it.",
            ),
            IDEMPOTENCY,
            req("body", Place::Body, ArgType::Text, "The message text."),
        ],
        about: "Send one follow-up message into a run's session.",
    },
    ToolSpec {
        name: "kontor_session_permission_respond",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/sessions/{agent_run_id}/permissions/{request_id}",
        kind: OpKind::Write,
        args: &[
            req(
                "agent_run_id",
                Place::Path,
                ArgType::AgentRunId,
                "The run whose session raised it.",
            ),
            req(
                "request_id",
                Place::Path,
                ArgType::Text,
                "The runtime's own identifier for the request.",
            ),
            IDEMPOTENCY,
            req(
                "decision",
                Place::Body,
                ArgType::Enum(PERMISSION_DECISIONS),
                "Whether the action may proceed.",
            ),
        ],
        about: "Answer one permission request raised inside a session.",
    },
    // ---- Role-slot accounting ---------------------------------------------------
    ToolSpec {
        name: "kontor_role_slot_waive",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/team-runs/{team_run_id}/role-slots/{role_slot_id}/waivers",
        kind: OpKind::Write,
        // Admin for the same reason a gate waiver is: it discharges an obligation
        // the frozen template imposed. Unlike `kontor_gate_record` the authority
        // does not vary with the arguments — there is no second disposition to
        // select, so no argument could lower it.
        //
        // No agent-run, binding or session argument exists, and that is the
        // contract rather than an omission: a slot that was never bound has no run
        // to name, and a caller that could name one could invent the very
        // settlement the waiver exists to avoid fabricating.
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "team_run_id",
                Place::Path,
                ArgType::TeamRunId,
                "The team run whose slot is being excused.",
            ),
            req(
                "role_slot_id",
                Place::Path,
                ArgType::OpenKey,
                "The declared slot, as the frozen template spells it.",
            ),
            IDEMPOTENCY,
            req(
                "expected_team_revision",
                Place::Body,
                ArgType::Revision,
                "The team revision the caller read the run at.",
            ),
            req(
                "authorized_by_role",
                Place::Body,
                ArgType::OpenKey,
                "The role the waiver is attributed to, checked against the frozen slot's own \
                 policy. Never a person, and never the caller.",
            ),
            req(
                "evidence",
                Place::Body,
                ArgType::TextArray,
                "Every evidence reference the frozen policy demands, at least.",
            ),
        ],
        about: "Waive one declared role slot that has never held a native binding, under the \
                frozen template's authority and evidence policy.",
    },
    ToolSpec {
        name: "kontor_memory_search",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/memory",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            opt(
                "q",
                Place::Query,
                ArgType::Text,
                "FTS5 query; omit to list approved current memory.",
            ),
            opt("limit", Place::Query, ArgType::U32, "Maximum results."),
        ],
        about: "Search or list current approved project memory.",
    },
    ToolSpec {
        name: "kontor_memory_history",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/memory/{item_id}/history",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "item_id",
                Place::Path,
                ArgType::OpenKey,
                "The memory aggregate.",
            ),
        ],
        about: "Read one memory aggregate's immutable revision history.",
    },
    ToolSpec {
        name: "kontor_memory_propose",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/memory/revisions:propose",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            IDEMPOTENCY,
            req(
                "item_id",
                Place::Body,
                ArgType::OpenKey,
                "The memory aggregate.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::U64,
                "The aggregate revision read by the caller.",
            ),
            req(
                "document",
                Place::Body,
                ArgType::Json,
                "The canonical memory document.",
            ),
            req(
                "provenance",
                Place::Body,
                ArgType::Json,
                "Source provenance.",
            ),
            req(
                "proposed_by",
                Place::Body,
                ArgType::ExternalName,
                "The proposer.",
            ),
        ],
        about: "Propose an immutable memory revision; never approves it.",
    },
    ToolSpec {
        name: "kontor_memory_approve",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/memory/revisions/{revision_id}/approval",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "revision_id",
                Place::Path,
                ArgType::OpenKey,
                "The proposed revision.",
            ),
            IDEMPOTENCY,
            req(
                "item_id",
                Place::Body,
                ArgType::OpenKey,
                "The memory aggregate.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The aggregate revision read by the approver.",
            ),
            req(
                "approved_by",
                Place::Body,
                ArgType::ExternalName,
                "The approver.",
            ),
        ],
        about: "Approve one proposed revision and atomically make it current.",
    },
    ToolSpec {
        name: "kontor_memory_tombstone",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/memory/{item_id}/tombstone",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "item_id",
                Place::Path,
                ArgType::OpenKey,
                "The memory aggregate.",
            ),
            IDEMPOTENCY,
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The aggregate revision read by the caller.",
            ),
            req(
                "by",
                Place::Body,
                ArgType::ExternalName,
                "Who tombstoned it.",
            ),
            req(
                "reason",
                Place::Body,
                ArgType::ExternalName,
                "Why it was tombstoned.",
            ),
        ],
        about: "Exclude an aggregate while retaining its history.",
    },
    ToolSpec {
        name: "kontor_memory_purge",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/memory/{item_id}/purge",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "item_id",
                Place::Path,
                ArgType::OpenKey,
                "The memory aggregate.",
            ),
            IDEMPOTENCY,
            req(
                "by",
                Place::Body,
                ArgType::ExternalName,
                "Who authorized destructive purge.",
            ),
        ],
        about: "Purge revision payloads while retaining a hashed purge receipt.",
    },
    ToolSpec {
        name: "kontor_memory_ingest_preview",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/memory/import:preview",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The destination project.",
            ),
            req(
                "schema_version",
                Place::Body,
                ArgType::U32,
                "Export schema.",
            ),
            req("source", Place::Body, ArgType::OpenKey, "Source authority."),
            req(
                "entries",
                Place::Body,
                ArgType::Json,
                "Legacy current values.",
            ),
            req(
                "snapshot_hash",
                Place::Body,
                ArgType::Text,
                "Final export hash.",
            ),
        ],
        about: "Validate a final AgentsRoom export without writing.",
    },
    ToolSpec {
        name: "kontor_memory_ingest_apply",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/memory/import:apply",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The destination project.",
            ),
            IDEMPOTENCY,
            req(
                "schema_version",
                Place::Body,
                ArgType::U32,
                "Export schema.",
            ),
            req("source", Place::Body, ArgType::OpenKey, "Source authority."),
            req(
                "entries",
                Place::Body,
                ArgType::Json,
                "Legacy current values.",
            ),
            req(
                "snapshot_hash",
                Place::Body,
                ArgType::Text,
                "Final export hash.",
            ),
        ],
        about: "Transactionally and idempotently import the frozen final export.",
    },
    ToolSpec {
        name: "kontor_memory_cutover_freeze",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/memory/cutover:freeze",
        kind: OpKind::Write,
        args: &[IDEMPOTENCY],
        about: "Freeze AgentsRoom memory writes before importing the final export.",
    },
    ToolSpec {
        name: "kontor_memory_cutover_switch",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/memory/cutover:switch",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The destination project.",
            ),
            IDEMPOTENCY,
            req(
                "source",
                Place::Body,
                ArgType::OpenKey,
                "The legacy authority.",
            ),
            req(
                "snapshot_hash",
                Place::Body,
                ArgType::Text,
                "The verified final export hash.",
            ),
        ],
        about: "Irreversibly switch memory authority to Kontor after verification.",
    },
    // ---- The topology vocabulary: kinds, roles and what every code means ----
    //
    // Draft and validate are POSTs that commit nothing, so they are reads and
    // take no key. Publication is the only write here, and it names both the
    // hash it was judged at and the revision it expects.
    ToolSpec {
        name: "kontor_topology_spec_draft",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/topology-specs:draft",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            opt(
                "base",
                Place::Body,
                ArgType::Json,
                "An `{id, version}` revision to start from.",
            ),
            req(
                "name",
                Place::Body,
                ArgType::ExternalName,
                "Human name for the specification.",
            ),
            req(
                "root_kind",
                Place::Body,
                ArgType::OpenKey,
                "The unique logical root kind.",
            ),
            req(
                "node_kinds",
                Place::Body,
                ArgType::Json,
                "The data-defined node-kind vocabulary, in declaration order.",
            ),
            opt(
                "historical_codes",
                Place::Body,
                ArgType::Json,
                "Codes this vocabulary explains but never declares as usable.",
            ),
        ],
        about: "Build one complete topology-specification candidate. Persists nothing.",
    },
    ToolSpec {
        name: "kontor_topology_spec_validate",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/topology-specs:validate",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "candidate",
                Place::Body,
                ArgType::Json,
                "One complete candidate document.",
            ),
        ],
        about: "Judge one candidate and return its ordered violations. Persists nothing.",
    },
    ToolSpec {
        name: "kontor_topology_spec_publish",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/topology-specs:publish",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            IDEMPOTENCY,
            req(
                "candidate",
                Place::Body,
                ArgType::Json,
                "The complete candidate to publish.",
            ),
            req(
                "validation_hash",
                Place::Body,
                ArgType::Text,
                "The hash the validation answered with.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The project revision the caller read.",
            ),
        ],
        about: "Publish one revalidated candidate as an immutable revision.",
    },
    ToolSpec {
        name: "kontor_topology_spec_get",
        tier: CallerTier::Admin,
        method: Method::Get,
        path: "/v1/projects/{project_id}/topology-specs/{spec_id}/{version}",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "spec_id",
                Place::Path,
                ArgType::TopologySpecId,
                "The specification identity.",
            ),
            req(
                "version",
                Place::Path,
                ArgType::SpecVersion,
                "The published revision.",
            ),
        ],
        about: "One exact immutable topology-specification document and its canonical hash.",
    },
    ToolSpec {
        name: "kontor_role_catalog_get",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/catalog/role-catalogs/{catalog_id}/{version}",
        kind: OpKind::Read,
        args: &[
            req(
                "catalog_id",
                Place::Path,
                ArgType::RoleCatalogId,
                "The catalog identity.",
            ),
            req(
                "version",
                Place::Path,
                ArgType::SpecVersion,
                "The revision.",
            ),
        ],
        about: "One whole role-catalog revision, in its declared order.",
    },
    ToolSpec {
        name: "kontor_role_get",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/catalog/role-catalogs/{catalog_id}/{version}/roles/{role_code}",
        kind: OpKind::Read,
        args: &[
            req(
                "catalog_id",
                Place::Path,
                ArgType::RoleCatalogId,
                "The catalog identity.",
            ),
            req(
                "version",
                Place::Path,
                ArgType::SpecVersion,
                "The revision.",
            ),
            req(
                "role_code",
                Place::Path,
                ArgType::OpenKey,
                "The stable role code.",
            ),
        ],
        about: "One resolved catalog entry. An unknown revision or code is refused, never guessed.",
    },
    ToolSpec {
        name: "kontor_code_help_get",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/epics/{epic_id}/code-help",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "epic_id",
                Place::Path,
                ArgType::MiniProjectId,
                "The epic whose pinned revisions are read.",
            ),
        ],
        about: "Every controlled code one epic's pinned revisions define, sorted and server-owned.",
    },
    // ---- Semantic topology: a scope is named, never a native shape ---------
    ToolSpec {
        name: "kontor_topology_inspect",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/topology:inspect",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            opt(
                "epic_id",
                Place::Query,
                ArgType::MiniProjectId,
                "Narrow to one epic's pinned subgraph.",
            ),
        ],
        about: "The stored authoritative topology, with each node's derived and observed shape.",
    },
    ToolSpec {
        name: "kontor_topology_drift",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/topology:drift",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            IDEMPOTENCY,
            req(
                "target",
                Place::Body,
                ArgType::Json,
                "The semantic scope: a project root, Quick session, epic, epic control, ticket, \
                 Advisor or Committee consultation.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read.",
            ),
        ],
        about: "Read the exact native identities back and record what was observed.",
    },
    ToolSpec {
        name: "kontor_topology_ensure",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/topology:ensure",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            IDEMPOTENCY,
            req(
                "target",
                Place::Body,
                ArgType::Json,
                "The semantic scope: a project root, Quick session, epic, epic control, ticket, \
                 Advisor or Committee consultation.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read.",
            ),
        ],
        about: "Ensure the logical nodes one semantic scope needs. No native effect.",
    },
    ToolSpec {
        name: "kontor_topology_materialize",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/topology:materialize",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            IDEMPOTENCY,
            req(
                "target",
                Place::Body,
                ArgType::Json,
                "The semantic scope: a project root, Quick session, epic, epic control, ticket, \
                 Advisor or Committee consultation.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read.",
            ),
        ],
        about: "Materialize or reconcile an ensured scope through the admission path.",
    },
    ToolSpec {
        name: "kontor_topology_retire",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/topology/nodes/{topology_node_id}/retire",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "topology_node_id",
                Place::Path,
                ArgType::TopologyNodeId,
                "The node a projection returned.",
            ),
            IDEMPOTENCY,
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read.",
            ),
            req(
                "reason",
                Place::Body,
                ArgType::ExternalName,
                "Why the node is leaving service. Recorded, never interpreted.",
            ),
        ],
        about: "Retire one already-returned node after child and seat policy checks.",
    },
    ToolSpec {
        name: "kontor_topology_archive",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/topology/nodes/{topology_node_id}/archive",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "topology_node_id",
                Place::Path,
                ArgType::TopologyNodeId,
                "The node a projection returned.",
            ),
            IDEMPOTENCY,
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read.",
            ),
            req(
                "reason",
                Place::Body,
                ArgType::ExternalName,
                "Why the node is leaving service. Recorded, never interpreted.",
            ),
        ],
        about: "Archive one already-retired node after exact readback.",
    },
    ToolSpec {
        name: "kontor_topology_upgrade_preview",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/epics/{epic_id}/topology:upgrade-preview",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "epic_id",
                Place::Path,
                ArgType::MiniProjectId,
                "The epic whose pin would move.",
            ),
            req(
                "target_spec",
                Place::Body,
                ArgType::Json,
                "An `{id, version}` published revision to diff against.",
            ),
        ],
        about: "What moving one epic's pinned specification would do. Commits nothing.",
    },
    ToolSpec {
        name: "kontor_topology_upgrade_apply",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/epics/{epic_id}/topology:upgrade-apply",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "epic_id",
                Place::Path,
                ArgType::MiniProjectId,
                "The epic whose pin moves.",
            ),
            IDEMPOTENCY,
            req(
                "preview_hash",
                Place::Body,
                ArgType::Text,
                "The hash the preview answered with.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read.",
            ),
        ],
        about: "Apply the named upgrade preview and return the new immutable pin.",
    },
    ToolSpec {
        name: "kontor_container_retitle_preview",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/topology/nodes/{topology_node_id}/container:retitle-preview",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "topology_node_id",
                Place::Path,
                ArgType::TopologyNodeId,
                "The node whose container it is.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The project revision the caller read.",
            ),
        ],
        // No title argument, and there will never be one: the title is derived
        // from the node's pinned topology and the runtime plane's typed scope.
        about: "What repairing one bound container's title would do. Commits nothing.",
    },
    ToolSpec {
        name: "kontor_container_retitle_apply",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/topology/nodes/{topology_node_id}/container:retitle-apply",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "topology_node_id",
                Place::Path,
                ArgType::TopologyNodeId,
                "The node whose container it is.",
            ),
            IDEMPOTENCY,
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The project revision the caller read.",
            ),
        ],
        about: "Repair one bound container's title, idempotently, and read it back.",
    },
    // ---- Native capacity: evidence is collected, never asserted ------------
    ToolSpec {
        name: "kontor_capacity_config_get",
        tier: CallerTier::Admin,
        method: Method::Get,
        path: "/v1/capacity/configuration",
        kind: OpKind::Read,
        args: &[],
        about: "The current immutable capacity configuration revision and its effective values.",
    },
    ToolSpec {
        name: "kontor_capacity_config_preview",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/capacity/configuration:preview",
        kind: OpKind::Read,
        args: &[
            req(
                "ceilings",
                Place::Body,
                ArgType::Json,
                "The complete set of ceilings to stand up.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The configuration revision the caller read.",
            ),
        ],
        about: "What a full capacity replacement would clamp. Commits nothing.",
    },
    ToolSpec {
        name: "kontor_capacity_config_apply",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/capacity/configuration:apply",
        kind: OpKind::Write,
        args: &[
            IDEMPOTENCY,
            req(
                "ceilings",
                Place::Body,
                ArgType::Json,
                "The complete set of ceilings to stand up.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The configuration revision the caller read.",
            ),
        ],
        about: "Apply a full capacity replacement under the expected revision.",
    },
    ToolSpec {
        name: "kontor_capacity_get",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/capacity",
        kind: OpKind::Read,
        args: &[req(
            "project_id",
            Place::Path,
            ArgType::ProjectId,
            "The owning project.",
        )],
        about: "One project's admission picture: availability, active TeamRuns and the adaptive window.",
    },
    ToolSpec {
        name: "kontor_capacity_refresh",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/capacity:refresh",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            IDEMPOTENCY,
            opt(
                "account_profile_ids",
                Place::Body,
                ArgType::TextArray,
                "Configured account profiles to collect. Empty means every one.",
            ),
        ],
        about: "Run the configured native collectors and fold what they report.",
    },
    ToolSpec {
        name: "kontor_capacity_observation_get",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/capacity/observations/{observation_id}",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "observation_id",
                Place::Path,
                ArgType::CapacityObservationId,
                "The raw observation.",
            ),
        ],
        about: "One redacted raw observation and the availability derived from it.",
    },
    ToolSpec {
        name: "kontor_capacity_override",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/provider-account-profiles/{account_profile_id}/availability:override",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "account_profile_id",
                Place::Path,
                ArgType::AccountProfileId,
                "The account profile.",
            ),
            IDEMPOTENCY,
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The account revision the caller read.",
            ),
            req(
                "available",
                Place::Body,
                ArgType::Bool,
                "What the operator asserts.",
            ),
            req(
                "reason",
                Place::Body,
                ArgType::ExternalName,
                "Why. Recorded, never interpreted.",
            ),
            opt(
                "expires_at",
                Place::Body,
                ArgType::Timestamp,
                "When the override lapses on its own.",
            ),
        ],
        about: "Stand an operator judgement beside an account's raw evidence, never over it.",
    },
    ToolSpec {
        name: "kontor_seat_attention",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/seat-bindings/{seat_binding_id}/attention",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "seat_binding_id",
                Place::Path,
                ArgType::SeatBindingId,
                "The exact binding a projection returned.",
            ),
            IDEMPOTENCY,
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The binding revision the caller read.",
            ),
            req(
                "reason",
                Place::Body,
                ArgType::ExternalName,
                "Why the seat is being looked at.",
            ),
        ],
        about: "Observe one exact bound seat and record typed attention evidence.",
    },
    ToolSpec {
        name: "kontor_seat_retire",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/seat-bindings/{seat_binding_id}/retire",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "seat_binding_id",
                Place::Path,
                ArgType::SeatBindingId,
                "The exact binding a projection returned.",
            ),
            IDEMPOTENCY,
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The binding revision the caller read.",
            ),
            req(
                "reason",
                Place::Body,
                ArgType::ExternalName,
                "Why the seat is being released.",
            ),
        ],
        about: "Retire and release one exact binding after supported readback; never a scan by name.",
    },
    // ---- Successor-ticket contracts: fixed now, answered when composed ----
    ToolSpec {
        name: "kontor_core_team_get",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/core-team",
        kind: OpKind::Read,
        args: &[req(
            "project_id",
            Place::Path,
            ArgType::ProjectId,
            "The owning project.",
        )],
        about: "One project's Core Team and the seats filling it.",
    },
    ToolSpec {
        name: "kontor_core_team_preview",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/core-team:preview",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "seats",
                Place::Body,
                ArgType::Json,
                "The roles the Core Team should seat, in order.",
            ),
        ],
        about: "What a Core Team change would do. Commits nothing.",
    },
    ToolSpec {
        name: "kontor_core_team_apply",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/core-team:apply",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            IDEMPOTENCY,
            req(
                "seats",
                Place::Body,
                ArgType::Json,
                "The roles the Core Team should seat, in order.",
            ),
            req(
                "preview_hash",
                Place::Body,
                ArgType::Text,
                "The hash the preview answered with.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read.",
            ),
        ],
        about: "Apply a named Core Team preview.",
    },
    ToolSpec {
        name: "kontor_core_team_materialize",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/epics/{epic_id}/core-team/seats:materialize",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("epic_id", Place::Path, ArgType::MiniProjectId, "The epic."),
            IDEMPOTENCY,
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read.",
            ),
        ],
        about: "Materialize the Core Team's seats for one epic.",
    },
    ToolSpec {
        name: "kontor_quick_roles_list",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/quick-roles",
        kind: OpKind::Read,
        args: &[req(
            "project_id",
            Place::Path,
            ArgType::ProjectId,
            "The owning project.",
        )],
        about: "The roles a Quick session may be opened against.",
    },
    ToolSpec {
        name: "kontor_quick_session_ensure",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/quick-sessions:ensure",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            IDEMPOTENCY,
            req(
                "role",
                Place::Body,
                ArgType::Json,
                "An `{catalog_revision, role_code}` selection.",
            ),
            req(
                "purpose",
                Place::Body,
                ArgType::ExternalName,
                "What the session is for.",
            ),
        ],
        about: "Open a Quick session, or return the one this key opened.",
    },
    ToolSpec {
        name: "kontor_promotion_preview",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/quick-sessions/{quick_session_id}/promotion:preview",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "quick_session_id",
                Place::Path,
                ArgType::QuickSessionId,
                "The Quick session.",
            ),
        ],
        about: "What promoting one Quick session would produce.",
    },
    ToolSpec {
        name: "kontor_promotion_apply",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/quick-sessions/{quick_session_id}/promotion:apply",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "quick_session_id",
                Place::Path,
                ArgType::QuickSessionId,
                "The Quick session.",
            ),
            IDEMPOTENCY,
            req(
                "preview_hash",
                Place::Body,
                ArgType::Text,
                "The hash the preview answered with.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read.",
            ),
        ],
        about: "Apply a named promotion preview.",
    },
    ToolSpec {
        name: "kontor_roster_upgrade_preview",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/epics/{epic_id}/roster:upgrade-preview",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("epic_id", Place::Path, ArgType::MiniProjectId, "The epic."),
            req(
                "target",
                Place::Body,
                ArgType::Json,
                "An `{id, version}` published revision to diff against.",
            ),
        ],
        about: "What moving one epic's pinned roster would do.",
    },
    ToolSpec {
        name: "kontor_roster_upgrade_apply",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/epics/{epic_id}/roster:upgrade-apply",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("epic_id", Place::Path, ArgType::MiniProjectId, "The epic."),
            IDEMPOTENCY,
            req(
                "preview_hash",
                Place::Body,
                ArgType::Text,
                "The hash the preview answered with.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read.",
            ),
        ],
        about: "Apply a named roster upgrade preview.",
    },
    ToolSpec {
        name: "kontor_advisor_profiles_list",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/advisor-profiles",
        kind: OpKind::Read,
        args: &[req(
            "project_id",
            Place::Path,
            ArgType::ProjectId,
            "The owning project.",
        )],
        about: "Every published Advisor profile revision.",
    },
    ToolSpec {
        name: "kontor_advisor_profile_preview",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/advisor-profiles:preview",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "definition",
                Place::Body,
                ArgType::Json,
                "The complete candidate definition.",
            ),
        ],
        about: "Judge one Advisor profile definition. Commits nothing.",
    },
    ToolSpec {
        name: "kontor_advisor_profile_apply",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/advisor-profiles:apply",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            IDEMPOTENCY,
            req(
                "definition",
                Place::Body,
                ArgType::Json,
                "The complete candidate definition.",
            ),
            req(
                "preview_hash",
                Place::Body,
                ArgType::Text,
                "The hash the preview answered with.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read.",
            ),
        ],
        about: "Publish one Advisor profile revision.",
    },
    ToolSpec {
        name: "kontor_advisor_run_invoke",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/epics/{epic_id}/advisor-runs:invoke",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("epic_id", Place::Path, ArgType::MiniProjectId, "The epic."),
            IDEMPOTENCY,
            req(
                "profile",
                Place::Body,
                ArgType::Json,
                "An `{id, version}` profile revision.",
            ),
            req(
                "question",
                Place::Body,
                ArgType::Text,
                "What is being asked.",
            ),
            req(
                "caller_seat_binding_id",
                Place::Body,
                ArgType::SeatBindingId,
                "The exact active epic seat invoking the consultation.",
            ),
            opt(
                "task_id",
                Place::Body,
                ArgType::TaskId,
                "An optional ticket in the addressed epic.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read.",
            ),
        ],
        about: "Invoke one Advisor consultation against an epic.",
    },
    ToolSpec {
        name: "kontor_advisor_run_settle",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/advisor-runs/{advisor_run_id}/settle",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "advisor_run_id",
                Place::Path,
                ArgType::AdvisorRunId,
                "The consultation.",
            ),
            IDEMPOTENCY,
            opt(
                "seat_binding_id",
                Place::Body,
                ArgType::SeatBindingId,
                "Omit for normal MCP use. A seat-scoped runtime credential supplies its own exact Advisor SeatBinding.",
            ),
            opt(
                "output",
                Place::Body,
                ArgType::Text,
                "The immutable Advisor output. Only the Advisor's seat-scoped runtime credential may submit it.",
            ),
            opt(
                "disposition",
                Place::Body,
                ArgType::Enum(&["accepted", "partially_accepted", "rejected", "superseded"]),
                "What the Realm operator decided about already-recorded advice.",
            ),
            opt(
                "rationale",
                Place::Body,
                ArgType::Text,
                "The Realm operator's disposition rationale.",
            ),
            opt(
                "receipt_ids",
                Place::Body,
                ArgType::TextArray,
                "Separately-authorized receipts cited by the disposition.",
            ),
            opt(
                "recommendation",
                Place::Body,
                ArgType::Text,
                "Committee-only; Advisor settlement refuses it.",
            ),
            opt(
                "tried_path",
                Place::Body,
                ArgType::Text,
                "Committee-only; Advisor settlement refuses it.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read.",
            ),
        ],
        about: "Submit seat-authored Advisor output or disposition already-recorded advice; the credential determines which step is permitted.",
    },
    ToolSpec {
        name: "kontor_advisor_run_get",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/advisor-runs/{advisor_run_id}",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "advisor_run_id",
                Place::Path,
                ArgType::AdvisorRunId,
                "The consultation.",
            ),
        ],
        about: "Read one Advisor consultation and its immutable result.",
    },
    ToolSpec {
        name: "kontor_committee_templates_list",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/committee-templates",
        kind: OpKind::Read,
        args: &[req(
            "project_id",
            Place::Path,
            ArgType::ProjectId,
            "The owning project.",
        )],
        about: "Every published Committee template revision.",
    },
    ToolSpec {
        name: "kontor_committee_template_preview",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/committee-templates:preview",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "definition",
                Place::Body,
                ArgType::Json,
                "The complete candidate definition.",
            ),
        ],
        about: "Judge one Committee template definition. Commits nothing.",
    },
    ToolSpec {
        name: "kontor_committee_template_apply",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/committee-templates:apply",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            IDEMPOTENCY,
            req(
                "definition",
                Place::Body,
                ArgType::Json,
                "The complete candidate definition.",
            ),
            req(
                "preview_hash",
                Place::Body,
                ArgType::Text,
                "The hash the preview answered with.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read.",
            ),
        ],
        about: "Publish one Committee template revision.",
    },
    ToolSpec {
        name: "kontor_committee_run_invoke",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/epics/{epic_id}/committee-runs:invoke",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("epic_id", Place::Path, ArgType::MiniProjectId, "The epic."),
            IDEMPOTENCY,
            req(
                "profile",
                Place::Body,
                ArgType::Json,
                "An `{id, version}` template revision.",
            ),
            req(
                "question",
                Place::Body,
                ArgType::Text,
                "What is being asked.",
            ),
            req(
                "caller_seat_binding_id",
                Place::Body,
                ArgType::SeatBindingId,
                "The exact active epic seat invoking the consultation.",
            ),
            opt(
                "task_id",
                Place::Body,
                ArgType::TaskId,
                "An optional ticket in the addressed epic.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read.",
            ),
        ],
        about: "Invoke one Committee consultation against an epic.",
    },
    ToolSpec {
        name: "kontor_committee_findings_record",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/committee-runs/{committee_run_id}/findings:record",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "committee_run_id",
                Place::Path,
                ArgType::CommitteeRunId,
                "The consultation.",
            ),
            IDEMPOTENCY,
            req("round", Place::Body, ArgType::U32, "The one-based round."),
            req(
                "verdict",
                Place::Body,
                ArgType::Enum(&["compliant", "non_compliant"]),
                "The typed reviewer or Judge conclusion.",
            ),
            req(
                "evidence_complete",
                Place::Body,
                ArgType::Bool,
                "Whether every required evidence reference is present.",
            ),
            req(
                "rationale",
                Place::Body,
                ArgType::Text,
                "The bounded finding rationale.",
            ),
            opt(
                "evidence_refs",
                Place::Body,
                ArgType::TextArray,
                "References to already-authoritative evidence.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read.",
            ),
        ],
        about: "Record one round of Committee findings.",
    },
    ToolSpec {
        name: "kontor_committee_run_get",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/committee-runs/{committee_run_id}",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "committee_run_id",
                Place::Path,
                ArgType::CommitteeRunId,
                "The consultation.",
            ),
        ],
        about: "Read one Committee run, its remediation, findings, and result.",
    },
    ToolSpec {
        name: "kontor_committee_run_settle",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/committee-runs/{committee_run_id}/settle",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "committee_run_id",
                Place::Path,
                ArgType::CommitteeRunId,
                "The consultation.",
            ),
            IDEMPOTENCY,
            opt(
                "seat_binding_id",
                Place::Body,
                ArgType::SeatBindingId,
                "Advisor-only; Committee settlement refuses it.",
            ),
            opt(
                "output",
                Place::Body,
                ArgType::Text,
                "Advisor-only; Committee settlement refuses it.",
            ),
            opt(
                "disposition",
                Place::Body,
                ArgType::Enum(&["accepted", "partially_accepted", "rejected", "superseded"]),
                "Advisor-only; Committee settlement refuses it.",
            ),
            opt(
                "rationale",
                Place::Body,
                ArgType::Text,
                "Advisor-only; Committee settlement refuses it.",
            ),
            opt(
                "receipt_ids",
                Place::Body,
                ArgType::TextArray,
                "Advisor-only; Committee settlement refuses it.",
            ),
            opt(
                "recommendation",
                Place::Body,
                ArgType::Text,
                "LSA recommendation required after a non-compliant verdict.",
            ),
            opt(
                "tried_path",
                Place::Body,
                ArgType::Text,
                "Exact remediation path tried before re-review or escalation.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read.",
            ),
        ],
        about: "Settle one Committee consultation.",
    },
    ToolSpec {
        name: "kontor_completion_profiles_list",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/completion-profiles",
        kind: OpKind::Read,
        args: &[req(
            "project_id",
            Place::Path,
            ArgType::ProjectId,
            "The owning project.",
        )],
        about: "Every published Completion profile revision.",
    },
    ToolSpec {
        name: "kontor_completion_profile_preview",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/completion-profiles:preview",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req(
                "definition",
                Place::Body,
                ArgType::Json,
                "The complete candidate definition.",
            ),
        ],
        about: "Judge one Completion profile definition. Commits nothing.",
    },
    ToolSpec {
        name: "kontor_completion_profile_apply",
        tier: CallerTier::Admin,
        method: Method::Post,
        path: "/v1/projects/{project_id}/completion-profiles:apply",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            IDEMPOTENCY,
            req(
                "definition",
                Place::Body,
                ArgType::Json,
                "The complete candidate definition.",
            ),
            req(
                "preview_hash",
                Place::Body,
                ArgType::Text,
                "The hash the preview answered with.",
            ),
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read.",
            ),
        ],
        about: "Publish one Completion profile revision.",
    },
    ToolSpec {
        name: "kontor_completion_get",
        tier: CallerTier::Observer,
        method: Method::Get,
        path: "/v1/projects/{project_id}/epics/{epic_id}/completion",
        kind: OpKind::Read,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("epic_id", Place::Path, ArgType::MiniProjectId, "The epic."),
        ],
        about: "One epic's completion state and what is still blocking it.",
    },
    ToolSpec {
        name: "kontor_completion_advance",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/epics/{epic_id}/completion:advance",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("epic_id", Place::Path, ArgType::MiniProjectId, "The epic."),
            IDEMPOTENCY,
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read.",
            ),
        ],
        about: "Advance one epic's completion.",
    },
    ToolSpec {
        name: "kontor_completion_remediate",
        tier: CallerTier::Operator,
        method: Method::Post,
        path: "/v1/projects/{project_id}/epics/{epic_id}/completion:remediate",
        kind: OpKind::Write,
        args: &[
            req(
                "project_id",
                Place::Path,
                ArgType::ProjectId,
                "The owning project.",
            ),
            req("epic_id", Place::Path, ArgType::MiniProjectId, "The epic."),
            IDEMPOTENCY,
            req(
                "expected_revision",
                Place::Body,
                ArgType::Revision,
                "The revision the caller read.",
            ),
            req(
                "action",
                Place::Body,
                ArgType::Json,
                "Which remediation authority is acting: an `lsa_proposal` naming the \
                 failed round, its evidence and the bounded correction, or a `tpm_route` \
                 naming the round and the routed task set.",
            ),
        ],
        about: "Record one epic's LSA remediation proposal or TPM next-round route.",
    },
];

/// The two connector reads are scoped the same way, so they share one list.
static CONNECTOR_ARGS: &[ArgSpec] = &[
    req(
        "project_id",
        Place::Path,
        ArgType::ProjectId,
        "The owning project.",
    ),
    req(
        "connector",
        Place::Path,
        ArgType::OpenKey,
        "The connector implementation key.",
    ),
];

/// The task-scoped reads that take nothing but their scope.
static TASK_SCOPE_ARGS: &[ArgSpec] = &[
    req(
        "project_id",
        Place::Path,
        ArgType::ProjectId,
        "The owning project.",
    ),
    req("task_id", Place::Path, ArgType::TaskId, "The task."),
];

/// The three selection routes take the same request, so they share one argument
/// list rather than three copies that could drift apart.
static SELECTION_ARGS: &[ArgSpec] = &[
    req(
        "project_id",
        Place::Path,
        ArgType::ProjectId,
        "The owning project.",
    ),
    req("task_id", Place::Path, ArgType::TaskId, "The task."),
    IDEMPOTENCY,
    req(
        "expected_revision",
        Place::Body,
        ArgType::Revision,
        "The revision the caller read the task at.",
    ),
    opt(
        "work_profile_category",
        Place::Body,
        ArgType::Text,
        "The work-profile category to pin.",
    ),
    opt(
        "team_template",
        Place::Body,
        ArgType::Json,
        "An `{id, version}` team-template revision to pin.",
    ),
    opt(
        "account_profile_id",
        Place::Body,
        ArgType::AccountProfileId,
        "The account profile to pin.",
    ),
    req(
        "reason",
        Place::Body,
        ArgType::ExternalName,
        "Why the selection is being made.",
    ),
];

/// The lifecycle transitions the daemon accepts, in its own spelling.
pub static LIFECYCLE_ACTIONS: &[&str] = &[
    "block",
    "resume",
    "complete_task",
    "reopen_task",
    "close_epic",
    "reopen_epic",
];

/// The two ways a permission request can be answered.
pub static PERMISSION_DECISIONS: &[&str] = &["allow", "deny"];

/// One public `/v1` route that deliberately has no tool, and why.
#[derive(Debug, Clone, Copy)]
pub struct NonAgentRoute {
    /// The method.
    pub method: Method,
    /// The path template, exactly as the contract spells it.
    pub path: &'static str,
    /// Why a Lead Architect does not reach it through a tool.
    pub reason: &'static str,
}

/// The only routes the parity oracle may find unmapped.
///
/// Every entry carries a reason, and the oracle fails on an entry that no longer
/// matches a real route as loudly as it fails on a route with no entry: a stale
/// allowlist is how an operation quietly stops being reviewed.
pub static NON_AGENT_ROUTES: &[NonAgentRoute] = &[
    NonAgentRoute {
        method: Method::Get,
        path: "/v1/health",
        reason: "a process probe, not a Lead action",
    },
    NonAgentRoute {
        method: Method::Get,
        path: "/v1/openapi.json",
        reason: "the contract document itself, consumed by tests and build tooling",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_tool_name_is_unique_and_every_route_is_mapped_once() {
        let names: BTreeSet<_> = REGISTRY.iter().map(|tool| tool.name).collect();
        assert_eq!(names.len(), REGISTRY.len(), "two tools share a name");

        let routes: BTreeSet<_> = REGISTRY
            .iter()
            .map(|tool| (tool.method, tool.path))
            .collect();
        assert_eq!(
            routes.len(),
            REGISTRY.len(),
            "two tools target the same operation"
        );
    }

    #[test]
    fn every_tool_targets_a_v1_route_and_declares_its_path_arguments() {
        for tool in REGISTRY {
            assert!(
                tool.path.starts_with("/v1/"),
                "{} targets {}, which is not a /v1 route",
                tool.name,
                tool.path
            );
            let declared: BTreeSet<_> = tool.args_in(Place::Path).map(|arg| arg.name).collect();
            let templated: BTreeSet<_> = tool
                .path
                .split('/')
                .filter_map(|segment| segment.strip_prefix('{')?.strip_suffix('}'))
                .collect();
            assert_eq!(
                declared, templated,
                "{}'s path arguments and its template disagree",
                tool.name
            );
        }
    }

    #[test]
    fn a_write_carries_an_idempotency_key_and_a_read_does_not() {
        for tool in REGISTRY {
            let keys = tool.args_in(Place::Header).count();
            match tool.kind {
                OpKind::Write => assert_eq!(
                    keys, 1,
                    "{} commits something and must take exactly one caller-supplied key",
                    tool.name
                ),
                OpKind::Read | OpKind::Stream => assert_eq!(
                    keys, 0,
                    "{} commits nothing and must not ask for an idempotency key",
                    tool.name
                ),
            }
        }
    }

    #[test]
    fn only_a_streamed_read_declares_frame_bounds() {
        for tool in REGISTRY {
            let bounds = tool.args_in(Place::Bound).count();
            if matches!(tool.kind, OpKind::Stream) {
                assert_eq!(bounds, 2, "{} must bound how much it reads", tool.name);
            } else {
                assert_eq!(bounds, 0, "{} does not stream", tool.name);
            }
        }
    }

    #[test]
    fn a_gate_waiver_demands_admin_and_an_ordinary_verdict_does_not() {
        let gate = ToolSpec::find("kontor_gate_record").expect("the gate tool");
        assert_eq!(
            gate.required_tier(&serde_json::json!({ "verdict": "pass" })),
            CallerTier::Operator
        );
        assert_eq!(
            gate.required_tier(&serde_json::json!({ "verdict": "waived" })),
            CallerTier::Admin,
            "waiving is authority-changing and is checked before the request exists"
        );
        // Every other tool's requirement is its declared tier, whatever it is sent.
        for tool in REGISTRY.iter().filter(|t| t.name != "kontor_gate_record") {
            assert_eq!(
                tool.required_tier(&serde_json::json!({ "verdict": "waived" })),
                tool.tier,
                "{} must not vary its authority with its arguments",
                tool.name
            );
        }
    }

    #[test]
    fn settlement_accepts_no_caller_supplied_outcome() {
        let settle = ToolSpec::find("kontor_runtime_settle").expect("the settlement tool");
        assert_eq!(
            settle.args_in(Place::Body).count(),
            0,
            "a client that could name an outcome could decide how a run ended"
        );
    }

    /// Every profile entry is a real registry tool at or below operator tier.
    ///
    /// This is the drift catch: a tool renamed in [`REGISTRY`] without its
    /// profile entry following would otherwise silently vanish from every seat
    /// that serves the profile, and an admin-tier entry would be a profile
    /// pretending to be authority.
    #[test]
    fn every_profile_entry_names_a_registry_tool_at_or_below_operator() {
        for profile in SERVE_PROFILES {
            let unique: BTreeSet<_> = profile.tools.iter().collect();
            assert_eq!(
                unique.len(),
                profile.tools.len(),
                "profile `{}` lists a tool twice",
                profile.name
            );
            for name in profile.tools {
                let tool = ToolSpec::find(name).unwrap_or_else(|| {
                    panic!(
                        "profile `{}` names `{name}`, which is not a registry tool",
                        profile.name
                    )
                });
                assert!(
                    CallerTier::Operator.at_least(tool.tier),
                    "profile `{}` names `{name}` at {} tier — a profile may not \
                     reach above operator",
                    profile.name,
                    tool.tier.as_str()
                );
            }
        }
    }

    #[test]
    fn the_worker_profile_is_the_sixteen_tools_the_plan_pinned() {
        let worker = ServeProfile::find("worker").expect("the worker profile is declared");
        assert_eq!(worker.tools.len(), 16, "worker v1 is exactly 16 tools");
    }

    #[test]
    fn the_allowlist_names_two_routes_no_tool_also_claims() {
        assert_eq!(NON_AGENT_ROUTES.len(), 2);
        for route in NON_AGENT_ROUTES {
            assert!(
                !route.reason.is_empty(),
                "{} is omitted without a reason",
                route.path
            );
            assert!(
                !REGISTRY
                    .iter()
                    .any(|tool| tool.method == route.method && tool.path == route.path),
                "{} is both mapped and allowlisted",
                route.path
            );
        }
    }
}
