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
    Json,
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
            Self::Json => "object",
        }
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
            let mut property = serde_json::Map::new();
            property.insert("type".into(), arg.ty.json_type().into());
            property.insert("description".into(), arg.about.into());
            if let ArgType::Enum(allowed) = arg.ty {
                property.insert("enum".into(), allowed.iter().copied().collect());
            }
            if matches!(arg.ty, ArgType::TextArray) {
                property.insert("items".into(), serde_json::json!({ "type": "string" }));
            }
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

/// Every operation a Paseo Lead Architect can reach, and nothing else.
///
/// The three public `/v1` routes deliberately absent are listed in
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
                ArgType::Json,
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
    NonAgentRoute {
        method: Method::Post,
        path: "/v1/commands/{kind}",
        reason: "the generic intent surface; the concrete application tools supersede it, \
                 and exposing it would bypass this closed tool vocabulary",
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

    #[test]
    fn the_allowlist_names_three_routes_no_tool_also_claims() {
        assert_eq!(NON_AGENT_ROUTES.len(), 3);
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
