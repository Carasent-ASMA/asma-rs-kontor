#!/usr/bin/env python3
"""Inspect affected packages or run an explicit impact-selected verification.

Igor's 2026-10-25 decision forbids automatic full suites. Package/dependent
selection is a planning hint, not caller/contract coverage. Execution requires
--impact and delegates to verify-tree.py; legacy lane names never broaden it.
Existing log parsing and compact receipts are shared with that verifier.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tomllib
from datetime import datetime, timezone
from pathlib import Path

try:  # scripts/ is on sys.path when this file runs as a script
    from kontor_project import ProjectConfig, ProjectConfigError, load_project
except ImportError:  # imported from scripts/tests by path
    import importlib.util

    _PROJECT_SPEC = importlib.util.spec_from_file_location(
        "kontor_project", Path(__file__).resolve().parent / "kontor_project.py"
    )
    assert _PROJECT_SPEC is not None and _PROJECT_SPEC.loader is not None
    _PROJECT_MODULE = importlib.util.module_from_spec(_PROJECT_SPEC)
    _PROJECT_SPEC.loader.exec_module(_PROJECT_MODULE)
    ProjectConfig = _PROJECT_MODULE.ProjectConfig
    ProjectConfigError = _PROJECT_MODULE.ProjectConfigError
    load_project = _PROJECT_MODULE.load_project

REPO_ROOT = Path(__file__).resolve().parent.parent
TIMINGS_PATH = REPO_ROOT / "scripts" / "test-timings.json"
HEAVY_THRESHOLD_SECONDS = 60.0
FALLBACK_HEAVY_PACKAGES = {"kontor-store", "kontor-daemon", "kontor-tests-e2e"}

RESULT_RE = re.compile(
    r"^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; "
    r"(\d+) ignored;.*finished in ([\d.]+)s"
)
RUNNING_EXE_RE = re.compile(r"^\s+(Running|Doc-tests)\s+(.*?)\s+\((\S+)\)\s*$")
RUNNING_RE = re.compile(r"^\s+(Running|Doc-tests)\s+(.*?)\s*$")
FAILED_TEST_RE = re.compile(r"^test (.+) \.\.\. FAILED$")
SECTION_RE = re.compile(r"^---- (.+) stdout ----$")


class LaneError(Exception):
    """Usage or tooling failure (exit 2)."""


def git(args: list[str], check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args], cwd=REPO_ROOT, capture_output=True, text=True, check=check
    )


def base_candidates(base_branch: str) -> tuple[str, str]:
    """The configured base branch, preferring the remote-tracking copy."""

    return (f"origin/{base_branch}", base_branch)


def resolve_base(requested: str | None, base_branch: str) -> str | None:
    candidates = (requested,) if requested else base_candidates(base_branch)
    for candidate in candidates:
        if candidate is None:
            continue
        probe = git(["rev-parse", "--verify", "--quiet", candidate], check=False)
        if probe.returncode == 0:
            return candidate
    if requested:
        raise LaneError(f"base revision not found: {requested}")
    return None


def changed_files(
    base: str | None, overrides: list[str], include_worktree: bool
) -> list[str]:
    files: set[str] = set(overrides)
    if base:
        diff = git(["diff", "--name-only", f"{base}...HEAD"], check=False)
        if diff.returncode == 0:
            files.update(line for line in diff.stdout.splitlines() if line.strip())
    if include_worktree:
        status = git(["status", "--porcelain"]).stdout
        for line in status.splitlines():
            path = line[3:]
            if " -> " in path:
                path = path.split(" -> ")[1]
            files.add(path.strip().strip('"'))
    return sorted(files)


def cargo_metadata() -> dict:
    proc = subprocess.run(
        ["cargo", "metadata", "--locked", "--format-version", "1"],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        raise LaneError("cargo metadata failed:\n" + proc.stderr)
    return json.loads(proc.stdout)


def package_graph(meta: dict) -> tuple[dict[str, Path], dict[str, set[str]]]:
    packages = {p["id"]: p for p in meta["packages"]}
    members = set(meta["workspace_members"])
    roots = {
        packages[m]["name"]: Path(packages[m]["manifest_path"]).parent for m in members
    }
    reverse: dict[str, set[str]] = {name: set() for name in roots}
    for node in meta["resolve"]["nodes"]:
        if node["id"] not in members:
            continue
        for dep in node["deps"]:
            if dep["pkg"] in members:
                reverse[packages[dep["pkg"]]["name"]].add(packages[node["id"]]["name"])
    return roots, reverse


def package_of(path: str, roots: dict[str, Path]) -> str | None:
    absolute = REPO_ROOT / path
    best = None
    for name, root in roots.items():
        if (absolute == root or root in absolute.parents) and (
            best is None or len(root.parts) > len(roots[best].parts)
        ):
            best = name
    return best


def heavy_packages() -> set[str]:
    heavy = set(FALLBACK_HEAVY_PACKAGES)
    if TIMINGS_PATH.exists():
        recorded = json.loads(TIMINGS_PATH.read_text())
        for target in recorded.get("targets", []):
            if (
                target.get("package")
                and target.get("seconds", 0.0) >= HEAVY_THRESHOLD_SECONDS
            ):
                heavy.add(target["package"])
    return heavy


def classify(
    files: list[str], roots: dict[str, Path], reverse: dict[str, set[str]], heavy: set[str]
) -> dict:
    selected: set[str] = set()
    console_tests = False
    verify_api = False
    e2e_required = False
    full_required = False
    for path in files:
        package = package_of(path, roots)
        if package is not None:
            selected.add(package)
            if "/src/" in path or path.endswith("Cargo.toml"):
                stack = list(reverse[package])
                while stack:
                    dependent = stack.pop()
                    if dependent not in selected:
                        selected.add(dependent)
                        stack.extend(reverse[dependent])
        elif path.startswith(("apps/console/src/", "apps/desktop/src-tauri/src/")):
            console_tests = True
            e2e_required = True
        if path.startswith("crates/kontor-api/contract/") or path.startswith(
            "crates/kontor-api/src/"
        ):
            verify_api = True
            console_tests = True
        if path in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml") or path.startswith(
            "scripts/"
        ):
            full_required = True
    light = sorted(selected - heavy)
    slow = sorted(selected & heavy)
    if "kontor-tests-e2e" in selected:
        e2e_required = True
    return {
        "light": light,
        "heavy": slow,
        "console_tests": console_tests,
        "verify_api": verify_api,
        "e2e_required": e2e_required,
        "full_required": full_required,
    }


def run_streaming(
    command: list[str], log_path: Path, stream: bool, cwd: Path | None = None
) -> int:
    log_path.parent.mkdir(parents=True, exist_ok=True)
    with log_path.open("x") as log:
        process = subprocess.Popen(
            command,
            cwd=cwd or REPO_ROOT,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
        )
        assert process.stdout is not None
        for line in process.stdout:
            log.write(line)
            if stream:
                print(line, end="", flush=True)
        return process.wait()


def match_running(line: str) -> tuple[str, str] | None:
    """Return (target label, executable) for a libtest Running/Doc-tests line."""
    with_executable = RUNNING_EXE_RE.match(line)
    if with_executable:
        kind, label, executable = with_executable.groups()
    else:
        plain = RUNNING_RE.match(line)
        if plain is None:
            return None
        kind, label, executable = plain.group(1), plain.group(2), ""
    if kind == "Doc-tests":
        label = f"Doc-tests {label}"
    return label, executable


def parse_log(text: str) -> dict:
    results = []
    failures: dict[str, str] = {}
    current_target: str | None = None
    current_failure: str | None = None
    excerpt: list[str] = []
    for line in text.splitlines():
        running = match_running(line)
        if running:
            current_target = running[0]
            continue
        section = SECTION_RE.match(line)
        if section:
            if current_failure and excerpt:
                failures[current_failure] = "\n".join(excerpt[:25])
            current_failure = section.group(1)
            excerpt = []
            continue
        if current_failure:
            if line.startswith(("failures:", "test result:")) or SECTION_RE.match(line):
                failures[current_failure] = "\n".join(excerpt[:25])
                current_failure = None
            elif line.strip():
                excerpt.append(line)
        failed = FAILED_TEST_RE.match(line)
        if failed and failed.group(1) not in failures:
            failures.setdefault(failed.group(1), "")
        result = RESULT_RE.match(line)
        if result and current_target:
            results.append(
                {
                    "target": current_target,
                    "passed": int(result.group(2)),
                    "failed": int(result.group(3)),
                    "ignored": int(result.group(4)),
                    "seconds": float(result.group(5)),
                }
            )
            current_target = None
    if current_failure and excerpt:
        failures[current_failure] = "\n".join(excerpt[:25])
    return {"results": results, "failures": failures}


def write_receipt(
    lane: str,
    receipts: list[dict],
    exit_code: int,
    streamed: bool,
    requested: str | None,
    project: ProjectConfig | None,
    extra: dict | None = None,
) -> Path:
    payload = {
        "config": project.as_record() if project else None,
        "lane": lane,
        "exit_code": exit_code,
        "streamed": streamed,
        "commands": [receipt["command"] for receipt in receipts],
        "results": [row for receipt in receipts for row in receipt.get("results", [])],
        "failures": {
            name: excerpt
            for receipt in receipts
            for name, excerpt in receipt.get("failures", {}).items()
        },
    }
    payload.update(extra or {})
    if requested:
        path = Path(requested)
    else:
        stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
        path = REPO_ROOT / "target" / "test-lanes" / f"{lane}-{stamp}.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x") as output:
        output.write(json.dumps(payload, indent=2) + "\n")
    return path


def print_summary(receipts: list[dict]) -> None:
    for receipt in receipts:
        rows = receipt.get("results", [])
        if not rows:
            continue
        passed = sum(row["passed"] for row in rows)
        failed = sum(row["failed"] for row in rows)
        total = sum(row["seconds"] for row in rows)
        print(
            f"  {receipt['command'][:70]}: {passed} passed, {failed} failed "
            f"across {len(rows)} targets ({total:.1f}s)"
        )
        for row in rows:
            if row["failed"]:
                print(f"    FAILED {row['target']}: {row['failed']} failing")
    failures = {}
    for receipt in receipts:
        failures.update(receipt.get("failures", {}))
    for name, excerpt in list(failures.items())[:5]:
        print(f"\n--- {name} ---")
        print(excerpt or "(no excerpt)")


def record_timings(log_path: Path) -> None:
    meta = cargo_metadata()
    packages = {p["id"]: p for p in meta["packages"]}
    names = {packages[m]["name"] for m in meta["workspace_members"]}
    # A test-only package (e.g. kontor-tests-contract) keeps its integration
    # test files at the package root, not under `tests/`, so every `.rs` file
    # outside `src/` can name the package a test target belongs to. Unit tests
    # are labeled `unittests src/lib.rs`, whose executable is named after the
    # lib or bin target, not after the package, so declared target names are
    # read from the manifests as well.
    file_to_package: dict[str, str] = {}
    target_to_package: dict[str, str] = {}
    for member in meta["workspace_members"]:
        manifest = Path(packages[member]["manifest_path"])
        package = packages[member]["name"]
        root = manifest.parent
        for source in root.rglob("*.rs"):
            relative = source.relative_to(root)
            if "src" in relative.parts or "target" in relative.parts:
                continue
            file_to_package.setdefault(source.stem, package)
        data = tomllib.loads(manifest.read_text())
        lib_name = data.get("lib", {}).get("name", package.replace("-", "_"))
        target_to_package.setdefault(lib_name, package)
        bins = [entry.get("name") for entry in data.get("bin", [])]
        if not bins and (root / "src" / "main.rs").exists():
            bins = [package.replace("-", "_")]
        for name in bins:
            if name:
                target_to_package.setdefault(name, package)
    targets = []
    current = None
    for line in log_path.read_text().splitlines():
        running = match_running(line)
        if running:
            current = running
            continue
        result = RESULT_RE.match(line)
        if result and current:
            label, executable = current
            package = None
            if label.startswith("Doc-tests "):
                stem = label.split(" ", 1)[1]
                package = target_to_package.get(stem)
                if package is None:
                    dashed = stem.replace("_", "-")
                    package = dashed if dashed in names else None
            elif label.startswith("unittests "):
                stem = Path(executable).name.rsplit("-", 1)[0]
                package = target_to_package.get(stem)
                if package is None:
                    dashed = stem.replace("_", "-")
                    package = dashed if dashed in names else None
            elif label.endswith(".rs"):
                package = file_to_package.get(Path(label).stem)
            targets.append(
                {
                    "package": package,
                    "target": label,
                    "seconds": float(result.group(5)),
                    "passed": int(result.group(2)),
                    "failed": int(result.group(3)),
                }
            )
            current = None
    TIMINGS_PATH.write_text(
        json.dumps(
            {"threshold_seconds": HEAVY_THRESHOLD_SECONDS, "targets": targets},
            indent=2,
        )
        + "\n"
    )
    print(f"recorded {len(targets)} targets into {TIMINGS_PATH.relative_to(REPO_ROOT)}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lane", choices=["fast", "heavy", "full"])
    parser.add_argument("--config-dir", help="explicit config folder")
    parser.add_argument("--config-file", help="explicit generated projection file")
    parser.add_argument(
        "--base",
        help="diff against this revision (default origin/<project.forge.base_branch>)",
    )
    parser.add_argument(
        "--changed", action="append", default=[], help="extra changed path (repeatable)"
    )
    parser.add_argument(
        "--no-worktree",
        action="store_true",
        help="ignore uncommitted files (selector testing and clean-checkout runs)",
    )
    parser.add_argument("--dry-run", action="store_true", help="print the plan only")
    parser.add_argument(
        "--stream",
        action="store_true",
        help="stream the full output (default: fast streams, heavy does not)",
    )
    parser.add_argument("--receipt", help="receipt path (default target/test-lanes/…)")
    parser.add_argument(
        "--record-timings",
        metavar="LOG",
        help="rebuild scripts/test-timings.json from a full cargo test log",
    )
    parser.add_argument("--impact", type=Path, help="reviewed exact-head JSON plan")
    parser.add_argument(
        "--mode", choices=["staged", "archive", "inplace"], default="inplace"
    )
    args = parser.parse_args()

    if args.record_timings:
        record_timings(Path(args.record_timings))
        return 0
    if not args.lane:
        parser.error("--lane is required unless --record-timings is used")

    if args.impact:
        command = [
            sys.executable, str(REPO_ROOT / "scripts/verify-tree.py"),
            "--mode", args.mode, "--impact", str(args.impact.resolve()),
        ]
        if args.receipt:
            command += ["--receipt", str(Path(args.receipt).resolve())]
        if args.dry_run:
            command.append("--dry-run")
        return subprocess.run(command, cwd=REPO_ROOT).returncode
    if not args.dry_run or args.lane == "full":
        raise LaneError("--impact is required; no automatic suite execution")

    try:
        project = load_project(
            config_dir=args.config_dir, config_file=args.config_file
        )
    except ProjectConfigError as exc:
        raise LaneError(str(exc)) from exc

    base = resolve_base(args.base, project.base_branch)
    files = changed_files(base, args.changed, not args.no_worktree)
    roots, reverse = package_graph(cargo_metadata())
    plan = classify(files, roots, reverse, heavy_packages())

    print(f"base: {base or '(none)'}  changed files: {len(files)}")
    for path in files:
        print(f"  - {path}")
    print(f"light packages: {', '.join(plan['light']) or '-'}")
    print(f"heavy packages: {', '.join(plan['heavy']) or '-'}")
    extra = []
    if plan["console_tests"]:
        extra.append("console typecheck+tests")
    if plan["verify_api"]:
        extra.append("console verify:api")
    if extra:
        print(f"extra checks: {', '.join(extra)}")

    print("Planning hints only; inspect direct callers/contracts, including JS "
          "consumers, then supply --impact. No commands were run.")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except LaneError as error:
        print(f"test-lanes: {error}", file=sys.stderr)
        sys.exit(2)
