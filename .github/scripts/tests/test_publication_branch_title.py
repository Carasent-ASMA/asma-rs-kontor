"""Focused tests for the trusted-base publication branch/title checker (ASMA-8103).

Run from the repository root:

    python3 -m unittest discover -s .github/scripts/tests -v
"""

from __future__ import annotations

import io
import json
import re
import sys
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
SCRIPTS_DIR = REPO_ROOT / ".github" / "scripts"
WORKFLOW_PATH = (
    REPO_ROOT / ".github" / "workflows" / "publication-branch-title.yml"
)
FIXTURE_PATH = (
    Path(__file__).resolve().parent
    / "fixtures"
    / "publication_branch_title_cases.json"
)

sys.path.insert(0, str(SCRIPTS_DIR))
import publication_branch_title as checker  # noqa: E402

REQUIRED_ACTIVITIES = (
    "edited",
    "opened",
    "ready_for_review",
    "reopened",
    "synchronize",
)
REASON_CODES = {
    value
    for name, value in vars(checker).items()
    if name.startswith("CODE_") and value != checker.CODE_OK
}


def _indented_block(text: str, key: str, indent: int | None = None) -> str | None:
    """Return the more-indented block following the first ``key:`` line."""

    lines = text.splitlines()
    for index, line in enumerate(lines):
        if line.strip() != key:
            continue
        current = len(line) - len(line.lstrip())
        if indent is not None and current != indent:
            continue
        collected: list[str] = []
        for follow in lines[index + 1 :]:
            if not follow.strip():
                collected.append(follow)
                continue
            follow_indent = len(follow) - len(follow.lstrip())
            if follow_indent <= current:
                break
            collected.append(follow)
        return "\n".join(collected)
    return None


class FixtureCasesTest(unittest.TestCase):
    """Every fixture case is a behavioural assertion on the exact checker."""

    @classmethod
    def setUpClass(cls) -> None:
        data = json.loads(FIXTURE_PATH.read_text(encoding="utf-8"))
        cls.default_head_sha = data["defaultHeadSha"]
        cls.cases = data["cases"]

    def test_fixture_names_are_unique(self) -> None:
        names = [case["name"] for case in self.cases]
        self.assertEqual(len(names), len(set(names)))

    def test_fixture_covers_both_outcomes(self) -> None:
        outcomes = {case["expect"]["accepted"] for case in self.cases}
        self.assertEqual(outcomes, {True, False})

    def test_fixture_exercises_every_reason_code(self) -> None:
        refused_codes = {
            case["expect"]["code"]
            for case in self.cases
            if not case["expect"]["accepted"]
        }
        self.assertEqual(refused_codes, REASON_CODES)

    def test_fixture_covers_every_ordinary_branch_type(self) -> None:
        release_re = re.compile(r"^releases/v[0-9]+\.[0-9]+\.[0-9]+$")
        accepted_types = {
            case["headRef"].split("/", 1)[0]
            for case in self.cases
            if case["expect"]["accepted"]
            and not release_re.fullmatch(case["headRef"])
        }
        self.assertEqual(accepted_types, set(checker.BRANCH_TYPES))

    def test_fixture_covers_the_keyless_release_form(self) -> None:
        release_cases = [
            case
            for case in self.cases
            if case["expect"]["accepted"]
            and re.fullmatch(r"releases/v[0-9]+\.[0-9]+\.[0-9]+", case["headRef"])
        ]
        self.assertTrue(release_cases)
        for case in release_cases:
            self.assertEqual(case["title"], f"Release v{case['expect']['identity']}")

    def test_every_fixture_case(self) -> None:
        for case in self.cases:
            with self.subTest(case=case["name"]):
                decision = checker.evaluate(
                    base_ref=case["baseRef"],
                    head_ref=case["headRef"],
                    title=case["title"],
                    head_sha=case.get("headSha", self.default_head_sha),
                )
                expect = case["expect"]
                self.assertEqual(
                    decision.accepted,
                    expect["accepted"],
                    f"{case['name']}: expected accepted={expect['accepted']}, "
                    f"got {decision.accepted} ({decision.code}: {decision.detail})",
                )
                self.assertEqual(
                    decision.code,
                    expect["code"],
                    f"{case['name']}: expected code {expect['code']}, "
                    f"got {decision.code} ({decision.detail})",
                )
                if "identity" in expect:
                    self.assertEqual(decision.identity, expect["identity"])


class WorkflowContractTest(unittest.TestCase):
    """The trusted-base workflow shape is part of the contract."""

    @classmethod
    def setUpClass(cls) -> None:
        cls.text = WORKFLOW_PATH.read_text(encoding="utf-8")

    def test_trigger_is_pull_request_target_with_required_activities(self) -> None:
        self.assertRegex(self.text, r"(?m)^on:\s*$")
        self.assertRegex(self.text, r"(?m)^\s{2}pull_request_target:\s*$")
        self.assertNotRegex(self.text, r"(?m)^\s{2}pull_request:\s*$")
        block = _indented_block(self.text, "types:")
        self.assertIsNotNone(block)
        activities = sorted(
            dict.fromkeys(re.findall(r"(?m)^\s*-\s*(\w+)\s*$", block or ""))
        )
        self.assertEqual(activities, sorted(REQUIRED_ACTIVITIES))

    def test_no_trigger_level_base_filter(self) -> None:
        self.assertNotRegex(self.text, r"(?m)^\s*branches:")

    def test_job_check_name_is_stable(self) -> None:
        matches = re.findall(
            r"(?m)^(\s*)name:\s*asma/publication-branch-title\s*$", self.text
        )
        self.assertEqual(len(matches), 1)
        self.assertEqual(len(matches[0]), 4, "the check name must be the job name")

    def test_asserted_head_is_the_pull_request_head_not_the_github_sha(self) -> None:
        self.assertRegex(
            self.text,
            r"(?m)^\s*HEAD_SHA:\s*\$\{\{\s*"
            r"github\.event\.pull_request\.head\.sha\s*\}\}\s*$",
        )
        self.assertNotIn("GITHUB_SHA", self.text)
        self.assertNotRegex(self.text, r"github\.sha\b")

    def test_checkout_is_the_exact_trusted_base_without_credentials(self) -> None:
        refs = re.findall(r"(?m)^\s*ref:\s*(.+?)\s*$", self.text)
        self.assertEqual(refs, ["${{ github.event.pull_request.base.sha }}"])
        self.assertRegex(self.text, r"(?m)^\s*persist-credentials:\s*false\s*$")
        self.assertNotIn("refs/pull", self.text)
        self.assertNotRegex(self.text, r"pull_request\.merge")
        for line in self.text.splitlines():
            if "pull_request.head.sha" in line:
                self.assertNotIn("uses:", line)
                self.assertNotIn("ref:", line)

    def test_actions_are_pinned_to_full_commit_shas(self) -> None:
        uses = re.findall(r"(?m)^\s*uses:\s*(\S+)(?:\s+#.*)?$", self.text)
        self.assertTrue(uses)
        for value in uses:
            self.assertRegex(value, r"^actions/checkout@[0-9a-f]{40}$", value)

    def test_runner_is_hosted(self) -> None:
        self.assertRegex(self.text, r"(?m)^\s*runs-on:\s*ubuntu-latest\s*$")

    def test_permissions_are_read_only(self) -> None:
        block = _indented_block(self.text, "permissions:", indent=0)
        self.assertIsNotNone(block)
        lines = [line.strip() for line in (block or "").splitlines() if line.strip()]
        self.assertEqual(lines, ["contents: read"])
        self.assertNotIn("secrets.", self.text)

    def test_metadata_is_passed_as_environment_data(self) -> None:
        expected = {
            "PR_ACTION": "github.event.action",
            "PR_NUMBER": "github.event.pull_request.number",
            "BASE_REF": "github.event.pull_request.base.ref",
            "HEAD_REF": "github.event.pull_request.head.ref",
            "HEAD_SHA": "github.event.pull_request.head.sha",
            "PR_TITLE": "github.event.pull_request.title",
        }
        for key, value in expected.items():
            self.assertRegex(
                self.text,
                rf"(?m)^\s*{key}:\s*\$\{{\{{\s*{re.escape(value)}\s*\}}\}}\s*$",
            )
        runs = re.findall(r"(?m)^\s*run:\s*(.+?)\s*$", self.text)
        self.assertEqual(
            runs, ["python3 .github/scripts/publication_branch_title.py"]
        )

    def test_concurrency_is_pr_scoped(self) -> None:
        self.assertRegex(
            self.text,
            r"(?m)^\s*group:\s*publication-branch-title-\$\{\{\s*"
            r"github\.event\.pull_request\.number\s*\}\}\s*$",
        )


class CheckerCliTest(unittest.TestCase):
    """The CLI contract: a JSON record, an exact head, no title in the record."""

    def _run(self, env: dict[str, str]) -> tuple[int, str, str]:
        out, err = io.StringIO(), io.StringIO()
        code = checker.main(env, out=out, err=err)
        return code, out.getvalue(), err.getvalue()

    def _valid_env(self) -> dict[str, str]:
        return {
            "PR_ACTION": "synchronize",
            "PR_NUMBER": "3101",
            "BASE_REF": "master",
            "HEAD_REF": "feat/ASMA-8101-publication-branch-title-check",
            "HEAD_SHA": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4",
            "PR_TITLE": "ASMA-8101 Publication branch and title check",
        }

    def test_valid_metadata_accepts_and_records_the_asserted_head(self) -> None:
        env = self._valid_env()
        code, out, _ = self._run(env)
        self.assertEqual(code, 0)
        record = json.loads(out.strip().splitlines()[-1])
        self.assertEqual(record["decision"], "accept")
        self.assertEqual(record["code"], "ok")
        self.assertEqual(record["head_sha"], env["HEAD_SHA"])
        self.assertEqual(record["pr_number"], 3101)
        self.assertEqual(record["base_ref"], "master")
        self.assertNotIn("title", record)

    def test_invalid_metadata_refuses_with_its_reason_code(self) -> None:
        env = self._valid_env()
        env["BASE_REF"] = "develop"
        code, out, err = self._run(env)
        self.assertEqual(code, 1)
        record = json.loads(out.strip().splitlines()[-1])
        self.assertEqual(record["decision"], "refuse")
        self.assertEqual(record["code"], "base_branch_not_default")
        self.assertIn("base_branch_not_default", err)

    def test_missing_head_sha_refuses(self) -> None:
        env = self._valid_env()
        env["HEAD_SHA"] = ""
        code, out, err = self._run(env)
        self.assertEqual(code, 1)
        self.assertEqual(json.loads(out.strip().splitlines()[-1])["code"], "head_sha_invalid")
        self.assertIn("head_sha_invalid", err)


if __name__ == "__main__":
    unittest.main()
