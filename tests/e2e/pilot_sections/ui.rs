//! The `ui/` half of the bundle — what the console proves about width and
//! keyboard reach, and what this tree cannot prove at all.
//!
//! This section answers no acceptance criterion. The desktop/phone claim itself
//! is a *contract* claim — the same canonical history, ids, cursors and
//! permission ledger regardless of client — and `session` owns it. What lives
//! here is the client-side supporting evidence, plus one honest gap.
//!
//! # The gap, stated once
//!
//! KON-MVP-18 asks for desktop and phone **screenshots** and an accessibility
//! sweep. `apps/console` has Playwright as a dev-dependency and no config and no
//! spec: there is no browser harness in this tree, and standing one up is not
//! this ticket's file list. So there are no PNGs in this bundle. What there is
//! instead is real: the console's own `shell.test.tsx` drives both widths
//! through `setViewport` and asserts the narrow layout is reachable and
//! dismissible from the keyboard. This section inventories that coverage so an
//! inspector can see exactly how much of the claim is machine-checked and by
//! which suite, rather than inferring it from a missing directory.

use std::fs;
use std::path::{Path, PathBuf};

use kontor_tests_e2e::Bundle;
use serde_json::json;

/// Inventory the console's width and accessibility coverage.
pub(crate) fn run(bundle: &mut Bundle) {
    let root = kontor_tests_e2e::repo_root();
    let console = root.join("apps/console/src");

    let suites = viewport_suites(&console, &root);
    let accessible = accessibility_surface(&console, &root);
    let browser_harness = root.join("apps/console/playwright.config.ts").exists()
        || spec_files(&console).next().is_some();

    let _ = bundle.artifact(
        "ui/viewport-coverage.json",
        &json!({
            "claim": "desktop and phone see one canonical session; the width changes the layout, \
                      never the history, the ids, the cursors or the permission ledger",
            "contract_half": "proved by the `session.history-parity` and `session.live-parity` \
                              cases against the daemon, which is where the invariant actually lives",
            "client_half": {
                "helper": "apps/console/src/test/viewport.ts",
                "mechanism": "jsdom performs no layout, so the console stubs `matchMedia` and the \
                              suite asserts which component tree each width produces",
                "rerun": "pnpm install --frozen-lockfile && pnpm -r test",
                "suites": suites,
            },
            "accessibility": {
                "note": "no axe sweep runs in this tree; what is machine-checked is the role and \
                         keyboard behaviour the console's own suite asserts",
                "annotated_sources": accessible,
            },
            "deviation": {
                "requested": "desktop and phone screenshots plus an accessibility audit",
                "delivered": "a component-tree and keyboard-reach audit from the console's vitest \
                              suite, and this inventory",
                "reason": if browser_harness {
                    "a browser harness exists but this driver does not drive it"
                } else {
                    "apps/console declares Playwright as a dev-dependency but ships no config and \
                     no spec, so no browser harness exists to capture a screenshot with"
                },
                "owner": "unassigned — no KON-MVP ticket establishes browser test infrastructure",
            },
        }),
    );
}

/// Every console test file that drives a viewport, and how many times.
fn viewport_suites(console: &Path, root: &Path) -> Vec<serde_json::Value> {
    let mut suites = Vec::new();
    for file in source_files(console) {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let phone = text.matches("setViewport('phone')").count();
        let desktop = text.matches("setViewport('desktop')").count();
        if phone == 0 && desktop == 0 {
            continue;
        }
        suites.push(json!({
            "file": relative(&file, root),
            "phone_cases": phone,
            "desktop_cases": desktop,
        }));
    }
    suites
}

/// Console sources carrying explicit roles or ARIA annotations.
fn accessibility_surface(console: &Path, root: &Path) -> Vec<serde_json::Value> {
    let mut annotated = Vec::new();
    for file in source_files(console) {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let aria = text.matches("aria-").count();
        let roles = text.matches("role=").count();
        if aria == 0 && roles == 0 {
            continue;
        }
        annotated.push(json!({
            "file": relative(&file, root),
            "aria_attributes": aria,
            "explicit_roles": roles,
        }));
    }
    annotated
}

/// Playwright specs, if any were ever added.
fn spec_files(console: &Path) -> impl Iterator<Item = PathBuf> {
    source_files(console).into_iter().filter(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".spec.ts") || name.ends_with(".spec.tsx"))
    })
}

/// A repository-relative path, for an artifact an inspector has to follow.
fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Every TypeScript source under `directory`, in a stable order.
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
            .is_some_and(|extension| matches!(extension, "ts" | "tsx"));
        if is_source {
            found.push(path);
        }
    }
    found.sort();
    found
}
