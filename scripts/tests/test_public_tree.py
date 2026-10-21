"""Tests for the public-tree refusal (ASMA-8511).

Run from the repository root:

    python3 -m unittest discover -s scripts/tests -v

The refused values are assembled from parts so that this file passes the check
it tests.
"""

from __future__ import annotations

import contextlib
import gzip
import importlib.util
import io
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = REPO_ROOT / "scripts"

sys.path.insert(0, str(SCRIPTS_DIR))
import public_tree  # noqa: E402

# A real-looking user that is not synthetic, and the two refused aliases.
OWNER = "jdoe"
MAC_HOME = "/" + "Users/" + OWNER
LINUX_HOME = "/" + "home/" + OWNER
WORK_ALIAS = "claude" + "-work"
PERSONAL_ALIAS = "codex" + "-personal"


def codes(path: str, text: str | bytes) -> list[str]:
    content = text.encode() if isinstance(text, str) else text
    return [refusal.code for refusal in public_tree.file_refusals(path, content)]


class HomePathTests(unittest.TestCase):
    def test_refuses_a_macos_home_path(self) -> None:
        self.assertEqual(
            codes("docs/NOTES.md", f"see {MAC_HOME}/state/realm\n"),
            [public_tree.CODE_HOME_PATH],
        )

    def test_refuses_a_linux_home_path(self) -> None:
        self.assertEqual(
            codes("docs/NOTES.md", f"cwd={LINUX_HOME}/work\n"),
            [public_tree.CODE_HOME_PATH],
        )

    def test_refuses_a_bare_home_directory(self) -> None:
        self.assertEqual(
            codes("docs/NOTES.md", f'"{MAC_HOME}"'), [public_tree.CODE_HOME_PATH]
        )

    def test_refuses_a_json_escaped_home_path(self) -> None:
        escaped = MAC_HOME.replace("/", "\\/")
        self.assertEqual(
            codes("docs/evidence/X/run.json", f'{{"cwd": "{escaped}\\/repo"}}'),
            [public_tree.CODE_HOME_PATH],
        )

    def test_refuses_a_home_path_inside_gzip_data(self) -> None:
        packed = gzip.compress(f"line one\n{LINUX_HOME}/.config\n".encode())
        refusals = public_tree.file_refusals("docs/evidence/X/log.txt.gz", packed)
        self.assertEqual([r.code for r in refusals], [public_tree.CODE_HOME_PATH])
        self.assertEqual(refusals[0].line, 2)
        self.assertIn("inside the gzip data", refusals[0].detail)

    def test_reads_gzip_data_by_its_magic_bytes(self) -> None:
        packed = gzip.compress(f"{MAC_HOME}/.kontor\n".encode())
        for path in ("docs/evidence/X/bundle.tgz", "docs/evidence/X/blob"):
            with self.subTest(path=path):
                refusals = public_tree.file_refusals(path, packed)
                self.assertEqual([r.code for r in refusals], [public_tree.CODE_HOME_PATH])
                self.assertIn("inside the gzip data", refusals[0].detail)

    def test_refuses_a_home_path_in_code_paths(self) -> None:
        # Also in a file the alias exemption lists: it exempts aliases only.
        for path in (
            "crates/kontor-core/src/lib.rs",
            "crates/kontor-daemon/src/applications.rs",
        ):
            with self.subTest(path=path):
                self.assertEqual(
                    codes(path, f'const P: &str = "{MAC_HOME}";'),
                    [public_tree.CODE_HOME_PATH],
                )

    def test_allows_synthetic_home_users(self) -> None:
        # Named here rather than read from the set, so dropping one fails.
        for user in ("me", "operator", "someone", "test"):
            with self.subTest(user=user):
                text = f"/Users/{user}/repo /home/{user}/.kontor"
                self.assertEqual(codes("crates/x/tests/fixture.json", text), [])

    def test_a_synthetic_user_is_matched_whole(self) -> None:
        self.assertEqual(
            codes("docs/NOTES.md", "/" + "Users/operators/repo"),
            [public_tree.CODE_HOME_PATH],
        )

    def test_a_path_segment_named_home_is_not_a_home_directory(self) -> None:
        text = "target/home/temp src/Users/list cognee/users/methods $HOME/x ~/x"
        self.assertEqual(codes("docs/NOTES.md", text), [])

    def test_names_the_location_not_the_value(self) -> None:
        refusals = public_tree.file_refusals(
            "docs/NOTES.md", f"one\ntwo {MAC_HOME}/x\n".encode()
        )
        self.assertEqual([(r.path, r.line) for r in refusals], [("docs/NOTES.md", 2)])
        self.assertNotIn(OWNER, refusals[0].render())


class RunReceiptTests(unittest.TestCase):
    def test_refuses_run_receipts_under_every_evidence_root(self) -> None:
        refused = [
            "docs/evidence/KON-MVP-18/run-0123456789abcdef/summary.md",
            "docs/evidence/ASMA-1/receipts/launch.json",
            "_docs/reports/evidence/ASMA-1/deployed-dddb7287/status.txt",
            "_docs/reports/evidence/ASMA-1/audit/publication-receipt.json",
            "evidence/ASMA-1/receipt.json",
        ]
        for path in refused:
            with self.subTest(path=path):
                refusals = public_tree.file_refusals(path, b"{}")
                self.assertEqual(
                    [(r.code, r.line) for r in refusals],
                    [(public_tree.CODE_RUN_RECEIPT, 0)],
                )

    def test_receipt_code_and_fixtures_outside_evidence_are_not_receipts(self) -> None:
        allowed = [
            "crates/kontor-core/src/receipt.rs",
            "crates/kontor-store/tests/fixtures/receipt.json",
            "docs/RECEIPTS.md",
            "docs/evidence/ASMA-1/receipt-schema.md",
            "docs/evidence/KON-MVP-18/README.md",
            "docs/evidence/removed-run-evidence-2026-10-17.tsv",
        ]
        for path in allowed:
            with self.subTest(path=path):
                self.assertEqual(codes(path, "{}"), [])


class AccountAliasTests(unittest.TestCase):
    def test_refuses_aliases_in_evidence_docs_plans_and_config(self) -> None:
        refused = [
            "docs/evidence/ASMA-1/report.md",
            "docs/CONFIGURATION.md",
            "_docs/plans/plan.md",
            "config/examples/orchestration/fleet.yml",
            "README.md",
            "tests/contract/fixture.json",
        ]
        for path in refused:
            for alias in (WORK_ALIAS, PERSONAL_ALIAS):
                with self.subTest(path=path, alias=alias):
                    self.assertEqual(
                        codes(path, f"accounts: [{alias}]\n"),
                        [public_tree.CODE_ACCOUNT_ALIAS],
                    )

    def test_refuses_aliases_inside_gzip_data(self) -> None:
        packed = gzip.compress(f"provider={WORK_ALIAS}\n".encode())
        self.assertEqual(
            codes("docs/evidence/X/log.gz", packed), [public_tree.CODE_ACCOUNT_ALIAS]
        )

    def test_the_code_exemption_lists_exact_files(self) -> None:
        exempt = [
            "crates/kontor-daemon/src/applications.rs",
            "crates/kontor-teams/fixtures/mvp-team-pack.json",
            "crates/kontor-api/contract/openapi.json",
            "apps/console/public/tokenomics/index.html",
        ]
        for path in exempt:
            with self.subTest(path=path):
                self.assertEqual(codes(path, f'"{WORK_ALIAS}"'), [])
        not_exempt = [
            "crates/kontor-daemon/src/new_module.rs",
            "crates/kontor-daemon/src/applications.rs.orig",
            "crates/kontor-daemon/src",
            "apps/console/src/state/teams.ts",
            "docs/crates/kontor-daemon/src/applications.rs",
        ]
        for path in not_exempt:
            with self.subTest(path=path):
                self.assertEqual(
                    codes(path, f'"{WORK_ALIAS}"'), [public_tree.CODE_ACCOUNT_ALIAS]
                )

    def test_neutral_aliases_and_signal_ids_are_not_refused(self) -> None:
        text = (
            "claude-a claude-b codex-a codex-b opencode cursor "
            "codex-usage-limit-b codex-workspace claude-working"
        )
        self.assertEqual(codes("config/examples/quota-signals.yml", text), [])


class LicenceAndAttributionTests(unittest.TestCase):
    def test_root_licence_and_attribution_files_are_not_scanned(self) -> None:
        text = f"{MAC_HOME}/upstream {WORK_ALIAS}"
        for path in sorted(public_tree.LICENCE_AND_ATTRIBUTION):
            with self.subTest(path=path):
                self.assertEqual(codes(path, text), [])

    def test_the_exception_is_by_exact_root_path(self) -> None:
        text = f"{MAC_HOME}/upstream"
        for path in ("docs/NOTICE", "docs/evidence/X/PROVENANCE.md", "NOTICE.md"):
            with self.subTest(path=path):
                self.assertEqual(codes(path, text), [public_tree.CODE_HOME_PATH])


class TreeTests(unittest.TestCase):
    def test_a_git_checkout_reads_unignored_files_only(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            git = ["git", "-c", "user.name=t", "-c", "user.email=t@example.invalid"]
            subprocess.run([*git, "init", "-q"], cwd=root, check=True)
            (root / ".gitignore").write_text("/ignored/\n")
            (root / "tracked.md").write_text(f"{WORK_ALIAS}\n")
            subprocess.run([*git, "add", "."], cwd=root, check=True)
            (root / "untracked.md").write_text(f"{LINUX_HOME}\n")
            (root / "ignored").mkdir()
            (root / "ignored" / "run.md").write_text(f"{MAC_HOME}\n")
            scanned, refusals = public_tree.tree_refusals(root)
        self.assertEqual(scanned, 3)
        self.assertEqual(
            sorted((r.path, r.code) for r in refusals),
            [
                ("tracked.md", public_tree.CODE_ACCOUNT_ALIAS),
                ("untracked.md", public_tree.CODE_HOME_PATH),
            ],
        )

    def test_an_exported_tree_skips_build_output_only(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for skipped in ("target/debug/x.d", "apps/console/node_modules/p/x.js"):
                (root / skipped).parent.mkdir(parents=True)
                (root / skipped).write_text(f"{MAC_HOME}\n")
            (root / "crates" / "target").mkdir(parents=True)
            (root / "crates" / "target" / "a.rs").write_text(f"{MAC_HOME}\n")
            self.assertEqual(public_tree.tree_files(root), ["crates/target/a.rs"])

    def test_the_cli_refuses_with_exit_code_one(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "docs").mkdir()
            (root / "docs" / "a.md").write_text(f"x\n{PERSONAL_ALIAS}\n")
            out, err = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                exit_code = public_tree.main([str(root)])
        self.assertEqual(exit_code, 1)
        self.assertEqual(
            out.getvalue().splitlines()[0].split(": ")[:2],
            ["docs/a.md:2", public_tree.CODE_ACCOUNT_ALIAS],
        )

    def test_every_exempt_file_still_names_an_alias(self) -> None:
        for path in sorted(public_tree.ALIAS_CODE_EXEMPTION):
            with self.subTest(path=path):
                content = (REPO_ROOT / path).read_bytes()
                self.assertIsNotNone(public_tree._ACCOUNT_ALIAS_RE.search(content))

    def test_the_repository_tree_passes(self) -> None:
        scanned, refusals = public_tree.tree_refusals(REPO_ROOT)
        self.assertGreater(scanned, 1000)
        self.assertEqual([r.render() for r in refusals], [])


class VerifyTreeTests(unittest.TestCase):
    def test_verify_tree_runs_the_check_and_its_tests_first(self) -> None:
        spec = importlib.util.spec_from_file_location(
            "verify_tree", SCRIPTS_DIR / "verify-tree.py"
        )
        assert spec is not None and spec.loader is not None
        verify_tree = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(verify_tree)
        commands: list[list[str]] = []

        def first_two_gates(cmd: list[str], cwd: Path) -> None:
            commands.append(cmd)
            if len(commands) == 2:
                raise SystemExit("stop after the second gate")

        verify_tree.run = first_two_gates
        with self.assertRaises(SystemExit):
            verify_tree.run_gates(Path("/nonexistent-tree"))
        self.assertEqual(
            commands,
            [
                [sys.executable, "scripts/public_tree.py", "."],
                [sys.executable, "-B", "-m", "unittest", "discover", "-s", "scripts/tests"],
            ],
        )


if __name__ == "__main__":
    unittest.main()
