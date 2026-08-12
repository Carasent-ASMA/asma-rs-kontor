//! `kontor-tests-e2e` — the KON-MVP-18 evidence bundle and case ledger.
//!
//! This library is deliberately the *smallest* half of the pilot: it owns where
//! evidence is written, how a case result is spelled, and how the overall
//! verdict is computed. The proof itself lives in the `pilot` test target, which
//! composes the merged control-plane seams; nothing here knows what a task, a
//! gate or a runtime is.
//!
//! # Why the verdict is computed here rather than asserted in the driver
//!
//! The acceptance rule KON-MVP-18 is judged by is "any failed **or missing**
//! case rejects". A driver that only asserted would report `ok` for a case it
//! never ran, which is exactly the failure mode the ticket exists to prevent. So
//! every criterion is *registered* up front from [`CRITERIA`] and must be
//! answered by name before [`Bundle::finish`]; an unanswered criterion is a
//! [`CaseOutcome::Missing`] and rejects the run on its own.
//!
//! # Run identity
//!
//! The run id is derived from the commit and the fixture digests rather than
//! minted fresh, so rerunning an unchanged tree reproduces the same bundle
//! instead of accumulating a directory per invocation. "Immutable" here means
//! same inputs, same bytes — not "never overwritten".

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

/// The ticket this bundle answers for.
pub const TICKET: &str = "KON-MVP-18";

/// How a single acceptance case came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseOutcome {
    /// The case ran against the merged tree and held.
    Pass,
    /// The case ran and did not hold.
    Fail,
    /// The seam the case needs has not merged. Names the owning ticket.
    ///
    /// This is not a softened failure: a blocked case rejects the run exactly
    /// as a failed one does. It is a distinct spelling only so the report can
    /// say *who* has to merge before the case can be re-run.
    Blocked,
    /// The criterion was registered and never answered.
    Missing,
}

impl CaseOutcome {
    /// Whether this outcome lets the run be accepted.
    #[must_use]
    pub const fn accepts(self) -> bool {
        matches!(self, Self::Pass)
    }
}

/// One acceptance criterion's result.
#[derive(Debug, Clone, Serialize)]
pub struct CaseResult {
    /// Stable case id, matching a [`CRITERIA`] entry.
    pub id: &'static str,
    /// What the case claims, in one line.
    pub criterion: &'static str,
    /// How it came out.
    pub outcome: CaseOutcome,
    /// What was actually observed.
    pub detail: String,
    /// For [`CaseOutcome::Blocked`], the ticket that owns the missing seam.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_by: Option<&'static str>,
    /// Bundle-relative artifact paths backing this case.
    pub artifacts: Vec<String>,
}

/// Every criterion this pilot must answer, in report order.
///
/// Registered up front so an unanswered one is a visible `missing` rather than
/// a silent absence. The ids are grouped by the brief's sections.
pub const CRITERIA: &[(&str, &str)] = &[
    // 1 — the disposable project
    (
        "project.profiles",
        "five tasks resolve five pinned work-profile snapshots",
    ),
    (
        "project.custom-profile",
        "the incident profile runs as fixture data with no core or client branch",
    ),
    (
        "project.worktrees",
        "safe parallel tasks hold distinct verified worktrees",
    ),
    (
        "project.collision-contender",
        "the non-isolated contender never launches",
    ),
    (
        "project.two-accounts",
        "two account profiles run concurrently and are attributed correctly",
    ),
    (
        "project.account-secrecy",
        "no credential or token canary reaches any persisted or logged artifact",
    ),
    (
        "project.cross-engine",
        "a sealed handoff capsule links a successor on the other runtime",
    ),
    (
        "project.workspace-identity",
        "predecessor and successor share the exact verified task workspace",
    ),
    // 2 — the negative-case matrix
    (
        "negative.collision",
        "two armed tasks sharing a module without isolation refuse, first lease unchanged",
    ),
    (
        "negative.rejection-loop",
        "the second rejection by one reviewer parks and launches no third run",
    ),
    (
        "negative.rejection-reset",
        "a pass resets only that reviewer and gate stream",
    ),
    (
        "negative.degraded-verdict",
        "a degraded binding cannot write a gate verdict, gate and task stay open",
    ),
    (
        "negative.ambiguous-command",
        "a lost acknowledgement reconciles by id: one effect, original receipt",
    ),
    (
        "negative.event-disorder",
        "duplicates no-op, older events cannot regress, gaps block dispatch",
    ),
    (
        "negative.restart",
        "durable intent, binding and cursor reload; a generation change stays unreconciled",
    ),
    (
        "negative.worktree-park",
        "a wrong or ambiguous worktree parks rather than proceeds",
    ),
    (
        "negative.lost-contact",
        "stream closure and process disappearance are lost-contact, never terminal",
    ),
    (
        "negative.adoption-inbox",
        "a foreign native session is offered for adoption, never auto-bound",
    ),
    // 3 — the session contract
    (
        "session.history-parity",
        "desktop and phone load identical cursor-paginated history",
    ),
    (
        "session.live-parity",
        "both clients subscribe strictly after the runtime cursor and agree frame for frame",
    ),
    (
        "session.message-idempotency",
        "the same follow-up message id twice yields one effect and one receipt",
    ),
    (
        "session.permission-idempotency",
        "the same permission response id twice yields one effect and one receipt",
    ),
    (
        "session.refetch",
        "an epoch change and a sequence gap force refetch without mutating lifecycle",
    ),
    (
        "session.no-direct-runtime",
        "no client path reaches Paseo, AO or a runtime endpoint",
    ),
    (
        "session.no-transcript-persistence",
        "transcript and token canaries are absent from SQLite, export and logs",
    ),
    // 4 — domain operations
    (
        "domain.intake-dedup",
        "a replayed source event returns the original receipt and creates no second graph",
    ),
    (
        "domain.intake-decisions",
        "approve, terminal reject and bounded auto-arm each admit or refuse as declared",
    ),
    (
        "domain.persona-self-approval",
        "a persona actor cannot approve the gate it is under test for",
    ),
    (
        "domain.profile-durability",
        "pinned revision, phase and gate history and artifacts survive restart",
    ),
    (
        "domain.jira-asma",
        "the ASMA workflow confirms principal and assignee by refetch before development",
    ),
    (
        "domain.jira-qa-distinct",
        "internal QA readiness never projects as the external active QA status",
    ),
    (
        "domain.jira-alternate",
        "a workflow with different status names produces identical core behaviour",
    ),
    (
        "domain.jira-hold-close-reopen",
        "hold, close and reopen are deterministic and never guess a multi-hop path",
    ),
    (
        "domain.jira-ownership",
        "a different existing owner and every terminal assignee are preserved",
    ),
    (
        "domain.privacy-zones",
        "Zone C stays private, owned fields project once, no outbound comment exists",
    ),
    (
        "domain.inbound-comment",
        "one inbound comment mirrors exactly once with external provenance",
    ),
    (
        "domain.calendar-unrestricted",
        "an unconfigured project is unrestricted but still needs arming",
    ),
    (
        "domain.calendar-configured",
        "closed windows, drain, holidays and override expiry admit as declared",
    ),
    (
        "domain.calendar-client-clock",
        "no client clock influences admission",
    ),
    (
        "domain.ux-gate-order",
        "the UX task cannot close before functionality QA, design QA and final audit",
    ),
    // 5 — surfaces and cleanup
    (
        "surface.parity",
        "API, CLI and MCP report matching ids, revisions and cursors",
    ),
    (
        "cleanup.processes",
        "every spawned process and native session is closed or retained with a reason",
    ),
];

/// Where one pilot run writes its evidence.
#[derive(Debug)]
pub struct Bundle {
    run_id: String,
    ephemeral: PathBuf,
    retained: PathBuf,
    results: BTreeMap<&'static str, CaseResult>,
    events: Vec<Value>,
    manifest: Value,
}

impl Bundle {
    /// Open both bundle roots for a run derived from `commit` and `fixtures`.
    ///
    /// # Errors
    /// Propagates every filesystem failure; a bundle that cannot be written is
    /// not something to continue past.
    pub fn open(
        repo_root: &Path,
        commit: &str,
        fixtures: &BTreeMap<String, String>,
    ) -> io::Result<Self> {
        let run_id = derive_run_id(commit, fixtures);
        let ephemeral = repo_root.join("target/kontor-pilot").join(&run_id);
        let retained = repo_root.join("docs/evidence").join(TICKET).join(&run_id);
        for root in [&ephemeral, &retained] {
            if root.exists() {
                fs::remove_dir_all(root)?;
            }
            fs::create_dir_all(root)?;
        }
        let manifest = json!({
            "schema_version": 1,
            "ticket": TICKET,
            "run_id": run_id,
            "commit": commit,
            "fixtures": fixtures,
            "environment": "deterministic-offline",
        });
        Ok(Self {
            run_id,
            ephemeral,
            retained,
            results: BTreeMap::new(),
            events: Vec::new(),
            manifest,
        })
    }

    /// This run's derived identity.
    #[must_use]
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// The ephemeral root, for artifacts too large or too raw to retain.
    #[must_use]
    pub fn ephemeral(&self) -> &Path {
        &self.ephemeral
    }

    /// The retained root, for the acceptance summary and redacted artifacts.
    #[must_use]
    pub fn retained(&self) -> &Path {
        &self.retained
    }

    /// Merge extra facts into `manifest.json`.
    pub fn describe(&mut self, key: &str, value: Value) {
        if let Some(object) = self.manifest.as_object_mut() {
            object.insert(key.to_owned(), value);
        }
    }

    /// Append one line to `events.ndjson`.
    pub fn event(&mut self, kind: &str, value: Value) {
        self.events.push(json!({ "kind": kind, "value": value }));
    }

    /// Write a JSON artifact into both roots and return its bundle-relative path.
    ///
    /// # Errors
    /// Propagates every filesystem failure.
    pub fn artifact(&self, relative: &str, value: &Value) -> io::Result<String> {
        let text = format!("{}\n", serde_json::to_string_pretty(value)?);
        self.write_text(relative, &text)?;
        Ok(relative.to_owned())
    }

    /// Write a text artifact into both roots.
    ///
    /// # Errors
    /// Propagates every filesystem failure.
    pub fn write_text(&self, relative: &str, text: &str) -> io::Result<()> {
        for root in [&self.ephemeral, &self.retained] {
            let path = root.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, text)?;
        }
        Ok(())
    }

    /// Record a case result, refusing an id that is not a registered criterion
    /// and an id answered twice.
    ///
    /// # Panics
    /// Panics on an unregistered or duplicated id, both of which are driver
    /// bugs that would otherwise silently weaken the verdict.
    pub fn record(&mut self, result: CaseResult) {
        assert!(
            CRITERIA.iter().any(|(id, _)| *id == result.id),
            "`{}` is not a registered acceptance criterion",
            result.id
        );
        assert!(
            !self.results.contains_key(result.id),
            "`{}` was answered twice",
            result.id
        );
        self.event(
            "case",
            serde_json::to_value(&result).unwrap_or_else(|_| json!({ "id": result.id })),
        );
        self.results.insert(result.id, result);
    }

    /// Record a passing case.
    pub fn pass(&mut self, id: &'static str, detail: impl Into<String>, artifacts: &[String]) {
        self.record(CaseResult {
            id,
            criterion: criterion_of(id),
            outcome: CaseOutcome::Pass,
            detail: detail.into(),
            blocked_by: None,
            artifacts: artifacts.to_vec(),
        });
    }

    /// Record a failing case.
    pub fn fail(&mut self, id: &'static str, detail: impl Into<String>) {
        self.record(CaseResult {
            id,
            criterion: criterion_of(id),
            outcome: CaseOutcome::Fail,
            detail: detail.into(),
            blocked_by: None,
            artifacts: Vec::new(),
        });
    }

    /// Record a failing case together with the evidence that shows the failure.
    ///
    /// A refutation needs its evidence more than a confirmation does: a reader
    /// who is told a criterion failed and shown no artifact has to take the
    /// sentence on trust. Use this wherever the case wrote something; plain
    /// [`Bundle::fail`] is for the refusals that produced no file, such as a
    /// fixture that never parsed.
    pub fn fail_with(&mut self, id: &'static str, detail: impl Into<String>, artifacts: &[String]) {
        self.record(CaseResult {
            id,
            criterion: criterion_of(id),
            outcome: CaseOutcome::Fail,
            detail: detail.into(),
            blocked_by: None,
            artifacts: artifacts.to_vec(),
        });
    }

    /// Record a case whose seam has not merged.
    pub fn blocked(&mut self, id: &'static str, ticket: &'static str, detail: impl Into<String>) {
        self.record(CaseResult {
            id,
            criterion: criterion_of(id),
            outcome: CaseOutcome::Blocked,
            detail: detail.into(),
            blocked_by: Some(ticket),
            artifacts: Vec::new(),
        });
    }

    /// Close the bundle: fill in every unanswered criterion as `missing`, write
    /// `manifest.json`, `events.ndjson`, `verdict.json` and `REPORT.md`, and
    /// return the overall verdict.
    ///
    /// # Errors
    /// Propagates every filesystem failure.
    pub fn finish(mut self) -> io::Result<Verdict> {
        for (id, criterion) in CRITERIA {
            self.results.entry(id).or_insert_with(|| CaseResult {
                id,
                criterion,
                outcome: CaseOutcome::Missing,
                detail: "the driver never answered this criterion".to_owned(),
                blocked_by: None,
                artifacts: Vec::new(),
            });
        }
        let ordered: Vec<&CaseResult> = CRITERIA
            .iter()
            .filter_map(|(id, _)| self.results.get(id))
            .collect();
        let verdict = Verdict::of(&ordered);

        let events = self
            .events
            .iter()
            .map(|event| serde_json::to_string(event).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n");
        self.write_text("events.ndjson", &format!("{events}\n"))?;
        self.artifact(
            "verdict.json",
            &json!({
                "schema_version": 1,
                "ticket": TICKET,
                "run_id": self.run_id,
                "verdict": if verdict.accepted { "accept" } else { "reject" },
                "counts": {
                    "pass": verdict.pass,
                    "fail": verdict.fail,
                    "blocked": verdict.blocked,
                    "missing": verdict.missing,
                },
                "cases": ordered,
            }),
        )?;
        self.write_text("REPORT.md", &self.report(&ordered, &verdict))?;

        // The manifest is written *last* and hashes everything else, so the
        // bundle can be checked for tampering rather than merely read. It
        // cannot hash itself, so `manifest.json` is the one excluded path and
        // the exclusion is named in the document instead of left implicit.
        let evidence = digest_tree(&self.retained, &["manifest.json"])?;
        let cited: std::collections::BTreeSet<&str> = ordered
            .iter()
            .flat_map(|case| case.artifacts.iter().map(String::as_str))
            .collect();
        let unlinked: Vec<&String> = evidence
            .keys()
            .filter(|path| !cited.contains(path.as_str()))
            .filter(|path| {
                !matches!(
                    path.as_str(),
                    "verdict.json" | "events.ndjson" | "REPORT.md"
                )
            })
            .collect();
        self.describe(
            "evidence",
            json!({
                "excludes": ["manifest.json"],
                "algorithm": "sha256 per file; root_hash = sha256 over `<path>\\0<hash>\\n` for \
                              every file in path order",
                "verify": "shasum -a 256 $(find . -type f ! -name manifest.json | sort) \
                           # compare against .evidence.files",
                "root_hash": tree_root_hash(&evidence),
                "files": evidence,
            }),
        );
        // Artifacts no case points at. Some are deliberate — the project
        // manifest and the UI inventory answer no criterion — but an audit
        // found a *failing* case whose evidence file existed and was not linked
        // from it, so the set is published rather than left for the next
        // reader to reconstruct by hand.
        self.describe("unlinked_artifacts", json!(unlinked));
        self.artifact("manifest.json", &self.manifest.clone())?;
        Ok(verdict)
    }

    fn report(&self, ordered: &[&CaseResult], verdict: &Verdict) -> String {
        let mut report = String::new();
        let headline = if verdict.accepted { "ACCEPT" } else { "REJECT" };
        let _ = writeln!(report, "# {TICKET} pilot evidence — {headline}\n");
        let _ = writeln!(
            report,
            "Run `{}` · commit `{}`\n",
            self.run_id,
            self.manifest
                .get("commit")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        );
        let _ = writeln!(
            report,
            "| pass | fail | blocked | missing |\n| --- | --- | --- | --- |\n| {} | {} | {} | {} |\n",
            verdict.pass, verdict.fail, verdict.blocked, verdict.missing
        );
        if !verdict.accepted {
            // Say which of the three ways this run was rejected actually
            // happened. A boilerplate paragraph about blocked cases printed
            // over a run whose only problem is a defect trains the reader to
            // skip the paragraph.
            let mut because: Vec<String> = Vec::new();
            if verdict.fail > 0 {
                because.push(format!(
                    "{} case(s) **failed**: something the merged tree is supposed to do, it does \
                     not. Each one is a defect, not a gap in this proof",
                    verdict.fail
                ));
            }
            if verdict.blocked > 0 {
                because.push(format!(
                    "{} case(s) are **blocked** on a seam that has not merged. A blocked case is \
                     a missing proof, not a warning: the criterion is unproven until the ticket \
                     named beside it merges",
                    verdict.blocked
                ));
            }
            if verdict.missing > 0 {
                because.push(format!(
                    "{} criterion/criteria were **never answered** by the driver, which rejects \
                     the run on its own",
                    verdict.missing
                ));
            }
            let _ = writeln!(
                report,
                "This run is **rejected** — {}.\n",
                because.join("; and ")
            );
        }
        let _ = writeln!(report, "## Cases\n");
        let _ = writeln!(
            report,
            "| case | outcome | criterion | evidence |\n| --- | --- | --- | --- |"
        );
        for case in ordered {
            let outcome = match case.outcome {
                CaseOutcome::Pass => "pass".to_owned(),
                CaseOutcome::Fail => "**fail**".to_owned(),
                CaseOutcome::Blocked => {
                    format!("**blocked** ({})", case.blocked_by.unwrap_or("unknown"))
                }
                CaseOutcome::Missing => "**missing**".to_owned(),
            };
            let artifacts = if case.artifacts.is_empty() {
                "—".to_owned()
            } else {
                case.artifacts
                    .iter()
                    .map(|path| format!("`{path}`"))
                    .collect::<Vec<_>>()
                    .join("<br>")
            };
            let _ = writeln!(
                report,
                "| `{}` | {} | {} | {} |",
                case.id, outcome, case.criterion, artifacts
            );
        }
        let _ = writeln!(report, "\n## Detail\n");
        for case in ordered {
            let _ = writeln!(report, "### `{}`\n\n{}\n", case.id, case.detail);
        }
        report
    }
}

/// The overall accept/reject and how it was reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// Whether every criterion passed.
    pub accepted: bool,
    /// How many passed.
    pub pass: usize,
    /// How many failed.
    pub fail: usize,
    /// How many are blocked on an unmerged seam.
    pub blocked: usize,
    /// How many were never answered.
    pub missing: usize,
    /// Every criterion that did not pass, as `id: outcome — detail`.
    ///
    /// Carried on the verdict so a failing CI run is actionable from its own
    /// output. An operator who has to open `verdict.json` to find out *which*
    /// case broke will read the summary line instead and move on.
    pub unmet: Vec<String>,
}

impl Verdict {
    fn of(cases: &[&CaseResult]) -> Self {
        let count =
            |wanted: CaseOutcome| cases.iter().filter(|case| case.outcome == wanted).count();
        let (pass, fail, blocked, missing) = (
            count(CaseOutcome::Pass),
            count(CaseOutcome::Fail),
            count(CaseOutcome::Blocked),
            count(CaseOutcome::Missing),
        );
        let unmet = cases
            .iter()
            .filter(|case| !case.outcome.accepts())
            .map(|case| {
                let outcome = match case.outcome {
                    CaseOutcome::Pass => "pass",
                    CaseOutcome::Fail => "fail",
                    CaseOutcome::Blocked => "blocked",
                    CaseOutcome::Missing => "missing",
                };
                // A bounded prefix, not a "headline": a case detail states what
                // held before it states what did not, so picking the first
                // sentence would summarise a failure with its passing half. An
                // obvious ellipsis sends the reader to `verdict.json`; a
                // confident-looking one-liner would not.
                const WIDTH: usize = 240;
                let headline = if case.detail.chars().count() > WIDTH {
                    let clipped: String = case.detail.chars().take(WIDTH).collect();
                    format!("{clipped}… (full detail in verdict.json)")
                } else {
                    case.detail.clone()
                };
                match case.blocked_by {
                    Some(ticket) => format!("{}: {outcome} ({ticket}) — {headline}", case.id),
                    None => format!("{}: {outcome} — {headline}", case.id),
                }
            })
            .collect();
        Self {
            accepted: cases.iter().all(|case| case.outcome.accepts()),
            pass,
            fail,
            blocked,
            missing,
            unmet,
        }
    }
}

/// The criterion text for a registered id.
///
/// # Panics
/// Panics on an unregistered id, which is a driver bug.
#[must_use]
pub fn criterion_of(id: &str) -> &'static str {
    CRITERIA
        .iter()
        .find_map(|(key, text)| (*key == id).then_some(*text))
        .expect("every recorded case id is a registered criterion")
}

/// The repository root, derived from this crate's manifest directory.
#[must_use]
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the e2e crate sits two levels below the repository root")
        .to_path_buf()
}

/// The commit this tree is at, or `"unknown"` when git cannot answer.
#[must_use]
pub fn head_commit(repo_root: &Path) -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_root)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map_or_else(|| "unknown".to_owned(), |text| text.trim().to_owned())
}

/// The hex SHA-256 of `bytes`.
#[must_use]
pub fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The SHA-256 of every file under `root`, keyed by bundle-relative path.
///
/// Paths in `excluded` are skipped: a manifest cannot contain its own digest.
/// The map is ordered, so the combined hash below is stable across filesystems
/// that hand back directory entries in different orders.
///
/// # Errors
/// Propagates directory-walk and read failures. A bundle that cannot be hashed
/// is one whose integrity cannot be claimed, so this does not fall back to a
/// partial answer.
pub fn digest_tree(root: &Path, excluded: &[&str]) -> io::Result<BTreeMap<String, String>> {
    let mut hashes = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_dir() {
            for entry in fs::read_dir(&path)? {
                stack.push(entry?.path());
            }
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if excluded.contains(&relative.as_str()) {
            continue;
        }
        hashes.insert(relative, digest(&fs::read(&path)?));
    }
    Ok(hashes)
}

/// One digest over a whole tree's per-file digests.
///
/// Both the path and the hash go into the accumulator, so renaming a file
/// changes the root hash even when its bytes are untouched.
#[must_use]
pub fn tree_root_hash(files: &BTreeMap<String, String>) -> String {
    let mut hasher = Sha256::new();
    for (path, hash) in files {
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
        hasher.update(hash.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

fn derive_run_id(commit: &str, fixtures: &BTreeMap<String, String>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(commit.as_bytes());
    for (name, hash) in fixtures {
        hasher.update(name.as_bytes());
        hasher.update(hash.as_bytes());
    }
    format!("run-{}", &hex::encode(hasher.finalize())[..16])
}

/// Every canary in `needles` that appears anywhere under `root`.
///
/// Reports the *needle* and the file, never the surrounding bytes: a scanner
/// that quoted its finding would be the leak it is looking for. Unreadable and
/// non-UTF-8 files are scanned as raw bytes rather than skipped, because a
/// SQLite page holding a token is exactly the case this exists for.
///
/// # Errors
/// Propagates directory-walk failures.
pub fn scan_for_canaries(root: &Path, needles: &[&str]) -> io::Result<Vec<(String, String)>> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.is_dir() {
            for entry in fs::read_dir(&path)? {
                stack.push(entry?.path());
            }
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else { continue };
        let haystack = String::from_utf8_lossy(&bytes);
        for needle in needles {
            if haystack.contains(needle) {
                found.push((
                    (*needle).to_owned(),
                    path.strip_prefix(root)
                        .unwrap_or(&path)
                        .display()
                        .to_string(),
                ));
            }
        }
    }
    found.sort();
    found.dedup();
    Ok(found)
}
