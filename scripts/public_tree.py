#!/usr/bin/env python3
"""Refuse owner data in the public Kontor tree (ASMA-8511).

The repository is public. This check refuses three kinds of content in it:

  home_path      an absolute path into a user's home directory, /Users/<name>
                 or /home/<name>, also in its JSON-escaped form and inside .gz
                 files. Users named in SYNTHETIC_HOME_USERS are allowed.
  run_receipt    a run receipt or deployment record under an evidence root.
                 Receipts stay with the realm that produced them; a document
                 cites them by path and SHA-256 instead.
  account_alias  a provider-account alias that names whose login it is (a
                 claude, codex, cursor or opencode alias ending in -personal
                 or -work) outside the named code exemption
                 ALIAS_CODE_EXEMPTION.

Licence and attribution files (LICENCE_AND_ATTRIBUTION) are not scanned.

Each refusal names the path, the line and the code, never the matched text.

Run from the repository root, or let `scripts/verify-tree.py` run it as its
first gate:

    python3 scripts/public_tree.py [tree]

In a git checkout the check reads every tracked or untracked file that git
does not ignore. In an exported tree (no .git) it reads every file except
build output.

Exit code 0 only when nothing is refused.
"""

from __future__ import annotations

import gzip
import os
import re
import subprocess
import sys
import zlib
from dataclasses import dataclass
from pathlib import Path

CODE_HOME_PATH = "home_path"
CODE_RUN_RECEIPT = "run_receipt"
CODE_ACCOUNT_ALIAS = "account_alias"

# Root-level licence notices and attribution records. Their copyright and
# provenance lines are not changed by this check.
LICENCE_AND_ATTRIBUTION = frozenset(
    {
        "LICENSE-APACHE",
        "LICENSE-MIT",
        "NOTICE",
        "PROVENANCE.md",
        "THIRD_PARTY_NOTICES.md",
    }
)

# Home directories of made-up users in synthetic fixtures and examples.
SYNTHETIC_HOME_USERS = frozenset({"me", "operator", "someone", "test"})

# Code paths that may still name provider-account aliases. The compiled route
# catalog, the Teams-editor provider catalog, the bundled team pack and the
# committee preset name them, and so do their tests, seeds and the generated
# contract. Making those catalogs config-driven and dropping this exemption is
# an open KR-30 gap (ASMA-8511, review 2026-10-24). Evidence, documentation,
# plans and configuration examples get no exemption.
ALIAS_CODE_EXEMPTION = ("apps/", "crates/")

EVIDENCE_ROOTS = ("docs/evidence/", "_docs/reports/evidence/", "evidence/")

# Directories that hold build output rather than source, skipped when the tree
# is not a git checkout. They mirror .gitignore.
BUILD_OUTPUT_DIRS = frozenset({".git", ".vite", "coverage", "dist", "node_modules"})
TOP_LEVEL_BUILD_OUTPUT_DIRS = frozenset({"target"})

_HOME_PATH_RE = re.compile(
    rb"(?<![\w.-])(?:\\/|/)(?:Users|home)(?:\\/|/)(?P<user>[A-Za-z0-9._-]+)"
)
_ACCOUNT_ALIAS_RE = re.compile(rb"\b(?:claude|codex|cursor|opencode)-(?:personal|work)\b")
_RUN_RECEIPT_PATH_RES = (
    re.compile(r"/receipts/"),
    re.compile(r"/KON-MVP-18/run-[0-9a-f]{16}/"),
    re.compile(r"/deployed-[0-9a-f]{7,40}/"),
    re.compile(r"/[^/]*receipt[^/]*\.json$"),
)


@dataclass(frozen=True)
class Refusal:
    """One refused location: the path, the 1-based line (0 for the path itself)."""

    path: str
    line: int
    code: str
    detail: str

    def render(self) -> str:
        return f"{self.path}:{self.line}: {self.code}: {self.detail}"


def tree_files(root: Path) -> list[str]:
    """List the tree's regular files as sorted POSIX paths relative to root."""

    if (root / ".git").exists():
        listed = subprocess.run(
            ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
            cwd=root,
            capture_output=True,
            check=True,
        ).stdout.decode()
        paths = [path for path in listed.split("\0") if path]
        return sorted(
            {
                path
                for path in paths
                if (root / path).is_file() and not (root / path).is_symlink()
            }
        )

    found = []
    for directory, subdirectories, files in os.walk(root):
        relative = Path(directory).relative_to(root)
        subdirectories[:] = [
            name
            for name in subdirectories
            if name not in BUILD_OUTPUT_DIRS
            and not (relative == Path(".") and name in TOP_LEVEL_BUILD_OUTPUT_DIRS)
        ]
        for name in files:
            path = Path(directory) / name
            if path.is_file() and not path.is_symlink():
                found.append(path.relative_to(root).as_posix())
    return sorted(found)


def _line_of(content: bytes, offset: int) -> int:
    return content.count(b"\n", 0, offset) + 1


def _content_refusals(
    path: str, label: str, content: bytes, *, aliases_refused: bool
) -> list[Refusal]:
    refusals = []
    for match in _HOME_PATH_RE.finditer(content):
        if match.group("user").decode() not in SYNTHETIC_HOME_USERS:
            refusals.append(
                Refusal(
                    path,
                    _line_of(content, match.start()),
                    CODE_HOME_PATH,
                    f"a home directory path{label}; write $HOME or a synthetic "
                    "user (" + ", ".join(sorted(SYNTHETIC_HOME_USERS)) + ")",
                )
            )
    if aliases_refused:
        for match in _ACCOUNT_ALIAS_RE.finditer(content):
            refusals.append(
                Refusal(
                    path,
                    _line_of(content, match.start()),
                    CODE_ACCOUNT_ALIAS,
                    f"a provider-account alias{label}; use a neutral alias "
                    "such as claude-a or codex-b",
                )
            )
    return refusals


def file_refusals(path: str, content: bytes) -> list[Refusal]:
    """Decide one file, given its path relative to the tree root and its bytes."""

    if path in LICENCE_AND_ATTRIBUTION:
        return []

    refusals = []
    if path.startswith(EVIDENCE_ROOTS) and any(
        pattern.search("/" + path) for pattern in _RUN_RECEIPT_PATH_RES
    ):
        refusals.append(
            Refusal(
                path,
                0,
                CODE_RUN_RECEIPT,
                "a run receipt or deployment record; cite it by path and "
                "SHA-256 instead",
            )
        )

    aliases_refused = not path.startswith(ALIAS_CODE_EXEMPTION)
    refusals += _content_refusals(path, "", content, aliases_refused=aliases_refused)
    if path.endswith(".gz"):
        try:
            inflated = gzip.decompress(content)
        except (OSError, EOFError, zlib.error):
            inflated = None
        if inflated is not None:
            refusals += _content_refusals(
                path, " (inside the gzip data)", inflated, aliases_refused=aliases_refused
            )
    return refusals


def tree_refusals(root: Path) -> tuple[int, list[Refusal]]:
    """Scan the tree; return the number of files read and every refusal."""

    paths = tree_files(root)
    refusals = []
    for path in paths:
        refusals += file_refusals(path, (root / path).read_bytes())
    return len(paths), refusals


def main(argv: list[str] | None = None) -> int:
    args = sys.argv[1:] if argv is None else argv
    if len(args) > 1:
        print("usage: public_tree.py [tree]", file=sys.stderr)
        return 2
    root = Path(args[0] if args else ".").resolve()
    scanned, refusals = tree_refusals(root)
    for refusal in refusals:
        print(refusal.render())
    if refusals:
        print(
            f"public tree: {len(refusals)} refusal(s) in {scanned} files",
            file=sys.stderr,
        )
        return 1
    print(f"public tree: {scanned} files, no home path, run receipt or account alias")
    return 0


if __name__ == "__main__":
    sys.exit(main())
