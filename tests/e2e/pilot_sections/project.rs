//! Section 1 — the disposable project and its five work profiles.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use kontor_profiles::pack::{PackCategoryKey, ProfilePackSpec, parse_pack, resolve_profile};
use kontor_profiles::seeds::bundled_pack;
use kontor_tests_e2e::Bundle;
use serde_json::json;

use super::fixture::PilotProject;
use crate::{INCIDENT_PACK, PROJECT_FIXTURE, at};

/// Resolve every pilot profile and prove the custom one needed no code.
pub(crate) async fn run(bundle: &mut Bundle) {
    let fixture = PilotProject::parse(PROJECT_FIXTURE);
    let seeds = bundled_pack().expect("the bundled profile pack loads");
    let incident = match parse_pack(INCIDENT_PACK) {
        Ok(pack) => pack,
        Err(error) => {
            bundle.fail(
                "project.custom-profile",
                format!("the incident pack did not validate: {error}"),
            );
            bundle.fail(
                "project.profiles",
                "the incident pack did not load, so the five snapshots were never resolved",
            );
            return;
        }
    };

    record_disposable_project(bundle, &fixture);
    resolve_every_profile(bundle, &fixture, &seeds, &incident);
    custom_profile_needs_no_branch(bundle, &incident);
}

/// Write down which project the pilot claims to be about.
///
/// This answers no criterion on its own, and it is here because an inspector
/// rerunning the proof needs to know what "the disposable project" *was* before
/// any of the cases below mean anything. Every section mints its own realm and
/// its own project id — a shared one would let a case pass on another case's
/// leftovers — so the fixture's identity is a declaration, not a row id, and it
/// is recorded as such.
fn record_disposable_project(bundle: &mut Bundle, fixture: &PilotProject) {
    let _ = bundle.artifact(
        "snapshots/pilot-project.json",
        &json!({
            "declared_name": fixture.project.name,
            "declared_root_path": fixture.project.root_path,
            "realm_scope": "each section opens its own realm and project so no case can pass on \
                            another case's residue; this is the fixture's declared identity, not \
                            a persisted row",
            "tasks": fixture
                .tasks
                .iter()
                .map(|task| json!({
                    "key": task.key,
                    "title": task.title,
                    "module": task.module,
                    "pack": task.pack,
                    "category": task.category,
                    "worktree": task.worktree,
                    "expected_to_be_admissible": task.isolated,
                }))
                .collect::<Vec<_>>(),
        }),
    );
}

/// Every pilot task resolves its pinned, content-hashed profile snapshot.
fn resolve_every_profile(
    bundle: &mut Bundle,
    fixture: &PilotProject,
    seeds: &ProfilePackSpec,
    incident: &ProfilePackSpec,
) {
    let resolved_at = at("2026-08-12T09:00:00Z");
    let mut snapshots = Vec::new();
    let mut problems = Vec::new();
    let mut artifacts = Vec::new();

    // The contender shares pilot-code's profile, so the five *distinct* pinned
    // snapshots come from the isolated tasks.
    for task in fixture.isolated() {
        let pack = if task.pack == "incident" {
            incident
        } else {
            seeds
        };
        let category = match PackCategoryKey::parse(&task.category) {
            Ok(category) => category,
            Err(error) => {
                problems.push(format!(
                    "{}: category is not a legal key ({error})",
                    task.key
                ));
                continue;
            }
        };
        let resolved = match resolve_profile(pack, &category, resolved_at) {
            Ok(resolved) => resolved,
            Err(error) => {
                problems.push(format!("{}: did not resolve ({error})", task.key));
                continue;
            }
        };
        if let Err(error) = resolved.verify() {
            problems.push(format!(
                "{}: bundle digest did not re-derive ({error})",
                task.key
            ));
            continue;
        }

        let declared: Vec<String> = resolved
            .profile
            .definition
            .phases
            .iter()
            .map(|phase| phase.id.to_string())
            .collect();
        if declared != task.expected_phases {
            problems.push(format!(
                "{}: declares {declared:?}, the fixture expects {:?}",
                task.key, task.expected_phases
            ));
            continue;
        }

        let relative = format!("snapshots/profiles/{}.json", task.key);
        let written = bundle
            .artifact(
                &relative,
                &json!({
                    "task": task.key,
                    "pack_id": resolved.pack_id.to_string(),
                    "pack_version": resolved.pack_version,
                    "category": resolved.category.to_string(),
                    "profile": resolved.profile.definition.id.to_string(),
                    "profile_version": resolved.profile.definition.version,
                    "definition_hash": resolved.profile.definition_hash.to_string(),
                    "bundle_hash": resolved.bundle_hash.to_string(),
                    "team_template": resolved.team.as_ref().map(|team| team.template_id.to_string()),
                    "phases": declared,
                    "gates": resolved
                        .profile
                        .definition
                        .gates
                        .iter()
                        .map(|gate| gate.id.to_string())
                        .collect::<Vec<_>>(),
                }),
            )
            .expect("the profile snapshot is written");
        artifacts.push(written);
        snapshots.push((task.key.clone(), resolved));
    }

    let distinct: BTreeSet<String> = snapshots
        .iter()
        .map(|(_, resolved)| resolved.bundle_hash.to_string())
        .collect();

    if !problems.is_empty() {
        bundle.fail("project.profiles", problems.join("; "));
        return;
    }
    if snapshots.len() != 5 || distinct.len() != 5 {
        bundle.fail(
            "project.profiles",
            format!(
                "expected five distinct pinned snapshots, got {} snapshots with {} distinct digests",
                snapshots.len(),
                distinct.len()
            ),
        );
        return;
    }
    bundle.pass(
        "project.profiles",
        format!(
            "five tasks resolved five distinct content-hashed bundles and each re-derived its own digest: {}",
            snapshots
                .iter()
                .map(|(key, resolved)| format!(
                    "{key}={} v{}",
                    resolved.profile.definition.id,
                    resolved.profile.definition.version.get()
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        &artifacts,
    );
}

/// The incident profile is data: no shipped source names any of its ids.
///
/// The scan is over the pilot-specific ids only. A phase spelled `recovery` is a
/// word the control plane may legitimately use for its own recovery episodes, so
/// scanning for it would report a collision that is not one. The ids below exist
/// nowhere but this fixture, which is what makes their absence meaningful.
fn custom_profile_needs_no_branch(bundle: &mut Bundle, incident: &ProfilePackSpec) {
    let profile = &incident.profiles[0];
    let mut needles: Vec<String> = vec![profile.id.to_string()];
    needles.extend(incident.roles.iter().map(|role| role.role.to_string()));
    needles.extend(incident.skills.iter().map(|skill| skill.skill.to_string()));
    needles.extend(profile.gates.iter().map(|gate| gate.id.to_string()));
    needles.extend(
        profile
            .artifacts
            .iter()
            .map(|artifact| artifact.key.to_string()),
    );
    needles.sort();
    needles.dedup();

    let root = kontor_tests_e2e::repo_root();
    let mut shipped: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = fs::read_dir(root.join("crates")) {
        for entry in entries.flatten() {
            let source = entry.path().join("src");
            if source.is_dir() {
                shipped.push(source);
            }
        }
    }
    shipped.push(root.join("apps/console/src"));
    shipped.push(root.join("apps/desktop/src-tauri/src"));

    let mut hits: Vec<String> = Vec::new();
    for directory in &shipped {
        for file in source_files(directory) {
            let Ok(text) = fs::read_to_string(&file) else {
                continue;
            };
            for needle in &needles {
                if text.contains(&format!("\"{needle}\"")) {
                    hits.push(format!(
                        "{} names \"{needle}\"",
                        file.strip_prefix(&root).unwrap_or(&file).display()
                    ));
                }
            }
        }
    }

    let resolved = resolve_profile(
        incident,
        &PackCategoryKey::parse("incident-response-v1").expect("a legal category key"),
        at("2026-08-12T09:00:00Z"),
    );
    let Ok(resolved) = resolved else {
        bundle.fail(
            "project.custom-profile",
            "the incident category did not resolve from its own pack",
        );
        return;
    };

    let artifact = bundle
        .artifact(
            "runtime/custom-profile-scan.json",
            &json!({
                "profile": profile.id.to_string(),
                "phases": profile.phases.iter().map(|phase| phase.id.to_string()).collect::<Vec<_>>(),
                "scanned_ids": needles,
                "scanned_directories": shipped
                    .iter()
                    .filter(|path| path.is_dir())
                    .map(|path| path.strip_prefix(&root).unwrap_or(path).display().to_string())
                    .collect::<Vec<_>>(),
                "hits": hits,
                "bundle_hash": resolved.bundle_hash.to_string(),
            }),
        )
        .expect("the scan is written");

    if hits.is_empty() {
        bundle.pass(
            "project.custom-profile",
            format!(
                "`{}` resolved and executed as fixture data; none of its {} pilot-specific ids appears \
                 as a literal in any shipped crate or client source file",
                profile.id,
                needles.len()
            ),
            &[artifact],
        );
    } else {
        bundle.fail(
            "project.custom-profile",
            format!(
                "shipped source branches on the custom profile: {}",
                hits.join("; ")
            ),
        );
    }
}

/// Every `.rs`, `.ts` and `.tsx` file under `directory`.
fn source_files(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![directory.to_path_buf()];
    while let Some(path) = stack.pop() {
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            if let Ok(entries) = fs::read_dir(&path) {
                for entry in entries.flatten() {
                    stack.push(entry.path());
                }
            }
            continue;
        }
        let is_source = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| matches!(extension, "rs" | "ts" | "tsx"));
        if is_source {
            found.push(path);
        }
    }
    found.sort();
    found
}
