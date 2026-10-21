"""Tests for the neutral coordination and planning skills (ASMA-8565).

Run from the repository root:

    python3 -m unittest discover -s scripts/tests -v

The tests prove the text contracts a renderer, a reviewer and the public-tree
check depend on: the expected skills exist once, every placeholder resolves
against the full nested project: path in the neutral and ASMA fixtures, no
private project value leaks outside a marked example, every relative and
same-file link resolves, and each repeated rule has one owner that the other
skills link to. They also prove every inventory row assigned to WP-03 is
recorded with a disposition.

These tests live in the scripts lane until WP-05 supplies `scripts/pack.py` and
the workflow-only lane; WP-05 owns moving or wiring them there.
"""

from __future__ import annotations

import re
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SKILLS = REPO_ROOT / "workflow" / "skills"
ORCHESTRATION = REPO_ROOT / "config" / "examples" / "orchestration" / "orchestration.yml"
ASMA_FIXTURE = REPO_ROOT / "tests" / "fixtures" / "config" / "asma-orchestration.yml"
PLAN_TEMPLATE = REPO_ROOT / "workflow" / "templates" / "plan.md"
PLACEMENT = SKILLS / "epic-orchestration" / "references" / "paseo-direct-native-placement.md"
EPIC = SKILLS / "epic-orchestration" / "SKILL.md"
LONG_TASK = SKILLS / "long-task-commit-checkpoints" / "SKILL.md"

LINK_RE = re.compile(r"\[[^\]]+\]\(([^)]+)\)")
PLACEHOLDER_RE = re.compile(r"\{\{\s*project\.([A-Za-z0-9_.]+)\s*\}\}")
ALL_PLACEHOLDER_RE = re.compile(r"\{\{(.*?)\}\}", re.DOTALL)
VALID_PLACEHOLDER_RE = re.compile(r"^\s*project\.[A-Za-z0-9_.]+\s*$")
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

EXPECTED_INVENTORY_ROWS = {
    f"WP03-INV-{number:02d}" for number in range(1, 9)
}


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
        if not target.startswith(("http://", "https://", "mailto:"))
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
    if target.startswith("#"):
        return source.resolve(), target[1:]
    file_part, _, fragment = target.partition("#")
    return (source.parent / file_part).resolve(), fragment


class YamlSubsetError(ValueError):
    """The file is outside the small YAML subset these fixtures use."""


def _strip_yaml_comment(line: str) -> str:
    in_single = in_double = False
    for index, character in enumerate(line):
        if character == "'" and not in_double:
            in_single = not in_single
        elif character == '"' and not in_single:
            in_double = not in_double
        elif character == "#" and not in_single and not in_double:
            if index == 0 or line[index - 1].isspace():
                return line[:index]
    return line


def _parse_scalar(value: str) -> object:
    value = value.strip()
    if len(value) >= 2 and value[0] in "'\"" and value[-1] == value[0]:
        return value[1:-1]
    if value in ("null", "~"):
        return None
    if value in ("true", "false"):
        return value == "true"
    try:
        return int(value)
    except ValueError:
        return value


def _tokenize_yaml(text: str) -> list[tuple[int, str]]:
    tokens: list[tuple[int, str]] = []
    for raw in text.splitlines():
        line = _strip_yaml_comment(raw).rstrip()
        if not line.strip():
            continue
        indent = len(line) - len(line.lstrip(" "))
        if "\t" in line[:indent]:
            raise YamlSubsetError("tab indentation is not supported")
        tokens.append((indent, line.strip()))
    return tokens


def _parse_block(
    tokens: list[tuple[int, str]], index: int, indent: int
) -> tuple[object, int]:
    if index >= len(tokens) or tokens[index][0] != indent:
        raise YamlSubsetError("unexpected indentation")
    if tokens[index][1].startswith("- "):
        items: list[object] = []
        while (
            index < len(tokens)
            and tokens[index][0] == indent
            and tokens[index][1].startswith("- ")
        ):
            content = tokens[index][1][2:].strip()
            index += 1
            if not content:
                if index < len(tokens) and tokens[index][0] > indent:
                    value, index = _parse_block(tokens, index, tokens[index][0])
                    items.append(value)
                else:
                    items.append(None)
            else:
                items.append(_parse_scalar(content))
        return items, index

    mapping: dict[str, object] = {}
    while index < len(tokens) and tokens[index][0] == indent:
        content = tokens[index][1]
        if content.startswith("- "):
            break
        if ":" not in content:
            raise YamlSubsetError(f"expected a mapping entry: {content}")
        key, _, rest = content.partition(":")
        key = key.strip()
        rest = rest.strip()
        index += 1
        if rest:
            mapping[key] = _parse_scalar(rest)
        elif index < len(tokens) and tokens[index][0] > indent:
            value, index = _parse_block(tokens, index, tokens[index][0])
            mapping[key] = value
        else:
            mapping[key] = None
    return mapping, index


def load_yaml_subset(path: Path) -> dict[str, object]:
    tokens = _tokenize_yaml(read(path))
    if not tokens:
        raise YamlSubsetError(f"{path} is empty")
    value, index = _parse_block(tokens, 0, tokens[0][0])
    if index != len(tokens):
        raise YamlSubsetError(f"{path} has trailing content at line {index}")
    if not isinstance(value, dict):
        raise YamlSubsetError(f"{path} is not a mapping")
    return value


def resolve_path(data: dict[str, object], dotted: str) -> object:
    current: object = data
    for part in dotted.split("."):
        if not isinstance(current, dict) or part not in current:
            raise KeyError(dotted)
        current = current[part]
    return current


def placeholder_errors(text: str) -> list[str]:
    errors = []
    for match in ALL_PLACEHOLDER_RE.finditer(text):
        if not VALID_PLACEHOLDER_RE.match(match.group(1)):
            errors.append(match.group(0))
    return errors


def used_placeholders() -> set[str]:
    used = set()
    for path in SKILLS.rglob("*.md"):
        if path.name == "README.md":
            continue
        for match in PLACEHOLDER_RE.finditer(read(path)):
            used.add(f"project.{match.group(1)}")
    return used


def capacity_section(text: str) -> str:
    match = re.search(r"^## Capacity\s*$(.*?)(?=^## |\Z)", text, re.M | re.S)
    if match is None:
        return ""
    return " ".join(match.group(1).split())


def capacity_placement_errors(section: str) -> list[str]:
    errors = []
    if "hard ceiling of `{{ project.capacity.max_parallel_tasks }}`" not in section:
        errors.append("the ceiling does not use max_parallel_tasks")
    if "Start admission at `{{ project.capacity.start_admission }}`" not in section:
        errors.append("start admission does not use start_admission")
    if "hard ceiling of `{{ project.capacity.max_delivery_seats }}`" in section:
        errors.append("the ceiling incorrectly uses max_delivery_seats")
    if "Start admission at `{{ project.capacity.max_parallel_tasks }}`" in section:
        errors.append("start admission incorrectly uses max_parallel_tasks")
    return errors


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
            if path.name == "README.md":
                continue
            text = read(path)
            with self.subTest(path=str(path.relative_to(SKILLS))):
                self.assertEqual(text.count("{{"), text.count("}}"))
                self.assertEqual(placeholder_errors(text), [], path)
                for match in PLACEHOLDER_RE.finditer(text):
                    self.assertTrue(match.group(1), f"empty placeholder in {path}")

    def test_every_used_placeholder_is_documented(self) -> None:
        readme = read(SKILLS / "README.md")
        documented = {
            match.group(1)
            for match in re.finditer(r"`(project\.[A-Za-z0-9_.]+)`", readme)
        }
        used = used_placeholders()
        self.assertTrue(used.issubset(documented), used - documented)
        self.assertGreater(len(used), 0)

    def test_every_placeholder_resolves_in_both_fixtures(self) -> None:
        used = sorted(used_placeholders())
        for fixture in (ORCHESTRATION, ASMA_FIXTURE):
            data = load_yaml_subset(fixture)
            for placeholder in used:
                with self.subTest(fixture=fixture.name, placeholder=placeholder):
                    resolve_path(data, placeholder)

    def test_an_unknown_placeholder_key_is_refused(self) -> None:
        data = load_yaml_subset(ORCHESTRATION)
        with self.assertRaises(KeyError):
            resolve_path(data, "project.tracker.architecture")

    def test_a_malformed_placeholder_is_refused(self) -> None:
        errors = placeholder_errors("{{ banana command }}")
        self.assertEqual(errors, ["{{ banana command }}"])

    def test_capacity_semantics_match_the_placement_rule(self) -> None:
        cases = (
            (ORCHESTRATION, 1, 2, 1),
            (ASMA_FIXTURE, 2, 7, 4),
        )
        for fixture, delivery_seats, ceiling, start_admission in cases:
            capacity = resolve_path(load_yaml_subset(fixture), "project.capacity")
            with self.subTest(fixture=fixture.name):
                self.assertEqual(capacity["max_delivery_seats"], delivery_seats)
                self.assertEqual(capacity["max_parallel_tasks"], ceiling)
                self.assertEqual(capacity["start_admission"], start_admission)
                self.assertLessEqual(
                    capacity["start_admission"], capacity["max_parallel_tasks"]
                )
        asma_capacity = resolve_path(
            load_yaml_subset(ASMA_FIXTURE), "project.capacity"
        )
        self.assertEqual(asma_capacity["max_parallel_tasks"], 7)
        self.assertEqual(asma_capacity["start_admission"], 4)

        section = capacity_section(read(PLACEMENT))
        self.assertNotEqual(section, "")
        self.assertEqual(capacity_placement_errors(section), [])
        rejected = (
            "## Capacity\n\n"
            "- One Paseo mini-project has a hard ceiling of\n"
            "  `{{ project.capacity.max_delivery_seats }}` concurrent team runs.\n"
            "- Start admission at `{{ project.capacity.max_parallel_tasks }}`."
        )
        self.assertTrue(capacity_placement_errors(rejected))
        swapped = section.replace("max_parallel_tasks", "__TMP__")
        swapped = swapped.replace("start_admission", "max_parallel_tasks")
        swapped = swapped.replace("__TMP__", "start_admission")
        self.assertTrue(capacity_placement_errors(swapped))


class TemplateContractTests(unittest.TestCase):
    def test_the_plan_template_matches_the_plan_graph_contract(self) -> None:
        text = read(PLAN_TEMPLATE)
        self.assertIn("### 2.5 Dependency Graph and Waves", text)
        self.assertEqual(text.count("```plan-graph"), 1)
        self.assertIn('"external_fences": []', text)
        self.assertIn("<!-- plan-graph:derived:start -->", text)
        self.assertIn("<!-- plan-graph:derived:end -->", text)


class NeutralityTests(unittest.TestCase):
    def test_marked_example_blocks_are_closed(self) -> None:
        for path in SKILLS.rglob("*.md"):
            text = read(path)
            with self.subTest(path=str(path.relative_to(SKILLS))):
                opens = len(
                    re.findall(
                        rf"^{re.escape(EXAMPLE_OPEN)}$", text, re.MULTILINE
                    )
                )
                closes = len(
                    re.findall(
                        rf"^{re.escape(EXAMPLE_CLOSE)}$", text, re.MULTILINE
                    )
                )
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
    def test_every_link_and_anchor_resolves(self) -> None:
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

    def test_a_same_file_anchor_is_checked(self) -> None:
        path = SKILLS / "epic-orchestration" / "SKILL.md"
        resolved, fragment = resolve_link(path, "#entry-gate")
        self.assertEqual(resolved, path.resolve())
        self.assertIn(fragment, anchors(read(path)))


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
                skill = (
                    SKILLS / name
                    if "/" in name
                    else SKILLS / name / "SKILL.md"
                )
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


def inventory_errors(rows: dict[str, list[str]]) -> list[str]:
    errors = []
    if set(rows) != EXPECTED_INVENTORY_ROWS:
        errors.append("inventory row set is incomplete or unknown")
    for row_id, row in rows.items():
        if len(row) != 3 or not row[2]:
            errors.append(f"{row_id} has no disposition")
    return errors


def disposition_parameter_errors(
    rows: dict[str, list[str]], pack_text: str
) -> list[str]:
    errors = []
    for row_id, row in rows.items():
        for parameter in re.findall(
            r"(?:`|\{\{\s*)(project\.[A-Za-z0-9_.]+)", row[2]
        ):
            if parameter not in pack_text:
                errors.append(f"{row_id} claims unused {parameter}")
    return errors


class InventoryRowTests(unittest.TestCase):
    def setUp(self) -> None:
        self.readme = read(SKILLS / "README.md")
        table = table_starting_with(self.readme, "Row")
        self.assertIsNotNone(table)
        self.rows = {row[0]: row for row in table[1:]}

    def test_every_wp03_inventory_row_is_recorded(self) -> None:
        self.assertEqual(set(self.rows), EXPECTED_INVENTORY_ROWS)

    def test_every_inventory_row_has_a_disposition(self) -> None:
        self.assertEqual(inventory_errors(self.rows), [])

    def test_inventory_validation_refuses_missing_or_incomplete_rows(self) -> None:
        missing = dict(self.rows)
        missing.pop("WP03-INV-01")
        self.assertTrue(inventory_errors(missing))
        incomplete = {row_id: list(row) for row_id, row in self.rows.items()}
        incomplete["WP03-INV-02"][2] = ""
        self.assertTrue(inventory_errors(incomplete))

    def test_every_inventory_source_is_in_the_pack(self) -> None:
        for row_id, row in self.rows.items():
            for name in re.findall(r"`([^`]+)`", row[1]):
                with self.subTest(row=row_id, source=name):
                    matches = list(SKILLS.rglob(f"{name}/SKILL.md"))
                    matches += list(SKILLS.rglob(f"{name}.md"))
                    self.assertTrue(matches, f"{name} is not in the pack")

    def test_inventory_claims_hold_in_the_pack(self) -> None:
        pack_text = "\n".join(
            read(path) for path in SKILLS.rglob("*.md") if path.name != "README.md"
        )
        self.assertEqual(disposition_parameter_errors(self.rows, pack_text), [])

        placement = read(PLACEMENT)
        self.assertIn("<state-root>/supervision.yml", placement)
        self.assertIn("{{ project.supervision }}", placement)
        self.assertIn("<state-root>/supervision.yml", read(EPIC))
        self.assertIn("{{ project.supervision }}", read(EPIC))

        long_task = read(LONG_TASK)
        self.assertIn("PR-title/squash-subject grammar", long_task)
        self.assertIn("validated checkpoint convention", long_task)
        self.assertNotIn(
            "message matching `{{ project.branch.commit_pattern }}`", long_task
        )

    def test_inventory_validation_refuses_a_claim_the_pack_does_not_use(self) -> None:
        pack_text = "\n".join(
            read(path) for path in SKILLS.rglob("*.md") if path.name != "README.md"
        )
        mutated = {row_id: list(row) for row_id, row in self.rows.items()}
        mutated["WP03-INV-01"][2] = "Claim `project.tracker.architecture`."
        self.assertEqual(
            disposition_parameter_errors(mutated, pack_text),
            ["WP03-INV-01 claims unused project.tracker.architecture"],
        )


if __name__ == "__main__":
    unittest.main()
