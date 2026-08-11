//! Fleet capacity, read and blocked through the public `asma fleet` commands.
//!
//! Kontor asks three questions and issues one instruction, all across the
//! executable boundary:
//!
//! | operation | `asma` invocation |
//! | --- | --- |
//! | capacity report | `asma fleet preflight --json` |
//! | stored availability | `asma fleet status --json` |
//! | block a model | `asma fleet block <model> --json` (dry-run by default) |
//!
//! There is no path to `~/.asma/fleet` in this module and no filesystem edge in
//! this crate: the fleet state directory has exactly one owner, and it is the
//! CLI. Reading the file directly would be a second reader of a lock-protected
//! store, which is how two writers eventually disagree.
//!
//! Fleet vocabulary — pool names, verdicts, enforcement classes — stays plain
//! text. Kontor reports it and branches only on the two typed booleans it is
//! entitled to act on: whether a model is `allowed`, and whether a reserve was
//! `breached`.

use kontor_core::DomainResult;
use kontor_core::id::{CanonicalDocument, SchemaVersion};
use serde::{Deserialize, Serialize};

use crate::{AsmaError, AsmaExecutable, WireTimestamp, ensure_wire_schema};

/// One typed failure a fleet command reports without failing the whole run.
///
/// A capacity report is deliberately fail-open: an unobservable pool is news,
/// not an outage. The distinction is preserved here rather than collapsed into an
/// error so a caller can act on partial capacity knowledge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetError {
    /// Short, stable reason token.
    pub reason: String,
    /// Human-readable detail.
    pub detail: String,
}

/// One capacity pool's reading, with the reserve policy applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetReading {
    /// The capacity pool.
    pub pool: String,
    /// How a reserve is (or is not) enforced for this pool.
    pub enforcement_class: String,
    /// Whether the pool was observed, unobservable or erroring.
    pub verdict: String,
    /// The measured value, when there is one.
    pub value: Option<String>,
    /// Age of the reading in seconds, when the source dates it.
    pub age_seconds: Option<u64>,
    /// Human-readable provenance.
    pub detail: String,
    /// Whether this pool's configured reserve was breached.
    pub breached: bool,
}

/// One requested model's dispatch verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetDecision {
    /// The canonical model key.
    pub model: String,
    /// Whether dispatch is permitted. The one field a caller branches on.
    pub allowed: bool,
    /// When a live block lifts, when one covers the key.
    pub blocked_until: Option<String>,
    /// The verbatim reason a block was recorded with.
    pub last_error: Option<String>,
}

/// One stored availability row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetRecord {
    /// `available` or `blocked`.
    pub status: String,
    /// When the block lifts, or `None` for indefinite.
    pub blocked_until: Option<String>,
    /// The verbatim reason.
    pub last_error: Option<String>,
    /// When the row was last written.
    pub recorded_at: Option<String>,
}

/// One stored availability row, with the key it belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetModelRecord {
    /// The canonical model key.
    pub model: String,
    /// The row.
    #[serde(flatten)]
    pub record: FleetRecord,
}

/// `asma fleet preflight --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetPreflight {
    /// Wire schema generation.
    pub schema_version: SchemaVersion,
    /// The operation that produced this document.
    pub operation: String,
    /// When the report was produced.
    pub observed_at: WireTimestamp,
    /// One row per capacity pool.
    pub readings: Vec<FleetReading>,
    /// One row per requested model.
    pub decisions: Vec<FleetDecision>,
    /// Typed failures that did not abort the report.
    pub errors: Vec<FleetError>,
}

/// `asma fleet status --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetStatus {
    /// Wire schema generation.
    pub schema_version: SchemaVersion,
    /// The operation that produced this document.
    pub operation: String,
    /// When the store was read.
    pub observed_at: WireTimestamp,
    /// The stored rows.
    pub records: Vec<FleetModelRecord>,
    /// Typed failures that did not abort the read.
    pub errors: Vec<FleetError>,
}

/// `asma fleet block <model> --json`.
///
/// `applied` is the whole safety story: it is `false` for a dry run, so a caller
/// can never mistake a preview for an effect. `would_change` answers the
/// idempotence question separately — a converged block writes nothing and still
/// reports success.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetBlock {
    /// Wire schema generation.
    pub schema_version: SchemaVersion,
    /// The operation that produced this document.
    pub operation: String,
    /// When the plan was computed.
    pub observed_at: WireTimestamp,
    /// The canonical model key.
    pub model: String,
    /// Whether the write actually happened.
    pub applied: bool,
    /// Whether the target differs from the current row.
    pub would_change: bool,
    /// The row before the operation, or `None` when the key was available.
    pub current: Option<FleetRecord>,
    /// The row the operation converges to.
    pub target: FleetRecord,
    /// Whether a re-read confirmed the applied row; `None` for a dry run.
    pub converged: Option<bool>,
    /// Typed failures.
    pub errors: Vec<FleetError>,
}

/// What to block, until when, and whether the caller authorizes the write.
///
/// `apply` defaults to `false` through [`Default`], so the safe operation is the
/// one you get by forgetting a field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlockRequest {
    /// The model (or whole provider) key to block.
    pub model: String,
    /// The structured reset moment as a Unix epoch. Never prose.
    pub resets_at: Option<i64>,
    /// The verbatim reason, stored and never parsed.
    pub error_text: Option<String>,
    /// Whether the caller authorizes the write. `false` is a dry run.
    pub apply: bool,
}

macro_rules! fleet_evidence {
    ($($name:ident),+ $(,)?) => {
        $(
            impl $name {
                /// Whether the command reported any typed failure.
                #[must_use]
                pub fn has_errors(&self) -> bool {
                    !self.errors.is_empty()
                }

                /// This answer as a canonical, hashed evidence document.
                ///
                /// A fleet reading is *evidence*, not a control-plane command: no
                /// [`kontor_core::receipt::CommandKind`] names it and no
                /// [`kontor_core::receipt::AggregateRef`] addresses a model, so
                /// minting a [`kontor_core::receipt::CommandReceipt`] here would
                /// mean picking a command and an aggregate that this operation is
                /// not. A caller that already holds a receipt attaches these
                /// bytes to it instead.
                ///
                /// # Errors
                /// Returns [`kontor_core::DomainError`] when the answer cannot be
                /// canonicalized — which includes an answer carrying credential
                /// material, because the canonicalizer refuses it.
                pub fn canonical_evidence(&self) -> DomainResult<CanonicalDocument> {
                    CanonicalDocument::from_serializable(self)
                }
            }
        )+
    };
}

fleet_evidence!(FleetPreflight, FleetStatus, FleetBlock);

/// Report capacity for every pool, and a verdict for each requested model.
///
/// # Errors
/// Returns [`AsmaError::Unavailable`] when the boundary could not answer or
/// answered a schema this build does not speak.
pub async fn preflight(
    asma: &AsmaExecutable,
    models: &[&str],
) -> Result<FleetPreflight, AsmaError> {
    let mut argv = vec!["fleet", "preflight", "--json"];
    for model in models {
        argv.push("--model");
        argv.push(model);
    }
    let answer: FleetPreflight = asma
        .run_json::<(), _>("fleet preflight", &argv, None)
        .await?;
    ensure_wire_schema("fleet preflight", answer.schema_version)?;
    Ok(answer)
}

/// Read the stored availability rows, for the named models or all of them.
///
/// # Errors
/// As [`preflight`].
pub async fn status(asma: &AsmaExecutable, models: &[&str]) -> Result<FleetStatus, AsmaError> {
    let mut argv = vec!["fleet", "status", "--json"];
    argv.extend_from_slice(models);
    let answer: FleetStatus = asma.run_json::<(), _>("fleet status", &argv, None).await?;
    ensure_wire_schema("fleet status", answer.schema_version)?;
    Ok(answer)
}

/// Plan — or, with explicit authority, apply — a model block.
///
/// # Errors
/// Returns [`AsmaError::Refused`] for an empty model key, and otherwise as
/// [`preflight`].
pub async fn block(asma: &AsmaExecutable, request: &BlockRequest) -> Result<FleetBlock, AsmaError> {
    if request.model.trim().is_empty() {
        return Err(AsmaError::refused(
            "fleet block",
            "a block needs a model key",
        ));
    }
    // Owned first, borrowed second: the epoch has to outlive the argv slice.
    let resets_at = request.resets_at.map(|epoch| epoch.to_string());
    let mut argv = vec!["fleet", "block", request.model.as_str(), "--json"];
    if let Some(resets_at) = resets_at.as_deref() {
        argv.push("--resets-at");
        argv.push(resets_at);
    }
    if let Some(error_text) = request.error_text.as_deref() {
        argv.push("--error");
        argv.push(error_text);
    }
    // The authority is spelled out even when it is absent: relying on the CLI's
    // own default would make this call site's safety depend on a flag default
    // somewhere else.
    argv.push(if request.apply {
        "--no-dry-run"
    } else {
        "--dry-run"
    });

    let answer: FleetBlock = asma.run_json::<(), _>("fleet block", &argv, None).await?;
    ensure_wire_schema("fleet block", answer.schema_version)?;
    if answer.applied && !request.apply {
        // The one answer that must never be believed: an unauthorized write.
        return Err(AsmaError::refused(
            "fleet block",
            "the boundary reported an applied write for an unauthorized dry run",
        ));
    }
    Ok(answer)
}
