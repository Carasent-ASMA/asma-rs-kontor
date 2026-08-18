//! `kontor-mcp` — the capability-gated MCP surface of one Kontor realm.
//!
//! # What this crate is
//!
//! A thin wrapper over `kontord`'s public `/v1` contract, and nothing else. One
//! tool invocation does exactly four things:
//!
//! 1. resolves a declared tool from [`registry::REGISTRY`];
//! 2. requires the tool's minimum authority against the one tier this process was
//!    configured with;
//! 3. validates the arguments against the tool's declared schema;
//! 4. makes exactly one authenticated loopback `/v1` request and returns the
//!    daemon's status and body unchanged.
//!
//! # What this crate is not
//!
//! It owns no business decision and no state. It does not reach SQLite, a
//! scheduler, a workflow, a profile catalogue, a team template, a ticket
//! integration, a runtime adapter, Paseo, Jira or AgentsRoom — and it has no
//! dependency path to any of them, which the contract crate asserts against the
//! real dependency graph rather than by inspection.
//!
//! `kontord` owns validation beyond the wire schema, idempotency, replay,
//! receipts, orchestration, scheduling, lifecycle, runtime settlement, ticket
//! reconciliation and every durable effect. This crate does not retry a write,
//! does not generate an idempotency key, does not cache an answer and does not
//! rewrite a status or a code. Where those rules could be broken quietly, the
//! mutant suite in the contract crate breaks them on purpose and requires the
//! break to fail a test.
//!
//! # The one authority rule
//!
//! A process holds exactly one credential tier, read from the realm's `0600`
//! credential file. There is no per-call authority and no escalation argument:
//! running at two authorities means running two servers. That is what makes the
//! seat configurations meaningful — a reviewer's observer server cannot mutate
//! anything, because it does not hold a secret that could.

pub mod capability;
pub mod client;
pub mod dispatch;
pub mod fake;
pub mod registry;
pub mod server;

pub use capability::{Admitted, Denied, Gate};
pub use client::{
    CallerTier, Credential, Endpoint, FrameBudget, HttpTransport, LocalError, Method, Reply,
    Request, Transport, TransportFailure,
};
pub use dispatch::{Dispatcher, Envelope, Failure};
pub use registry::{
    ArgSpec, ArgType, FieldSpec, NON_AGENT_ROUTES, OpKind, Place, REGISTRY, ToolSpec,
};
pub use server::{KontorMcp, serve, serve_stdio};

/// Build the dispatcher one seat is configured with.
///
/// This is the only place a credential is selected, and it selects exactly the
/// tier it was told. A caller cannot widen it afterwards: the gate is built from
/// the transport's tier, and the transport holds the one secret it read.
///
/// # Errors
/// Returns [`LocalError`] when the endpoint is not a loopback address, when the
/// realm's credential file is missing or unreadable, or when the HTTP client
/// cannot be built. Nothing is dispatched in any of those cases.
pub fn connect(
    state_root: &std::path::Path,
    base_url: Option<&str>,
    tier: CallerTier,
) -> Result<Dispatcher, LocalError> {
    let endpoint = Endpoint::resolve(state_root, base_url)?;
    let credential = Credential::read(state_root, tier)?;
    let transport = HttpTransport::new(endpoint, credential)?;
    Ok(Dispatcher::new(Box::new(transport)))
}
