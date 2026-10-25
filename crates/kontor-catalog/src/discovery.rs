//! Local discovery of configured repositories and worktrees.
//!
//! # What is read
//!
//! Exactly the plan-shaped Markdown the existing direct reader recognises:
//! tracked files under `_docs/`, `plan/` and `plans/`; `.github/plans` only on a
//! root that explicitly permits it; and non-ignored untracked files marked as
//! drafts. A QNR file under `.github/plans/qnr` is an execution unit, never an
//! epic.
//!
//! # What is never read
//!
//! A symlink reached through repository discovery, a file over 2 MiB, anything
//! outside a configured root, and any file whose path is not plan-shaped. The
//! scanner does not open `.env`, credential stores, tokens or provider homes.
//!
//! # Why `git` and not a watch
//!
//! `git ls-files` is the one operation that can distinguish tracked from
//! non-ignored untracked content without re-implementing `.gitignore` rules.
//! Watches would only accelerate a sweep; correctness never depends on one
//! being delivered, so this module installs none.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use kontor_core::id::Timestamp;
use serde::Deserialize;

use crate::config::{
    CatalogSettings, OperatorSource, RepositoryConfig, SourceKind, SourceRevisionKind, valid_epic,
    valid_logical_plan_id, valid_workstream,
};
use crate::contract::{
    AssociationKind, AssociationRelation, Conflict, ConflictKind, ContentKind, ContentRevision,
    Diagnostic, DiagnosticSeverity, EntryKind, RegistryState, RegistryStatus, RepositoryCoverage,
    RepositoryState, RevisionKind,
};
use crate::store::{ObservedAssociation, ObservedEntry, ObservedOccurrence};

/// A bounded file read's ceiling, matching the existing direct reader.
pub const MAX_SOURCE_BYTES: u64 = 2 * 1024 * 1024;
/// The maximum plan documents one sweep will retain.
pub const MAX_ENTRIES: usize = 4096;
/// The maximum Markdown paths one repository will consider.
pub const MAX_PATHS: usize = 8192;
/// The per-git-command deadline.
const GIT_TIMEOUT: Duration = Duration::from_secs(10);

/// The result of one discovery pass.
#[derive(Debug, Clone)]
pub struct Discovered {
    /// Staged entries.
    pub entries: Vec<ObservedEntry>,
    /// Per-repository coverage.
    pub coverage: Vec<RepositoryCoverage>,
    /// Attempt-level diagnostics.
    pub diagnostics: Vec<String>,
    /// The registry projection.
    pub registry: RegistryStatus,
    /// Files considered.
    pub files_seen: usize,
    /// Whether every configured root was attempted before the budget expired.
    pub finished: bool,
    /// Repository ids whose complete contents were read in this pass.
    ///
    /// Only these may be used to prove absence: a failed or unreached root has
    /// not been read, so nothing it held can be called deleted.
    pub scanned_repository_ids: Vec<String>,
}

/// Discover everything the effective settings name.
///
/// The budget is checked between roots and between files. Running out is a
/// partial pass (`finished = false`), never an error and never deletion.
pub async fn discover(settings: &CatalogSettings, now: Timestamp) -> Discovered {
    let budget = Duration::from_secs(settings.scan_budget_seconds.clamp(1, 3600));
    discover_with_budget(settings, now, budget).await
}

/// Discover with an explicit budget.
pub async fn discover_with_budget(
    settings: &CatalogSettings,
    now: Timestamp,
    budget: Duration,
) -> Discovered {
    let deadline = tokio::time::Instant::now() + budget;
    let mut diagnostics = Vec::new();
    let registry = read_registry(settings.registry_path.as_deref(), &mut diagnostics);
    let mut coverage = Vec::new();
    let mut staged: BTreeMap<String, ObservedEntry> = BTreeMap::new();
    let mut files_seen = 0usize;
    let mut finished = true;
    let mut scanned_repository_ids = Vec::new();

    for repository in &settings.repositories {
        if tokio::time::Instant::now() >= deadline {
            finished = false;
            coverage.push(RepositoryCoverage {
                root: repository.root.display().to_string(),
                repository_id: None,
                head: None,
                state: RepositoryState::NotReached,
                files_seen: 0,
                entries_observed: 0,
                github_plans_permitted: repository.include_github_plans,
                error: None,
            });
            continue;
        }
        let outcome = scan_repository(repository, &registry, now, deadline).await;
        files_seen += outcome.coverage.files_seen;
        if outcome.coverage.state == RepositoryState::Scanned
            && let Some(id) = &outcome.coverage.repository_id
        {
            scanned_repository_ids.push(id.clone());
        }
        diagnostics.extend(outcome.diagnostics);
        for entry in outcome.entries {
            merge_entry(&mut staged, entry);
        }
        coverage.push(outcome.coverage);
    }

    let identities: Vec<(PathBuf, String)> = coverage
        .iter()
        .filter_map(|repository| {
            repository
                .repository_id
                .clone()
                .map(|id| (PathBuf::from(&repository.root), id))
        })
        .collect();
    for source in &settings.sources {
        if source.kind != SourceKind::Plan {
            continue;
        }
        if tokio::time::Instant::now() >= deadline {
            finished = false;
            diagnostics.push(format!(
                "The scan budget expired before explicit source `{}` was read.",
                source.id
            ));
            continue;
        }
        match read_explicit_source(source, &identities, now).await {
            Ok((entry, extra)) => {
                diagnostics.extend(extra);
                merge_entry(&mut staged, entry);
            }
            Err(diagnostic) => diagnostics.push(diagnostic.message),
        }
    }

    // Explicit and declared associations can disagree. The operator's explicit
    // choice is retained; the declaration is retained as a conflicting
    // declaration, and neither creates a registry entry or a verified join.
    for entry in staged.values_mut() {
        apply_association_conflicts(entry, &registry, now);
    }

    let mut entries: Vec<ObservedEntry> = staged.into_values().collect();
    if entries.len() > MAX_ENTRIES {
        diagnostics.push(format!(
            "The sweep observed more than {MAX_ENTRIES} plan documents; the extra documents were not retained."
        ));
        entries.truncate(MAX_ENTRIES);
    }
    scan_epic_duplicates(&mut entries, now, &mut diagnostics);

    Discovered {
        entries,
        coverage,
        diagnostics,
        registry: registry.status,
        files_seen,
        finished,
        scanned_repository_ids,
    }
}

struct RepositoryScan {
    coverage: RepositoryCoverage,
    entries: Vec<ObservedEntry>,
    diagnostics: Vec<String>,
}

struct RegistryProjection {
    status: RegistryStatus,
    keys: BTreeSet<String>,
}

async fn scan_repository(
    repository: &RepositoryConfig,
    registry: &RegistryProjection,
    now: Timestamp,
    deadline: tokio::time::Instant,
) -> RepositoryScan {
    let mut diagnostics = Vec::new();
    let configured = match std::fs::canonicalize(&repository.root) {
        Ok(root) if root.is_dir() => root,
        Ok(_) => {
            return failed_repository(
                repository,
                "The configured repository root is not a directory.".to_owned(),
            );
        }
        Err(error) => {
            return failed_repository(
                repository,
                format!(
                    "The configured repository root could not be resolved ({}).",
                    error.kind()
                ),
            );
        }
    };
    let top_level = match git(&configured, &["rev-parse", "--show-toplevel"], deadline).await {
        Ok(text) => canonicalize_text(&text),
        Err(error) => {
            return failed_repository(
                repository,
                format!("The configured root is not a readable git working tree ({error})."),
            );
        }
    };
    if top_level.as_deref() != Some(configured.as_path()) {
        return failed_repository(
            repository,
            "The configured root is not the repository top level; a wider scan was refused."
                .to_owned(),
        );
    }
    let common = match git(&configured, &["rev-parse", "--git-common-dir"], deadline).await {
        Ok(text) => common_directory(&configured, &text),
        Err(error) => {
            return failed_repository(
                repository,
                format!("The repository identity could not be read ({error})."),
            );
        }
    };
    let head = git(&configured, &["rev-parse", "HEAD"], deadline)
        .await
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty());
    let repository_id = format!(
        "repo-{}",
        &crate::config::sha256(common.to_string_lossy().as_bytes())[..16]
    );

    let tracked = match git(&configured, &["ls-files", "-z", "--", "*.md"], deadline).await {
        Ok(text) => split_paths(&text),
        Err(error) => {
            return failed_repository(
                repository,
                format!("Tracked files could not be listed ({error})."),
            );
        }
    };
    let untracked = match git(
        &configured,
        &[
            "ls-files",
            "-z",
            "--others",
            "--exclude-standard",
            "--",
            "*.md",
        ],
        deadline,
    )
    .await
    {
        Ok(text) => split_paths(&text),
        Err(error) => {
            diagnostics.push(format!(
                "Ignored/untracked files could not be listed for one repository ({error})."
            ));
            Vec::new()
        }
    };

    let mut candidates: Vec<(String, ContentKind)> = Vec::new();
    for path in tracked {
        if candidate_path(&path, repository.include_github_plans) {
            candidates.push((path, ContentKind::Tracked));
        }
    }
    for path in untracked {
        if candidate_path(&path, repository.include_github_plans) {
            candidates.push((path, ContentKind::UntrackedDraft));
        }
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    candidates.dedup_by(|left, right| left.0 == right.0);
    if candidates.len() > MAX_PATHS {
        diagnostics.push(format!(
            "One repository listed more than {MAX_PATHS} candidate Markdown paths; the extra paths were not read."
        ));
        candidates.truncate(MAX_PATHS);
    }

    let mut entries = Vec::new();
    let mut files_seen = 0usize;
    for (relative, content_kind) in candidates {
        if tokio::time::Instant::now() >= deadline {
            diagnostics.push(
                "The scan budget expired inside a repository; its absence evidence is withheld."
                    .to_owned(),
            );
            return RepositoryScan {
                coverage: RepositoryCoverage {
                    root: configured.display().to_string(),
                    repository_id: Some(repository_id),
                    head,
                    state: RepositoryState::NotReached,
                    files_seen,
                    entries_observed: entries.len(),
                    github_plans_permitted: repository.include_github_plans,
                    error: None,
                },
                entries,
                diagnostics,
            };
        }
        files_seen += 1;
        let path = configured.join(&relative);
        let bytes = match bounded_read_inside(&configured, &path) {
            Ok(bytes) => bytes,
            Err(Skip::Diagnostic(diagnostic)) => {
                diagnostics.push(diagnostic);
                continue;
            }
            Err(Skip::Silent) => continue,
        };
        let (modified_at, byte_length) = match file_instant(&path) {
            Ok((modified_at, byte_length)) => (modified_at, byte_length),
            Err(diagnostic) => {
                diagnostics.push(diagnostic.message);
                continue;
            }
        };
        let document = parse_document(&bytes, &relative);
        if !document.plan_shaped {
            continue;
        }
        let kind = if relative.starts_with(".github/plans/qnr/") {
            EntryKind::ExecutionUnit
        } else {
            EntryKind::Plan
        };
        let logical_plan_id = document.explicit_id.as_deref().map_or_else(
            || generated_logical_id(&repository_id, &relative),
            ToOwned::to_owned,
        );
        let observed = ObservedEntry {
            logical_plan_id,
            explicit_id: document.explicit_id.clone(),
            preserve_id: document.explicit_id.is_some(),
            kind,
            title: document.title.clone(),
            occurrences: vec![ObservedOccurrence {
                occurrence_id: generated_occurrence_id(
                    &repository_id,
                    &configured,
                    &relative,
                    content_kind,
                ),
                repository_id: repository_id.clone(),
                repository_root: configured.display().to_string(),
                relative_path: relative.clone(),
                content_kind,
                revision_kind: match content_kind {
                    ContentKind::UntrackedDraft => RevisionKind::SavedDraft,
                    _ => RevisionKind::WorkingCopy,
                },
                content: ContentRevision {
                    revision_id: crate::config::sha256(&bytes),
                    byte_length,
                    modified_at,
                    title: document.title.clone(),
                    epic: document.epic.clone(),
                    workstream: document.workstream.clone(),
                },
                source_ids: Vec::new(),
                expected_sha256: None,
            }],
            associations: document.associations(&relative, registry, &now, &mut diagnostics),
            diagnostics: document.diagnostics(&relative, &now),
            conflicts: Vec::new(),
            rename_alias: None,
        };
        entries.push(observed);
    }

    RepositoryScan {
        coverage: RepositoryCoverage {
            root: configured.display().to_string(),
            repository_id: Some(repository_id),
            head,
            state: RepositoryState::Scanned,
            files_seen,
            entries_observed: entries.len(),
            github_plans_permitted: repository.include_github_plans,
            error: None,
        },
        entries,
        diagnostics,
    }
}

fn failed_repository(repository: &RepositoryConfig, error: String) -> RepositoryScan {
    RepositoryScan {
        coverage: RepositoryCoverage {
            root: repository.root.display().to_string(),
            repository_id: None,
            head: None,
            state: RepositoryState::Failed,
            files_seen: 0,
            entries_observed: 0,
            github_plans_permitted: repository.include_github_plans,
            error: Some(error.clone()),
        },
        entries: Vec::new(),
        diagnostics: vec![error],
    }
}

async fn read_explicit_source(
    source: &OperatorSource,
    repositories: &[(PathBuf, String)],
    now: Timestamp,
) -> Result<(ObservedEntry, Vec<String>), Diagnostic> {
    let mut diagnostics = Vec::new();
    let path = std::fs::canonicalize(&source.path).map_err(|error| Diagnostic {
        code: "explicit_source_unreadable".to_owned(),
        severity: DiagnosticSeverity::Warning,
        message: format!(
            "Operator source `{}` could not be resolved ({}).",
            source.id,
            error.kind()
        ),
        subject: Some(source.id.clone()),
        logical_plan_id: None,
        observed_at: now.to_string(),
    })?;
    let metadata = std::fs::symlink_metadata(&path).map_err(|error| Diagnostic {
        code: "explicit_source_unreadable".to_owned(),
        severity: DiagnosticSeverity::Warning,
        message: format!(
            "Operator source `{}` could not be inspected ({}).",
            source.id,
            error.kind()
        ),
        subject: Some(source.id.clone()),
        logical_plan_id: None,
        observed_at: now.to_string(),
    })?;
    if !metadata.is_file() || metadata.len() > MAX_SOURCE_BYTES {
        return Err(Diagnostic {
            code: "explicit_source_unbounded".to_owned(),
            severity: DiagnosticSeverity::Warning,
            message: format!(
                "Operator source `{}` is not a regular file of at most 2 MiB.",
                source.id
            ),
            subject: Some(source.id.clone()),
            logical_plan_id: None,
            observed_at: now.to_string(),
        });
    }
    let bytes = std::fs::read(&path).map_err(|error| Diagnostic {
        code: "explicit_source_unreadable".to_owned(),
        severity: DiagnosticSeverity::Warning,
        message: format!(
            "Operator source `{}` could not be read ({}).",
            source.id,
            error.kind()
        ),
        subject: Some(source.id.clone()),
        logical_plan_id: None,
        observed_at: now.to_string(),
    })?;
    let revision_id = crate::config::sha256(&bytes);
    let (modified_at, byte_length) = file_instant(&path).map_err(|diagnostic| *diagnostic)?;
    let document = parse_document(&bytes, &source.id.clone());
    let workstream = match (&source.workstream, &document.workstream) {
        (Some(explicit), Some(declared)) if explicit != declared => {
            diagnostics.push(format!(
                "Operator source `{}` retains its explicit workstream `{explicit}`; the plan declaration `{declared}` conflicts.",
                source.id
            ));
            Some(explicit.clone())
        }
        (Some(explicit), _) => Some(explicit.clone()),
        (None, declared) => declared.clone(),
    };
    let epic = match (&source.epic, &document.epic) {
        (Some(explicit), Some(declared)) if explicit != declared => {
            diagnostics.push(format!(
                "Operator source `{}` retains its explicit epic `{explicit}`; the plan declaration `{declared}` conflicts.",
                source.id
            ));
            Some(explicit.clone())
        }
        (Some(explicit), _) => Some(explicit.clone()),
        (None, declared) => declared.clone(),
    };
    let pin_matches = source
        .expected_sha256
        .as_deref()
        .map(|expected| expected == revision_id);
    let mut conflicts = Vec::new();
    if pin_matches == Some(false) {
        conflicts.push(Conflict {
            kind: ConflictKind::PinMismatch,
            logical_plan_id: None,
            message: format!(
                "Operator source `{}` does not match its declared pin.",
                source.id
            ),
            observed_at: now.to_string(),
        });
    }
    if source
        .workstream
        .as_ref()
        .zip(document.workstream.as_ref())
        .is_some_and(|(explicit, declared)| explicit != declared)
    {
        conflicts.push(Conflict {
            kind: ConflictKind::AssociationConflict,
            logical_plan_id: None,
            message: format!(
                "Operator source `{}` and its plan disagree about the workstream.",
                source.id
            ),
            observed_at: now.to_string(),
        });
    }
    let repository = repository_for(&path, repositories);
    let logical_plan_id = source
        .logical_plan_id
        .clone()
        .unwrap_or_else(|| generated_logical_id(&repository.0, &repository.2));
    let occurrence_id = generated_occurrence_id(
        &repository.0,
        &repository.1,
        &repository.2,
        ContentKind::ExternalSource,
    );
    let mut associations = Vec::new();
    if let Some(workstream) = &workstream {
        associations.push(ObservedAssociation {
            relation: AssociationRelation::Workstream,
            value: workstream.clone(),
            kind: AssociationKind::ExplicitOperator,
            source_id: Some(source.id.clone()),
            note: None,
        });
    }
    if let Some(epic) = &epic {
        associations.push(ObservedAssociation {
            relation: AssociationRelation::Epic,
            value: epic.clone(),
            kind: AssociationKind::ExplicitOperator,
            source_id: Some(source.id.clone()),
            note: None,
        });
    }
    if let Some(project) = &source.project_id {
        associations.push(ObservedAssociation {
            relation: AssociationRelation::Project,
            value: project.clone(),
            kind: AssociationKind::ExplicitOperator,
            source_id: Some(source.id.clone()),
            note: None,
        });
    }
    // The plan's own declaration stays visible even when the operator chose a
    // different value. It is a declaration, never a verified join.
    if let Some(declared) = &document.workstream
        && source.workstream.as_deref() != Some(declared.as_str())
    {
        associations.push(ObservedAssociation {
            relation: AssociationRelation::Workstream,
            value: declared.clone(),
            kind: AssociationKind::PlanDeclaration,
            source_id: None,
            note: None,
        });
    }
    if let Some(declared) = &document.epic
        && source.epic.as_deref() != Some(declared.as_str())
    {
        associations.push(ObservedAssociation {
            relation: AssociationRelation::Epic,
            value: declared.clone(),
            kind: AssociationKind::PlanDeclaration,
            source_id: None,
            note: None,
        });
    }
    let entry = ObservedEntry {
        logical_plan_id,
        explicit_id: document.explicit_id.clone(),
        preserve_id: true,
        kind: EntryKind::Plan,
        title: source
            .title
            .clone()
            .unwrap_or_else(|| document.title.clone()),
        occurrences: vec![ObservedOccurrence {
            occurrence_id,
            repository_id: repository.0,
            repository_root: repository.1.display().to_string(),
            relative_path: repository.2,
            content_kind: ContentKind::ExternalSource,
            revision_kind: match source.revision_kind {
                SourceRevisionKind::SavedDraft => RevisionKind::SavedDraft,
                SourceRevisionKind::ApprovedRevision => RevisionKind::ApprovedRevision,
                SourceRevisionKind::WorkingCopy => RevisionKind::WorkingCopy,
            },
            content: ContentRevision {
                revision_id,
                byte_length,
                modified_at,
                title: document.title.clone(),
                epic,
                workstream,
            },
            source_ids: vec![source.id.clone()],
            expected_sha256: source.expected_sha256.clone(),
        }],
        associations,
        diagnostics: Vec::new(),
        conflicts,
        rename_alias: None,
    };
    Ok((entry, diagnostics))
}

fn repository_for(path: &Path, repositories: &[(PathBuf, String)]) -> (String, PathBuf, String) {
    for (root, repository_id) in repositories {
        if let Ok(relative) = path.strip_prefix(root) {
            return (
                repository_id.clone(),
                root.clone(),
                relative.to_string_lossy().replace('\\', "/"),
            );
        }
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("/"));
    let repository_id = format!(
        "external-{}",
        &crate::config::sha256(parent.to_string_lossy().as_bytes())[..16]
    );
    (
        repository_id,
        parent.to_path_buf(),
        path.file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
    )
}

fn merge_entry(staged: &mut BTreeMap<String, ObservedEntry>, incoming: ObservedEntry) {
    // The same file may arrive twice: once from repository discovery and once
    // from an explicit operator source. Identity is the file, then the id: an
    // explicit source's validated logical id renames the discovered row rather
    // than creating a second answer for one file.
    let matched = staged.iter().find_map(|(key, existing)| {
        existing
            .occurrences
            .iter()
            .any(|current| {
                incoming
                    .occurrences
                    .iter()
                    .any(|next| same_occurrence(current, next))
            })
            .then(|| key.clone())
    });
    let key = matched.or_else(|| {
        staged
            .contains_key(&incoming.logical_plan_id)
            .then(|| incoming.logical_plan_id.clone())
    });
    match key {
        Some(key) => absorb_staged(staged, key, incoming),
        None => {
            staged.insert(incoming.logical_plan_id.clone(), incoming);
        }
    }
}

fn absorb_staged(
    staged: &mut BTreeMap<String, ObservedEntry>,
    key: String,
    incoming: ObservedEntry,
) {
    let mut existing = staged.remove(&key).expect("the matched entry");
    let final_id = if incoming.preserve_id {
        incoming.logical_plan_id.clone()
    } else {
        key
    };
    if final_id != existing.logical_plan_id
        && let Some(other) = staged.remove(&final_id)
    {
        absorb(&mut existing, other);
    }
    absorb(&mut existing, incoming);
    existing.logical_plan_id = final_id.clone();
    staged.insert(final_id, existing);
}

fn absorb(existing: &mut ObservedEntry, mut incoming: ObservedEntry) {
    for occurrence in incoming.occurrences.drain(..) {
        if let Some(current) = existing
            .occurrences
            .iter_mut()
            .find(|current| same_occurrence(current, &occurrence))
        {
            if occurrence.content_kind == ContentKind::ExternalSource {
                current.revision_kind = occurrence.revision_kind;
                current.expected_sha256 = occurrence.expected_sha256.clone();
                current.content.epic = occurrence.content.epic;
                current.content.workstream = occurrence.content.workstream;
            }
            for source_id in occurrence.source_ids {
                if !current.source_ids.contains(&source_id) {
                    current.source_ids.push(source_id);
                }
            }
            current.source_ids.sort();
        } else {
            existing.occurrences.push(occurrence);
        }
    }
    for association in incoming.associations {
        if !existing.associations.iter().any(|current| {
            current.relation == association.relation
                && current.value == association.value
                && current.kind == association.kind
                && current.source_id == association.source_id
        }) {
            existing.associations.push(association);
        }
    }
    for diagnostic in incoming.diagnostics {
        if !existing
            .diagnostics
            .iter()
            .any(|current| current.code == diagnostic.code && current.subject == diagnostic.subject)
        {
            existing.diagnostics.push(diagnostic);
        }
    }
    for conflict in incoming.conflicts {
        if !existing
            .conflicts
            .iter()
            .any(|current| current.kind == conflict.kind)
        {
            existing.conflicts.push(conflict);
        }
    }
    if incoming.explicit_id.is_some() {
        existing.explicit_id = incoming.explicit_id;
    }
    existing.preserve_id |= incoming.preserve_id;
}

fn same_occurrence(left: &ObservedOccurrence, right: &ObservedOccurrence) -> bool {
    left.repository_id == right.repository_id
        && left.repository_root == right.repository_root
        && left.relative_path == right.relative_path
        && (left.content_kind == right.content_kind
            || left.content_kind == ContentKind::ExternalSource
            || right.content_kind == ContentKind::ExternalSource)
}

fn apply_association_conflicts(
    entry: &mut ObservedEntry,
    registry: &RegistryProjection,
    now: Timestamp,
) {
    for occurrence in &entry.occurrences {
        if let Some(workstream) = &occurrence.content.workstream {
            let explicit: Vec<&str> = entry
                .associations
                .iter()
                .filter(|association| {
                    association.relation == AssociationRelation::Workstream
                        && association.kind == AssociationKind::ExplicitOperator
                })
                .map(|association| association.value.as_str())
                .collect();
            if explicit.iter().any(|value| *value != workstream) {
                entry.conflicts.push(Conflict {
                    kind: ConflictKind::AssociationConflict,
                    logical_plan_id: Some(entry.logical_plan_id.clone()),
                    message: "An explicit operator workstream conflicts with the plan declaration."
                        .to_owned(),
                    observed_at: now.to_string(),
                });
            } else if !entry.associations.iter().any(|association| {
                association.relation == AssociationRelation::Workstream
                    && association.value == *workstream
            }) {
                entry.associations.push(ObservedAssociation {
                    relation: AssociationRelation::Workstream,
                    value: workstream.clone(),
                    kind: AssociationKind::PlanDeclaration,
                    source_id: None,
                    note: registry_note(registry, workstream),
                });
            }
        }
        if let Some(epic) = &occurrence.content.epic
            && !entry.associations.iter().any(|association| {
                association.relation == AssociationRelation::Epic && association.value == *epic
            })
        {
            entry.associations.push(ObservedAssociation {
                relation: AssociationRelation::Epic,
                value: epic.clone(),
                kind: AssociationKind::PlanDeclaration,
                source_id: None,
                note: None,
            });
        }
    }
    let mut seen = BTreeSet::new();
    entry.associations.retain(|association| {
        seen.insert((
            association.relation,
            association.value.clone(),
            association.kind,
            association.source_id.clone(),
        ))
    });
}

fn registry_note(registry: &RegistryProjection, workstream: &str) -> Option<String> {
    match registry.status.state {
        RegistryState::Absent => Some("No workstream registry is configured.".to_owned()),
        RegistryState::Invalid => Some("The workstream registry could not be read.".to_owned()),
        RegistryState::Ready => {
            if registry.keys.contains(workstream) {
                None
            } else {
                Some(format!(
                    "Workstream `{workstream}` is not declared in the selected registry."
                ))
            }
        }
    }
}

fn scan_epic_duplicates(
    entries: &mut [ObservedEntry],
    _now: Timestamp,
    diagnostics: &mut Vec<String>,
) {
    let mut by_epic: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for entry in entries.iter() {
        for occurrence in &entry.occurrences {
            if let Some(epic) = &occurrence.content.epic {
                by_epic
                    .entry(epic.clone())
                    .or_default()
                    .insert(entry.logical_plan_id.clone());
            }
        }
    }
    for (epic, plans) in by_epic {
        if plans.len() > 1 {
            diagnostics.push(format!(
                "{} logical plans declare epic {epic}; operator choice remains explicit.",
                plans.len()
            ));
        }
    }
}

/// The document facts local discovery uses.
struct PlanDocument {
    plan_shaped: bool,
    title: String,
    workstream: Option<String>,
    epic: Option<String>,
    explicit_id: Option<String>,
    diagnostics: Vec<(String, DiagnosticSeverity, String)>,
}

impl PlanDocument {
    fn diagnostics(&self, subject: &str, now: &Timestamp) -> Vec<Diagnostic> {
        self.diagnostics
            .iter()
            .map(|(code, severity, message)| Diagnostic {
                code: code.clone(),
                severity: *severity,
                message: message.clone(),
                subject: Some(subject.to_owned()),
                logical_plan_id: None,
                observed_at: now.to_string(),
            })
            .collect()
    }

    fn associations(
        &self,
        _subject: &str,
        registry: &RegistryProjection,
        now: &Timestamp,
        diagnostics: &mut Vec<String>,
    ) -> Vec<ObservedAssociation> {
        let mut associations = Vec::new();
        if let Some(workstream) = &self.workstream {
            match registry.status.state {
                RegistryState::Ready if !registry.keys.contains(workstream) => {
                    diagnostics.push(format!(
                        "Plan declares unknown workstream `{workstream}`; it remains a declaration and creates no registry entry."
                    ));
                }
                RegistryState::Absent => diagnostics.push(
                    "Plan declares a workstream but no registry is configured; it remains a declaration."
                        .to_owned(),
                ),
                _ => {}
            }
            associations.push(ObservedAssociation {
                relation: AssociationRelation::Workstream,
                value: workstream.clone(),
                kind: AssociationKind::PlanDeclaration,
                source_id: None,
                note: registry_note(registry, workstream),
            });
        }
        if let Some(epic) = &self.epic {
            associations.push(ObservedAssociation {
                relation: AssociationRelation::Epic,
                value: epic.clone(),
                kind: AssociationKind::PlanDeclaration,
                source_id: None,
                note: None,
            });
        }
        let _ = now;
        associations
    }
}

fn parse_document(bytes: &[u8], subject: &str) -> PlanDocument {
    let Ok(content) = std::str::from_utf8(bytes) else {
        return PlanDocument {
            plan_shaped: false,
            title: subject.to_owned(),
            workstream: None,
            epic: None,
            explicit_id: None,
            diagnostics: vec![(
                "source_not_utf8".to_owned(),
                DiagnosticSeverity::Warning,
                "The plan is not valid UTF-8 and its metadata remains unknown.".to_owned(),
            )],
        };
    };
    let mut document = PlanDocument {
        plan_shaped: false,
        title: title_of(content).unwrap_or_else(|| subject.to_owned()),
        workstream: None,
        epic: None,
        explicit_id: None,
        diagnostics: Vec::new(),
    };
    let fence = graph_fence(content);
    document.plan_shaped = fence.is_some() || primary_plan_path(subject);
    let front = front_matter(content);
    let mut frontmatter_epic = None;
    match front {
        FrontMatter::Absent => {}
        FrontMatter::Unterminated => document.diagnostics.push((
            "frontmatter_unterminated".to_owned(),
            DiagnosticSeverity::Warning,
            "Unterminated front matter; plan metadata remains unknown.".to_owned(),
        )),
        FrontMatter::Invalid => document.diagnostics.push((
            "frontmatter_invalid".to_owned(),
            DiagnosticSeverity::Warning,
            "Unsupported or invalid front matter; plan metadata remains unknown.".to_owned(),
        )),
        FrontMatter::Value(value) => {
            let Some(mapping) = value.as_mapping() else {
                document.diagnostics.push((
                    "frontmatter_invalid".to_owned(),
                    DiagnosticSeverity::Warning,
                    "Front matter is not a mapping; plan metadata remains unknown.".to_owned(),
                ));
                return finish_graph_with(document, fence.as_deref(), content, None);
            };
            let text = |key: &str| {
                mapping
                    .get(serde_yaml_ng::Value::String(key.to_owned()))
                    .and_then(serde_yaml_ng::Value::as_str)
            };
            if let Some(workstream) = text("workstream") {
                if valid_workstream(workstream) {
                    document.workstream = Some(workstream.to_owned());
                } else {
                    document.diagnostics.push((
                        "workstream_invalid".to_owned(),
                        DiagnosticSeverity::Warning,
                        "Unsupported workstream declaration; plan workstream remains unknown."
                            .to_owned(),
                    ));
                }
            }
            let declarations: Vec<&str> = ["epic", "jira_epic", "jira"]
                .into_iter()
                .filter_map(text)
                .collect();
            if declarations.is_empty() {
                // No epic declaration is valid.
            } else if declarations.iter().all(|value| valid_epic(value))
                && declarations.iter().collect::<BTreeSet<_>>().len() == 1
            {
                frontmatter_epic = Some(declarations[0].to_owned());
            } else {
                document.diagnostics.push((
                    "epic_invalid".to_owned(),
                    DiagnosticSeverity::Warning,
                    "Invalid or conflicting front matter epic declarations; epic remains unknown."
                        .to_owned(),
                ));
            }
            let ids: Vec<&str> = ["catalog_id", "plan_id"]
                .into_iter()
                .filter_map(text)
                .collect();
            if !ids.is_empty() && ids.iter().all(|value| *value == ids[0]) {
                // The same id under both spellings is one declaration.
                if valid_logical_plan_id(ids[0]) {
                    document.explicit_id = Some(ids[0].to_owned());
                } else {
                    document.diagnostics.push((
                        "explicit_id_invalid".to_owned(),
                        DiagnosticSeverity::Warning,
                        "An explicit plan id is invalid and was ignored.".to_owned(),
                    ));
                }
            } else if !ids.is_empty() {
                document.diagnostics.push((
                    "explicit_id_invalid".to_owned(),
                    DiagnosticSeverity::Warning,
                    "An explicit plan id is conflicting and was ignored.".to_owned(),
                ));
            }
        }
    }
    finish_graph_with(document, fence.as_deref(), content, frontmatter_epic)
}

fn finish_graph_with(
    mut document: PlanDocument,
    fence: Option<&str>,
    _content: &str,
    frontmatter_epic: Option<String>,
) -> PlanDocument {
    let graph_epic =
        fence.and_then(
            |fence| match serde_json::from_str::<serde_json::Value>(fence) {
                Ok(value) => value
                    .get("epic")
                    .and_then(|epic| epic.get("jira"))
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned),
                Err(_) => {
                    document.diagnostics.push((
                        "graph_invalid".to_owned(),
                        DiagnosticSeverity::Warning,
                        "The plan-graph block is not valid JSON; its epic remains unknown."
                            .to_owned(),
                    ));
                    None
                }
            },
        );
    document.epic = match (graph_epic, frontmatter_epic) {
        (Some(graph), Some(front)) if graph != front => {
            document.diagnostics.push((
                "epic_conflict".to_owned(),
                DiagnosticSeverity::Warning,
                "The plan graph and front matter declare different epics; epic remains unknown."
                    .to_owned(),
            ));
            None
        }
        (Some(graph), _) => {
            if valid_epic(&graph) {
                Some(graph)
            } else {
                document.diagnostics.push((
                    "epic_invalid".to_owned(),
                    DiagnosticSeverity::Warning,
                    "The plan graph epic is not an ASMA epic key; epic remains unknown.".to_owned(),
                ));
                None
            }
        }
        (None, front) => front,
    };
    document
}

enum FrontMatter {
    Absent,
    Unterminated,
    Invalid,
    Value(serde_yaml_ng::Value),
}

fn front_matter(content: &str) -> FrontMatter {
    let trimmed = content.strip_prefix('\u{feff}').unwrap_or(content);
    let Some(rest) = trimmed
        .strip_prefix("---\n")
        .or_else(|| trimmed.strip_prefix("---\r\n"))
    else {
        return FrontMatter::Absent;
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let stripped = line.trim_end_matches(['\r', '\n']);
        if stripped == "---" || stripped == "..." {
            let header = &rest[..offset];
            return match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(header) {
                Ok(value) => FrontMatter::Value(value),
                Err(_) => FrontMatter::Invalid,
            };
        }
        offset += line.len();
    }
    FrontMatter::Unterminated
}

fn graph_fence(content: &str) -> Option<String> {
    let mut lines = content.lines();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start_matches(' ');
        if line.len() - trimmed.len() > 3 {
            continue;
        }
        let Some(rest) = trimmed
            .strip_prefix("```")
            .or_else(|| trimmed.strip_prefix("~~~"))
        else {
            continue;
        };
        if rest.trim() != "plan-graph" {
            continue;
        }
        let mut block = String::new();
        for line in lines.by_ref() {
            if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
                return Some(block);
            }
            block.push_str(line);
            block.push('\n');
        }
        return Some(block);
    }
    None
}

fn title_of(content: &str) -> Option<String> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    for line in content.lines() {
        if let Some(title) = line.strip_prefix("# ") {
            let title = title.trim();
            if !title.is_empty() {
                return Some(title.chars().take(300).collect());
            }
        }
    }
    None
}

fn primary_plan_path(relative: &str) -> bool {
    if relative.starts_with(".github/plans/")
        || relative
            .split('/')
            .any(|part| part == "plan" || part == "plans")
    {
        return true;
    }
    let file = relative.rsplit('/').next().unwrap_or(relative);
    let lower = file.to_ascii_lowercase();
    if lower.starts_with("plan-") || lower.starts_with("plan_") || lower == "plan.md" {
        return true;
    }
    lower.contains("-plan.") || lower.contains("_plan.") || lower.contains("-plan-")
}

/// Whether a repository-relative Markdown path is in scope at all.
///
/// Every Markdown document in an in-scope directory is inspected, because a
/// plan under `_docs` may carry a `plan-graph` fence without a plan-shaped file
/// name. The document parser decides whether it is plan-shaped; this function
/// only refuses directories that are never plan material.
fn candidate_path(relative: &str, include_github_plans: bool) -> bool {
    let in_scope = relative.starts_with("_docs/")
        || relative.starts_with("plan/")
        || relative.starts_with("plans/")
        || (include_github_plans && relative.starts_with(".github/plans/"));
    in_scope && !excluded_path(relative)
}

fn excluded_path(relative: &str) -> bool {
    let parts: Vec<&str> = relative.split('/').collect();
    let file = parts.last().copied().unwrap_or(relative);
    let lower = file.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "readme.md" | "index.md" | "adoption-record.md"
    ) || lower.ends_with("-router.md")
        || lower.ends_with("_router.md")
    {
        return true;
    }
    parts.iter().any(|part| {
        matches!(
            *part,
            "skills"
                | "node_modules"
                | "audit"
                | "audits"
                | "report"
                | "reports"
                | "history"
                | "evidence"
                | "reference"
        )
    })
}

enum Skip {
    Silent,
    Diagnostic(String),
}

fn bounded_read_inside(root: &Path, path: &Path) -> Result<Vec<u8>, Skip> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| {
        Skip::Diagnostic("A listed plan disappeared before it was read.".to_owned())
    })?;
    if metadata.file_type().is_symlink() {
        return Err(Skip::Diagnostic(
            "A plan path is a symlink and was not followed.".to_owned(),
        ));
    }
    if !metadata.is_file() {
        return Err(Skip::Silent);
    }
    if metadata.len() > MAX_SOURCE_BYTES {
        return Err(Skip::Diagnostic(
            "A plan is larger than 2 MiB and was not read.".to_owned(),
        ));
    }
    let canonical = std::fs::canonicalize(path).map_err(|_| {
        Skip::Diagnostic("A plan path could not be resolved inside its root.".to_owned())
    })?;
    if !canonical.starts_with(root) {
        return Err(Skip::Diagnostic(
            "A plan resolved outside its configured root and was not read.".to_owned(),
        ));
    }
    std::fs::read(&canonical).map_err(|_| Skip::Diagnostic("A plan could not be read.".to_owned()))
}

fn file_instant(path: &Path) -> Result<(String, u64), Box<Diagnostic>> {
    let metadata = std::fs::metadata(path).map_err(|_| {
        Box::new(Diagnostic {
            code: "source_unreadable".to_owned(),
            severity: DiagnosticSeverity::Warning,
            message: "A plan's metadata could not be read.".to_owned(),
            subject: None,
            logical_plan_id: None,
            observed_at: Timestamp::now().to_string(),
        })
    })?;
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .and_then(|seconds| Timestamp::from_second(seconds).ok())
        .map_or_else(|| Timestamp::now().to_string(), |time| time.to_string());
    Ok((modified_at, metadata.len()))
}

fn split_paths(output: &str) -> Vec<String> {
    output
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(|path| path.replace('\\', "/"))
        .collect()
}

fn generated_logical_id(repository_id: &str, relative: &str) -> String {
    format!(
        "plan-{}",
        &crate::config::sha256(format!("{repository_id}:{relative}").as_bytes())[..24]
    )
}

fn generated_occurrence_id(
    repository_id: &str,
    root: &Path,
    relative: &str,
    content_kind: ContentKind,
) -> String {
    format!(
        "occ-{}",
        &crate::config::sha256(
            format!(
                "{repository_id}|{}|{relative}|{}",
                root.display(),
                match content_kind {
                    ContentKind::Tracked => "tracked",
                    ContentKind::UntrackedDraft => "untracked_draft",
                    ContentKind::ExternalSource => "external_source",
                }
            )
            .as_bytes()
        )[..24]
    )
}

async fn git(
    root: &Path,
    arguments: &[&str],
    deadline: tokio::time::Instant,
) -> Result<String, String> {
    let mut command = tokio::process::Command::new("git");
    command
        .arg("-C")
        .arg(root)
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null());
    let output = tokio::time::timeout_at(
        deadline.min(tokio::time::Instant::now() + GIT_TIMEOUT),
        command.output(),
    )
    .await
    .map_err(|_| "git did not answer before its deadline".to_owned())?
    .map_err(|error| format!("git could not be started ({})", error.kind()))?;
    if !output.status.success() {
        return Err(format!(
            "git exited with {}",
            output.status.code().unwrap_or(-1)
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| "git produced non-UTF-8 output".to_owned())
}

fn canonicalize_text(text: &str) -> Option<PathBuf> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    std::fs::canonicalize(trimmed).ok()
}

fn common_directory(root: &Path, text: &str) -> PathBuf {
    let trimmed = text.trim();
    let path = Path::new(trimmed);
    if path.is_absolute() {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    } else {
        std::fs::canonicalize(root.join(path)).unwrap_or_else(|_| root.join(path))
    }
}

#[derive(Deserialize)]
struct RegistryDocument {
    schema_version: u32,
    workstreams: BTreeMap<String, serde_json::Value>,
}

fn read_registry(path: Option<&Path>, diagnostics: &mut Vec<String>) -> RegistryProjection {
    let Some(path) = path else {
        return RegistryProjection {
            status: RegistryStatus {
                path: None,
                revision: None,
                state: RegistryState::Absent,
                workstreams: 0,
                diagnostics: Vec::new(),
            },
            keys: BTreeSet::new(),
        };
    };
    let display = path.display().to_string();
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            diagnostics.push(format!(
                "The workstream registry could not be read ({}); the registry is absent.",
                error.kind()
            ));
            return RegistryProjection {
                status: RegistryStatus {
                    path: Some(display),
                    revision: None,
                    state: RegistryState::Invalid,
                    workstreams: 0,
                    diagnostics: Vec::new(),
                },
                keys: BTreeSet::new(),
            };
        }
    };
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        diagnostics.push(
            "The workstream registry is larger than 2 MiB; the registry is absent.".to_owned(),
        );
        return RegistryProjection {
            status: RegistryStatus {
                path: Some(display),
                revision: None,
                state: RegistryState::Invalid,
                workstreams: 0,
                diagnostics: Vec::new(),
            },
            keys: BTreeSet::new(),
        };
    }
    let revision = crate::config::sha256(&bytes);
    let document: RegistryDocument = match serde_json::from_slice(&bytes) {
        Ok(document) => document,
        Err(error) => {
            diagnostics.push(format!(
                "The workstream registry is invalid at line {} column {}.",
                error.line(),
                error.column()
            ));
            return RegistryProjection {
                status: RegistryStatus {
                    path: Some(display),
                    revision: Some(revision),
                    state: RegistryState::Invalid,
                    workstreams: 0,
                    diagnostics: Vec::new(),
                },
                keys: BTreeSet::new(),
            };
        }
    };
    if document.schema_version != 1 {
        diagnostics.push(format!(
            "The workstream registry schema_version {} is unsupported.",
            document.schema_version
        ));
        return RegistryProjection {
            status: RegistryStatus {
                path: Some(display),
                revision: Some(revision),
                state: RegistryState::Invalid,
                workstreams: 0,
                diagnostics: Vec::new(),
            },
            keys: BTreeSet::new(),
        };
    }
    let registry_diagnostics: Vec<String> = document
        .workstreams
        .keys()
        .filter(|key| !valid_workstream(key))
        .map(|key| format!("The registry declares an invalid workstream key `{key}`."))
        .collect();
    RegistryProjection {
        status: RegistryStatus {
            path: Some(display),
            revision: Some(revision),
            state: RegistryState::Ready,
            workstreams: document.workstreams.len(),
            diagnostics: registry_diagnostics,
        },
        keys: document.workstreams.keys().cloned().collect(),
    }
}
