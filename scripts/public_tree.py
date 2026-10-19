#!/usr/bin/env python3
"""Refuse owner data in the public Kontor tree (ASMA-8511).

The repository is public. This check refuses three kinds of content in it:

  home_path      an absolute path into a user's home directory, /Users/<name>
                 or /home/<name>, also in its JSON-escaped form and inside gzip
                 data (a .gz name or the gzip magic bytes). Users named in
                 SYNTHETIC_HOME_USERS are allowed.
  run_receipt    a run receipt or deployment record under an evidence root.
                 Receipts stay with the realm that produced them; a document
                 cites them by path and SHA-256 instead.
  account_alias  a provider-account alias that names whose login it is (a
                 claude, codex, cursor or opencode alias ending in -personal
                 or -work) in any file not listed in ALIAS_CODE_EXEMPTION.

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

# The files that still name provider-account aliases, each by its exact path,
# pending a host decision (ASMA-8511, KR-30 gap, review 2026-10-24). The
# deployment registers these aliases as its runtime provider ids, so the
# compiled route catalog and the Teams-editor provider catalog
# (kontor-daemon/src/applications.rs), the bundled team pack and committee
# preset, the generated contract and the Tokenomics store labels name them, and
# so do the doc comments and tests that follow those catalogs. Any other file
# is refused, a new one under crates/ or apps/ included. Every entry must still
# name an alias (the repository test checks it), so the list only shrinks.
ALIAS_CODE_EXEMPTION = frozenset(
    {
        "apps/console/public/tokenomics/index.html",
        "apps/console/src/api/schema.d.ts",
        "apps/console/src/views/ProjectView.test.tsx",
        "crates/kontor-accounts/src/pin.rs",
        "crates/kontor-accounts/src/signals.rs",
        "crates/kontor-accounts/tests/quota_classification.rs",
        "crates/kontor-api/contract/openapi.json",
        "crates/kontor-api/src/applications.rs",
        "crates/kontor-cli/src/commands.rs",
        "crates/kontor-cli/tests/fleet_bundle.rs",
        "crates/kontor-cli/tests/local_resolve.rs",
        "crates/kontor-core/src/tpm_lineage.rs",
        "crates/kontor-core/tests/planning_pair.rs",
        "crates/kontor-daemon/src/applications.rs",
        "crates/kontor-daemon/src/fleet.rs",
        "crates/kontor-daemon/src/orchestration.rs",
        "crates/kontor-daemon/src/provider_config.rs",
        "crates/kontor-daemon/src/quota_observation.rs",
        "crates/kontor-daemon/src/runtimes.rs",
        "crates/kontor-daemon/src/usage.rs",
        "crates/kontor-daemon/tests/account_pinning.rs",
        "crates/kontor-daemon/tests/harness/mod.rs",
        "crates/kontor-daemon/tests/loopback/committee_evidence.rs",
        "crates/kontor-daemon/tests/loopback/desk_adams.rs",
        "crates/kontor-daemon/tests/loopback/planning_pair.rs",
        "crates/kontor-daemon/tests/loopback/provider_refresh.rs",
        "crates/kontor-daemon/tests/loopback_api.rs",
        "crates/kontor-daemon/tests/mcp_journey.rs",
        "crates/kontor-daemon/tests/quota_observation.rs",
        "crates/kontor-fleet-activation/src/tests.rs",
        "crates/kontor-fleet/src/tests.rs",
        "crates/kontor-profiles/fixtures/consultation-presets.json",
        "crates/kontor-profiles/tests/consultation_presets.rs",
        "crates/kontor-runtime-paseo/src/adapter.rs",
        "crates/kontor-runtime-paseo/src/client.rs",
        "crates/kontor-runtime-paseo/src/posture.rs",
        "crates/kontor-runtime-paseo/src/seat_mcp.rs",
        "crates/kontor-runtime-paseo/src/seat_mcp_directory_tests.rs",
        "crates/kontor-runtime-paseo/src/wire.rs",
        "crates/kontor-runtime-paseo/tests/contract.rs",
        "crates/kontor-runtime-paseo/tests/contract/hosted_directory.rs",
        "crates/kontor-runtime/src/planning_pair.rs",
        "crates/kontor-runtime/src/planning_pair/context/tests.rs",
        "crates/kontor-runtime/src/provenance.rs",
        "crates/kontor-store/tests/advisor_multi_seat_advice.rs",
        "crates/kontor-store/tests/event_replay.rs",
        "crates/kontor-store/tests/native_capacity.rs",
        "crates/kontor-store/tests/planning_pair_store.rs",
        "crates/kontor-store/tests/schema_v1.rs",
        "crates/kontor-teams/fixtures/mvp-team-pack.json",
        "crates/kontor-teams/tests/team_contract.rs",
    }
)

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

    aliases_refused = path not in ALIAS_CODE_EXEMPTION
    refusals += _content_refusals(path, "", content, aliases_refused=aliases_refused)
    if path.endswith(".gz") or content[:2] == b"\x1f\x8b":
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
    print(
        f"public tree: {scanned} files, no home path or run receipt, and no "
        f"account alias outside the {len(ALIAS_CODE_EXEMPTION)} files of "
        "ALIAS_CODE_EXEMPTION"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
