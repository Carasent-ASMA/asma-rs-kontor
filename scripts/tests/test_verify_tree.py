"""Focused regression tests for changed-code selection and export evidence."""

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

SCRIPT = Path(__file__).resolve().parents[1] / "verify-tree.py"
SPEC = importlib.util.spec_from_file_location("verify_tree", SCRIPT)
verifier = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(verifier)


class ImpactTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name) / "repo"
        self.root.mkdir()
        self.patch = mock.patch.object(verifier, "REPO_ROOT", self.root)
        self.patch.start()
        self.addCleanup(self.patch.stop)
        self.git("init", "-q")
        self.git("config", "user.name", "Synthetic verifier test")
        self.git("config", "user.email", "test@example.invalid")
        (self.root / "source.py").write_text("before\n")
        self.commit()
        self.base = self.git("rev-parse", "HEAD")
        (self.root / "source.py").write_text("after\n")
        (self.root / "caller.py").write_text("direct contract consumer\n")
        self.commit()
        self.plan = {
            "base": self.base,
            "head": self.git("rev-parse", "HEAD"),
            "tree": self.git("rev-parse", "HEAD^{tree}"),
            "impact_analysis": "Changed behavior plus its direct contract caller",
            "impact": [
                {
                    "path": p,
                    "reason": "changed/called behavior",
                    "tests": ["focused_case"],
                }
                for p in ["source.py", "caller.py"]
            ],
            "checks": [
                {
                    "kind": "tests",
                    "cwd": ".",
                    "argv": [sys.executable, "-c", "print('focused')"],
                    "tests": ["focused_case"],
                    "reason": "changed behavior",
                }
            ],
            "not_applicable": {
                k: "No affected artifact in this synthetic diff"
                for k in verifier.LOCAL_CHECKS - {"tests"}
            },
            "external": {
                k: "Separate owner disposition; not granted by runner"
                for k in verifier.EXTERNAL_CHECKS
            },
        }
        self.impact = Path(self.tmp.name) / "impact.json"

    def git(self, *args: str) -> str:
        return subprocess.check_output(
            ["git", *args], cwd=self.root, text=True, stderr=subprocess.PIPE
        ).strip()

    def commit(self) -> None:
        self.git("add", ".")
        self.git("commit", "-qm", "Synthetic fixture")

    def read_plan(self, mode="archive"):
        self.impact.write_text(json.dumps(self.plan))
        return verifier.read_plan(self.impact, mode)

    def test_exact_head_tree_diff_and_caller_coverage(self) -> None:
        plan, changed = self.read_plan()
        self.assertEqual(changed, ["caller.py", "source.py"])
        self.assertEqual(plan["head"], self.git("rev-parse", "HEAD"))

    def test_wrong_head_or_tree_refused(self) -> None:
        for field in ["head", "tree"]:
            old = self.plan[field]
            self.plan[field] = "0" * 40
            with self.assertRaisesRegex(ValueError, "exact candidate"):
                self.read_plan()
            self.plan[field] = old

    def test_omitted_changed_path_refused(self) -> None:
        self.plan["impact"].pop()
        with self.assertRaisesRegex(ValueError, "omits"):
            self.read_plan()

    def test_missing_inapplicability_and_external_obligation_refused(
        self,
    ) -> None:
        del self.plan["not_applicable"]["security"]
        with self.assertRaisesRegex(ValueError, "security"):
            self.read_plan()
        self.plan["not_applicable"]["security"] = "No affected dependency"
        del self.plan["external"]["qa"]
        with self.assertRaisesRegex(ValueError, "qa"):
            self.read_plan()

    def test_command_cwd_cannot_escape_export(self) -> None:
        self.plan["checks"][0]["cwd"] = "../outside"
        with self.assertRaisesRegex(ValueError, "traversal"):
            self.read_plan()

    def test_test_selection_cannot_be_empty(self) -> None:
        self.plan["checks"][0]["tests"] = []
        with self.assertRaisesRegex(ValueError, "explicit selected tests"):
            self.read_plan()

    def test_archive_excludes_dirty_and_untracked_overlays(self) -> None:
        (self.root / "source.py").write_text("dirty\n")
        (self.root / "untracked.py").write_text("hidden\n")
        self.read_plan()
        dest = Path(self.tmp.name) / "export"
        dest.mkdir()
        verifier.export_archive(dest)
        self.assertEqual((dest / "source.py").read_text(), "after\n")
        self.assertFalse((dest / "untracked.py").exists())
        self.assertFalse((dest / ".git").exists())
        with self.assertRaisesRegex(ValueError, "tracked edits"):
            self.read_plan("inplace")

    def test_staged_tree_binding_refuses_old_commit_tree(self) -> None:
        (self.root / "source.py").write_text("staged\n")
        self.git("add", "source.py")
        with self.assertRaisesRegex(ValueError, "exact candidate"):
            self.read_plan("staged")
        self.plan["tree"] = self.git("write-tree")
        self.read_plan("staged")

    def test_historical_failure_is_hash_bound_never_relabelled(self) -> None:
        import hashlib

        prior = Path(self.tmp.name) / "failed.json"
        prior.write_text('{"exit_code": 1}')
        self.plan["prior_receipts"] = [
            {
                "path": str(prior),
                "sha256": hashlib.sha256(prior.read_bytes()).hexdigest(),
                "disposition": "Still FAIL; outside changed runtime boundary",
            }
        ]
        self.read_plan()
        prior.write_text('{"exit_code": 0}')
        with self.assertRaisesRegex(ValueError, "prior receipt hash"):
            self.read_plan()

    def test_selected_failure_is_saved_and_stops_remaining_checks(
        self,
    ) -> None:
        plan, _ = self.read_plan()
        plan["checks"] *= 2
        receipt = Path(self.tmp.name) / "result.json"
        calls = []

        def run(argv, log, stream, cwd):
            calls.append(argv)
            log.write_text("controlled failure\n")
            return 0 if len(calls) == 1 else 7

        with mock.patch.object(verifier.lanes, "run_streaming", run):
            self.assertEqual(
                verifier.run_gates(self.root, plan, receipt, "archive"), 1
            )
        saved = json.loads(receipt.read_text())
        self.assertEqual(saved["exit_code"], 1)
        self.assertEqual(saved["checks"][-1]["exit_code"], 7)
        self.assertEqual(len(saved["not_run"]), 1)
        self.assertTrue(saved["committed_export"])
        original = receipt.read_bytes()
        with self.assertRaises(FileExistsError):
            verifier.lanes.write_receipt(
                "impact", [], 0, True, str(receipt), None
            )
        self.assertEqual(receipt.read_bytes(), original)

    def test_command_mutating_tracked_export_cannot_pass(self) -> None:
        plan, _ = self.read_plan()
        receipt = Path(self.tmp.name) / "changed-source.json"

        def run(argv, log, stream, cwd):
            log.write_text("synthetic command\n")
            if "public_tree.py" not in " ".join(argv):
                (cwd / "source.py").write_text("mutated")
            return 0

        with mock.patch.object(verifier.lanes, "run_streaming", run):
            self.assertEqual(
                verifier.run_gates(self.root, plan, receipt, "archive"), 1
            )
        saved = json.loads(receipt.read_text())
        self.assertIn("source.py", saved["source_error"])
        self.assertEqual(saved["exit_code"], 1)

    def test_existing_log_is_never_appended_or_replaced(self) -> None:
        plan, _ = self.read_plan()
        receipt = Path(self.tmp.name) / "old.json"
        log = receipt.with_name(receipt.name + ".0.log")
        log.write_text("immutable old failure\n")
        original = log.read_bytes()
        with self.assertRaises(FileExistsError):
            verifier.run_gates(self.root, plan, receipt, "archive")
        self.assertEqual(log.read_bytes(), original)
        self.assertFalse(receipt.exists())

    def test_no_default_implicit_full_gate(self) -> None:
        with mock.patch.object(
            sys, "argv", ["verify-tree.py", "--mode", "archive"]
        ):
            with self.assertRaises(SystemExit) as raised:
                verifier.main()
        self.assertEqual(raised.exception.code, 2)

    def test_missing_executable_retains_failed_receipt(self) -> None:
        self.plan["checks"][0]["argv"] = [
            "missing-executable-for-synthetic-test"
        ]
        plan, _ = self.read_plan()
        receipt = Path(self.tmp.name) / "spawn.json"
        original = verifier.lanes.run_streaming

        def run(argv, log, stream, cwd):
            if "public_tree.py" in " ".join(argv):
                log.write_text("public-tree synthetic control\n")
                return 0
            return original(argv, log, stream, cwd)

        with mock.patch.object(verifier.lanes, "run_streaming", run):
            self.assertEqual(
                verifier.run_gates(self.root, plan, receipt, "archive"), 1
            )
        self.assertEqual(
            json.loads(receipt.read_text())["checks"][-1]["exit_code"], 127
        )


class BroadSuiteTests(unittest.TestCase):
    def test_old_broad_recipes_are_refused(self) -> None:
        for argv in [
            ["cargo", "test", "--workspace"],
            ["cargo", "test", "-p", "kontor-store"],
            ["pnpm", "-r", "test"],
            ["pnpm", "exec", "vitest", "run"],
            ["python3", "-m", "unittest", "discover", "-s", "scripts/tests"],
            ["python3", "-m", "unittest", "discover", "-p", "test_*.py"],
        ]:
            with self.subTest(argv=argv), self.assertRaises(ValueError):
                verifier.validate_test_command(argv)

    def test_explicit_focused_targets_are_supported(self) -> None:
        for argv in [
            [
                "cargo",
                "test",
                "-p",
                "kontor-runtime-paseo",
                "--test",
                "contract",
                "close_proof",
                "--",
                "--exact",
            ],
            [
                "pnpm",
                "exec",
                "vitest",
                "run",
                "test/session-retirement.test.ts",
            ],
            [
                "python3",
                "-m",
                "unittest",
                "discover",
                "-p",
                "test_verify_tree.py",
            ],
        ]:
            verifier.validate_test_command(argv)


if __name__ == "__main__":
    unittest.main()
