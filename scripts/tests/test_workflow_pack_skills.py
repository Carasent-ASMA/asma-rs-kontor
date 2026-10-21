"""Tests for the neutral coordination and planning skills (ASMA-8565).

Run from the repository root:

    python3 -m unittest discover -s scripts/tests -v

The tests prove the text contracts a renderer, a reviewer and the public-tree
check depend on: the expected skills exist once, every placeholder resolves from
the project: block, no private project value leaks outside a marked example,
every relative link and anchor resolves, and each repeated rule has one owner
that the other skills link to. They also prove every inventory row assigned to
WP-03 is recorded with a disposition.
"""

from __future__ import annotations

import re
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SKILLS = REPO_ROOT / "workflow" / "skills"
ORCHESTRATION = REPO_ROOT / "config" / "examples" / "orchestration" / "orchestration.yml"
SCHEMA = REPO_ROOT / "config" / "examples" / "orchestration" / "orchestration.schema.yml"

LINK_RE = re.compile(r"\[[^\]]+\]\(([^)]+)\)")
PLACEHOLDER_RE = re.compile(r"\{\{\s*project\.([A-Za-z0-9_.]+)\s*\}\}")
EXAMPLE_OPEN = "<!-- example: project-specific -->"
EXAMPLE_CLOSE = "<!-- /example: project-specific -->"

EXPECTED_SKILLS = (
    "epic-orchestration",
    "mini-project-kickoff",
    "agent-dispatch",
    "seat-handoff",
    "seat-takeover",
    "ticket-triage-tiers",
    "create-implementation-plan",
    "tracker-linked-creation",
    "long-task-commit-checkpoints",
)

# Private values that must never appear in the public pack, even inside an
# example. They identify a real project, person, account or private checkout.
NEVER_PUBLIC = (
    "ASMA",
    "Carasent",
    "Igor",
    "asma-modules",
    "asma-rs-kontor",
    "_docs/",
    "/Users/",
    "/home/",
    "agentsroom",
    "DEC-0",
    "ASMA-",
    "ASMA_",
    "claude" + "-work",
    "claude" + "-personal",
    "codex" + "-work",
    "codex" + "-personal",
)


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def strip_examples(text: str) -> str:
    """Replace every marked project-specific example block with a blank line."""
    pattern = re.compile(
        re.escape(EXAMPLE_OPEN) + r".*?" + re.escape(EXAMPLE_CLOSE),
        re.DOTALL,
    )
    return pattern.sub("\n", text)


def markdown_tables(text: str) -> list[list[list[str]]]:
    """Return every Markdown table in a document as rows of stripped cells."""
    tables: list[list[list[str]]] = []
    current: list[list[str]] = []
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("|") and stripped.endswith("|"):
            cells = [cell.strip() for cell in stripped.strip("|").split("|")]
            if all(re.fullmatch(r":?-{3,}:?", cell) for cell in cells):
                continue
            current.append(cells)
        elif current:
            tables.append(current)
            current = []
    if current:
        tables.append(current)
    return tables


def table_starting_with(text: str, first_header: str) -> list[list[str]] | None:
    for table in markdown_tables(text):
        if table and table[0] and table[0][0] == first_header:
            return table
    return None


def link_targets(text: str) -> list[str]:
    return [
        target
        for target in LINK_RE.findall(text)
        if not target.startswith(("http://", "https://", "mailto:", "#"))
        and "{{" not in target
    ]


def heading_slug(heading: str) -> str:
    slug = heading.strip().lower()
    slug = re.sub(r"[^\w\s-]", "", slug)
    slug = re.sub(r"[\s_]+", "-", slug)
    return slug.strip("-")


def anchors(text: str) -> set[str]:
    found = set(re.findall(r'<a id="([^"]+)"', text))
    for line in text.splitlines():
        match = re.match(r"^(#{1,6})\s+(.*)$", line)
        if match:
            found.add(heading_slug(match.group(2)))
    return found


def resolve_link(source: Path, target: str) -> tuple[Path, str]:
    file_part, _, fragment = target.partition("#")
    return (source.parent / file_part).resolve(), fragment


class PackLayoutTests(unittest.TestCase):
    def test_each_listed_skill_exists_once(self) -> None:
        found = {path.parent.name for path in SKILLS.glob("*/SKILL.md")}
        self.assertEqual(found, set(EXPECTED_SKILLS))
        for name in EXPECTED_SKILLS:
            with self.subTest(name=name):
                self.assertTrue((SKILLS / name / "SKILL.md").is_file())

    def test_every_skill_frontmatter_names_its_directory(self) -> None:
        for name in EXPECTED_SKILLS:
            text = read(SKILLS / name / "SKILL.md")
            with self.subTest(name=name):
                self.assertIn(f"name: {name}\n", text)
                self.assertIn("\ndescription:", text)

    def test_the_readme_routes_every_skill(self) -> None:
        text = read(SKILLS / "README.md")
        for name in EXPECTED_SKILLS:
            with self.subTest(name=name):
                self.assertIn(f"]({name}/SKILL.md)", text)


class PlaceholderTests(unittest.TestCase):
    def test_placeholders_are_well_formed(self) -> None:
        for path in SKILLS.rglob("*.md"):
            text = read(path)
            with self.subTest(path=str(path.relative_to(SKILLS))):
                self.assertEqual(text.count("{{"), text.count("}}"))
                for match in PLACEHOLDER_RE.finditer(text):
                    self.assertTrue(match.group(1), f"empty placeholder in {path}")

    def test_every_used_placeholder_is_documented(self) -> None:
        readme = read(SKILLS / "README.md")
        documented = {
            match.group(1)
            for match in re.finditer(r"`(project\.[A-Za-z0-9_.]+)`", readme)
        }
        used = set()
        for path in SKILLS.rglob("*.md"):
            if path.name == "README.md":
                continue
            for match in PLACEHOLDER_RE.finditer(read(path)):
                used.add(f"project.{match.group(1)}")
        self.assertTrue(used.issubset(documented), used - documented)
        self.assertGreater(len(used), 0)

    def test_every_placeholder_key_exists_in_the_neutral_config(self) -> None:
        example = read(ORCHESTRATION)
        schema = read(SCHEMA)
        for path in sorted(
            {
                match.group(1)
                for file in SKILLS.rglob("*.md")
                for match in PLACEHOLDER_RE.finditer(read(file))
            }
        ):
            segments = path.split(".")
            leaf = segments[-1]
            with self.subTest(path=path):
                self.assertIn(f"{leaf}:", example + schema)
                self.assertIn(f"{segments[0]}:", example)


class NeutralityTests(unittest.TestCase):
    def test_marked_example_blocks_are_closed(self) -> None:
        for path in SKILLS.rglob("*.md"):
            text = read(path)
            with self.subTest(path=str(path.relative_to(SKILLS))):
                opens = len(re.findall(rf"^{re.escape(EXAMPLE_OPEN)}$", text, re.MULTILINE))
                closes = len(re.findall(rf"^{re.escape(EXAMPLE_CLOSE)}$", text, re.MULTILINE))
                self.assertEqual(opens, closes)

    def test_no_private_value_leaks_outside_a_marked_example(self) -> None:
        for path in SKILLS.rglob("*.md"):
            text = strip_examples(read(path)).lower()
            with self.subTest(path=str(path.relative_to(SKILLS))):
                for value in NEVER_PUBLIC:
                    self.assertNotIn(value.lower(), text, f"{value} in {path}")

    def test_private_identity_and_host_paths_never_appear_even_in_examples(self) -> None:
        never_anywhere = ("Igor", "Carasent", "/Users/", "/home/", "asma-modules")
        for path in SKILLS.rglob("*.md"):
            text = read(path)
            with self.subTest(path=str(path.relative_to(SKILLS))):
                for value in never_anywhere:
                    self.assertNotIn(value, text)


class LinkTests(unittest.TestCase):
    def test_every_relative_link_and_anchor_resolves(self) -> None:
        for path in SKILLS.rglob("*.md"):
            text = read(path)
            for target in link_targets(text):
                with self.subTest(path=str(path.relative_to(SKILLS)), target=target):
                    resolved, fragment = resolve_link(path, target)
                    self.assertTrue(resolved.is_file(), f"{path}: {target}")
                    if fragment:
                        self.assertIn(
                            fragment,
                            anchors(read(resolved)),
                            f"{path}: missing anchor {target}",
                        )


class RuleOwnershipTests(unittest.TestCase):
    def setUp(self) -> None:
        self.readme = read(SKILLS / "README.md")
        table = table_starting_with(self.readme, "Rule")
        self.assertIsNotNone(table)
        self.rows = table[1:]

    def test_every_row_has_exactly_one_owner(self) -> None:
        for row in self.rows:
            with self.subTest(rule=row[0]):
                self.assertEqual(len(row), 3)
                owner_links = re.findall(r"\]\(([^)]+)\)", row[1])
                self.assertEqual(len(owner_links), 1, row[1])

    def test_every_owner_anchor_is_defined_once(self) -> None:
        for row in self.rows:
            owner = re.search(r"\]\(([^)]+)\)", row[1]).group(1)
            owner_path, _, fragment = owner.partition("#")
            resolved = (SKILLS / owner_path).resolve()
            with self.subTest(rule=row[0]):
                self.assertTrue(resolved.is_file(), owner)
                if not fragment:
                    continue
                matches = [
                    path
                    for path in SKILLS.rglob("*.md")
                    if path.name != "README.md"
                    and fragment in anchors(read(path))
                ]
                self.assertEqual(matches, [resolved], f"{fragment} is not unique")

    def test_every_named_linking_skill_links_to_the_owner(self) -> None:
        for row in self.rows:
            owner = re.search(r"\]\(([^)]+)\)", row[1]).group(1)
            owner_path, _, owner_fragment = owner.partition("#")
            owner_resolved = (SKILLS / owner_path).resolve()
            linked_from = [
                name.strip() for name in row[2].split(",") if name.strip()
            ]
            self.assertGreater(len(linked_from), 0, row[0])
            for name in linked_from:
                skill = SKILLS / name / "SKILL.md"
                with self.subTest(rule=row[0], skill=name):
                    self.assertTrue(skill.is_file(), name)
                    targets = {
                        (resolved, fragment)
                        for target in link_targets(read(skill))
                        for resolved, fragment in [resolve_link(skill, target)]
                    }
                    matching = [
                        (resolved, fragment)
                        for resolved, fragment in targets
                        if resolved == owner_resolved
                        and (not owner_fragment or fragment == owner_fragment)
                    ]
                    self.assertTrue(
                        matching,
                        f"{name} does not link to {owner}",
                    )

    def test_the_seat_lifecycle_is_split_not_duplicated(self) -> None:
        text = re.sub(r"\s+", " ", self.readme.replace("`", ""))
        self.assertIn("agent-dispatch owns starting a seat", text)
        self.assertIn("seat-handoff owns a phase transition", text)
        self.assertIn("seat-takeover owns resuming", text)


class InventoryRowTests(unittest.TestCase):
    def setUp(self) -> None:
        self.readme = read(SKILLS / "README.md")
        table = table_starting_with(self.readme, "Row")
        self.assertIsNotNone(table)
        self.rows = {row[0]: row for row in table[1:]}

    def test_every_wp03_inventory_row_is_recorded(self) -> None:
        self.assertEqual(
            set(self.rows),
            {f"WP03-INV-{number:02d}" for number in range(1, 9)},
        )

    def test_every_inventory_row_has_a_disposition(self) -> None:
        for row_id, row in self.rows.items():
            with self.subTest(row=row_id):
                self.assertEqual(len(row), 3)
                self.assertTrue(row[2], row_id)

    def test_every_inventory_source_is_in_the_pack(self) -> None:
        for row_id, row in self.rows.items():
            for name in re.findall(r"`([^`]+)`", row[1]):
                with self.subTest(row=row_id, source=name):
                    matches = list(SKILLS.rglob(f"{name}/SKILL.md"))
                    matches += list(SKILLS.rglob(f"{name}.md"))
                    self.assertTrue(matches, f"{name} is not in the pack")


if __name__ == "__main__":
    unittest.main()
