#!/usr/bin/env python3
"""Run reviewed changed-code checks against an exact staged/committed export.

No default full suite. --impact supplies caller/contract coverage, focused
commands, explicit inapplicability and external review/runtime obligations.
Archive mode is merge evidence; staged/inplace are development evidence only.
Igor decision 2026-10-25: see DEC-002 in the local verification policy.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO_ROOT / "scripts"))
SPEC = importlib.util.spec_from_file_location(
    "test_lanes", REPO_ROOT / "scripts/test-lanes.py"
)
assert SPEC is not None and SPEC.loader is not None
lanes = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(lanes)
LOCAL_CHECKS = {
    "tests",
    "build",
    "typecheck",
    "format",
    "dependencies",
    "security",
}
EXTERNAL_CHECKS = {"qa", "aud", "browser", "installed"}


def run(cmd: list[str], cwd: Path) -> None:
    """Run an export operation, refusing a failed export."""
    subprocess.run(cmd, cwd=cwd, check=True)


def export_staged(dest: Path) -> None:
    """Copy the staged tree without .git or untracked files."""
    run(["git", "checkout-index", "--prefix", f"{dest}/", "-a"], REPO_ROOT)


def export_archive(dest: Path) -> None:
    """Extract the exact committed tree without .git or worktree overlays."""
    archive = subprocess.run(
        ["git", "archive", "HEAD"],
        cwd=REPO_ROOT,
        capture_output=True,
        check=True,
    )
    subprocess.run(
        ["tar", "-x", "-C", str(dest)], input=archive.stdout, check=True
    )


def git(*args: str) -> str:
    """Read candidate identity and changed paths from this checkout."""
    return subprocess.check_output(
        ["git", *args], cwd=REPO_ROOT, text=True
    ).strip()


def relative_path(value: str) -> str:
    """Refuse paths outside the selected export."""
    if not isinstance(value, str) or not value or Path(value).is_absolute():
        raise ValueError("expected a nonempty repository-relative path")
    if ".." in Path(value).parts:
        raise ValueError("parent traversal is not allowed")
    return value


def validate_test_command(argv: list[str]) -> None:
    """Refuse the previous broad recipes; owners still review test relevance."""
    if "cargo" in argv[0] and "test" in argv:
        if "--workspace" in argv or not ({"-p", "--package"} & set(argv)):
            raise ValueError(
                "Cargo tests need a package and focused target/filter"
            )
        if not ({"--test", "--exact"} & set(argv)):
            raise ValueError("Cargo tests need a focused target/filter")
    if Path(argv[0]).name in {"pnpm", "vitest", "npm"}:
        if {"-r", "--recursive", "--all"} & set(argv):
            raise ValueError("recursive test suites are not impact selection")
        if not any("test" in x and ("/" in x or "." in x) for x in argv[1:]):
            raise ValueError("JS tests need explicit focused files")
    if "discover" in argv:
        if (
            "-p" not in argv
            or argv[-1] == "-p"
            or "*" in argv[argv.index("-p") + 1]
        ):
            raise ValueError(
                "unittest discovery needs one focused file pattern"
            )


def read_plan(path: Path, mode: str) -> tuple[dict, list[str]]:
    """Bind reviewed impact coverage to the actual head, tree and base diff."""
    plan = json.loads(path.read_text())
    head = git("rev-parse", "HEAD")
    tree = (
        git("write-tree")
        if mode == "staged"
        else git("rev-parse", "HEAD^{tree}")
    )
    if plan["head"] != head or plan["tree"] != tree:
        raise ValueError(
            "impact plan does not match the exact candidate head/tree"
        )
    base = git("rev-parse", plan["base"] + "^{commit}")
    if base != plan["base"]:
        raise ValueError("base must be an exact commit ID")
    changed = git(
        "diff", "--name-only", "--no-renames", base, tree
    ).splitlines()
    impacts = plan["impact"]
    for row in impacts:
        relative_path(row["path"])
        if not row["reason"].strip() or not isinstance(row["tests"], list):
            raise ValueError(
                "each impact needs a reason and explicit test list"
            )
    if not set(changed) <= {row["path"] for row in impacts}:
        raise ValueError("impact list omits changed/deleted paths")
    if not plan["impact_analysis"].strip():
        raise ValueError(
            "explain caller/contract analysis, not just filenames"
        )
    checks = plan["checks"]
    for check in checks:
        argv = check["argv"]
        if (
            not isinstance(argv, list)
            or not argv
            or not all(isinstance(x, str) and x for x in argv)
        ):
            raise ValueError("commands must be nonempty argv arrays; no shell")
        relative_path(check["cwd"])
        if check["kind"] not in LOCAL_CHECKS or not check["reason"].strip():
            raise ValueError(
                "unknown check kind or missing applicability reason"
            )
        if check["kind"] == "tests":
            if not check.get("tests"):
                raise ValueError("test commands need explicit selected tests")
            validate_test_command(argv)
    omissions = plan["not_applicable"]
    for kind in LOCAL_CHECKS - {check["kind"] for check in checks}:
        if not omissions.get(kind, "").strip():
            raise ValueError(
                f"missing check or exact inapplicability reason: {kind}"
            )
    for kind in EXTERNAL_CHECKS:
        if not plan["external"].get(kind, "").strip():
            raise ValueError(
                f"record the separate {kind} obligation/disposition"
            )
    if mode == "inplace" and git("diff", "HEAD", "--name-only"):
        raise ValueError("inplace has tracked edits; use staged or archive")
    for ref in plan.get("prior_receipts", []):
        if (
            hashlib.sha256(Path(ref["path"]).read_bytes()).hexdigest()
            != ref["sha256"]
        ):
            raise ValueError("prior receipt hash mismatch")
        if not ref["disposition"].strip():
            raise ValueError(
                "prior receipts need an unchanged result/disposition"
            )
    plan["plan_sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
    plan["changed_paths"] = changed
    return plan, changed


def export_fingerprint(tree: Path, expected_tree: str) -> dict[str, str]:
    """Verify tracked blobs/modes and detect commands changing the export."""
    entries = subprocess.check_output(
        ["git", "ls-tree", "-rz", expected_tree], cwd=REPO_ROOT
    )
    result = {}
    for entry in entries.split(b"\0"):
        if not entry:
            continue
        metadata, name = entry.split(b"\t", 1)
        mode, kind, expected = metadata.decode().split()
        if kind != "blob":
            raise ValueError("unsupported non-blob export entry")
        path = tree / name.decode()
        if mode == "120000":
            raw = str(path.readlink()).encode()
            actual_mode = "120000"
        else:
            if path.is_symlink():
                raise ValueError(f"unexpected export symlink: {name.decode()}")
            raw = path.read_bytes()
            actual_mode = "100755" if path.stat().st_mode & 0o111 else "100644"
        header = b"blob " + str(len(raw)).encode() + b"\0"
        actual = hashlib.sha1(header + raw).hexdigest()
        if actual != expected or actual_mode != mode:
            raise ValueError(f"export blob/mode changed: {name.decode()}")
        result[name.decode()] = actual
    return result


def run_gates(tree: Path, plan: dict, receipt: Path, mode: str) -> int:
    """Run only explicit local checks, retaining failed and unrun outcomes."""
    source_before = export_fingerprint(tree, plan["tree"])
    commands = [
        {
            "kind": "public_tree",
            "cwd": ".",
            "reason": "public export boundary",
            "argv": [sys.executable, "-B", "scripts/public_tree.py", "."],
        }
    ] + plan["checks"]
    for check in commands:
        cwd = (tree / check["cwd"]).resolve()
        if not cwd.is_relative_to(tree.resolve()) or not cwd.is_dir():
            raise ValueError("check cwd is outside/missing from export")
    results = []
    code = 0
    for index, check in enumerate(commands):
        cwd = (tree / check["cwd"]).resolve()
        log = receipt.with_name(receipt.name + f".{index}.log")
        print("$ " + subprocess.list2cmdline(check["argv"]), flush=True)
        try:
            status = lanes.run_streaming(check["argv"], log, True, cwd)
        except FileExistsError:
            raise  # Never append to an existing log, including a raced writer.
        except OSError as exc:
            # The runner already opened its fresh log before spawning.
            with log.open("a") as output:
                output.write(f"command could not start: {exc}\n")
            status = 127
        parsed = lanes.parse_log(log.read_text())
        results.append(
            {
                **parsed,
                **check,
                "command": check["argv"],
                "exit_code": status,
                "log": str(log),
                "log_sha256": hashlib.sha256(log.read_bytes()).hexdigest(),
            }
        )
        if status:
            code = 1
            break
    executed_count = len(results)
    source_error = None
    try:
        export_fingerprint(tree, plan["tree"])
    except (ValueError, OSError) as exc:
        source_error = str(exc)
        code = 1
    for ref in plan.get("prior_receipts", []):
        try:
            current = hashlib.sha256(
                Path(ref["path"]).read_bytes()
            ).hexdigest()
            failure = None if current == ref["sha256"] else "evidence changed"
        except OSError as exc:
            failure = f"evidence could not be read: {exc}"
        if failure is not None:
            code = 1
            results.append(
                {
                    "command": ["prior-receipt-readback"],
                    "exit_code": 1,
                    "results": [],
                    "failures": {"prior_receipt": failure},
                }
            )
    # Do not reinterpret old FAILs or grant QA/AUD/runtime acceptance here.
    lanes.write_receipt(
        "impact",
        results,
        code,
        True,
        str(receipt),
        None,
        {
            "mode": mode,
            "head": plan["head"],
            "tree": plan["tree"],
            "base": plan["base"],
            "changed_paths": plan["changed_paths"],
            "impact_plan_sha256": plan["plan_sha256"],
            "impact": plan["impact"],
            "impact_analysis": plan["impact_analysis"],
            "not_applicable": plan["not_applicable"],
            "external": plan["external"],
            "prior_receipts": plan.get("prior_receipts", []),
            "checks": results,
            "not_run": commands[executed_count:],
            "committed_export": mode == "archive",
            "verified_export_blobs": len(source_before),
            "source_error": source_error,
            "scope": "Selected local checks only; external acceptance is separate",
        },
    )
    return code


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--mode", choices=["staged", "archive", "inplace"], required=True
    )
    parser.add_argument("--impact", type=Path, required=True)
    parser.add_argument("--receipt", type=Path)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    plan, changed = read_plan(args.impact, args.mode)
    if args.dry_run:
        print(json.dumps({"changed_paths": changed, "plan": plan}, indent=2))
        return 0
    stamp = lanes.datetime.now(lanes.timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    receipt = (
        args.receipt
        or REPO_ROOT / "target/test-lanes" / f"impact-{stamp}.json"
    ).resolve()
    receipt.parent.mkdir(parents=True, exist_ok=True)
    if (
        receipt.exists()
        or receipt.is_symlink()
        or list(receipt.parent.glob(receipt.name + ".*.log"))
    ):
        raise ValueError(
            "receipt/log already exists; never overwrite prior evidence"
        )
    if args.mode == "inplace":
        return run_gates(REPO_ROOT, plan, receipt, args.mode)
    with tempfile.TemporaryDirectory(prefix="kontor-verify-") as tmp:
        tree = Path(tmp) / "tree"
        tree.mkdir()
        if args.mode == "staged":
            export_staged(tree)
        else:
            export_archive(tree)
        return run_gates(tree, plan, receipt, args.mode)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (
        ValueError,
        KeyError,
        OSError,
        subprocess.CalledProcessError,
    ) as exc:
        print(f"verify-tree: {exc}", file=sys.stderr)
        sys.exit(2)
