"""Tests for the neutral workflow templates and config examples (ASMA-8564).

Run from the repository root:

    python3 -m unittest discover -s scripts/tests -v

The YAML structure is proved by `crates/kontor-fleet/tests/config_examples.rs`.
This module proves the text contracts a renderer and a reviewer depend on:
required headings, the four-field hold block, placeholders instead of project
values, and relative links that resolve.
"""

from __future__ import annotations

import re
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
TEMPLATES = REPO_ROOT / "workflow" / "templates"
EXAMPLES = REPO_ROOT / "config" / "examples"

LINK_RE = re.compile(r"\[[^\]]+\]\(([^)]+)\)")
PLACEHOLDER_RE = re.compile(r"\{\{([^}]*)\}\}")


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


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
