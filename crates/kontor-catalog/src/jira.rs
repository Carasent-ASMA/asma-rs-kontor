//! The catalog's read-only Jira adapter.
//!
//! It composes [`kontor_jira::JiraReadOnly`] and never the governed
//! reconciliation connector or the daemon's Jira barrier. A sweep is this
//! module's own generation with its own checkpoint; the local discovery sweep
//! and the governed loop can run or fail independently.
//!
//! # The rules this module exists to keep
//!
//! * A declared key is read directly. A search result cannot prove a key the
//!   operator named; only that key's own answer can.
//! * A Story stays a Story. The actual type, hierarchy level and parent come
//!   from Jira, never from a plan title or a label.
//! * A scope search is the discovery path; a complete relevant census is the
//!   only path that may remove a fact.
//! * A 404, a failed page, a rate limit and permission loss are uncertainty.
//!   They retain the prior fact and make coverage say so.
//! * Search consistency lag is detected by directly rechecking a previously
//!   observed issue; a direct answer that still matches the configured scope
//!   keeps it active and marks `search_lag_suspected`.
//! * `ws:<key>` never verifies a join by itself: it must be exactly one label
//!   on an actual Epic and match a declared workstream.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use kontor_core::id::{RealmId, SignedDuration, Timestamp};
use kontor_jira::{
    JiraConfig, JiraReadConfig, JiraReadError, JiraReadFailure, JiraReadIssue, JiraReadLimits,
    JiraReadOnly, JiraReadStats, JiraRetryPolicy,
};
use tracing::{info, warn};

use crate::config::JiraSettings;
use crate::jira_contract::{
    EpicIdentitySource, JIRA_EVIDENCE_SCHEMA, JIRA_EVIDENCE_VERSION, JiraAlias,
    JiraCollectionCoverage, JiraCollectionState, JiraEvidenceDocument, JiraEvidenceProvenance,
    JiraEvidenceSource, JiraIssueObservation, JiraObservationState, JiraObservationVia,
    JiraProjectionError, JiraScope, JiraSweepMode, epic_identity_source, key_order,
    valid_issue_key, valid_project_key,
};
use crate::jira_store::{
    JIRA_ADAPTER, JIRA_COLLECTOR, JiraCheckpoint, JiraRemoval, JiraSweepOutcome, ObservedJiraIssue,
};
use crate::{Catalog, CatalogError, StoreError};

/// Why the Jira adapter could not compose or collect.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum JiraCatalogError {
    /// The settings did not name a composable connection.
    #[error("the Jira catalog settings are unavailable: {detail}")]
    Configuration {
        /// A structural explanation.
        detail: String,
    },
    /// The read-only seam refused.
    #[error(transparent)]
    Read(#[from] JiraReadError),
    /// The catalog refused.
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    /// The catalog store refused.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The compatibility projection refused the evidence.
    #[error(transparent)]
    Projection(#[from] JiraProjectionError),
    /// No complete generation exists yet.
    #[error("the Jira observation is unavailable: {detail}")]
    Unavailable {
        /// A structural explanation.
        detail: String,
    },
}

/// One Jira sweep's result.
#[derive(Debug, Clone)]
pub struct JiraSweepReport {
    /// The attempt generation.
    pub generation: u64,
    /// The mode.
    pub mode: JiraSweepMode,
    /// The monotonic projection order.
    pub order: u64,
    /// Complete, incomplete or failed.
    pub state: JiraCollectionState,
    /// Whether every request needed by this mode succeeded.
    pub complete: bool,
    /// Issues returned by the scope search.
    pub scope_issues: usize,
    /// Distinct issues observed.
    pub observed: usize,
    /// Enhanced-search pages read.
    pub pages: usize,
    /// Observation pages committed to the catalog database.
    pub observation_pages: usize,
    /// Requests attempted.
    pub requests: usize,
    /// Retries performed.
    pub retries: usize,
    /// Measured duration.
    pub duration_ms: u64,
    /// Honest coverage.
    pub coverage: JiraCollectionCoverage,
    /// Structural diagnostics.
    pub diagnostics: Vec<String>,
}

/// The read-only Jira observation adapter.
#[derive(Debug)]
pub struct JiraCatalog {
    catalog: Arc<Catalog>,
    reader: JiraReadOnly,
    settings: JiraSettings,
    realm: String,
    base_url: String,
    declared: Vec<String>,
    foreign_declared: Vec<String>,
}

impl JiraCatalog {
    /// Compose the adapter from a catalog and the effective catalog settings.
    ///
    /// The reader uses the governed realm's credential address but none of its
    /// write authority.
    ///
    /// # Errors
    /// A refused settings document, an unavailable credential scope, or a
    /// malformed `jira.json`.
    pub fn open(
        catalog: Arc<Catalog>,
        realm: RealmId,
        credential_root: &Path,
    ) -> Result<Option<Self>, JiraCatalogError> {
        let effective = catalog.effective_settings();
        let Some(settings) = effective.settings.jira.clone() else {
            return Ok(None);
        };
        if !settings.enabled {
            return Ok(None);
        }
        let (endpoint, credential_alias) = resolve_connection(&settings, credential_root)?;
        let settings = normalize_settings(settings)?;
        let read_config = JiraReadConfig {
            endpoint,
            project_key: settings.project.clone(),
            scope_jql: settings.scope_jql.clone(),
            credential_alias,
            limits: JiraReadLimits {
                max_pages: settings.max_pages,
                max_issues: settings.max_issues,
                page_size: settings.page_size,
                ..JiraReadLimits::default()
            },
            retry: JiraRetryPolicy {
                max_attempts: settings.max_attempts,
                ..JiraRetryPolicy::default()
            },
        };
        let reader = JiraReadOnly::new(credential_root, realm, read_config)?;
        Self::from_parts(catalog, reader, settings).map(Some)
    }

    /// Compose from an explicit read-only seam. This is the qualification seam
    /// for a controlled fixture; it does not add a write method.
    ///
    /// # Errors
    /// A refused or inconsistent settings document.
    pub fn from_parts(
        catalog: Arc<Catalog>,
        reader: JiraReadOnly,
        settings: JiraSettings,
    ) -> Result<Self, JiraCatalogError> {
        let settings = normalize_settings(settings)?;
        if reader.project_key() != settings.project {
            return Err(JiraCatalogError::Configuration {
                detail: "the reader project and the settings project disagree".to_owned(),
            });
        }
        if reader.scope_jql() != settings.scope_jql {
            return Err(JiraCatalogError::Configuration {
                detail: "the reader scope and the settings scope disagree".to_owned(),
            });
        }
        let base_url = reader.origin();
        let parsed = url::Url::parse(&base_url).map_err(|_| JiraCatalogError::Configuration {
            detail: "the read-only origin is not an absolute URL".to_owned(),
        })?;
        let realm = parsed
            .host_str()
            .filter(|host| !host.is_empty())
            .ok_or_else(|| JiraCatalogError::Configuration {
                detail: "the read-only origin has no host".to_owned(),
            })?
            .to_owned();
        let declared: Vec<String> = settings
            .declared_keys
            .iter()
            .filter(|key| key_project(key) == Some(settings.project.as_str()))
            .cloned()
            .collect();
        let foreign_declared: Vec<String> = settings
            .declared_keys
            .iter()
            .filter(|key| key_project(key) != Some(settings.project.as_str()))
            .cloned()
            .collect();
        Ok(Self {
            catalog,
            reader,
            settings,
            realm,
            base_url,
            declared,
            foreign_declared,
        })
    }

    /// The effective settings.
    #[must_use]
    pub fn settings(&self) -> &JiraSettings {
        &self.settings
    }

    /// The current scheduling checkpoint.
    ///
    /// # Errors
    /// SQLite refused.
    pub fn checkpoint(&self) -> Result<JiraCheckpoint, JiraCatalogError> {
        Ok(self.catalog.store().jira_checkpoint()?)
    }

    /// Run one read-only Jira sweep.
    ///
    /// # Errors
    /// A lease, store or composition refusal. A Jira request failure is
    /// recorded in coverage and returned in the report; it does not erase a
    /// prior fact.
    pub async fn collect_once(&self) -> Result<JiraSweepReport, JiraCatalogError> {
        self.collect_once_at(Timestamp::now()).await
    }

    /// The same sweep at an explicit instant, for deterministic recovery tests.
    ///
    /// # Errors
    /// A lease, store or composition refusal.
    pub async fn collect_once_at(
        &self,
        now: Timestamp,
    ) -> Result<JiraSweepReport, JiraCatalogError> {
        let started = Instant::now();
        let lease_seconds = self.settings.cadence_seconds.saturating_mul(2).max(90);
        self.catalog.acquire_owner_at(&now, lease_seconds)?;
        let checkpoint = self.catalog.store().jira_checkpoint()?;
        let full_interval = Duration::from_secs(self.settings.full_sweep_seconds.max(60));
        let full_due = checkpoint
            .next_full_sweep_at
            .as_deref()
            .and_then(|value| value.parse::<Timestamp>().ok())
            .is_none_or(|due| now >= due)
            || checkpoint.last_full_sweep_at.is_none();
        let was_overdue = checkpoint
            .next_full_sweep_at
            .as_deref()
            .and_then(|value| value.parse::<Timestamp>().ok())
            .is_some_and(|due| now > due);
        let mode = if full_due {
            JiraSweepMode::Full
        } else {
            JiraSweepMode::Incremental
        };
        let window_start = if mode == JiraSweepMode::Incremental {
            checkpoint
                .last_complete_at
                .as_deref()
                .and_then(|value| value.parse::<Timestamp>().ok())
                .map(|complete| {
                    complete - SignedDuration::from_secs(self.settings.overlap_seconds as i64)
                })
        } else {
            None
        };
        let sweep = self.catalog.store().begin_jira_sweep(
            self.catalog.owner_id(),
            mode,
            window_start.as_ref(),
            &now,
        )?;
        let mut attempt = self
            .attempt(
                mode,
                window_start.as_ref(),
                checkpoint.next_full_sweep_at.clone(),
                was_overdue,
                now,
            )
            .await;
        let complete = attempt.complete && !attempt.failed;
        let next_full_sweep_at = if mode == JiraSweepMode::Full && complete {
            Some((now + SignedDuration::from_secs(full_interval.as_secs() as i64)).to_string())
        } else {
            checkpoint.next_full_sweep_at.clone()
        };
        attempt.next_full_sweep_at = next_full_sweep_at.clone();

        let mut observed: Vec<ObservedJiraIssue> = attempt.observed.values().cloned().collect();
        observed.sort_by_key(|item| key_order(&item.key));
        let pages = observed.len().div_ceil(128);
        let mut diagnostics = attempt.diagnostics.clone();
        for (index, page) in observed.chunks(128).enumerate() {
            if let Err(error) = self.catalog.store().commit_jira_page(
                &sweep,
                u32::try_from(index).unwrap_or(u32::MAX),
                &now,
                page,
            ) {
                let detail = format!("A Jira observation page could not be committed ({error}).");
                diagnostics.push(detail.clone());
                attempt.next_full_sweep_at = checkpoint.next_full_sweep_at.clone();
                let coverage = self.coverage(
                    &attempt,
                    mode,
                    JiraCollectionState::Failed,
                    false,
                    false,
                    window_start.as_ref(),
                    now,
                    Some(JiraReadFailure::Transport),
                    Some(detail.clone()),
                );
                let outcome = JiraSweepOutcome {
                    complete: false,
                    failed: true,
                    census: false,
                    coverage,
                    diagnostics,
                    aliases: Vec::new(),
                    removals: Vec::new(),
                    next_full_sweep_at,
                    issues_seen: 0,
                    pages: attempt.search_pages,
                    requests: attempt.stats.requests,
                    retries: attempt.stats.retries,
                    permission_loss: attempt.permission_loss,
                    credential_failure: attempt.credential_failure,
                    backoff_until: attempt.stats.backoff_until.clone(),
                    failure: Some(JiraReadFailure::Transport),
                    error: Some(detail),
                };
                let _ = self
                    .catalog
                    .store()
                    .finalize_jira_sweep(&sweep, &outcome, &now);
                return Err(JiraCatalogError::Store(error));
            }
        }
        let census = complete && mode == JiraSweepMode::Full && attempt.recovery_complete;
        let state = if attempt.failed {
            JiraCollectionState::Failed
        } else if complete {
            JiraCollectionState::Complete
        } else {
            JiraCollectionState::Incomplete
        };
        let coverage = self.coverage(
            &attempt,
            mode,
            state,
            complete,
            census,
            window_start.as_ref(),
            now,
            attempt.failure,
            attempt.error.clone(),
        );
        diagnostics.extend(attempt.diagnostics.clone());
        diagnostics.sort();
        diagnostics.dedup();
        let mut aliases: Vec<JiraAlias> = attempt.aliases.into_values().collect();
        aliases.sort_by_key(|item| key_order(&item.old_key));
        let mut removals: Vec<JiraRemoval> = attempt.removals.into_values().collect();
        removals.sort_by_key(|item| key_order(&item.issue_id));
        let outcome = JiraSweepOutcome {
            complete,
            failed: attempt.failed,
            census,
            coverage: coverage.clone(),
            diagnostics: diagnostics.clone(),
            aliases,
            removals,
            next_full_sweep_at,
            issues_seen: observed.len(),
            pages: coverage.pages,
            requests: attempt.stats.requests,
            retries: attempt.stats.retries,
            permission_loss: attempt.permission_loss,
            credential_failure: attempt.credential_failure,
            backoff_until: attempt.stats.backoff_until.clone(),
            failure: attempt.failure,
            error: attempt.error.clone(),
        };
        self.catalog
            .store()
            .finalize_jira_sweep(&sweep, &outcome, &now)?;
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        info!(
            generation = sweep.generation,
            mode = mode.as_str(),
            observed = observed.len(),
            scope_issues = coverage.scope_issues,
            pages,
            duration_ms,
            %state,
            "Jira catalog sweep finished"
        );
        Ok(JiraSweepReport {
            generation: sweep.generation,
            mode,
            order: sweep.order,
            state,
            complete,
            scope_issues: coverage.scope_issues,
            observed: observed.len(),
            pages: coverage.pages,
            observation_pages: pages,
            requests: outcome.requests,
            retries: outcome.retries,
            duration_ms,
            coverage,
            diagnostics,
        })
    }

    /// The durable evidence view of the last complete generation.
    ///
    /// # Errors
    /// No complete generation, a store refusal, or an inconsistent scope.
    pub fn evidence(&self) -> Result<JiraEvidenceDocument, JiraCatalogError> {
        let read = self.catalog.store().read_jira_evidence(false)?;
        if read.generation == 0 {
            return Err(JiraCatalogError::Unavailable {
                detail: "no complete Jira generation exists".to_owned(),
            });
        }
        let coverage = read.coverage.ok_or_else(|| JiraCatalogError::Unavailable {
            detail: "the active Jira generation has no coverage".to_owned(),
        })?;
        if coverage.state != JiraCollectionState::Complete || !coverage.complete {
            return Err(JiraCatalogError::Unavailable {
                detail: "the active Jira generation is not complete".to_owned(),
            });
        }
        let observed_epics: BTreeSet<String> = read
            .issues
            .iter()
            .filter(|issue| issue.is_actual_epic())
            .map(|issue| issue.key.clone())
            .collect();
        let mut named_epics = observed_epics.clone();
        for alias in &read.aliases {
            if observed_epics.contains(&alias.current_key) {
                named_epics.insert(alias.old_key.clone());
            }
        }
        let dangling = self
            .catalog
            .store()
            .read_jira_dangling_associations(&named_epics)?;
        let scope = JiraScope {
            jql: self.settings.scope_jql.clone(),
            project: self.settings.project.clone(),
            declared: self.declared.clone(),
        };
        let answered: BTreeSet<&str> = read
            .issues
            .iter()
            .map(|issue| issue.key.as_str())
            .chain(coverage.missing.iter().map(String::as_str))
            .collect();
        let mut unanswered: Vec<String> = scope
            .declared
            .iter()
            .filter(|key| {
                !answered.contains(key.as_str())
                    && !read.aliases.iter().any(|alias| alias.old_key == **key)
            })
            .cloned()
            .collect();
        unanswered.sort_by_key(|item| key_order(item));
        let mut issues = read.issues.clone();
        issues.sort_by_key(|issue| key_order(&issue.key));
        let mut aliases = read.aliases.clone();
        aliases.sort_by_key(|alias| key_order(&alias.old_key));
        Ok(JiraEvidenceDocument {
            schema: JIRA_EVIDENCE_SCHEMA.to_owned(),
            evidence_version: JIRA_EVIDENCE_VERSION,
            generation: read.generation,
            order: read.order,
            as_of: read
                .as_of
                .clone()
                .ok_or_else(|| JiraCatalogError::Unavailable {
                    detail: "the active Jira generation has no completion instant".to_owned(),
                })?,
            source: JiraEvidenceSource {
                kind: "jira-rest".to_owned(),
                realm: self.realm.clone(),
                base_url: self.base_url.clone(),
            },
            scope,
            provenance: JiraEvidenceProvenance {
                collector: JIRA_COLLECTOR.to_owned(),
                adapter: JIRA_ADAPTER.to_owned(),
                credential_reference: self.reader.credential_reference().to_owned(),
            },
            coverage: coverage.clone(),
            collection_state: read.latest_state,
            latest_error: read.latest_error.clone(),
            conflicts: aliases.clone(),
            issues,
            aliases,
            dangling_associations: dangling,
            unanswered,
        })
    }

    /// Project the durable evidence into the frozen `kontor/epic-identity@1`
    /// view.
    ///
    /// # Errors
    /// No complete generation, or a refused projection.
    pub fn export_epic_identity(&self) -> Result<EpicIdentitySource, JiraCatalogError> {
        let document = self.evidence()?;
        Ok(epic_identity_source(&document)?)
    }

    async fn attempt(
        &self,
        mode: JiraSweepMode,
        window_start: Option<&Timestamp>,
        next_full_sweep_at: Option<String>,
        was_overdue: bool,
        now: Timestamp,
    ) -> Attempt<'_> {
        let mut attempt = Attempt::new(self, mode, next_full_sweep_at, was_overdue, now);
        match self.catalog.store().read_jira_evidence(false) {
            Ok(read) => {
                attempt.previous = read
                    .issues
                    .into_iter()
                    .map(|issue| (issue.id.clone(), issue))
                    .collect();
            }
            Err(error) => {
                attempt.complete = false;
                attempt.error = Some(format!(
                    "The previous Jira facts could not be read ({error})."
                ));
            }
        }
        let scope_jql = match (mode, window_start) {
            (JiraSweepMode::Incremental, Some(start)) => format!(
                "({}) AND updated >= \"{}\"",
                self.settings.scope_jql,
                jql_minute(start)
            ),
            _ => self.settings.scope_jql.clone(),
        };
        match self.reader.search_all(&scope_jql).await {
            Ok(search) => {
                attempt.stats.merge(&search.stats);
                attempt.search_pages = attempt.search_pages.saturating_add(search.pages);
                for issue in search.issues {
                    let in_project =
                        key_project(&issue.key) == Some(self.settings.project.as_str());
                    attempt.take_issue(issue, JiraObservationVia::Scope);
                    if in_project {
                        attempt.scope_issues = attempt.scope_issues.saturating_add(1);
                    }
                }
            }
            Err(error) => {
                attempt.record_error(error, "the configured scope search failed");
                if attempt.failed {
                    return attempt;
                }
            }
        }
        // Declared keys are read directly. A search result for the same key is
        // accepted, but a key the operator named is never assumed from absence.
        for key in self.declared.clone() {
            if attempt.answered_key(&key) {
                attempt.found.insert(key);
                continue;
            }
            match self.reader.issue(&key).await {
                Ok(answer) => {
                    attempt.stats.merge(&answer.stats);
                    match answer.issue {
                        Some(issue) => {
                            if issue.key != key
                                && (key_project(&issue.key) == Some(self.settings.project.as_str())
                                    || attempt.previous.contains_key(&issue.id))
                            {
                                attempt.record_move(&key, &issue);
                            }
                            if key_project(&issue.key) != Some(self.settings.project.as_str()) {
                                attempt.out_of_scope.insert(key);
                                continue;
                            }
                            attempt.take_issue(issue, JiraObservationVia::Declared);
                            attempt.found.insert(key);
                        }
                        None => {
                            attempt.missing.insert(key);
                        }
                    }
                }
                Err(error) => {
                    attempt.unanswered.insert(key.clone());
                    attempt.record_error(error, "a declared key could not be answered");
                }
            }
        }
        // Parents come from Jira. A bounded number of rounds resolves a short
        // chain without inventing an epic from a label.
        let rounds = self.settings.parent_rounds.clamp(1, 10);
        for _ in 0..rounds {
            let wanted = attempt.wanted_parents();
            if wanted.is_empty() {
                break;
            }
            for key in wanted {
                if attempt.missing_parents.contains(&key) {
                    continue;
                }
                match self.reader.issue(&key).await {
                    Ok(answer) => {
                        attempt.stats.merge(&answer.stats);
                        match answer.issue {
                            Some(issue) => {
                                if issue.key != key
                                    && (key_project(&issue.key)
                                        == Some(self.settings.project.as_str())
                                        || attempt.previous.contains_key(&issue.id))
                                {
                                    attempt.record_move(&key, &issue);
                                }
                                if key_project(&issue.key) == Some(self.settings.project.as_str()) {
                                    attempt.take_issue(issue, JiraObservationVia::Parent);
                                } else {
                                    attempt.missing_parents.insert(key.clone());
                                    attempt.diagnostics.push(format!(
                                        "Parent {key} is outside the configured project."
                                    ));
                                }
                            }
                            None => {
                                attempt.missing_parents.insert(key.clone());
                            }
                        }
                    }
                    Err(error) => {
                        attempt.missing_parents.insert(key.clone());
                        attempt.record_error(error, "a parent key could not be read");
                    }
                }
            }
        }
        // Periodically recheck every fact not already answered by this sweep.
        // This is how a removed `ws:` label and a moved key are observed
        // without letting search consistency lag become a deletion.
        //
        // Incremental sweeps run far more often than full sweeps, so their
        // recheck slice has its own bound. Using `max_recheck` here would let a
        // small project direct-GET every prior issue every incremental cycle.
        const INCREMENTAL_RECHECK_LIMIT: usize = 12;
        let mut recheck: Vec<JiraIssueObservation> = attempt
            .previous
            .values()
            .filter(|issue| {
                issue.state == JiraObservationState::Active && !attempt.observed_by_id(&issue.id)
            })
            .cloned()
            .collect();
        recheck.sort_by_key(|item| key_order(&item.key));
        let incremental_limit = self.settings.max_recheck.min(INCREMENTAL_RECHECK_LIMIT);
        let recheck_limit = match mode {
            JiraSweepMode::Full => self.settings.max_recheck,
            JiraSweepMode::Incremental => incremental_limit,
        };
        if recheck.len() > recheck_limit {
            let total = recheck.len();
            let limit = recheck_limit;
            match mode {
                JiraSweepMode::Full => {
                    attempt.recovery_complete = false;
                    attempt.complete = false;
                    attempt.diagnostics.push(format!(
                        "{total} previously observed issues exceeded the {limit}-issue recheck bound; prior facts were retained and no removal was committed."
                    ));
                    recheck.truncate(limit);
                }
                JiraSweepMode::Incremental => {
                    // Recheck a bounded rotating slice. A full recheck would
                    // defeat the incremental window; a fixed prefix would
                    // starve every issue after the bound forever.
                    let offset =
                        usize::try_from(attempt.now.as_millisecond()).unwrap_or_default() % total;
                    let mut rotated = Vec::with_capacity(limit);
                    rotated.extend(recheck.iter().skip(offset).take(limit).cloned());
                    if rotated.len() < limit {
                        rotated.extend(recheck.iter().take(limit - rotated.len()).cloned());
                    }
                    recheck = rotated;
                    attempt.diagnostics.push(format!(
                        "{total} previously observed issues exceeded the {limit}-issue recheck bound; a rotating slice was rechecked and no removal was committed from the remainder."
                    ));
                }
            }
        }
        for issue in recheck {
            match self.reader.issue(&issue.key).await {
                Ok(answer) => {
                    attempt.stats.merge(&answer.stats);
                    match answer.issue {
                        Some(observed) => {
                            if observed.id != issue.id {
                                attempt.recovery_complete = false;
                                attempt.complete = false;
                                attempt.diagnostics.push(format!(
                                    "Jira reused key {} for a different immutable issue; it was not replaced.",
                                    issue.key
                                ));
                                attempt.retained_unknown.insert(issue.key.clone());
                                continue;
                            }
                            if observed.key != issue.key {
                                attempt.record_move(&issue.key, &observed);
                            }
                            let declared_key = self.declared.iter().find(|key| {
                                key.as_str() == issue.key || key.as_str() == observed.key
                            });
                            if let Some(declared_key) = declared_key {
                                let declared_key = declared_key.clone();
                                attempt.take_issue(observed, JiraObservationVia::Declared);
                                attempt.found.insert(declared_key);
                            } else if attempt.is_parent_of_observed(&observed) {
                                attempt.take_issue(observed, JiraObservationVia::Parent);
                            } else if matches_scope(
                                &self.settings,
                                &observed,
                                &self.settings.project,
                            ) {
                                if mode == JiraSweepMode::Full {
                                    attempt.search_lag = true;
                                    attempt.diagnostics.push(format!(
                                        "The complete scope search omitted {}; a direct read still proves it in scope. Search lag is not deletion.",
                                        observed.key
                                    ));
                                }
                                attempt.take_issue(observed, JiraObservationVia::Scope);
                            } else {
                                attempt.removal_candidates.insert(observed.id.clone());
                            }
                        }
                        None => {
                            attempt.retained_unknown.insert(issue.key.clone());
                        }
                    }
                }
                Err(error) => {
                    attempt.recovery_complete = false;
                    attempt.retained_unknown.insert(issue.key.clone());
                    attempt
                        .record_error(error, "a previously observed issue could not be rechecked");
                }
            }
        }
        attempt.finish();
        attempt
    }

    #[allow(clippy::too_many_arguments)]
    fn coverage(
        &self,
        attempt: &Attempt,
        mode: JiraSweepMode,
        state: JiraCollectionState,
        complete: bool,
        census: bool,
        window_start: Option<&Timestamp>,
        now: Timestamp,
        failure: Option<JiraReadFailure>,
        error: Option<String>,
    ) -> JiraCollectionCoverage {
        let declared = self.declared.len();
        let mut missing: Vec<String> = attempt.missing.iter().cloned().collect();
        missing.sort_by_key(|item| key_order(item));
        let mut out_of_scope: BTreeSet<String> = self.foreign_declared.iter().cloned().collect();
        out_of_scope.extend(attempt.out_of_scope.iter().cloned());
        let mut out_of_scope: Vec<String> = out_of_scope.into_iter().collect();
        out_of_scope.sort_by_key(|item| key_order(item));
        let mut unresolved: Vec<String> = attempt.unresolved.iter().cloned().collect();
        unresolved.sort_by_key(|item| key_order(item));
        let mut retained_unknown: Vec<String> = attempt.retained_unknown.iter().cloned().collect();
        retained_unknown.sort_by_key(|item| key_order(item));
        JiraCollectionCoverage {
            mode,
            state,
            complete,
            census,
            scope_issues: attempt.scope_issues,
            declared,
            found: attempt.found.len(),
            missing,
            out_of_scope,
            unresolved,
            pages: attempt.search_pages,
            requests: attempt.stats.requests,
            retries: attempt.stats.retries,
            rate_limited: attempt.stats.rate_limited,
            permission_loss: attempt.permission_loss,
            credential_failure: attempt.credential_failure,
            search_lag_suspected: attempt.search_lag,
            retained_unknown,
            removed: attempt.removals.len(),
            backoff_until: attempt.stats.backoff_until.clone(),
            window_start: window_start.map(ToString::to_string),
            window_end: Some(now.to_string()),
            started_at: now.to_string(),
            finished_at: now.to_string(),
            last_complete_at: if complete {
                Some(now.to_string())
            } else {
                self.checkpoint()
                    .ok()
                    .and_then(|checkpoint| checkpoint.last_complete_at)
            },
            last_full_sweep_at: if complete && mode == JiraSweepMode::Full {
                Some(now.to_string())
            } else {
                self.checkpoint()
                    .ok()
                    .and_then(|checkpoint| checkpoint.last_full_sweep_at)
            },
            next_full_sweep_at: attempt.next_full_sweep_at.clone(),
            overdue: attempt.was_overdue && mode == JiraSweepMode::Full,
            diagnostics: attempt.diagnostics.clone(),
            failure,
            error,
        }
    }
}

/// Poll Jira observation until the realm stops.
pub async fn poll_jira_until_stopped(
    jira: Arc<JiraCatalog>,
    mut stops: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        match jira.collect_once().await {
            Ok(report) => {
                if report.state != JiraCollectionState::Complete {
                    warn!(
                        generation = report.generation,
                        ?report.state,
                        "the Jira catalog sweep was not complete; prior facts were retained"
                    );
                }
            }
            Err(error) => warn!(detail = %error, "the Jira catalog sweep failed"),
        }
        let cadence = Duration::from_secs(jira.settings.cadence_seconds.clamp(1, 3600));
        let delay = jira
            .checkpoint()
            .ok()
            .and_then(|checkpoint| checkpoint.backoff_until)
            .and_then(|value| value.parse::<Timestamp>().ok())
            .and_then(|until| {
                let now = Timestamp::now();
                (until > now).then(|| {
                    let millis = until.as_millisecond() - now.as_millisecond();
                    Duration::from_millis(u64::try_from(millis).unwrap_or(0))
                })
            })
            .map_or(cadence, |until| {
                cadence.max(until.min(Duration::from_secs(3600)))
            });
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            changed = stops.changed() => {
                if changed.is_err() || *stops.borrow() {
                    return;
                }
            }
        }
    }
}

struct Attempt<'a> {
    jira: &'a JiraCatalog,
    mode: JiraSweepMode,
    next_full_sweep_at: Option<String>,
    was_overdue: bool,
    now: Timestamp,
    previous: BTreeMap<String, JiraIssueObservation>,
    observed: BTreeMap<String, ObservedJiraIssue>,
    aliases: BTreeMap<String, JiraAlias>,
    scope_issues: usize,
    search_pages: usize,
    found: BTreeSet<String>,
    missing: BTreeSet<String>,
    missing_parents: BTreeSet<String>,
    out_of_scope: BTreeSet<String>,
    unanswered: BTreeSet<String>,
    retained_unknown: BTreeSet<String>,
    unresolved: BTreeSet<String>,
    removal_candidates: BTreeSet<String>,
    removals: BTreeMap<String, JiraRemoval>,
    diagnostics: Vec<String>,
    stats: JiraReadStats,
    complete: bool,
    failed: bool,
    recovery_complete: bool,
    permission_loss: bool,
    credential_failure: bool,
    failure: Option<JiraReadFailure>,
    error: Option<String>,
    search_lag: bool,
}

impl<'a> Attempt<'a> {
    fn new(
        jira: &'a JiraCatalog,
        mode: JiraSweepMode,
        next_full_sweep_at: Option<String>,
        was_overdue: bool,
        now: Timestamp,
    ) -> Self {
        Self {
            jira,
            mode,
            next_full_sweep_at,
            was_overdue,
            now,
            previous: BTreeMap::new(),
            observed: BTreeMap::new(),
            aliases: BTreeMap::new(),
            scope_issues: 0,
            search_pages: 0,
            found: BTreeSet::new(),
            missing: BTreeSet::new(),
            missing_parents: BTreeSet::new(),
            out_of_scope: BTreeSet::new(),
            unanswered: BTreeSet::new(),
            retained_unknown: BTreeSet::new(),
            unresolved: BTreeSet::new(),
            removal_candidates: BTreeSet::new(),
            removals: BTreeMap::new(),
            diagnostics: Vec::new(),
            stats: JiraReadStats::default(),
            complete: true,
            failed: false,
            recovery_complete: true,
            permission_loss: false,
            credential_failure: false,
            failure: None,
            error: None,
            search_lag: false,
        }
    }

    fn take_issue(&mut self, issue: JiraReadIssue, via: JiraObservationVia) {
        let project = self.jira.settings.project.as_str();
        if key_project(&issue.key) != Some(project) {
            if via == JiraObservationVia::Declared {
                self.out_of_scope.insert(issue.key);
            } else {
                self.diagnostics.push(format!(
                    "Issue {} is outside the configured project and was not observed.",
                    issue.key
                ));
            }
            return;
        }
        if let Some(previous) = self.observed.remove(&issue.id) {
            if previous.key != issue.key && !is_stale_issue(&previous, &issue) {
                self.record_move(&previous.key, &issue);
            }
            let observed_at = self.now.to_string();
            let merged = merge_issue(previous, issue, via, &observed_at);
            self.observed.insert(merged.id.clone(), merged);
            return;
        }
        let observation = ObservedJiraIssue {
            id: issue.id,
            key: issue.key,
            issue_type: issue.issue_type,
            level: issue.level,
            parent: issue.parent_key,
            parent_id: issue.parent_id,
            labels: issue.labels,
            status: issue.status,
            cat: issue.status_category,
            updated: issue.updated,
            summary: issue.summary,
            observed_at: self.now.to_string(),
            via,
        };
        self.observed.insert(observation.id.clone(), observation);
    }

    fn record_move(&mut self, old_key: &str, issue: &JiraReadIssue) {
        if old_key == issue.key {
            return;
        }
        self.aliases
            .entry(old_key.to_owned())
            .and_modify(|alias| {
                alias.current_key = issue.key.clone();
                alias.observed_at = self.now.to_string();
            })
            .or_insert_with(|| JiraAlias {
                issue_id: issue.id.clone(),
                old_key: old_key.to_owned(),
                current_key: issue.key.clone(),
                observed_at: self.now.to_string(),
            });
        self.diagnostics.push(format!(
            "Jira moved {old_key} to {}; the old key is retained as an alias.",
            issue.key
        ));
    }

    fn answered_key(&self, key: &str) -> bool {
        self.observed.values().any(|issue| issue.key == key)
            || self.aliases.contains_key(key)
            || self.missing.contains(key)
    }

    fn observed_by_id(&self, id: &str) -> bool {
        self.observed.contains_key(id)
    }

    fn wanted_parents(&self) -> BTreeSet<String> {
        let mut wanted = BTreeSet::new();
        let observed_keys: BTreeSet<&str> = self
            .observed
            .values()
            .map(|issue| issue.key.as_str())
            .collect();
        for issue in self.observed.values() {
            if issue.level >= 1 {
                continue;
            }
            let Some(parent) = issue.parent.as_deref() else {
                continue;
            };
            let current = self.alias_current(parent);
            if current.is_empty()
                || observed_keys.contains(current)
                || self.missing_parents.contains(parent)
                || self.missing_parents.contains(current)
            {
                continue;
            }
            wanted.insert(current.to_owned());
        }
        wanted
    }

    fn alias_current<'b>(&'b self, key: &'b str) -> &'b str {
        self.aliases
            .get(key)
            .map(|alias| alias.current_key.as_str())
            .unwrap_or(key)
    }

    fn is_parent_of_observed(&self, issue: &JiraReadIssue) -> bool {
        self.observed.values().any(|child| {
            child.level < 1
                && child.parent.as_deref().is_some_and(|parent| {
                    let current = self.alias_current(parent);
                    current == issue.key || parent == issue.key
                })
        })
    }

    fn record_error(&mut self, error: JiraReadError, context: &str) {
        self.stats.merge(&error.stats);
        self.failure = Some(error.failure);
        match error.failure {
            JiraReadFailure::Credential => {
                self.credential_failure = true;
                self.failed = true;
            }
            JiraReadFailure::Permission => {
                self.permission_loss = true;
                self.failed = true;
            }
            JiraReadFailure::Configuration | JiraReadFailure::Rejected => {
                self.failed = true;
            }
            _ => {
                self.complete = false;
            }
        }
        let detail = format!("{context}: {}", error.detail());
        self.error = Some(detail.clone());
        self.diagnostics.push(detail);
    }

    fn finish(&mut self) {
        if !self.unanswered.is_empty() {
            self.complete = false;
        }
        if self.failed {
            self.complete = false;
        }
        for issue in self.observed.values() {
            if let Some(previous) = self.previous.get(&issue.id) {
                let previous_labels: BTreeSet<&str> = previous
                    .labels
                    .iter()
                    .filter(|label| label.starts_with("ws:"))
                    .map(String::as_str)
                    .collect();
                let current_labels: BTreeSet<&str> = issue
                    .labels
                    .iter()
                    .filter(|label| label.starts_with("ws:"))
                    .map(String::as_str)
                    .collect();
                for removed in previous_labels.difference(&current_labels) {
                    self.diagnostics.push(format!(
                        "The {removed} label on {} was removed or changed upstream; the issue stays visible and no association is deleted.",
                        issue.key
                    ));
                }
            }
        }
        self.diagnostics.sort();
        self.diagnostics.dedup();
        let unresolved = self.compute_unresolved();
        self.unresolved = unresolved;
        if self.complete
            && self.mode == JiraSweepMode::Full
            && self.recovery_complete
            && !self.failed
        {
            for id in std::mem::take(&mut self.removal_candidates) {
                self.removals.insert(
                    id.clone(),
                    JiraRemoval {
                        issue_id: id,
                        reason: "out_of_scope".to_owned(),
                    },
                );
            }
        } else {
            self.removal_candidates.clear();
        }
    }

    fn compute_unresolved(&self) -> BTreeSet<String> {
        let observed_keys: BTreeSet<&str> = self
            .observed
            .values()
            .map(|issue| issue.key.as_str())
            .collect();
        let mut unresolved = BTreeSet::new();
        for issue in self.observed.values() {
            if issue.level >= 1 {
                continue;
            }
            let Some(parent) = issue.parent.as_deref() else {
                continue;
            };
            let current = self.alias_current(parent);
            if !observed_keys.contains(current) && !observed_keys.contains(parent) {
                unresolved.insert(issue.key.clone());
            }
        }
        unresolved
    }
}

fn merge_issue(
    previous: ObservedJiraIssue,
    issue: JiraReadIssue,
    via: JiraObservationVia,
    observed_at: &str,
) -> ObservedJiraIssue {
    if is_stale_issue(&previous, &issue) {
        return previous;
    }
    let via = if via_rank(via) > via_rank(previous.via) {
        via
    } else {
        previous.via
    };
    ObservedJiraIssue {
        id: issue.id,
        key: issue.key,
        issue_type: issue.issue_type,
        level: issue.level,
        parent: issue.parent_key,
        parent_id: issue.parent_id,
        labels: issue.labels,
        status: issue.status,
        cat: issue.status_category,
        updated: issue.updated,
        summary: issue.summary,
        observed_at: observed_at.to_owned(),
        via,
    }
}

fn is_stale_issue(previous: &ObservedJiraIssue, issue: &JiraReadIssue) -> bool {
    previous
        .updated
        .parse::<Timestamp>()
        .ok()
        .zip(issue.updated.parse::<Timestamp>().ok())
        .is_some_and(|(previous, next)| next < previous)
}

const fn via_rank(via: JiraObservationVia) -> u8 {
    match via {
        JiraObservationVia::Scope => 3,
        JiraObservationVia::Declared => 2,
        JiraObservationVia::Parent => 1,
    }
}

fn matches_scope(settings: &JiraSettings, issue: &JiraReadIssue, project: &str) -> bool {
    key_project(&issue.key) == Some(project) && (!settings.scope_is_epics || issue.level == 1)
}

fn key_project(key: &str) -> Option<&str> {
    key.rsplit_once('-').map(|(project, _)| project)
}

fn jql_minute(timestamp: &Timestamp) -> String {
    let text = (*timestamp - SignedDuration::from_secs(60)).to_string();
    text.get(0..16).unwrap_or(text.as_str()).replace('T', " ")
}

fn resolve_connection(
    settings: &JiraSettings,
    credential_root: &Path,
) -> Result<(String, String), JiraCatalogError> {
    let explicit_endpoint = settings.endpoint.clone();
    let explicit_alias = settings.credential_alias.clone();
    let config = read_jira_config(credential_root);
    let project = match config {
        Ok(Some(config)) => {
            let mut matching = config
                .projects
                .iter()
                .filter(|project| project.project_key.as_str() == settings.project);
            let first = matching.next().cloned();
            if matching.next().is_some() {
                return Err(JiraCatalogError::Configuration {
                    detail: "jira.json names the configured project more than once".to_owned(),
                });
            }
            first
        }
        Ok(None) => None,
        Err(error) if explicit_endpoint.is_some() && explicit_alias.is_some() => {
            let _ = error;
            None
        }
        Err(error) => return Err(error),
    };
    let endpoint = explicit_endpoint
        .or_else(|| project.as_ref().map(|project| project.endpoint.clone()))
        .ok_or_else(|| JiraCatalogError::Configuration {
            detail:
                "no Jira endpoint is configured in catalog.json or the matching jira.json project"
                    .to_owned(),
        })?;
    let credential_alias = explicit_alias
        .or_else(|| {
            project
                .as_ref()
                .map(|project| project.credential_alias.clone())
        })
        .ok_or_else(|| JiraCatalogError::Configuration {
            detail:
                "no credential alias is configured in catalog.json or the matching jira.json project"
                    .to_owned(),
        })?;
    Ok((endpoint, credential_alias))
}

fn read_jira_config(root: &Path) -> Result<Option<JiraConfig>, JiraCatalogError> {
    let path = root.join("jira.json");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(JiraCatalogError::Configuration {
                detail: format!("jira.json could not be read ({})", error.kind()),
            });
        }
    };
    let config: JiraConfig =
        serde_json::from_slice(&bytes).map_err(|_| JiraCatalogError::Configuration {
            detail: "jira.json is not a strict supported document".to_owned(),
        })?;
    if config.schema_version != 1 {
        return Err(JiraCatalogError::Configuration {
            detail: "jira.json declares an unsupported schema version".to_owned(),
        });
    }
    Ok(Some(config))
}

fn normalize_settings(mut settings: JiraSettings) -> Result<JiraSettings, JiraCatalogError> {
    settings.project = settings.project.trim().to_owned();
    settings.scope_jql = settings.scope_jql.trim().to_owned();
    if !valid_project_key(&settings.project) {
        return Err(JiraCatalogError::Configuration {
            detail: "the Jira project is not an uppercase project key".to_owned(),
        });
    }
    if settings.scope_jql.trim().is_empty()
        || settings.scope_jql.chars().count() > 2000
        || settings.scope_jql.chars().any(char::is_control)
    {
        return Err(JiraCatalogError::Configuration {
            detail: "the Jira scope JQL is empty, oversized or contains a control character"
                .to_owned(),
        });
    }
    if settings.cadence_seconds == 0
        || settings.full_sweep_seconds < settings.cadence_seconds
        || settings.overlap_seconds > settings.full_sweep_seconds
        || settings.max_pages == 0
        || settings.max_issues == 0
        || settings.max_recheck == 0
        || !(1..=100).contains(&settings.page_size)
        || !(1..=10).contains(&settings.max_attempts)
        || !(1..=10).contains(&settings.parent_rounds)
    {
        return Err(JiraCatalogError::Configuration {
            detail: "a Jira scheduling or request bound is outside its supported range".to_owned(),
        });
    }
    let mut declared = Vec::new();
    let mut seen = BTreeSet::new();
    for key in settings.declared_keys {
        let key = key.trim().to_ascii_uppercase();
        if !valid_issue_key(&key) {
            return Err(JiraCatalogError::Configuration {
                detail: "a declared key is not a Jira issue key".to_owned(),
            });
        }
        if seen.insert(key.clone()) {
            declared.push(key);
        }
    }
    if declared.len() > 5000 {
        return Err(JiraCatalogError::Configuration {
            detail: "more than 5000 declared keys are configured".to_owned(),
        });
    }
    declared.sort_by_key(|item| key_order(item));
    Ok(JiraSettings {
        declared_keys: declared,
        ..settings
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jira_contract::EPIC_IDENTITY_SCHEMA;

    #[test]
    fn jql_windows_use_jira_minute_text() {
        let instant: Timestamp = "2026-10-25T04:05:06Z".parse().expect("an instant");
        assert_eq!(
            jql_minute(&instant),
            "2026-10-25 04:04",
            "the incremental window starts one minute early so a minute-truncated JQL bound cannot miss an update"
        );
    }

    #[test]
    fn a_story_is_out_of_an_epic_scope_but_an_epic_is_not() {
        let settings = JiraSettings {
            enabled: true,
            project: "ASMA".to_owned(),
            scope_jql: "project = ASMA AND issuetype = Epic".to_owned(),
            declared_keys: Vec::new(),
            endpoint: None,
            credential_alias: None,
            cadence_seconds: 120,
            full_sweep_seconds: 900,
            overlap_seconds: 180,
            max_pages: 200,
            max_issues: 5000,
            max_recheck: 500,
            page_size: 100,
            max_attempts: 3,
            scope_is_epics: true,
            parent_rounds: 5,
        };
        let mut issue = JiraReadIssue {
            id: "1".to_owned(),
            key: "ASMA-1".to_owned(),
            issue_type: "Story".to_owned(),
            level: 0,
            parent_key: None,
            parent_id: None,
            labels: Vec::new(),
            status: "New".to_owned(),
            status_category: "new".to_owned(),
            updated: "2026-10-25T00:00:00Z".to_owned(),
            summary: "Synthetic".to_owned(),
        };
        assert!(!matches_scope(&settings, &issue, "ASMA"));
        issue.level = 1;
        assert!(matches_scope(&settings, &issue, "ASMA"));
        issue.key = "OTHER-1".to_owned();
        assert!(!matches_scope(&settings, &issue, "ASMA"));
    }

    #[test]
    fn a_scope_declaration_is_normalized_and_deduplicated() {
        let settings = JiraSettings {
            enabled: true,
            project: "ASMA".to_owned(),
            scope_jql: "project = ASMA".to_owned(),
            declared_keys: vec![
                "asma-2".to_owned(),
                "ASMA-2".to_owned(),
                "ASMA-1".to_owned(),
            ],
            endpoint: None,
            credential_alias: None,
            cadence_seconds: 120,
            full_sweep_seconds: 900,
            overlap_seconds: 180,
            max_pages: 200,
            max_issues: 5000,
            max_recheck: 500,
            page_size: 100,
            max_attempts: 3,
            scope_is_epics: true,
            parent_rounds: 5,
        };
        let normalized = normalize_settings(settings).expect("valid settings");
        assert_eq!(normalized.declared_keys, vec!["ASMA-1", "ASMA-2"]);
    }

    #[test]
    fn a_matching_jira_project_supplies_the_catalog_connection() {
        let root = tempfile::tempdir().expect("a state root");
        std::fs::write(
            root.path().join("jira.json"),
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "projects": [{
                    "project_id": "018f0000-0000-7000-8000-000000000001",
                    "endpoint": "https://tracker.example",
                    "project_key": "ASMA",
                    "credential_alias": "work"
                }]
            }))
            .expect("the fixture"),
        )
        .expect("jira.json written");
        let settings = JiraSettings {
            enabled: true,
            project: "ASMA".to_owned(),
            scope_jql: "project = ASMA".to_owned(),
            declared_keys: Vec::new(),
            endpoint: None,
            credential_alias: None,
            cadence_seconds: 120,
            full_sweep_seconds: 900,
            overlap_seconds: 180,
            max_pages: 200,
            max_issues: 5000,
            max_recheck: 500,
            page_size: 100,
            max_attempts: 3,
            scope_is_epics: true,
            parent_rounds: 5,
        };
        let (endpoint, alias) =
            resolve_connection(&settings, root.path()).expect("the matching project");
        assert_eq!(endpoint, "https://tracker.example");
        assert_eq!(alias, "work");
        let explicit = JiraSettings {
            endpoint: Some("https://catalog.example".to_owned()),
            credential_alias: Some("catalog-work".to_owned()),
            ..settings.clone()
        };
        let (endpoint, alias) =
            resolve_connection(&explicit, root.path()).expect("explicit values win");
        assert_eq!(endpoint, "https://catalog.example");
        assert_eq!(alias, "catalog-work");
        let missing = JiraSettings {
            project: "OTHER".to_owned(),
            ..settings
        };
        assert!(resolve_connection(&missing, root.path()).is_err());
    }

    #[test]
    fn the_epic_identity_schema_constant_is_owned_here() {
        assert_eq!(EPIC_IDENTITY_SCHEMA, "kontor/epic-identity@1");
    }
}
