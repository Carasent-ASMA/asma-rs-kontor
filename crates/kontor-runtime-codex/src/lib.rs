//! `kontor-runtime-codex` — the narrow direct Codex adapter, behind
//! [`kontor_runtime::RuntimeAdapter`].
//!
//! Every other runtime in the fleet runs work under whichever coding account its
//! daemon happens to be logged in as, and says so:
//! `kontor-runtime-ao` and `kontor-runtime-paseo` both declare
//! `account_env: false`. This one exists because Codex binds its identity to a
//! directory — `CODEX_HOME` — so it is the only runtime here that can answer
//! *which account executed this work* with evidence instead of an assumption.
//!
//! That is the whole of its remit. It is **not** a general session or terminal
//! runtime, and it is deliberately narrower than its siblings:
//!
//! ```text
//! one task worktree      ->  one verified directory (nothing is created)
//!   one admitted seat    ->  one `codex exec --json` child process
//!     its stdout         ->  the only session content that exists
//! ```
//!
//! Everything else follows from those three lines. There is no session server, so
//! there is nothing to discover, adopt, resume, page or send a second message
//! into — each of those is declared unsupported and refused before a process is
//! started. And a one-shot process cannot tell a finished turn from a crash, so
//! **no ending is ever a verdict**: every observation this adapter produces
//! returns `None` from `terminal_evidence()`, by two independent routes.
//!
//! # Layout
//!
//! * [`wire`] — the pinned `codex exec --json` frame shape, the ways a process
//!   ends, and the operator's non-secret home marker. Pure.
//! * [`client`] — the argv builder, the transport seam, and the live transport
//!   that actually spawns and reads a child.
//! * [`fixture`] — a recorded process with a redacted dispatch ledger, so a claim
//!   about the process table is a count rather than an inference.
//! * [`adapter`] — account isolation, admission, workspace verification, process
//!   evidence and session content.
//! * [`usage`] — the pre-flight quota probe. The one module here that reads a
//!   credential, and fenced accordingly; see its own documentation for why the
//!   adapter's "never opens `auth.json`" rule is a rule about the launch path.

pub mod adapter;
pub mod client;
pub mod fixture;
pub mod usage;
pub mod wire;

pub use adapter::{
    CodexAccountAdmission, CodexAccountAuthority, CodexAccountReceipt, CodexAccountRequest,
    CodexAdapter, CodexCheckpoint, CodexConfig, CodexExecutionRecord, CodexPinnedAccounts,
    UNSUPPORTED,
};
pub use client::{
    CodexCommand, CodexDrained, CodexLiveTransport, CodexLiveness, CodexStarted, CodexTransport,
    EXEC_ROUTE, PreparedCommand,
};
pub use fixture::{CodexDispatch, CodexScript, RecordedCodex};
pub use usage::{
    AUTH_FILE_NAME, CodexLiveUsageProbe, CodexRateLimits, CodexUsage, CodexUsageProbe,
    CodexUsageToken, CodexWindow, ObservedHeadroom, USAGE_ENDPOINT, classify_usage,
};
pub use wire::{
    CODEX_EXEC_SCHEMA, CODEX_HOME, CodexEnding, CodexFrame, CodexHomeMarker, MARKER_FILE_NAME,
    MARKER_SCHEMA_VERSION,
};
