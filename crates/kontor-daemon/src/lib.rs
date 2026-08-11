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

pub mod credentials;
pub mod lock;
pub mod runtimes;

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

use kontor_api::state::{
    ApiParts, ApiState, BarrierState, RuntimeRegistry, SchedulingBarrier, SessionRegistry,
    StreamSignals,
};
use kontor_core::id::RealmId;
use kontor_store::{CommandRecovery, SqliteStore};
use tracing::{info, warn};

use crate::credentials::CredentialError;
use crate::lock::{LockError, StateRootLock};
use crate::runtimes::FleetError;

/// The database file's name inside a state root.
pub const DATABASE_FILE: &str = "kontor.db";

/// The loopback port a Kontor daemon binds by default.
pub const DEFAULT_PORT: u16 = 7717;

/// How old a confirmation may be and still count as fresh, in seconds.
pub const DEFAULT_EVIDENCE_WINDOW_SECONDS: i64 = 60;

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
        }
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
    /// Returns [`StartupError`] when the address is not loopback, the state root
    /// cannot be prepared or claimed, the database cannot be opened, or the
    /// credentials cannot be established. Every one of them leaves the state root
    /// exactly as it was.
    pub fn start(config: DaemonConfig, runtimes: RuntimeRegistry) -> Result<Self, StartupError> {
        // The address is judged before anything is created, so a misconfigured
        // daemon does not leave a lock file and a database behind.
        config.ensure_loopback()?;
        std::fs::create_dir_all(&config.state_root)
            .map_err(|source| StartupError::StateRoot { source })?;
        let lock = StateRootLock::acquire(&config.state_root)?;
        let store = SqliteStore::open(&config.state_root.join(DATABASE_FILE))
            .map_err(|source| StartupError::Store { source })?;
        let credentials = credentials::open_or_create(&config.state_root)?;
        let realm_id = store.realm_id();

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
        });
        info!(
            realm_id = %realm_id,
            state_root = %config.state_root.display(),
            bind = %config.bind,
            "realm claimed; scheduling is shut until reconciliation finishes"
        );
        Ok(Self {
            state,
            config,
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
        let settings = runtimes::read(&config.state_root)?;
        let registry = runtimes::build_registry(&settings)?;
        Self::start(config, registry)
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
            let held: Vec<_> = bindings
                .iter()
                .filter(|binding| binding.binding.identity.runtime_kind == family)
                .filter_map(|binding| self.state.sessions().get(binding.binding.id))
                .collect();
            match adapter.reconcile(&held).await {
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
}
