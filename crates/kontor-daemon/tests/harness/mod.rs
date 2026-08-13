//! The black-box harness: a real daemon, a real router, a scripted runtime.
//!
//! Three constraints shape it, and each one is why something here is not the
//! obvious thing:
//!
//! * **No socket, no child process, no daemon binary** (TST-001). Requests go
//!   through `tower::ServiceExt::oneshot` against the same `axum::Router` the
//!   binary serves, so what is exercised is the real middleware, the real
//!   extractors and the real handlers — but nothing listens anywhere.
//! * **A real state root.** Each world gets its own `TempDir`, so the lock, the
//!   credential file and the database are the real ones and two worlds are two
//!   genuinely separate Realms.
//! * **A real runtime contract.** Every session in these tests comes out of
//!   `ScriptedFakeRuntime` as a real `RuntimeBindingSnapshot`, through admission
//!   and launch. Nothing here fabricates a binding, because a fabricated one is
//!   exactly what the API is supposed to refuse.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use kontor_api::state::RuntimeRegistry;
use kontor_core::id::{
    AgentRunId, BoundedText, ExternalId, ExternalName, ProjectId, RealmId, RoleSlotId,
    RuntimeBindingId, RuntimeKindKey, SCHEMA_VERSION, TaskId, TeamRunId, Timestamp,
    parse_utc_timestamp,
};
use kontor_core::repository::{
    NewAgentRun, NewProject, NewTask, NewTeamRun, ProjectRepository, RunRepository, RuntimeBinding,
    SpecRepository,
};
use kontor_core::spec::TeamRunSnapshot;
use kontor_core::state::{NativeRuntimeIdentity, TaskState};
use kontor_daemon::{Daemon, DaemonConfig};
use kontor_profiles::pack::{PackAvailability, resolve_profile};
use kontor_profiles::seeds::bundled_pack;
use kontor_runtime::adapter::RuntimeAdapter;
use kontor_runtime::admission::{AdmissionRequest, RoleSlotKey};
use kontor_runtime::capability::{
    RuntimeBindingSnapshot, RuntimeCapabilities, RuntimeCapability, RuntimeLimits, TrustGrade,
};
use kontor_runtime::fake::{RuntimeScript, ScriptedFakeRuntime};
use kontor_runtime::request::LaunchParts;
use kontor_runtime::workspace::{WorkspaceBindingId, WorkspacePrepareRequest, WorkspaceRoot};
use tempfile::TempDir;
use tower::ServiceExt;

/// The runtime family the scripted fake answers to.
///
/// It is the fake's own built-in family key. The harness asserts the registry and
/// the persisted binding agree on it — see `the_registry_key_matches_what_the_fake_issues`
/// — so a change on either side fails loudly rather than making every session
/// route quietly answer "no such runtime".
pub(crate) fn fake_family() -> RuntimeKindKey {
    RuntimeKindKey::parse("fake.runtime").expect("the fake's family key is a valid open key")
}

/// A canonical fixture instant.
pub(crate) fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical UTC timestamp")
}

/// A fixture display name.
pub(crate) fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid external name")
}

/// Everything the fake declares by default.
pub(crate) fn every_capability() -> RuntimeCapabilities {
    RuntimeCapabilities {
        trust_grade: TrustGrade::A,
        supported: RuntimeCapability::ALL.iter().copied().collect(),
        account_env: true,
        limits: RuntimeLimits {
            max_message_bytes: 4096,
            max_history_page: 64,
            max_concurrent_sessions: 8,
        },
    }
}

/// The same declaration, minus the named capabilities.
pub(crate) fn capabilities_without(missing: &[RuntimeCapability]) -> RuntimeCapabilities {
    let mut declared = every_capability();
    for capability in missing {
        declared.supported.remove(capability);
    }
    declared
}

/// One started Realm, its router and the runtime behind it.
pub(crate) struct World {
    /// Kept for its `Drop`: the state root outlives every request in a test.
    pub(crate) directory: TempDir,
    pub(crate) daemon: Daemon,
    pub(crate) router: Router,
    pub(crate) fake: Arc<ScriptedFakeRuntime>,
    pub(crate) project: ProjectId,
    pub(crate) task: TaskId,
    pub(crate) team_run: TeamRunId,
}

impl World {
    /// Start a Realm whose runtime declares everything.
    pub(crate) async fn open() -> Self {
        Self::open_with(every_capability()).await
    }

    /// Start a Realm with a configured fleet and *nothing else*.
    ///
    /// No project, no task, no team run: this is a `kontord` that has been
    /// installed and configured and never used. It is the only honest starting
    /// point for the bootstrap journey, because a seeded fixture would prove that
    /// the public operations work against rows something else created.
    ///
    /// `project`, `task` and `team_run` still carry ids, and none of them is
    /// persisted — a test that reaches for one in this mode is asking about a row
    /// that does not exist, which is what it should find.
    pub(crate) async fn open_empty() -> Self {
        Self::compose_with(every_capability(), true, false).await
    }

    /// Start a Realm holding *no* adapter at all.
    ///
    /// The fake is still built, so a test can name its family on a persisted
    /// binding, but it is never registered — which is the state a realm is in
    /// before its fleet is configured.
    pub(crate) async fn open_unconfigured() -> Self {
        Self::compose(every_capability(), false).await
    }

    /// Persist a run bound to `family`, without launching anything.
    ///
    /// There is no adapter to launch through — that is the point — so the binding
    /// is written directly. It is honest evidence of what a realm looks like when
    /// its fleet has not been configured: the run and its binding are durable, and
    /// the runtime that owns the session is simply absent.
    pub(crate) fn bind_to_family(&self, family: &RuntimeKindKey) -> AgentRunId {
        let agent_run_id = AgentRunId::generate();
        self.daemon.state().with_store(|store| {
            store
                .create_agent_run(&NewAgentRun {
                    id: agent_run_id,
                    project_id: self.project,
                    team_run_id: self.team_run,
                    parent_agent_run_id: None,
                    role: RoleSlotId::parse("unconfigured-seat")
                        .expect("a valid slot key")
                        .into_role_key(),
                    account_profile_id: None,
                    binding: Some(RuntimeBinding {
                        id: RuntimeBindingId::generate(),
                        agent_run_id,
                        identity: NativeRuntimeIdentity {
                            runtime_kind: family.clone(),
                            host: name("unconfigured-host"),
                            generation: 1,
                            native_id: ExternalId::parse("native-unconfigured")
                                .expect("a valid native id"),
                        },
                        bound_at: at("2026-08-10T09:00:00Z"),
                    }),
                    created_at: at("2026-08-10T09:00:00Z"),
                })
                .expect("the run and its binding are persisted");
        });
        agent_run_id
    }

    /// Start a Realm whose runtime declares exactly `capabilities`.
    pub(crate) async fn open_with(capabilities: RuntimeCapabilities) -> Self {
        Self::compose(capabilities, true).await
    }

    /// Start a Realm, registering the fake only when `configured`.
    async fn compose(capabilities: RuntimeCapabilities, configured: bool) -> Self {
        Self::compose_with(capabilities, configured, true).await
    }

    /// Start a Realm, registering the fake only when `configured` and seeding the
    /// fixture work graph only when `seeded`.
    async fn compose_with(
        capabilities: RuntimeCapabilities,
        configured: bool,
        seeded: bool,
    ) -> Self {
        let directory = TempDir::new().expect("a temporary directory");
        let fake = Arc::new(ScriptedFakeRuntime::new(capabilities));
        let registry = if configured {
            RuntimeRegistry::new().with(fake_family(), Arc::clone(&fake) as Arc<dyn RuntimeAdapter>)
        } else {
            RuntimeRegistry::new()
        };
        let daemon = Daemon::start(DaemonConfig::at(directory.path()).with_port(0), registry)
            .expect("the realm starts");
        let router = daemon.router();

        let project = ProjectId::generate();
        let task = TaskId::generate();
        let team_run = TeamRunId::generate();
        if seeded {
            daemon.state().with_store(|store| {
                store
                    .create_project(&NewProject {
                        id: project,
                        name: name("Loopback project"),
                        root_path: name("/tmp/kontor-loopback"),
                        created_at: at("2026-08-10T09:00:00Z"),
                    })
                    .expect("a project is created");
                store
                    .create_task(&NewTask {
                        id: task,
                        project_id: project,
                        mini_project_id: None,
                        title: name("A loopback task"),
                        module: None,
                        state: TaskState::Ready,
                        created_at: at("2026-08-10T09:00:00Z"),
                    })
                    .expect("a task is created");

                // The team revision comes from the bundled pack rather than from a
                // hand-rolled document: the run's foreign key demands a stored
                // revision, and inventing one would test a shape no deployment has.
                let pack = bundled_pack().expect("the bundled pack loads");
                let entry = pack
                    .manifest
                    .iter()
                    .find(|entry| entry.availability == PackAvailability::Seeded)
                    .expect("the bundled pack seeds at least one category");
                let bundle = resolve_profile(&pack, &entry.category, at("2026-08-10T09:00:00Z"))
                    .expect("the seeded category resolves");
                let revision = bundle.team.clone().expect("the profile pinned a team");
                store
                    .insert_work_profile(project, &bundle.profile.definition)
                    .expect("the profile revision is stored");
                store
                    .insert_team_template(project, &revision)
                    .expect("the team revision is stored");
                store
                    .create_team_run(&NewTeamRun {
                        id: team_run,
                        project_id: project,
                        task_id: task,
                        snapshot: TeamRunSnapshot::from_revision(&revision, SCHEMA_VERSION),
                        created_at: at("2026-08-10T09:00:00Z"),
                    })
                    .expect("the team run is created");
            });
        }

        Self {
            directory,
            daemon,
            router,
            fake,
            project,
            task,
            team_run,
        }
    }

    /// Load a declarative runtime script.
    pub(crate) fn script(&self, json: &str) {
        let script: RuntimeScript = serde_json::from_str(json).expect("a runtime script fixture");
        self.fake
            .load_script(&script, &[])
            .expect("the script loads");
    }

    /// This Realm's identity.
    pub(crate) fn realm_id(&self) -> RealmId {
        self.daemon.realm_id()
    }

    /// Launch one session through admission, persist the run and its binding, and
    /// record the frozen snapshot the way a real launch path does.
    ///
    /// Every step is the real one. In particular the snapshot handed to the
    /// session registry is the one the runtime issued — not a copy a test built —
    /// because a snapshot the runtime will not vouch for is precisely what the API
    /// has to refuse.
    pub(crate) async fn launch(&self) -> (AgentRunId, RuntimeBindingSnapshot) {
        let agent_run_id = AgentRunId::generate();
        let binding_id = RuntimeBindingId::generate();
        let role_slot_id = RoleSlotId::parse("harness-seat").expect("a valid slot key");
        let workspace = self
            .fake
            .prepare_workspace(&WorkspacePrepareRequest {
                team_run_id: self.team_run,
                task_id: self.task,
                workspace_binding_id: WorkspaceBindingId::generate(),
                root: WorkspaceRoot::parse("/w/loopback-task").expect("an absolute path"),
                requested_at: at("2026-08-10T08:59:00Z"),
            })
            .await
            .expect("the runtime prepares the task workspace")
            .snapshot;
        let parts = LaunchParts {
            agent_run_id,
            team_run_id: self.team_run,
            role_slot_id: role_slot_id.clone(),
            task_id: self.task,
            binding_id,
            workspace: Some(workspace.clone()),
            cwd: workspace.root().clone(),
            account_profile_id: None,
            prompt: BoundedText::parse("do the loopback work").expect("bounded text"),
            requested_at: at("2026-08-10T09:00:00Z"),
        };
        let authority = self
            .fake
            .admit_launch(&AdmissionRequest {
                slot: RoleSlotKey::new(self.team_run, role_slot_id.clone()),
                agent_run_id,
                binding_id,
                replaces: None,
                requested_at: at("2026-08-10T09:00:00Z"),
            })
            .await
            .expect("the runtime admits the seat")
            .into_authority()
            .expect("a vacant seat is admitted rather than resumed");
        let outcome = self
            .fake
            .launch(&authority.into_request(parts))
            .await
            .expect("the seat launches");

        self.daemon.state().with_store(|store| {
            store
                .create_agent_run(&NewAgentRun {
                    id: agent_run_id,
                    project_id: self.project,
                    team_run_id: self.team_run,
                    parent_agent_run_id: None,
                    role: role_slot_id.clone().into_role_key(),
                    account_profile_id: None,
                    binding: Some(RuntimeBinding {
                        id: outcome.snapshot.binding_id(),
                        agent_run_id,
                        identity: outcome.snapshot.identity().clone(),
                        bound_at: at("2026-08-10T09:00:00Z"),
                    }),
                    created_at: at("2026-08-10T09:00:00Z"),
                })
                .expect("the run and its binding are persisted");
        });
        // What a launch path does last: hand the frozen snapshot to the process so
        // the session can be addressed at the evidence quality it was bound at.
        self.daemon
            .state()
            .sessions()
            .record(outcome.snapshot.clone());
        (agent_run_id, outcome.snapshot)
    }

    /// A run persisted with no binding at all.
    pub(crate) fn unbound_run(&self) -> AgentRunId {
        let agent_run_id = AgentRunId::generate();
        self.daemon.state().with_store(|store| {
            store
                .create_agent_run(&NewAgentRun {
                    id: agent_run_id,
                    project_id: self.project,
                    team_run_id: self.team_run,
                    parent_agent_run_id: None,
                    role: RoleSlotId::parse("unbound-seat")
                        .expect("a valid slot key")
                        .into_role_key(),
                    account_profile_id: None,
                    binding: None,
                    created_at: at("2026-08-10T09:00:00Z"),
                })
                .expect("an unbound run is persisted");
        });
        agent_run_id
    }
}

/// One authority tier's credential, read from the Realm's own file.
///
/// The tests read the secrets the same way the desktop shell will: out of the
/// `0600` file in the state root. Nothing in the test tree knows how one is
/// generated.
pub(crate) fn secret(world: &World, tier: &str) -> String {
    let path = kontor_daemon::credentials::path_in(world.directory.path());
    let bytes = std::fs::read(&path).expect("the realm wrote its credential file");
    let document: serde_json::Value =
        serde_json::from_slice(&bytes).expect("the credential file is JSON");
    document
        .get(tier)
        .and_then(serde_json::Value::as_str)
        .expect("the credential file names every tier")
        .to_owned()
}

/// A request builder that is loopback-shaped and authenticated by default.
pub(crate) struct Call {
    method: axum::http::Method,
    uri: String,
    host: String,
    origin: Option<String>,
    token: Option<String>,
    idempotency_key: Option<String>,
    extra: Vec<(String, String)>,
    body: Body,
}

impl Call {
    /// A `GET` as an observer.
    pub(crate) fn get(uri: impl Into<String>) -> Self {
        Self {
            method: axum::http::Method::GET,
            uri: uri.into(),
            host: "127.0.0.1:7717".to_owned(),
            origin: None,
            token: None,
            idempotency_key: None,
            extra: Vec::new(),
            body: Body::empty(),
        }
    }

    /// A `POST` carrying a JSON document.
    pub(crate) fn post(uri: impl Into<String>, body: &serde_json::Value) -> Self {
        Self {
            method: axum::http::Method::POST,
            uri: uri.into(),
            host: "127.0.0.1:7717".to_owned(),
            origin: None,
            token: None,
            idempotency_key: None,
            extra: Vec::new(),
            body: Body::from(serde_json::to_vec(body).expect("a serializable body")),
        }
    }

    /// Present this Realm's credential for `tier`.
    pub(crate) fn signed_as(mut self, world: &World, tier: &str) -> Self {
        self.token = Some(secret(world, tier));
        self
    }

    /// Present an arbitrary credential.
    pub(crate) fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Present no credential at all.
    pub(crate) fn anonymous(mut self) -> Self {
        self.token = None;
        self
    }

    /// Claim a different `Host`.
    pub(crate) fn claiming_host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    /// Claim an `Origin`.
    pub(crate) fn claiming_origin(mut self, origin: impl Into<String>) -> Self {
        self.origin = Some(origin.into());
        self
    }

    /// Carry an idempotency key.
    pub(crate) fn with_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }

    /// Carry one more header verbatim — `Last-Event-ID`, for instance.
    pub(crate) fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra.push((name.into(), value.into()));
        self
    }

    /// Add one header verbatim.
    fn build(self) -> Request<Body> {
        let mut builder = Request::builder()
            .method(self.method)
            .uri(self.uri)
            .header("host", self.host)
            .header("content-type", "application/json");
        if let Some(origin) = self.origin {
            builder = builder.header("origin", origin);
        }
        if let Some(token) = self.token {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        if let Some(key) = self.idempotency_key {
            builder = builder.header("idempotency-key", key);
        }
        for (name, value) in self.extra {
            builder = builder.header(name, value);
        }
        builder.body(self.body).expect("a well-formed request")
    }

    /// Drive the real router and read the whole answer.
    pub(crate) async fn send(self, world: &World) -> Answer {
        self.send_to(&world.router).await
    }

    /// Drive an arbitrary router — a restarted daemon's, for instance.
    pub(crate) async fn send_to(self, router: &Router) -> Answer {
        let response = router
            .clone()
            .oneshot(self.build())
            .await
            .expect("the router answers");
        Answer::read(response).await
    }
}

/// One whole answer, body included.
#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) status: StatusCode,
    pub(crate) body: String,
}

impl Answer {
    async fn read(response: Response<Body>) -> Self {
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .expect("the whole body is readable");
        Self {
            status,
            body: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }

    /// The body as JSON.
    pub(crate) fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or_else(|error| {
            panic!(
                "the body is JSON: {error}\nstatus {}\nbody {}",
                self.status, self.body
            )
        })
    }

    /// The `realm_id` this answer names, at the top level.
    pub(crate) fn realm(&self) -> RealmId {
        let value = self.json();
        let text = value
            .get("realm_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("every answer names its realm; body was {}", self.body));
        RealmId::parse(text).expect("a canonical realm id")
    }

    /// The stable error code this refusal carries.
    pub(crate) fn code(&self) -> String {
        self.json()
            .get("code")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("every refusal carries a code; body was {}", self.body))
            .to_owned()
    }

    /// Every SSE frame, as `(event, id, data)`.
    ///
    /// Parsed rather than deserialized whole, because what is being asserted is
    /// the *framing*: the ids a subscriber would resume from, in delivery order.
    pub(crate) fn frames(&self) -> Vec<(String, String, serde_json::Value)> {
        let mut frames = Vec::new();
        for block in self.body.split("\n\n") {
            let mut event = String::new();
            let mut id = String::new();
            let mut data = String::new();
            for line in block.lines() {
                if let Some(rest) = line.strip_prefix("event:") {
                    event = rest.trim().to_owned();
                } else if let Some(rest) = line.strip_prefix("id:") {
                    id = rest.trim().to_owned();
                } else if let Some(rest) = line.strip_prefix("data:") {
                    data.push_str(rest.trim());
                }
            }
            if data.is_empty() {
                continue;
            }
            let parsed = serde_json::from_str(&data)
                .unwrap_or_else(|error| panic!("an SSE frame carries JSON: {error} in {data}"));
            frames.push((event, id, parsed));
        }
        frames
    }
}
