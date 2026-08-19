//! `kontor-daemon` — Single-instance process and composition root for the Kontor control plane
//!
//! This crate is where every decision `kontor-api` is not allowed to make gets
//! made, exactly once:
//!
//! * which state root this process owns, and the exclusive filesystem lock that
//!   proves it;
//! * which database file is opened, and therefore which Realm this process *is*;
//! * the Realm's bearer secrets, generated on first start into a `0600` file;
//! * which runtime adapters exist and how to reach them;
//! * how many simultaneous runs the Realm admits, at every scope;
//! * whether startup reconciliation finished, and therefore whether scheduling
//!   may run;
//! * which loopback address is bound.
//!
//! # Startup order, and why it is this order
//!
//! ```text
//! validate config (loopback only)
//!   → claim the state root (exclusive, no waiting)
//!     → open + migrate the database  → the Realm's identity
//!       → read or generate credentials
//!         → build the adapter registry
//!           → recover unfinished receipts, reconcile open bindings
//!             → open the scheduling barrier
//!               → serve
//! ```
//!
//! The barrier is last because everything above it answers a question a scheduler
//! would otherwise guess. A restarted process knows nothing new about a command it
//! had dispatched, and nothing new about a session it had bound; until the runtimes
//! have been asked, dispatching would mean re-sending effects that may already have
//! happened.
//!
//! # What a restart may and may not do
//!
//! [`recover`] classifies every unsettled receipt through the durable record and
//! never through elapsed time. Exactly one classification authorizes a fresh
//! dispatch — the one that proves nothing was ever sent. Every other one requires a
//! native lookup by the persisted correlation first, which is the scheduler's work
//! and not this crate's; the daemon's job is to make sure the barrier stays shut
//! until that inventory has been taken.

pub mod applications;
pub mod credentials;
pub mod endpoint;
pub mod lock;
pub mod logging;
pub mod recovery;
pub mod runtimes;
pub mod supervision;

use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

use kontor_api::state::{
    ApiParts, ApiState, BarrierState, RuntimeRegistry, SchedulingBarrier, SessionRegistry,
    StreamSignals,
};
use kontor_core::id::{RealmId, RuntimeBindingId};
use kontor_runtime::capability::RuntimeBindingSnapshot;
use kontor_scheduler::model::{AdaptiveWindowConfig, CapacityConfig};
use kontor_store::{CommandRecovery, SqliteStore};
use tracing::{info, warn};

use crate::credentials::CredentialError;
use crate::lock::{LockError, StateRootLock};
use crate::runtimes::FleetError;
use crate::supervision::{SupervisionError, SupervisionPolicy};

/// The database file's name inside a state root.
pub const DATABASE_FILE: &str = "kontor.db";

/// The loopback port a Kontor daemon binds by default.
pub const DEFAULT_PORT: u16 = 7717;

/// How old a confirmation may be and still count as fresh, in seconds.
pub const DEFAULT_EVIDENCE_WINDOW_SECONDS: i64 = 60;

/// How many simultaneous runs a Realm admits before the planner refuses.
///
/// These are the numbers a Realm ran under when they were compiled into the
/// composition root, and they stay the default so a daemon started the way every
/// existing caller starts one admits exactly what it admitted before. A
/// deployment that needs other ceilings sets [`DaemonConfig::capacity`]; the
/// scheduler itself has no default, because the numbers are a deployment's.
///
/// ponytail: a public default plus a public field, and no operator flag for each
/// of the ten knobs. `--global-max-in-flight` and nine siblings is a wider
/// surface than the one thing that was actually missing — a way to *set* the
/// ceilings — and the upgrade, if an operator ever has to change them without
/// recomposing, is to read this whole struct out of the state root next to
/// `runtimes.json`, not to spread ten numbers across the argument parser.
pub const DEFAULT_CAPACITY: CapacityConfig = CapacityConfig {
    global_max_in_flight: 16,
    project_max_in_flight: 8,
    mission_max_in_flight: 7,
    account_max_in_flight: 4,
    provider_max_in_flight: 4,
    runtime_max_in_flight: 8,
    adaptive: AdaptiveWindowConfig {
        initial: 4,
        floor: 1,
        ceiling: 7,
        growth_step: 1,
    },
};

/// Why a daemon could not start.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StartupError {
    /// The configured bind address is not loopback.
    ///
    /// There is deliberately no flag that widens this. A Kontor Realm holds the
    /// credentials and the transcripts of every run on the machine; remote bind is
    /// an architecture follow-on with its own transport, pairing and account model,
    /// and an "allow any interface" switch is how a local-first control plane
    /// quietly becomes an unauthenticated remote one.
    #[error("kontor binds loopback only, and {address} is not a loopback address")]
    NotLoopback {
        /// The address that was refused.
        address: SocketAddr,
    },
    /// The configured admission ceilings are not a set the domain accepts.
    ///
    /// Judged before the state root is touched, for the same reason the bind
    /// address is: a zero ceiling reads as "no work allowed" in one place and "no
    /// limit" in another, and a Realm that starts on one would either admit
    /// nothing or admit everything. Neither is a configuration an operator can
    /// tell apart from a working one by watching it.
    #[error("the configured admission capacity is not one a realm may admit work under: {source}")]
    Capacity {
        /// The domain's own refusal.
        #[source]
        source: kontor_core::DomainError,
    },
    /// The state root does not exist and could not be created.
    #[error("the state root could not be prepared: {source}")]
    StateRoot {
        /// The underlying failure.
        #[source]
        source: std::io::Error,
    },
    /// Another daemon already owns the state root.
    #[error(transparent)]
    Lock(#[from] LockError),
    /// The database could not be opened or migrated.
    #[error("the control-plane database could not be opened: {source}")]
    Store {
        /// The underlying failure.
        #[source]
        source: kontor_store::StoreError,
    },
    /// The Realm's credentials could not be established.
    #[error(transparent)]
    Credentials(#[from] CredentialError),
    /// The configured runtime fleet could not be composed.
    #[error(transparent)]
    Fleet(#[from] FleetError),
    /// The bundled profile pack this build ships does not validate.
    ///
    /// It is a build-time defect in the shipped data rather than a runtime
    /// condition, and it refuses the start rather than serving a Realm whose
    /// catalogs are empty.
    #[error("the bundled profile pack could not be composed: {source}")]
    Applications {
        /// The underlying refusal.
        #[source]
        source: kontor_core::DomainError,
    },
    /// The configured ASMA connector executable is absent or invalid.
    #[error("the Jira connector boundary could not be composed: {source}")]
    Connector {
        /// The boundary's own typed refusal.
        #[source]
        source: kontor_integrations_asma::AsmaError,
    },
    /// The configured seat-supervision policy could not be loaded.
    #[error(transparent)]
    Supervision(#[from] SupervisionError),
}

/// Everything a daemon is configured with.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// The directory that holds this Realm's database, lock and credentials.
    pub state_root: PathBuf,
    /// The loopback address to bind. Validated before anything is opened.
    pub bind: SocketAddr,
    /// Which browser origins to answer, beyond the loopback host rule.
    pub allowed_origins: Vec<String>,
    /// How old a confirmation may be and still count as fresh.
    pub evidence_window_seconds: i64,
    /// How many simultaneous runs this Realm admits, at every scope.
    ///
    /// Defaults to [`DEFAULT_CAPACITY`], which is what the composition root used
    /// to hold as a compile-time constant. Validated at startup rather than here,
    /// so setting the field is infallible and a refused set of ceilings refuses
    /// the *start* — the one moment an operator is watching.
    pub capacity: CapacityConfig,
    /// The supported ASMA executable used as Jira's single wire boundary.
    ///
    /// `None` deliberately composes no Jira transport. Realms that do not use
    /// Jira keep starting without the ASMA CLI installed, while a Realm that
    /// configures the boundary validates it before serving any request.
    pub asma_executable: Option<PathBuf>,
}

impl DaemonConfig {
    /// A configuration rooted at `state_root`, bound to loopback.
    #[must_use]
    pub fn at(state_root: impl Into<PathBuf>) -> Self {
        Self {
            state_root: state_root.into(),
            bind: SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT)),
            allowed_origins: kontor_api::auth::IngressPolicy::default().allowed_origins,
            evidence_window_seconds: DEFAULT_EVIDENCE_WINDOW_SECONDS,
            capacity: DEFAULT_CAPACITY,
            asma_executable: None,
        }
    }

    /// Admit work under different ceilings than [`DEFAULT_CAPACITY`].
    ///
    /// Infallible on purpose: a set the domain refuses is caught by
    /// [`Daemon::start`], so a caller assembling a configuration never has to
    /// handle a failure at a point where it cannot yet act on one.
    #[must_use]
    pub const fn with_capacity(mut self, capacity: CapacityConfig) -> Self {
        self.capacity = capacity;
        self
    }

    /// Compose Jira through one explicitly resolved ASMA executable.
    #[must_use]
    pub fn with_asma_executable(mut self, executable: impl Into<PathBuf>) -> Self {
        self.asma_executable = Some(executable.into());
        self
    }

    /// Bind a different loopback port.
    #[must_use]
    pub fn with_port(mut self, port: u16) -> Self {
        self.bind.set_port(port);
        self
    }

    /// Bind an explicit address. Still validated as loopback at startup.
    #[must_use]
    pub const fn with_bind(mut self, bind: SocketAddr) -> Self {
        self.bind = bind;
        self
    }

    /// Prove the configured address is one this machine may be reached at only
    /// from itself.
    ///
    /// # Errors
    /// Returns [`StartupError::NotLoopback`] for anything else, including the
    /// wildcard addresses — `0.0.0.0` and `::` are not loopback, they are *every*
    /// interface, and accepting them is the single mistake that would expose a
    /// Realm to the network.
    pub const fn ensure_loopback(&self) -> Result<(), StartupError> {
        let loopback = match self.bind.ip() {
            IpAddr::V4(address) => address.is_loopback(),
            IpAddr::V6(address) => address.is_loopback(),
        };
        if loopback {
            Ok(())
        } else {
            Err(StartupError::NotLoopback { address: self.bind })
        }
    }
}

/// A started, locked, reconciled daemon.
///
/// Holding one means this process owns its state root: the lock lives here, so
/// dropping the daemon is what releases it.
pub struct Daemon {
    state: ApiState,
    config: DaemonConfig,
    supervision: Option<SupervisionPolicy>,
    /// Held for its `Drop`. The claim on the state root lasts exactly as long as
    /// this value does.
    lock: StateRootLock,
}

impl std::fmt::Debug for Daemon {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Daemon")
            .field("realm_id", &self.realm_id())
            .field("bind", &self.config.bind)
            .field("state_root", &self.config.state_root)
            .finish_non_exhaustive()
    }
}

impl Daemon {
    /// Claim a state root and bring one Realm up to the point of serving.
    ///
    /// Reconciliation is *not* run here: [`Daemon::reconcile`] does that, so a
    /// caller can observe the barrier shut before it opens and a test can prove
    /// that scheduling is blocked until it does.
    ///
    /// # Errors
    /// Returns [`StartupError`] when the address is not loopback, the configured
    /// capacity is not a set the domain accepts, the state root cannot be prepared
    /// or claimed, the database cannot be opened, or the credentials cannot be
    /// established. Every one of them leaves the state root exactly as it was.
    pub fn start(config: DaemonConfig, runtimes: RuntimeRegistry) -> Result<Self, StartupError> {
        Self::start_with_supervision(config, runtimes, None)
    }

    fn start_with_supervision(
        config: DaemonConfig,
        runtimes: RuntimeRegistry,
        supervision: Option<SupervisionPolicy>,
    ) -> Result<Self, StartupError> {
        // The address and the ceilings are judged before anything is created, so a
        // misconfigured daemon does not leave a lock file and a database behind.
        config.ensure_loopback()?;
        config
            .capacity
            .validate()
            .map_err(|source| StartupError::Capacity { source })?;
        std::fs::create_dir_all(&config.state_root)
            .map_err(|source| StartupError::StateRoot { source })?;
        let lock = StateRootLock::acquire(&config.state_root)?;
        let store = SqliteStore::open(&config.state_root.join(DATABASE_FILE))
            .map_err(|source| StartupError::Store { source })?;
        let credentials = credentials::open_or_create(&config.state_root)?;
        let realm_id = store.realm_id();
        // The services and the state are mutually dependent — the state serves
        // the routes the services back, and the services read the store the state
        // holds — so the services are built first, handed in, and given the state
        // immediately afterwards. Nothing can serve a request in between: the
        // router does not exist yet.
        let asma = config
            .asma_executable
            .as_ref()
            .map(kontor_integrations_asma::AsmaExecutable::new)
            .transpose()
            .map_err(|source| StartupError::Connector { source })?;
        let applications = applications::Services::new(
            realm_id,
            config.capacity,
            asma,
            config.state_root.join("runtime-roots"),
        )
        .map_err(|source| StartupError::Applications { source })?;

        let state = ApiState::new(ApiParts {
            store,
            credentials,
            ingress: kontor_api::auth::IngressPolicy {
                allowed_origins: config.allowed_origins.clone(),
            },
            runtimes,
            sessions: SessionRegistry::new(),
            barrier: SchedulingBarrier::new(),
            signals: StreamSignals::new(),
            evidence_window_seconds: config.evidence_window_seconds,
            applications: applications.clone(),
        });
        applications.attach(state.clone());
        info!(
            realm_id = %realm_id,
            state_root = %config.state_root.display(),
            bind = %config.bind,
            // The ceilings in force, because there is otherwise nowhere an operator
            // can read back which set a serving Realm was composed with.
            capacity = ?config.capacity,
            "realm claimed; scheduling is shut until reconciliation finishes"
        );
        Ok(Self {
            state,
            config,
            supervision,
            lock,
        })
    }

    /// Claim a state root and bring up the Realm *with the fleet it is configured
    /// with*.
    ///
    /// This is the path the executable takes, and the difference from
    /// [`Daemon::start`] is the whole point: `start` takes whatever registry it is
    /// handed, which is what a test or an embedding caller needs, while this reads
    /// `runtimes.json` from the state root and composes the real adapters. A
    /// daemon started the other way with an empty registry serves the entire
    /// control plane and answers every session route as unconfigured — correct for
    /// a Realm with no fleet, and a defect for one that has configured it.
    ///
    /// # Errors
    /// As [`Daemon::start`], plus [`StartupError::Fleet`] when the settings file
    /// cannot be read or a configured runtime cannot be composed. A misconfigured
    /// fleet refuses the start rather than silently serving a Realm whose session
    /// routes go nowhere.
    pub fn start_configured(config: DaemonConfig) -> Result<Self, StartupError> {
        // The address is judged before the filesystem is touched, exactly as in
        // `start`, so a misconfigured bind does not read a fleet it will not use.
        config.ensure_loopback()?;
        let supervision = supervision::read(&config.state_root)?;
        let settings = runtimes::read(&config.state_root)?;
        // Seat MCP composition is resolved here — once, at daemon level — so the
        // `KONTOR_SEAT_MCP=off` kill switch governs every plane at once.
        let seat_mcp = runtimes::seat_mcp(&config.state_root);
        let registry = runtimes::build_registry(&settings, seat_mcp.as_ref())?;
        Self::start_with_supervision(config, registry, supervision)
    }

    /// This Realm's identity.
    #[must_use]
    pub fn realm_id(&self) -> RealmId {
        self.state.realm_id()
    }

    /// The handler state, for the router and for the services this daemon owns.
    #[must_use]
    pub fn state(&self) -> ApiState {
        self.state.clone()
    }

    /// The configuration this daemon started with.
    #[must_use]
    pub const fn config(&self) -> &DaemonConfig {
        &self.config
    }

    /// The versioned seat-supervision policy loaded from this Realm, if any.
    ///
    /// Absence means Kontor does not supervise turn liveness. It never invents
    /// timeout behavior when the operator supplied no policy.
    #[must_use]
    pub const fn supervision_policy(&self) -> Option<&SupervisionPolicy> {
        self.supervision.as_ref()
    }

    /// The lock file proving this process owns the state root.
    #[must_use]
    pub fn lock_path(&self) -> &Path {
        self.lock.path()
    }

    /// The HTTP surface of this Realm.
    pub fn router(&self) -> axum::Router {
        kontor_api::router(self.state())
    }

    /// Take the inventory a restart owes, then open the scheduling barrier.
    ///
    /// Two sweeps, and neither of them changes a run's outcome:
    ///
    /// * **Receipts.** Every unsettled command is classified from its durable
    ///   record. A command that was, or may have been, dispatched is reported and
    ///   left alone — the only way past it is a native lookup by the persisted
    ///   correlation, which is a decision about a specific runtime and belongs to
    ///   the dispatcher.
    /// * **Bindings.** Every open run's binding is presented to its runtime, which
    ///   classifies it. A missing session becomes lost contact, never a completion,
    ///   and a binding the runtime will not vouch for stays unattached rather than
    ///   being re-graded against fresh capabilities.
    ///
    /// The barrier opens only when every configured runtime answered. A runtime
    /// that could not be reached leaves it [`BarrierState::Failed`]: a census that
    /// did not finish proves nothing about what it did not reach, and scheduling on
    /// that basis is exactly the mistake the barrier exists to prevent.
    pub async fn reconcile(&self) -> BarrierState {
        let realm_id = self.realm_id();
        let recovered = self.state.with_store(recover);
        match &recovered {
            Ok(report) => info!(
                realm_id = %realm_id,
                undispatched = report.undispatched,
                ambiguous = report.ambiguous,
                settled = report.settled,
                "unfinished command receipts recovered"
            ),
            Err(detail) => warn!(
                realm_id = %realm_id,
                detail = %detail,
                "unfinished command receipts could not be inventoried"
            ),
        }

        let bindings = self.state.with_store(SqliteStore::open_bindings);
        let outcome = match (recovered, bindings) {
            (Ok(_), Ok(bindings)) => self.reconcile_bindings(&bindings).await,
            _ => BarrierState::Failed,
        };
        self.state.barrier().settle(outcome);
        // Follow-ups that a previous process derived and never handed over are
        // finished here, on the seam that already owns "what did this realm
        // leave unfinished?". Nothing is *derived* at startup — a follow-up
        // exists only because a turn was settled — so a restart cannot invent
        // work, and the dispatch table's key makes a retry idempotent.
        if outcome == BarrierState::Open {
            match self
                .state
                .applications()
                .retry_undelivered_dispatches()
                .await
            {
                Ok(0) => {}
                Ok(delivered) => info!(
                    realm_id = %realm_id,
                    delivered,
                    "follow-ups derived before the restart were handed over"
                ),
                Err(error) => warn!(
                    realm_id = %realm_id,
                    detail = %error.code.as_str(),
                    "follow-ups derived before the restart could not be handed over"
                ),
            }
        }
        info!(
            realm_id = %realm_id,
            barrier = ?outcome,
            "startup reconciliation finished"
        );
        outcome
    }

    /// Ask each configured runtime about the bindings this Realm holds for it.
    async fn reconcile_bindings(&self, bindings: &[kontor_store::OpenBinding]) -> BarrierState {
        let mut settled = BarrierState::Open;
        // Read once, for every family. A document that no longer matches its
        // digest fails the read outright rather than being skipped: a claim
        // edited underneath the daemon is not the claim this Realm made.
        let persisted: BTreeMap<RuntimeBindingId, RuntimeBindingSnapshot> =
            match self.state.with_store(SqliteStore::list_binding_snapshots) {
                Ok(rows) => {
                    let mut claims = BTreeMap::new();
                    for row in &rows {
                        // A row that will not parse is a *corrupt claim*, and it
                        // fails loudly. Skipping it would make a binding
                        // disappear from every later census — the quietest
                        // possible way to lose a live session, and
                        // indistinguishable from never having had one.
                        match serde_json::from_str::<RuntimeBindingSnapshot>(&row.document) {
                            Ok(snapshot) => {
                                claims.insert(row.binding_id, snapshot);
                            }
                            Err(detail) => {
                                warn!(
                                    realm_id = %self.realm_id(),
                                    binding = %row.binding_id,
                                    detail = %detail,
                                    "a persisted binding snapshot is unreadable; \
                                     scheduling stays shut"
                                );
                                return BarrierState::Failed;
                            }
                        }
                    }
                    claims
                }
                Err(detail) => {
                    warn!(
                        realm_id = %self.realm_id(),
                        detail = %detail,
                        "persisted binding snapshots could not be read; scheduling stays shut"
                    );
                    return BarrierState::Failed;
                }
            };
        for family in self
            .state
            .runtimes()
            .families()
            .cloned()
            .collect::<Vec<_>>()
        {
            let Some(adapter) = self.state.runtimes().get(&family) else {
                continue;
            };
            // The claims this Realm persisted for *this* family, read from the
            // store rather than from the in-process registry. Reading the
            // registry here was the whole of the restart defect: a fresh process
            // holds nothing, so the census was taken over an empty list and
            // classified none of the realm's open bindings.
            let claimed: Vec<_> = bindings
                .iter()
                .filter(|binding| binding.binding.identity.runtime_kind == family)
                .filter_map(|binding| persisted.get(&binding.binding.id).cloned())
                .collect();
            // The plane's own container has to exist before a census can be
            // taken inside it. A runtime that holds one answers "the project has
            // not been prepared" to *every* question until this call has
            // succeeded, and an unprepared plane looks exactly like an
            // unreachable one — which is why it is prepared here, on the path
            // that already owns "is this runtime ready?", rather than left to
            // whichever operation happened to ask first.
            if let Err(error) = adapter.prepare_plane().await {
                warn!(
                    realm_id = %self.realm_id(),
                    runtime = %family,
                    detail = %error,
                    "runtime plane could not be prepared; scheduling stays shut"
                );
                settled = BarrierState::Failed;
                continue;
            }
            // Hand the claims back to the runtime that issued them. It confirms
            // each session still exists in the same generation and re-records
            // the snapshot *verbatim*, so the binding keeps the grade, limits,
            // correlation and native identity it was issued under. Nothing here
            // re-derives capabilities; a session bound at a degraded grade must
            // not come back promoted because the runtime answers better today.
            let held = match adapter.restore_bindings(&claimed).await {
                Ok(restored) => restored,
                Err(error) => {
                    warn!(
                        realm_id = %self.realm_id(),
                        runtime = %family,
                        detail = %error,
                        "runtime could not re-attest this realm's bindings; scheduling stays shut"
                    );
                    settled = BarrierState::Failed;
                    continue;
                }
            };
            // Only what the runtime vouched for enters this process's registry.
            // A claim it did not restore is a binding nothing may operate, which
            // is the honest answer for a session that did not survive.
            for snapshot in &held {
                self.state.sessions().record(snapshot.clone());
            }
            if !held.is_empty() {
                info!(
                    realm_id = %self.realm_id(),
                    runtime = %family,
                    restored = held.len(),
                    claimed = claimed.len(),
                    "runtime re-attested bindings this realm held before restart"
                );
            }
            // The census runs over **every** claim, not only the restored ones.
            // A claim the runtime refused to attest is reported as `Unattested`
            // rather than vanishing: an unreviewed binding is how a forged one
            // survives, and this is the review.
            if held.len() != claimed.len() {
                warn!(
                    realm_id = %self.realm_id(),
                    runtime = %family,
                    claimed = claimed.len(),
                    restored = held.len(),
                    "this realm holds binding claims the runtime would not attest"
                );
            }
            match adapter.reconcile(&claimed).await {
                Ok(report) => {
                    info!(
                        realm_id = %self.realm_id(),
                        runtime = %family,
                        findings = report.findings.len(),
                        needs_review = report.needs_review(),
                        "runtime classified this realm's bindings"
                    );
                }
                Err(error) => {
                    // A runtime that could not be reached has classified nothing.
                    // Absence from a sweep that never completed is not evidence.
                    warn!(
                        realm_id = %self.realm_id(),
                        runtime = %family,
                        detail = %error,
                        "runtime could not be reconciled; scheduling stays shut"
                    );
                    settled = BarrierState::Failed;
                }
            }
        }
        settled
    }

    /// Mint a new credential set, publish it and swap it in, in that order.
    ///
    /// The order is the safety property. The file is written and flushed first,
    /// so a crash between the two steps leaves a Realm whose *next* start
    /// authorizes the new tokens — the operator has them, and they work. The
    /// reverse order would leave a process answering to secrets that exist
    /// nowhere on disk, and a restart would silently revive the tokens the
    /// rotation was meant to kill.
    ///
    /// Every previously issued token is refused from the next authorization
    /// onwards. In-flight calls that are already past the auth layer finish;
    /// a long-lived stream reconnects with the new token like any other client.
    /// Native sessions, runtime bindings and command receipts are untouched:
    /// they are identified by this Realm's own ids, not by the credential a
    /// client authenticated with.
    ///
    /// # Errors
    /// Returns [`CredentialError`] when the platform's entropy source cannot be
    /// reached or the file cannot be replaced. The previous credentials stay in
    /// force, on disk and in memory.
    pub fn rotate_credentials(&self) -> Result<(), CredentialError> {
        let rotated = credentials::rotate(&self.config.state_root)?;
        self.state.credentials().replace(rotated);
        info!(
            realm_id = %self.realm_id(),
            "realm credentials rotated; every previously issued token is refused from now on"
        );
        Ok(())
    }

    /// Take a verified snapshot of this Realm while it serves, then prune.
    ///
    /// # Errors
    /// Returns [`recovery::RecoveryError`] when the database fails verification
    /// or the snapshot cannot be published; nothing is pruned in that case.
    pub fn snapshot(
        &self,
        into: Option<&Path>,
    ) -> Result<kontor_store::backup::SnapshotOutcome, recovery::RecoveryError> {
        recovery::snapshot(
            &self.config.state_root,
            into,
            kontor_core::id::Timestamp::now(),
        )
        .map(|(outcome, _pruned)| outcome)
    }

    /// Export this Realm's redacted state.
    ///
    /// # Errors
    /// Returns [`kontor_store::backup::BackupError`] when the canary scan
    /// refuses the document or the database cannot be read.
    pub fn export(
        &self,
    ) -> Result<kontor_store::backup::KontorExportV1, kontor_store::backup::BackupError> {
        let now = kontor_core::id::Timestamp::now();
        self.state
            .with_store(|store| kontor_store::backup::export_realm(store, now))
    }

    /// Stop scheduling, end every open stream and release the state root.
    ///
    /// Streams end at a frame boundary rather than being torn: a subscriber's last
    /// delivered `id` is a position it really reached, so its next connection
    /// resumes without a gap or a repeat.
    pub fn shutdown(self) {
        let realm_id = self.realm_id();
        self.state.barrier().settle(BarrierState::Pending);
        self.state.signals().stop();
        info!(realm_id = %realm_id, "realm released");
        // `self` is consumed, so the lock is dropped here and the state root is
        // free for the next daemon.
    }
}

/// What a restart found waiting for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RecoveryReport {
    /// Commands that provably never left this process. Safe to dispatch.
    pub undispatched: usize,
    /// Commands that were, or may have been, dispatched. A native lookup by the
    /// persisted correlation is the only way past one of these.
    pub ambiguous: usize,
    /// Commands a restart changes nothing about.
    pub settled: usize,
}

/// Classify every unsettled command receipt in the Realm.
///
/// Nothing is dispatched, retried or rewritten here. The point of the sweep is
/// that the *count* of ambiguous commands is known before scheduling opens, so a
/// dispatcher cannot mistake "we restarted" for "nothing was sent".
///
/// # Errors
/// Returns the repository's own refusal when the inventory cannot be read.
pub fn recover(
    store: &SqliteStore,
) -> Result<RecoveryReport, kontor_core::repository::RepositoryError> {
    let mut report = RecoveryReport::default();
    for (project_id, receipt_id) in store.unsettled_receipts()? {
        match store.classify_command_recovery(project_id, receipt_id)? {
            CommandRecovery::Undispatched { .. } => report.undispatched += 1,
            CommandRecovery::AmbiguousOrLaunched { .. } => report.ambiguous += 1,
            CommandRecovery::Settled { .. } => report.settled += 1,
            // A classification this build does not know is not "safe to send".
            // Counting it as ambiguous keeps the one rule that matters: only a
            // provably undispatched command may be dispatched again.
            _ => report.ambiguous += 1,
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_loopback_bind_is_refused_including_the_wildcards() {
        for address in ["0.0.0.0:7717", "192.168.1.10:7717", "[::]:7717"] {
            let config = DaemonConfig::at("/tmp/kontor-not-created")
                .with_bind(address.parse().expect("a socket address"));
            assert!(
                config.ensure_loopback().is_err(),
                "{address} must be refused"
            );
        }
        for address in ["127.0.0.1:7717", "[::1]:7717"] {
            let config = DaemonConfig::at("/tmp/kontor-not-created")
                .with_bind(address.parse().expect("a socket address"));
            config.ensure_loopback().expect("loopback is admitted");
        }
    }

    /// The ceilings are configuration, and the default is the Operational set.
    /// Written out as literals rather than compared against `DEFAULT_CAPACITY`
    /// itself, because a test that read the constant would agree with any edit
    /// to it — including the one that silently changes what every existing
    /// deployment admits.
    ///
    /// The mission ceiling and the adaptive ceiling are seven, not eight. Seven
    /// is the number the Operational policy fixes, and the pair matters: the
    /// adaptive window may never grow wider than the mission ceiling it admits
    /// against, or a pass could admit work the mission budget then refuses.
    #[test]
    fn the_default_capacity_is_the_operational_set() {
        assert_eq!(DEFAULT_CAPACITY.global_max_in_flight, 16);
        assert_eq!(DEFAULT_CAPACITY.project_max_in_flight, 8);
        assert_eq!(DEFAULT_CAPACITY.mission_max_in_flight, 7);
        assert_eq!(DEFAULT_CAPACITY.account_max_in_flight, 4);
        assert_eq!(DEFAULT_CAPACITY.provider_max_in_flight, 4);
        assert_eq!(DEFAULT_CAPACITY.runtime_max_in_flight, 8);
        assert_eq!(DEFAULT_CAPACITY.adaptive.initial, 4);
        assert_eq!(DEFAULT_CAPACITY.adaptive.floor, 1);
        assert_eq!(DEFAULT_CAPACITY.adaptive.ceiling, 7);
        assert_eq!(DEFAULT_CAPACITY.adaptive.growth_step, 1);
        const {
            assert!(
                DEFAULT_CAPACITY.adaptive.ceiling <= DEFAULT_CAPACITY.mission_max_in_flight,
                "the window may not grow past the ceiling it admits against"
            );
        }
        DEFAULT_CAPACITY
            .validate()
            .expect("the shipped default is a set the domain accepts");
        assert_eq!(
            DaemonConfig::at("/tmp/kontor-not-created").capacity,
            DEFAULT_CAPACITY,
            "a daemon configured the ordinary way admits what it always admitted"
        );
    }

    /// A start under ceilings the domain refuses stops before the state root is
    /// touched. The whole point of the check being in `start` and not in the
    /// builder: the operator finds out at the moment they are watching, and the
    /// directory is not left holding a lock and a database.
    #[test]
    fn a_capacity_the_domain_refuses_refuses_the_start_and_creates_nothing() {
        let directory = tempfile::TempDir::new().expect("a temporary directory");
        let state_root = directory.path().join("realm");
        let refused = DEFAULT_CAPACITY;
        let refused = CapacityConfig {
            account_max_in_flight: 0,
            ..refused
        };
        let error = Daemon::start(
            DaemonConfig::at(&state_root)
                .with_port(0)
                .with_capacity(refused),
            RuntimeRegistry::new(),
        )
        .expect_err("a zero ceiling is not a set a realm may admit work under");
        assert!(
            matches!(error, StartupError::Capacity { .. }),
            "the refusal names the capacity and not the pack: {error}"
        );
        assert!(
            !state_root.exists(),
            "a refused start leaves no state root behind"
        );
    }
}
