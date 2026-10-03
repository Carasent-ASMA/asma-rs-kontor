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

FETCH_STEP_NAME = "Fetch the checker from the exact workflow revision"
EVALUATE_STEP_NAME = "Evaluate the current branch and title"
CHECKER_RELATIVE_PATH = ".github/scripts/publication_branch_title.py"
RUNNER_TEMP_CHECKER = "${{ runner.temp }}/publication_branch_title.py"
WORKFLOW_SHA_EXPRESSION = "${{ github.workflow_sha }}"
HEAD_SHA_EXPRESSION = "${{ github.event.pull_request.head.sha }}"


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


def _steps(text: str) -> list[str]:
    """Split the workflow's job ``steps`` list into one text per step."""

    block = _indented_block(text, "steps:", indent=4)
    if block is None:
        return []
    lines = block.splitlines()
    base = min(
        len(line) - len(line.lstrip()) for line in lines if line.strip()
    )
    steps: list[list[str]] = []
    current: list[str] | None = None
    for line in lines:
        if (
            line.strip().startswith("- ")
            and len(line) - len(line.lstrip()) == base
        ):
            current = [line]
            steps.append(current)
        elif current is not None:
            current.append(line)
    return ["\n".join(step).rstrip() for step in steps]


def _step_named(steps: list[str], name: str) -> str:
    """Return the step whose ``- name:`` line matches exactly."""

    marker = f"- name: {name}"
    for step in steps:
        if marker in step:
            return step
    raise AssertionError(f"workflow step {name!r} is missing")


def _step_env(step: str) -> dict[str, str]:
    """Return the step's ``env`` mapping as plain key/value strings."""

    block = _indented_block(step, "env:")
    if block is None:
        return {}
    env: dict[str, str] = {}
    for line in block.splitlines():
        if not line.strip():
            continue
        key, separator, value = line.strip().partition(":")
        if separator:
            env[key.strip()] = value.strip()
    return env


def _step_run(step: str) -> str:
    """Return the shell body of the step's ``run`` key, block or inline."""

    lines = step.splitlines()
    for index, line in enumerate(lines):
        stripped = line.strip()
        if not stripped.startswith("run:"):
            continue
        if stripped not in ("run: |", "run: >"):
            return stripped.split("run:", 1)[1].strip()
        base = len(line) - len(line.lstrip())
        body: list[str] = []
        for follow in lines[index + 1 :]:
            if not follow.strip():
                body.append("")
                continue
            if len(follow) - len(follow.lstrip()) <= base:
                break
            body.append(follow)
        return "\n".join(body).strip("\n")
    return ""


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
    """The trusted-source workflow shape is part of the contract."""

    @classmethod
    def setUpClass(cls) -> None:
        cls.text = WORKFLOW_PATH.read_text(encoding="utf-8")
        cls.steps = _steps(cls.text)
        cls.fetch = _step_named(cls.steps, FETCH_STEP_NAME)
        cls.evaluate = _step_named(cls.steps, EVALUATE_STEP_NAME)

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

    def test_steps_are_fetch_then_evaluate(self) -> None:
        names = [
            line.strip().removeprefix("- name: ")
            for step in self.steps
            for line in step.splitlines()
            if line.strip().startswith("- name: ")
        ]
        self.assertEqual(names, [FETCH_STEP_NAME, EVALUATE_STEP_NAME])

    def test_no_checkout_or_repository_git_operation(self) -> None:
        self.assertNotIn("uses:", self.text)
        self.assertNotIn("checkout", self.text)
        self.assertNotIn("actions/cache", self.text)
        self.assertNotIn("upload-artifact", self.text)
        self.assertNotIn("download-artifact", self.text)
        self.assertEqual(re.findall(r"(?m)^\s*ref:\s*", self.text), [])
        for step in self.steps:
            run = _step_run(step)
            self.assertIsNone(re.search(r"\bgit\b", run), run)
            self.assertNotIn("clone", run)
            self.assertNotIn("submodule", run)

    def test_checker_source_is_exact_workflow_sha(self) -> None:
        self.assertEqual(
            _step_env(self.fetch).get("CHECKER_SOURCE_SHA"),
            WORKFLOW_SHA_EXPRESSION,
        )
        self.assertEqual(self.text.count(WORKFLOW_SHA_EXPRESSION), 1)
        self.assertEqual(
            re.findall(r"ref=([^\s\"'\\]+)", _step_run(self.fetch)),
            ["${CHECKER_SOURCE_SHA}"],
        )

    def test_event_base_sha_is_evidence_only(self) -> None:
        self.assertEqual(
            _step_env(self.fetch).get("EVENT_BASE_SHA"),
            "${{ github.event.pull_request.base.sha }}",
        )
        self.assertEqual(
            self.text.count("github.event.pull_request.base.sha"), 1
        )
        run = _step_run(self.fetch)
        for line in run.splitlines():
            if "EVENT_BASE_SHA" in line:
                self.assertRegex(line.strip(), r"^printf ")
        url_lines = [line for line in run.splitlines() if "contents/" in line]
        self.assertEqual(len(url_lines), 1)
        self.assertNotIn("EVENT_BASE_SHA", url_lines[0])
        self.assertNotIn("base.sha", url_lines[0])
        self.assertNotIn("EVENT_BASE_SHA", self.evaluate)
        self.assertNotIn("base.sha", _step_run(self.evaluate))

    def test_pr_head_and_merge_refs_are_never_sources(self) -> None:
        self.assertNotIn("refs/pull", self.text)
        self.assertNotIn("pull_request.merge", self.text)
        self.assertNotIn("merge_commit_sha", self.text)
        self.assertNotIn("head.repo", self.text)
        self.assertNotIn("head.repository", self.text)
        self.assertEqual(self.text.count("github.event.pull_request.head.sha"), 1)
        for step in self.steps:
            run = _step_run(step)
            self.assertNotIn("pull_request", run)
            self.assertNotIn("github.event", run)

    def test_source_fetch_fails_closed(self) -> None:
        run = _step_run(self.fetch)
        self.assertIn("--fail-with-body", run)
        self.assertIn('test "${#CHECKER_SOURCE_SHA}" -eq 40', run)
        self.assertIn("*[!0-9a-f]*", run)
        self.assertIn("Accept: application/vnd.github.raw+json", run)
        self.assertIn("X-GitHub-Api-Version: 2022-11-28", run)
        self.assertIn("contents/" + CHECKER_RELATIVE_PATH, run)
        self.assertIn('test -s "$CHECKER_PATH"', run)
        self.assertEqual(run.count("curl "), 1)
        self.assertNotIn("||", run)
        self.assertNotIn("wget", run)
        self.assertNotIn("master", run)

    def test_fetch_token_is_step_scoped(self) -> None:
        self.assertEqual(
            _step_env(self.fetch).get("GH_TOKEN"), "${{ github.token }}"
        )
        self.assertEqual(self.text.count("github.token"), 1)
        self.assertEqual(self.text.count("GH_TOKEN"), 2)
        self.assertNotIn("GH_TOKEN", self.evaluate)
        self.assertNotIn("github.token", self.evaluate)
        self.assertNotIn("secrets.", self.text)
        self.assertIsNone(_indented_block(self.text, "env:", indent=0))
        self.assertIsNone(_indented_block(self.text, "env:", indent=4))

    def test_permissions_are_exactly_contents_read(self) -> None:
        block = _indented_block(self.text, "permissions:", indent=0)
        self.assertIsNotNone(block)
        lines = [
            line.strip() for line in (block or "").splitlines() if line.strip()
        ]
        self.assertEqual(lines, ["contents: read"])
        self.assertEqual(
            re.findall(r"(?m)^permissions:", self.text), ["permissions:"]
        )
        self.assertNotIn("secrets.", self.text)

    def test_downloaded_checker_is_the_only_python_source(self) -> None:
        self.assertEqual(_step_run(self.evaluate), 'python3 "$CHECKER_PATH"')
        self.assertEqual(self.text.count("python3"), 1)
        self.assertEqual(
            _step_env(self.fetch).get("CHECKER_PATH"), RUNNER_TEMP_CHECKER
        )
        self.assertEqual(
            _step_env(self.evaluate).get("CHECKER_PATH"), RUNNER_TEMP_CHECKER
        )
        self.assertNotIn(".github/scripts", _step_run(self.evaluate))
        self.assertNotIn("continue-on-error", self.evaluate)
        self.assertNotIn("if:", self.evaluate)

    def test_asserted_head_is_pr_head_metadata(self) -> None:
        self.assertEqual(
            _step_env(self.evaluate).get("HEAD_SHA"), HEAD_SHA_EXPRESSION
        )
        self.assertNotIn("GITHUB_SHA", self.text)
        self.assertIsNone(re.search(r"github\.sha\b", self.text))

    def test_checker_path_exists_and_is_regular_source(self) -> None:
        checker_path = REPO_ROOT / CHECKER_RELATIVE_PATH
        self.assertTrue(checker_path.is_file(), str(checker_path))
        self.assertFalse(checker_path.is_symlink(), str(checker_path))

    def test_runner_is_hosted(self) -> None:
        self.assertRegex(self.text, r"(?m)^\s*runs-on:\s*ubuntu-latest\s*$")

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
        self.assertEqual(_step_run(self.evaluate), 'python3 "$CHECKER_PATH"')
        for step in self.steps:
            self.assertNotIn("${{", _step_run(step))

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
