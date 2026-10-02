//! `kontor-bootstrap` — the local installation surface of a Kontor realm.
//!
//! This crate is deliberately not a semantic Kontor tool: it owns no `/v1`
//! route, no MCP tool and no registry entry. It exists so one non-interactive,
//! agent-runnable program can install one coherent artifact set, describe the
//! per-user service that would run it, and patch exactly one `kontor` entry into
//! each supported MCP client without touching anything else.
//!
//! # Boundaries this unit enforces
//!
//! - Every root is injected. The crate never resolves a home directory and
//!   never writes outside the roots a caller names, so qualification runs reach
//!   a synthetic fixture tree only.
//! - The service port has no production implementation here: only a fake
//!   manager that records what a real manager would be asked to do, and an
//!   unsupported-platform result. Nothing executes `launchctl`, `systemctl`,
//!   an installer or any other host operation.
//! - Receipts are typed and redacted: artifact names, versions, digests and
//!   typed dispositions only. No path, argv, credential value or secret
//!   material is serialized.
//!
//! # Reference
//!
//! The client shapes implemented here are the ones recorded by the accepted
//! KON-OP-08 architecture table (`mcp_servers.kontor` for Codex,
//! `mcp.servers.kontor` for OpenCode, `servers.kontor` for VS Code/Copilot,
//! `mcpServers.kontor` for Cursor, and the native `claude mcp add-json` command
//! boundary for Claude Code). A shape the repository does not document is not
//! guessed: the adapter refuses it with a typed conflict instead.

pub mod artifact;
pub mod client;
pub mod fault;
pub mod receipt;
pub mod service;

pub use artifact::install::{
    ArtifactStatus, InstallError, InstallOutcome, Installer, RecoveryOutcome,
};
pub use artifact::manifest::{ArtifactManifest, ArtifactName, ManifestError};
pub use client::claude::{ClaudeAdapter, ClaudeBoundary, FakeClaudeBoundary};
pub use client::file_adapter::{ClientHome, FileClientAdapter};
pub use client::{
    AdapterError, AdapterResult, ClientAdapter, ClientId, ClientReceipt, ConflictReason,
    EntryState, ObservedHash, ServerSpec,
};
pub use fault::{FailPoint, FaultInjector, NoFaults};
pub use receipt::{
    ArtifactDisposition, ArtifactReceipt, BootstrapReceipt, ClientReport, ReceiptError,
};
pub use service::{
    FakeServiceManager, ServiceCall, ServiceError, ServicePlan, ServicePlatform, ServiceReceipt,
    ServiceState, UnsupportedServiceManager, UserServiceManager,
};
