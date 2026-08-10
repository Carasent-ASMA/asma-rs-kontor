#!/usr/bin/env python3
"""Verify the committed Kontor tree without relying on a .git directory.

Modes (KON-MVP-02 verification requirement):
  --mode staged   export the current git STAGED tree via `git checkout-index`
                  into a temp dir (no .git inside) and run every gate there.
                  Use before any authorized commit exists.
  --mode archive  export `git archive HEAD` into a temp dir, byte-compare the
                  regenerated Cargo.lock against the committed one, and run
                  every gate there. Use after an authorized commit.
  --mode inplace  no git export at all: run gates against the current tree
                  (the tree was already extracted from git archive / checkout).

Every mode runs the full gate list: cargo fmt --check, clippy -D warnings,
workspace tests, cargo audit, cargo deny check, frozen pnpm install,
typecheck, vitest run and the production dependency audit.

Exit code 0 only when every gate passes.
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent


def run(cmd: list[str], cwd: Path) -> None:
    """Run one gate; raise on failure."""
    print(f"$ {subprocess.list2cmdline(cmd)}")
    result = subprocess.run(cmd, cwd=cwd)
    if result.returncode != 0:
        raise SystemExit(f"gate failed (exit {result.returncode}): {' '.join(cmd)}")


def export_staged(dest: Path) -> None:
    """Copy the staged tree to dest using git checkout-index (no .git)."""
    run(["git", "checkout-index", "--prefix", f"{dest}/", "-a"], REPO_ROOT)
    print(f"staged tree exported to {dest}")


def export_archive(dest: Path) -> None:
    """Extract git archive HEAD to dest (no .git inside)."""
    archive = subprocess.run(
        ["git", "archive", "HEAD"], cwd=REPO_ROOT, capture_output=True, check=True
    )
    tar = subprocess.run(
        ["tar", "-x", "-C", str(dest)], input=archive.stdout, check=True
    )
    assert tar.returncode == 0
    print(f"archive HEAD extracted to {dest}")


def verify_lockfile_reproducible(tree: Path) -> None:
    """Regenerate Cargo.lock in the exported tree and byte-compare it with the
    committed one (which is inside the exported tree for archive mode)."""
    committed = tree / "Cargo.lock"
    if not committed.exists():
        raise SystemExit("Cargo.lock missing from the exported tree")
    original = committed.read_bytes()
    run(["cargo", "generate-lockfile"], tree)
    regenerated = committed.read_bytes()
    if regenerated != original:
        raise SystemExit(
            "Cargo.lock regeneration differs byte-for-byte from the committed lockfile"
        )
    print("Cargo.lock byte-compare: identical")


def run_gates(tree: Path) -> None:
    """Run every gate inside the exported tree."""
    run(["cargo", "fmt", "--all", "--", "--check"], tree)
    run(
        ["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"],
        tree,
    )
    run(["cargo", "test", "--workspace", "--locked"], tree)
    run(["cargo", "audit"], tree)
    run(["cargo", "deny", "check"], tree)

    pnpm = shutil.which("pnpm")
    if pnpm is None:
        raise SystemExit(
            "pnpm not found on PATH: the console gates (frozen install, "
            "typecheck, vitest, production audit) are mandatory in every mode "
            "and cannot be skipped"
        )
    run([pnpm, "install", "--frozen-lockfile"], tree)
    run([pnpm, "-r", "typecheck"], tree)
    run([pnpm, "-r", "test"], tree)
    run([pnpm, "audit", "--prod"], tree)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--mode",
        choices=["staged", "archive", "inplace"],
        required=True,
        help="which tree to verify (see module docstring)",
    )
    args = parser.parse_args()

    if args.mode == "inplace":
        run_gates(REPO_ROOT)
        return 0

    with tempfile.TemporaryDirectory(prefix="kontor-verify-") as tmp:
        tree = Path(tmp) / "tree"
        tree.mkdir()
        if args.mode == "staged":
            export_staged(tree)
        else:
            export_archive(tree)
        verify_lockfile_reproducible(tree)
        run_gates(tree)
    return 0


if __name__ == "__main__":
    sys.exit(main())
