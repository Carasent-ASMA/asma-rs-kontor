"""Tests for the neutral workflow templates and config examples (ASMA-8564).

Run from the repository root:

    python3 -m unittest discover -s scripts/tests -v

The YAML structure is proved by `crates/kontor-fleet/tests/config_examples.rs`.
This module proves the text contracts a renderer and a reviewer depend on:
required headings, the four-field hold block, placeholders instead of project
values, and relative links that resolve.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
TEMPLATES = REPO_ROOT / "workflow" / "templates"
EXAMPLES = REPO_ROOT / "config" / "examples"

LINK_RE = re.compile(r"\[[^\]]+\]\(([^)]+)\)")
PLACEHOLDER_RE = re.compile(r"\{\{([^}]*)\}\}")


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def canonical_plan_graph() -> Path | None:
    for root in (REPO_ROOT, *REPO_ROOT.parents):
        candidate = root / "_tools" / "ai-orchestration" / "plan_graph.py"
        if candidate.is_file():
            return candidate
    return None


def replace_once(text: str, old: str, new: str) -> str:
    if old not in text:
        raise AssertionError(f"template is missing {old!r}")
    return text.replace(old, new, 1)


def filled_plan_template() -> str:
    text = read(TEMPLATES / "plan.md")
    text = replace_once(
        text,
        "orchestration: '{{ document.orchestration }}'",
        "orchestration: paseo",
    )
    text = replace_once(text, '"{{ document.epic }}"', '"ASMA-999999"')
    text = replace_once(
        text,
        '    {"id": "TASK-001", "title": "{{ task.title }}", "phase": "Phase 1", "jira": "{{ task.key }}", "kontor_id": null, "depends_on": []},',
        '    {"id": "TASK-001", "title": "Neutral task one", "phase": "Phase 1", "jira": "ASMA-999998", "kontor_id": null, "depends_on": []},',
    )
    text = replace_once(
        text,
        '    {"id": "TASK-002", "title": "{{ task.title }}", "phase": "Phase 1", "jira": "{{ task.key }}", "kontor_id": null, "depends_on": ["TASK-001"]}',
        '    {"id": "TASK-002", "title": "Neutral task two", "phase": "Phase 1", "jira": "ASMA-999997", "kontor_id": null, "depends_on": ["TASK-001"]}',
    )
    text = replace_once(
        text,
        '| TASK-001 | {{ task.key }} | {{ task.description }} | {{ task.closure }} | | |',
        '| TASK-001 | ASMA-999998 | Neutral task one | Neutral closure one | | |',
    )
    text = replace_once(
        text,
        '| TASK-002 | {{ task.key }} | {{ task.description }} | {{ task.closure }} | | |',
        '| TASK-002 | ASMA-999997 | Neutral task two | Neutral closure two | | |',
    )
    return text


def relative_links(path: Path) -> list[str]:
    links: list[str] = []
    for target in LINK_RE.findall(read(path)):
        if target.startswith(("http://", "https://", "mailto:", "#")):
            continue
        if "{{" in target:
            continue
        links.append(target)
    return links


class TemplateFilesTests(unittest.TestCase):
    def test_every_template_exists(self) -> None:
        for name in [
            "AGENTS.md",
            "document-header.md",
            "index-router.md",
            "plan.md",
            "epic.md",
            "task.md",
        ]:
            with self.subTest(name=name):
                self.assertTrue((TEMPLATES / name).is_file())

    def test_every_template_uses_project_placeholders(self) -> None:
        for path in TEMPLATES.glob("*.md"):
            with self.subTest(path=path.name):
                self.assertIn("{{ project.", read(path))

    def test_placeholders_are_well_formed(self) -> None:
        for path in TEMPLATES.glob("*.md"):
            text = read(path)
            with self.subTest(path=path.name):
                self.assertEqual(text.count("{{"), text.count("}}"))
                for match in PLACEHOLDER_RE.finditer(text):
                    self.assertTrue(
                        match.group(1).strip(),
                        f"empty placeholder in {path.name}",
                    )

    def test_no_template_names_a_project_value(self) -> None:
        for path in TEMPLATES.glob("*.md"):
            text = read(path)
            with self.subTest(path=path.name):
                for value in (
                    "ASMA",
                    "Carasent-ASMA",
                    "asma/",
                    "/Users/",
                    "/home/",
                    "claude" + "-work",
                    "claude" + "-personal",
                    "codex" + "-work",
                    "codex" + "-personal",
                ):
                    self.assertNotIn(value, text)

    def test_every_relative_template_link_resolves(self) -> None:
        for path in TEMPLATES.glob("*.md"):
            for target in relative_links(path):
                with self.subTest(path=path.name, target=target):
                    resolved = (path.parent / target.split("#")[0]).resolve()
                    self.assertTrue(resolved.exists(), f"{path.name}: {target}")

    def test_the_examples_readme_links_resolve(self) -> None:
        path = EXAMPLES / "README.md"
        for target in relative_links(path):
            with self.subTest(target=target):
                resolved = (path.parent / target.split("#")[0]).resolve()
                self.assertTrue(resolved.exists(), target)


class ContentContractTests(unittest.TestCase):
    def test_the_epic_contract_has_the_required_headings_in_order(self) -> None:
        text = read(TEMPLATES / "epic.md")
        positions = [
            text.index("## Summary / purpose"),
            text.index("## Goal"),
            text.index("## Scope and sub-goals"),
            text.index("## Done means"),
            text.index("## Detailed plan"),
        ]
        self.assertEqual(positions, sorted(positions))

    def test_the_task_contract_has_the_required_headings_in_order(self) -> None:
        text = read(TEMPLATES / "task.md")
        positions = [
            text.index("## Summary / purpose"),
            text.index("## Goal"),
            text.index("## Scope and sub-goals"),
            text.index("## Delivery context"),
            text.index("## Verification and evidence"),
            text.index("## Detailed plan"),
            text.index("## Done means"),
        ]
        self.assertEqual(positions, sorted(positions))

    def test_the_plan_template_carries_the_four_hold_fields(self) -> None:
        text = read(TEMPLATES / "plan.md")
        self.assertIn("hold:\n", text)
        for field in ("reason:", "owner:", "lifts_when:", "review_by:"):
            with self.subTest(field=field):
                self.assertIn(field, text)
        self.assertIn("waiting_on_dependency", text)
        self.assertIn("at most 30 days ahead", text)

    def test_the_plan_template_has_one_canonical_plan_graph(self) -> None:
        text = read(TEMPLATES / "plan.md")
        self.assertEqual(text.count("```plan-graph"), 1)
        self.assertIn("<!-- plan-graph:derived:start -->", text)
        self.assertIn("<!-- plan-graph:derived:end -->", text)

    def test_the_plan_template_matches_the_plan_graph_contract(self) -> None:
        text = read(TEMPLATES / "plan.md")
        self.assertIn("## 2. Implementation Steps", text)
        self.assertIn("### 2.5 Dependency Graph and Waves", text)
        self.assertNotIn("### 2.5 Dependency graph", text)
        self.assertIn(
            "| Task | Tracker key | Description | Closure condition | Completed | Date |",
            text,
        )

        match = re.search(r"```plan-graph\n(.*?)\n```", text, re.DOTALL)
        self.assertIsNotNone(match, "the template has one plan-graph JSON block")
        graph = json.loads(match.group(1))

        self.assertEqual(
            set(graph),
            {"schema_version", "epic", "tasks", "external_fences"},
            "the graph root matches the v1 contract",
        )
        self.assertEqual(graph["schema_version"], 1)
        self.assertEqual(set(graph["epic"]), {"jira", "kontor_id"})
        self.assertEqual(graph["external_fences"], [])
        self.assertTrue(graph["tasks"], "the graph has at least one task")

        task_keys = {"id", "title", "phase", "jira", "kontor_id", "depends_on"}
        task_ids: list[str] = []
        for task in graph["tasks"]:
            self.assertEqual(set(task), task_keys)
            self.assertIsInstance(task["depends_on"], list)
            task_ids.append(task["id"])
        self.assertEqual(len(task_ids), len(set(task_ids)), "task ids are unique")
        for task in graph["tasks"]:
            self.assertEqual(
                len(task["depends_on"]),
                len(set(task["depends_on"])),
                f"{task['id']} has unique prerequisites",
            )
            for prerequisite in task["depends_on"]:
                self.assertIn(prerequisite, task_ids)
                self.assertNotEqual(prerequisite, task["id"])

        section = text.split("## 2. Implementation Steps", 1)[1].split(
            "### 2.5 Dependency Graph and Waves", 1
        )[0]
        table_ids = re.findall(r"^\|\s*(TASK-[0-9]+)\s*\|", section, re.MULTILINE)
        self.assertEqual(
            table_ids,
            task_ids,
            "the Task table IDs agree with the graph tasks",
        )

    def test_the_plan_template_derived_view_has_the_canonical_shape(self) -> None:
        text = read(TEMPLATES / "plan.md")
        for expected in [
            "wave 0   TASK-001   (1 nodes)",
            "wave 1   TASK-002   (1 nodes)",
            "Dependencies (prerequisite -> dependent):",
            "  TASK-001 -> TASK-002",
            "{{ document.epic }}",
            "  TASK-001 · {{ task.key }} · Phase 1 · wave 0 · {{ task.title }}",
            "  TASK-002 · {{ task.key }} · Phase 1 · wave 1 · {{ task.title }}",
            "Structural waves only; external fences, gates and capacity still govern admission.",
        ]:
            self.assertIn(expected, text)

    def test_a_filled_plan_template_passes_the_canonical_validator(self) -> None:
        validator = canonical_plan_graph()
        if validator is None:
            self.skipTest("canonical plan-graph validator is not available")
        with tempfile.TemporaryDirectory() as directory:
            plan = Path(directory) / "plan.md"
            plan.write_text(filled_plan_template(), encoding="utf-8")
            sync = subprocess.run(
                [sys.executable, str(validator), "sync", str(plan)],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(sync.returncode, 0, sync.stderr)
            validated = subprocess.run(
                [sys.executable, str(validator), "validate", str(plan)],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(validated.returncode, 0, validated.stderr)
            self.assertIn("PASS", validated.stdout)
            self.assertIn("2 tasks", validated.stdout)

    def test_the_agents_template_has_both_parts(self) -> None:
        text = read(TEMPLATES / "AGENTS.md")
        self.assertIn("<!-- template-part: workflow -->", text)
        self.assertIn("<!-- template-part: project -->", text)
        self.assertLess(
            text.index("<!-- template-part: workflow -->"),
            text.index("<!-- template-part: project -->"),
        )

    def test_the_index_router_has_one_row_per_routed_document(self) -> None:
        text = read(TEMPLATES / "index-router.md")
        self.assertIn("| Document | Summary | Status | When to load |", text)
        self.assertIn("one row per document", text.lower())


class NeutralExampleTests(unittest.TestCase):
    def test_the_example_folder_is_yaml_only(self) -> None:
        allowed = {".yml", ".yaml", ".md", ".gitignore"}
        for path in EXAMPLES.rglob("*"):
            if path.is_dir():
                continue
            with self.subTest(path=str(path.relative_to(EXAMPLES))):
                self.assertTrue(
                    path.name == ".gitignore" or path.suffix in allowed,
                    f"{path} is YAML, Markdown or the host ignore file",
                )

    def test_the_neutral_fleet_holds_only_placeholder_accounts(self) -> None:
        text = read(EXAMPLES / "orchestration" / "fleet.yml")
        for alias in ("claude-primary", "claude-secondary", "codex-primary"):
            self.assertIn(alias, text)
        for owner_alias in (
            "claude" + "-work",
            "claude" + "-personal",
            "codex" + "-work",
            "codex" + "-personal",
        ):
            self.assertNotIn(owner_alias, text)
        self.assertIn("schema_version: 2", text)

    def test_the_neutral_quota_signals_are_not_captured_vendor_copy(self) -> None:
        text = read(EXAMPLES / "quota-signals.yml")
        self.assertIn("example.invalid", text)
        self.assertIn("example credit balance exhausted", text)
        self.assertNotIn("insufficient", text)
        self.assertNotIn("claude.ai/settings/usage", text)
        self.assertNotIn("chatgpt.com/codex", text)

    def test_the_github_app_docs_example_uses_placeholders(self) -> None:
        text = read(REPO_ROOT / "docs" / "CONFIGURATION.md")
        self.assertIn("app_id: 123456", text)
        self.assertIn("installation_id: 654321", text)
        self.assertIn("project_id: <kontor-project-id>", text)
        self.assertIn("repositories: [<owner>/<repository>]", text)
        self.assertNotIn('"project_id": "01a0064a-e056', text)
        self.assertTrue(
            (EXAMPLES / "forge" / "github-app.example.yml").is_file(),
            "the forge directory named by the inventory has its committed example",
        )

    def test_the_route_qualifications_example_is_content_bound(self) -> None:
        text = read(EXAMPLES / "orchestration" / "route-qualifications.yml")
        self.assertIn("schema_version: 1", text)
        self.assertNotIn("activated:", text)
        self.assertNotIn("activated_at:", text)
        self.assertIn("record_hash:", text)
        self.assertIn("canonical_model_id:", text)
        self.assertIn("route_model_id:", text)
        self.assertIn("glm-acp-agent", text)


if __name__ == "__main__":
    unittest.main()
