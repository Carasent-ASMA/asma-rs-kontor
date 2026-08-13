//! Everything the handlers are given, and nothing they could have opened
//! themselves.
//!
//! This crate never opens a database, never locks a state root, never mints a
//! credential and never learns a runtime endpoint. It is handed an already-open
//! store, already-minted secrets, an already-built adapter registry and the
//! barrier that says whether startup reconciliation has finished. That is what
//! keeps the composition root the only place those decisions are made.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use kontor_core::id::{RealmId, RuntimeBindingId, RuntimeKindKey};
use kontor_core::realm::RealmMetadata;
use kontor_runtime::adapter::RuntimeAdapter;
use kontor_runtime::capability::RuntimeBindingSnapshot;
use kontor_store::SqliteStore;
use tokio::sync::watch;

use crate::applications::Applications;
use crate::auth::{IngressPolicy, RealmCredentials};
use crate::error::{ApiError, ApiErrorCode};

/// How far startup reconciliation has got, and therefore whether scheduling may
/// run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BarrierState {
    /// Reconciliation has not finished. Nothing may be scheduled yet.
    Pending,
    /// Reconciliation finished and scheduling is open.
    Open,
    /// Reconciliation could not complete. Scheduling stays shut, deliberately:
    /// a sweep that did not finish proves nothing about what it did not reach.
    Failed,
}

impl BarrierState {
    /// Whether scheduling may run.
    #[must_use]
    pub const fn is_open(self) -> bool {
        matches!(self, Self::Open)
    }
}

/// The gate between "the process started" and "work may be dispatched".
///
/// It is a watch channel rather than a flag so a scheduler can *wait* on it
/// instead of polling, and so `/v1/health` reports the same value the scheduler
/// is blocked on rather than a second copy of it.
#[derive(Debug, Clone)]
pub struct SchedulingBarrier {
    sender: Arc<watch::Sender<BarrierState>>,
}

impl Default for SchedulingBarrier {
    fn default() -> Self {
        Self::new()
    }
}

impl SchedulingBarrier {
    /// A barrier that starts shut.
    #[must_use]
    pub fn new() -> Self {
        Self {
            sender: Arc::new(watch::channel(BarrierState::Pending).0),
        }
    }

    /// The current state.
    #[must_use]
    pub fn state(&self) -> BarrierState {
        *self.sender.borrow()
    }

    /// Record how reconciliation ended, opening the barrier only on success.
    pub fn settle(&self, state: BarrierState) {
        // `send_replace`, not `send`: a watch `send` refuses when no receiver is
        // subscribed *and does not store the value*, so a barrier nobody happens
        // to be waiting on would silently stay shut for the health route too.
        // Recording the state must not depend on somebody already listening.
        self.sender.send_replace(state);
    }

    /// Wait until the barrier is no longer pending, and report how it settled.
    pub async fn settled(&self) -> BarrierState {
        let mut receiver = self.sender.subscribe();
        loop {
            let state = *receiver.borrow_and_update();
            if state != BarrierState::Pending {
                return state;
            }
            if receiver.changed().await.is_err() {
                return state;
            }
        }
    }
}

/// The runtimes this daemon was configured with, addressed by family.
///
/// The registry holds adapters, never endpoints or credentials: what a Paseo or
/// AO adapter needs to reach its runtime is inside the adapter, built by the
/// composition root from configuration this crate never sees.
#[derive(Clone, Default)]
pub struct RuntimeRegistry {
    adapters: BTreeMap<RuntimeKindKey, Arc<dyn RuntimeAdapter>>,
}

impl std::fmt::Debug for RuntimeRegistry {
    /// Names the families only. An adapter's own `Debug` may carry client
    /// configuration, and this type is reachable from a state dump.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeRegistry")
            .field("families", &self.adapters.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl RuntimeRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register the adapter for one runtime family.
    #[must_use]
    pub fn with(mut self, kind: RuntimeKindKey, adapter: Arc<dyn RuntimeAdapter>) -> Self {
        self.adapters.insert(kind, adapter);
        self
    }

    /// The adapter for one family, if this daemon was configured with it.
    #[must_use]
    pub fn get(&self, kind: &RuntimeKindKey) -> Option<Arc<dyn RuntimeAdapter>> {
        self.adapters.get(kind).map(Arc::clone)
    }

    /// Every configured family, in key order.
    pub fn families(&self) -> impl Iterator<Item = &RuntimeKindKey> {
        self.adapters.keys()
    }
}

/// The frozen binding snapshots this process is holding.
///
/// A binding's *capabilities* are frozen evidence about the moment the session
/// was bound, and they are deliberately not persisted: the durable log stores
/// bindings, receipts and continuity evidence, not a copy of what a runtime once
/// claimed it could do. So the snapshots live here, recorded when a session is
/// launched, adopted or re-established by reconciliation.
///
/// A session this process has no snapshot for is answered as
/// [`ApiErrorCode::StaleBinding`] rather than by rebuilding a plausible one from
/// fresh discovery. That refusal is the point: re-grading an old session against
/// today's capabilities is exactly the freeze violation the adapter contract
/// exists to prevent.
#[derive(Debug, Clone, Default)]
pub struct SessionRegistry {
    bindings: Arc<Mutex<BTreeMap<RuntimeBindingId, RuntimeBindingSnapshot>>>,
}

impl SessionRegistry {
    /// A registry holding nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the snapshot a runtime issued for a binding.
    pub fn record(&self, snapshot: RuntimeBindingSnapshot) {
        let mut held = self.bindings.lock().unwrap_or_else(PoisonError::into_inner);
        held.insert(snapshot.binding_id(), snapshot);
    }

    /// Forget a binding, as a closed run or a generation change must.
    pub fn forget(&self, binding_id: RuntimeBindingId) {
        let mut held = self.bindings.lock().unwrap_or_else(PoisonError::into_inner);
        held.remove(&binding_id);
    }

    /// The frozen snapshot for one binding, if this process holds it.
    #[must_use]
    pub fn get(&self, binding_id: RuntimeBindingId) -> Option<RuntimeBindingSnapshot> {
        let held = self.bindings.lock().unwrap_or_else(PoisonError::into_inner);
        held.get(&binding_id).cloned()
    }

    /// How many bindings are held.
    #[must_use]
    pub fn len(&self) -> usize {
        let held = self.bindings.lock().unwrap_or_else(PoisonError::into_inner);
        held.len()
    }

    /// Whether the registry holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A durable subscriber's wake-up signal and the graceful-shutdown flag.
///
/// The control-plane feed reads committed rows and then *waits*: it never polls a
/// clock, so a quiet Realm costs nothing and a new event is delivered as soon as
/// the writer says one landed. Shutdown ends every open stream rather than
/// dropping it mid-frame.
#[derive(Debug, Clone)]
pub struct StreamSignals {
    appended: Arc<watch::Sender<u64>>,
    stopping: Arc<watch::Sender<bool>>,
}

impl Default for StreamSignals {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamSignals {
    /// Fresh signals: nothing appended, not stopping.
    #[must_use]
    pub fn new() -> Self {
        Self {
            appended: Arc::new(watch::channel(0).0),
            stopping: Arc::new(watch::channel(false).0),
        }
    }

    /// Tell every waiting subscriber that the control-plane log moved.
    pub fn appended(&self) {
        self.appended
            .send_modify(|ticks| *ticks = ticks.wrapping_add(1));
    }

    /// Begin a graceful stop: every open stream ends at its next boundary.
    ///
    /// `send_replace` for the same reason the barrier uses it: a watch `send`
    /// with no current subscriber discards the value, which would make a stop
    /// requested before the first subscriber connects vanish — and that
    /// subscriber would then wait for a shutdown that already happened.
    pub fn stop(&self) {
        self.stopping.send_replace(true);
    }

    /// Whether a graceful stop has begun.
    #[must_use]
    pub fn is_stopping(&self) -> bool {
        *self.stopping.borrow()
    }

    /// A receiver for append ticks.
    #[must_use]
    pub fn appends(&self) -> watch::Receiver<u64> {
        self.appended.subscribe()
    }

    /// A receiver for the shutdown flag.
    #[must_use]
    pub fn stops(&self) -> watch::Receiver<bool> {
        self.stopping.subscribe()
    }
}

/// Everything the composition root hands the HTTP surface.
pub struct ApiParts {
    /// The open, migrated store this Realm's rows live in.
    pub store: SqliteStore,
    /// The Realm's bearer secrets, one per tier.
    pub credentials: RealmCredentials,
    /// Which hosts and origins to answer.
    pub ingress: IngressPolicy,
    /// The configured runtime adapters.
    pub runtimes: RuntimeRegistry,
    /// The frozen binding snapshots this process holds.
    pub sessions: SessionRegistry,
    /// The startup-reconciliation gate.
    pub barrier: SchedulingBarrier,
    /// Stream wake-up and shutdown.
    pub signals: StreamSignals,
    /// How old a confirmation may be and still count as fresh, in seconds.
    pub evidence_window_seconds: i64,
    /// The composed application services the public operations run through.
    pub applications: Applications,
}

struct Inner {
    realm: RealmMetadata,
    /// One SQLite connection, serialized.
    ///
    /// ponytail: one global store lock. SQLite calls here are short synchronous
    /// reads and single-statement transactions on a loopback, single-operator
    /// daemon, so contention is not the bottleneck. If it ever is, the upgrade is
    /// a connection pool inside `kontor-store` — not a second lock here.
    store: Mutex<SqliteStore>,
    credentials: RealmCredentials,
    ingress: IngressPolicy,
    runtimes: RuntimeRegistry,
    sessions: SessionRegistry,
    barrier: SchedulingBarrier,
    signals: StreamSignals,
    evidence_window_seconds: i64,
    applications: Applications,
}

/// The handler state. Cheap to clone; one Realm for its whole lifetime.
#[derive(Clone)]
pub struct ApiState(Arc<Inner>);

impl std::fmt::Debug for ApiState {
    /// Realm and configuration shape only. Credentials, adapters and the store
    /// are all reachable from here and none of them belongs in a log line.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApiState")
            .field("realm_id", &self.realm_id())
            .field("barrier", &self.barrier().state())
            .finish_non_exhaustive()
    }
}

impl ApiState {
    /// Assemble the handler state from what the composition root opened.
    #[must_use]
    pub fn new(parts: ApiParts) -> Self {
        let realm = parts.store.realm_metadata().clone();
        Self(Arc::new(Inner {
            realm,
            store: Mutex::new(parts.store),
            credentials: parts.credentials,
            ingress: parts.ingress,
            runtimes: parts.runtimes,
            sessions: parts.sessions,
            barrier: parts.barrier,
            signals: parts.signals,
            evidence_window_seconds: parts.evidence_window_seconds,
            applications: parts.applications,
        }))
    }

    /// The composed application services.
    ///
    /// Handlers reach the work graph, the scheduler and the lifecycle only
    /// through this port, so a handler cannot open a transaction, resolve a
    /// profile or address a runtime on its own.
    #[must_use]
    pub fn applications(&self) -> &Applications {
        &self.0.applications
    }

    /// This Realm's immutable identity.
    #[must_use]
    pub fn realm_id(&self) -> RealmId {
        self.0.realm.realm_id
    }

    /// This Realm's immutable metadata.
    #[must_use]
    pub fn realm(&self) -> &RealmMetadata {
        &self.0.realm
    }

    /// The Realm's credentials.
    #[must_use]
    pub fn credentials(&self) -> &RealmCredentials {
        &self.0.credentials
    }

    /// The configured ingress policy.
    #[must_use]
    pub fn ingress(&self) -> &IngressPolicy {
        &self.0.ingress
    }

    /// The configured runtime adapters.
    #[must_use]
    pub fn runtimes(&self) -> &RuntimeRegistry {
        &self.0.runtimes
    }

    /// The frozen binding snapshots this process holds.
    #[must_use]
    pub fn sessions(&self) -> &SessionRegistry {
        &self.0.sessions
    }

    /// The startup-reconciliation gate.
    #[must_use]
    pub fn barrier(&self) -> &SchedulingBarrier {
        &self.0.barrier
    }

    /// Stream wake-up and shutdown.
    #[must_use]
    pub fn signals(&self) -> &StreamSignals {
        &self.0.signals
    }

    /// How old a confirmation may be and still count as fresh.
    #[must_use]
    pub fn evidence_window_seconds(&self) -> i64 {
        self.0.evidence_window_seconds
    }

    /// Run one short synchronous unit of store work.
    ///
    /// The guard never crosses an `await`: a handler reads or writes here, drops
    /// the lock, and only then talks to a runtime. A poisoned lock is recovered
    /// rather than propagated — the store's own transactions are all-or-nothing,
    /// so a panic in one handler leaves no half-written state for the next one to
    /// trip over.
    pub fn with_store<T>(&self, work: impl FnOnce(&SqliteStore) -> T) -> T {
        let store = self.0.store.lock().unwrap_or_else(PoisonError::into_inner);
        work(&store)
    }

    /// Build a refusal in this Realm.
    #[must_use]
    pub fn refuse(&self, code: ApiErrorCode, rule: &'static str) -> ApiError {
        ApiError::new(self.realm_id(), code, rule)
    }
}
