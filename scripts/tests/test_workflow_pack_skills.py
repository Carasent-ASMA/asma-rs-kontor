"""Tests for the neutral workflow skills (ASMA-8565 and ASMA-8566).

Run from the repository root:

    python3 -m unittest discover -s scripts/tests -v

The tests prove the text contracts a renderer, a reviewer and the public-tree
check depend on: the expected skills exist once, every placeholder resolves
against the full nested project: path in the neutral and ASMA fixtures, no
private project value leaks outside a marked example, every relative and
same-file link resolves, and each repeated rule has one owner that the other
skills link to. They also prove every inventory row assigned to WP-03 and WP-04
is recorded with a disposition, and that the seat cards and the role crosswalk
cover every role the workflow teams use.

These tests live in the scripts lane until WP-05 supplies `scripts/pack.py` and
the workflow-only lane; WP-05 owns moving or wiring them there.
"""

from __future__ import annotations

import json
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
    "kontor-communication",
    "pr-review",
    "committee-protocol",
    "seat-cards",
    "experience-memory",
    "document-organization",
    "docs-navigation",
    "decision-records",
    "mutation-testing",
    "ubiquitous-language",
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

EXPECTED_WP04_ROWS = {
    f"WP04-INV-{number:02d}" for number in range(1, 8)
}

SEAT_CARDS = SKILLS / "seat-cards" / "SKILL.md"
TEAM_PACK = REPO_ROOT / "crates" / "kontor-teams" / "fixtures" / "mvp-team-pack.json"
OPERATIONAL_DOMAIN = REPO_ROOT / "crates" / "kontor-profiles" / "fixtures" / "operational-domain.json"
CONSULTATION_PRESETS = REPO_ROOT / "crates" / "kontor-profiles" / "fixtures" / "consultation-presets.json"
CORE_TEAM = REPO_ROOT / "config" / "examples" / "orchestration" / "teams" / "core-team.yml"
PR_REVIEW = SKILLS / "pr-review" / "SKILL.md"
FLEET = REPO_ROOT / "config" / "examples" / "orchestration" / "fleet.yml"

NON_SEAT_FLEET_SLOTS = ("fallback", "open")
INVENTED_RESEARCH_LABELS = ("RESEARCHER A", "RESEARCHER B", "SYNTHESIZER", "FINAL REVIEWER")
DESK_PERSONA_LABELS = {
    "TPM • <epic key> • <topic>",
    "TPM • <tracker key> • <repository>#<number>",
}

SEAT_CARD_FIELDS = (
    "Purpose",
    "Inputs",
    "Outputs",
    "Tool profile",
    "Stop conditions",
    "Personality",
)


def load_json(path: Path) -> dict[str, object]:
    return json.loads(read(path))


def latest_team_definition() -> dict[str, object]:
    definitions = load_json(OPERATIONAL_DOMAIN)["team_definitions"]
    return max(definitions, key=lambda definition: definition.get("version", 0))


def latest_teams() -> list[dict[str, object]]:
    teams: dict[str, dict[str, object]] = {}
    for team in load_json(TEAM_PACK)["teams"]:
        template_id = team["template_id"]
        current = teams.get(template_id)
        if current is None or team.get("version", 0) > current.get("version", 0):
            teams[template_id] = team
    return list(teams.values())


def seat_template_token(container: dict[str, object]) -> str:
    template = container.get("seat_name_template") or {}
    for segment in template.get("segments", []):
        if segment.get("kind") == "token":
            return segment["value"]
    return ""


def tsw_contract() -> tuple[dict[str, str], str]:
    for container in latest_team_definition().get("containers", []):
        if container.get("kind") != "TSW":
            continue
        slots = {
            slot["slot_id"]: slot["role_code"]
            for slot in container.get("team_slots", [])
        }
        return slots, seat_template_token(container)
    raise AssertionError("the Team Definition has no TSW container")


def naming_templates() -> dict[str, str]:
    templates: dict[str, str] = {}
    for container in latest_team_definition().get("containers", []):
        kind = container.get("kind")
        token = seat_template_token(container)
        if kind and token:
            templates[kind] = token
    return templates


def csw_display_names() -> dict[str, str]:
    for container in latest_team_definition().get("containers", []):
        if container.get("kind") != "CSW":
            continue
        return {
            slot["slot_id"]: slot["display_name"]
            for slot in container.get("slots", [])
        }
    raise AssertionError("the Team Definition has no CSW container")


def fleet_keys() -> list[str]:
    return re.findall(
        r"^  ((?:team|committee|advisor|leadership)/\S+):\s*\S+$",
        read(FLEET),
        re.MULTILINE,
    )


def fleet_team_slots() -> set[str]:
    return {
        key.rsplit("/", 1)[1]
        for key in fleet_keys()
        if key.startswith("team/")
    }


def fleet_advisor_slots() -> set[str]:
    return {
        key.split("/", 1)[1]
        for key in fleet_keys()
        if key.startswith("advisor/")
    }


def team_contract() -> tuple[
    dict[str, str], dict[str, str], dict[str, str], dict[str, str], dict[str, str]
]:
    """Return the authoritative role bindings and the slots each family uses."""
    operational = load_json(OPERATIONAL_DOMAIN)
    definition = latest_team_definition()
    bindings = {
        binding["role"]: binding["role_code"]
        for binding in operational["delivery"]["role_bindings"]
    }
    team_slots: dict[str, str] = {}
    for team in latest_teams():
        for slot in team["slots"]:
            team_slots[slot["id"]] = slot["role"]["role"]
    core_slots = dict(
        re.findall(
            r"role_slot_id:\s*(\S+)\s*\n\s*role_code:\s*(\S+)",
            read(CORE_TEAM),
        )
    )
    presets = load_json(CONSULTATION_PRESETS)
    committee_slots = {
        slot["id"]: slot["role"]
        for slot in presets["committee_templates"][0]["slots"]
    }
    asw_slots: dict[str, str] = {}
    for container in definition.get("containers", []):
        if container.get("kind") != "ASW":
            continue
        for slot in container.get("slots", []):
            asw_slots[slot["slot_id"]] = slot["role_code"]
    return bindings, team_slots, core_slots, committee_slots, asw_slots


def desk_slots() -> dict[str, str]:
    """Derive the PR desk slots from its own seat table and role bindings."""
    text = read(PR_REVIEW)
    table = table_starting_with(text, "Seat")
    if table is None:
        raise AssertionError("the PR review skill has no seat table")
    slots: dict[str, str] = {}
    for row in table[1:]:
        label = row[0].split(" — ", 1)[0].strip().strip("`")
        if label == "PR Adam":
            slots["adam"] = "TPM"
        elif label == "SWE":
            slots["swe"] = "SWE"
        else:
            slots[label.lower().replace(" ", "-")] = "AUD"
    return slots


def card_sections(text: str) -> tuple[dict[str, str], list[str]]:
    sections: dict[str, str] = {}
    duplicates: list[str] = []
    for block in re.split(r"^## ", text, flags=re.MULTILINE)[1:]:
        heading, _, body = block.partition("\n")
        name = heading.split(" — ", 1)[0].strip()
        if name in ("Crosswalk", "Common tool boundary", "Keeping the cards current"):
            continue
        if name in sections:
            duplicates.append(name)
        sections[name] = body
    return sections, duplicates


def field_values(body: str, field: str) -> set[str]:
    match = re.search(
        rf"- \*\*{re.escape(field)}:\*\* (.*?)(?=\n- \*\*|\Z)",
        body,
        re.DOTALL,
    )
    if match is None:
        return set()
    return {value.strip() for value in re.findall(r"`([^`]+)`", match.group(1))}


def card_records(text: str) -> tuple[dict[str, dict[str, set[str]]], list[str]]:
    sections, duplicates = card_sections(text)
    records: dict[str, dict[str, set[str]]] = {}
    for name, body in sections.items():
        records[name] = {
            "role_codes": field_values(body, "Role code"),
            "seed_roles": field_values(body, "Seed role"),
            "labels": field_values(body, "Seat labels"),
        }
    return records, duplicates


def crosswalk_rows(text: str) -> list[dict[str, object]]:
    table = table_starting_with(text, "Role code")
    if table is None:
        raise AssertionError("the seat-cards skill has no crosswalk")
    if len(table[0]) != 5:
        raise AssertionError(f"the crosswalk has {len(table[0])} columns, expected 5")
    rows: list[dict[str, object]] = []
    for row in table[1:]:
        if len(row) != 5:
            raise AssertionError(f"crosswalk row has {len(row)} columns: {row}")
        role = re.match(r"`([^`]+)`", row[0])
        if role is None:
            raise AssertionError(f"crosswalk row has no role code: {row[0]}")
        rows.append(
            {
                "role": role.group(1),
                "slots": [slot.strip().strip("`") for slot in row[1].split(",")],
                "seed": row[2].strip().strip("`"),
                "label": row[3].strip().strip("`"),
                "placement": row[4].strip(),
            }
        )
    return rows


def is_pending(row: dict[str, object]) -> bool:
    return "unregistered" in str(row["placement"])


def is_alternative(row: dict[str, object]) -> bool:
    return str(row["placement"]).startswith("alternative template")


def desk_label_roles() -> dict[str, str]:
    """Derive the PR desk's exact seat labels from its own seat table."""
    text = read(PR_REVIEW)
    table = table_starting_with(text, "Seat")
    if table is None:
        raise AssertionError("the PR review skill has no seat table")
    labels: dict[str, str] = {}
    for row in table[1:]:
        label = row[0].split(" — ", 1)[0].strip().strip("`")
        if label == "PR Adam":
            labels[label] = "TPM"
        elif label == "SWE":
            labels[label] = "SWE"
        else:
            labels[label] = "AUD"
    return labels


def crosswalk_slot_errors(
    rows: list[dict[str, object]],
    team_slots: dict[str, str],
    tsw_slots: dict[str, str],
    core_slots: dict[str, str],
    committee_slots: dict[str, str],
    desk_slots_: dict[str, str],
    advisor_slots: set[str],
) -> list[str]:
    errors: list[str] = []
    for row in rows:
        for slot in row["slots"]:
            if slot.startswith("leadership/"):
                target = slot.rsplit("/", 1)[1]
                if target not in core_slots:
                    errors.append(f"{row['label']} uses unknown core slot {target}")
            elif slot.startswith("committee/"):
                target = slot.rsplit("/", 1)[1]
                if target not in committee_slots:
                    errors.append(f"{row['label']} uses unknown committee slot {target}")
            elif slot.startswith("team/<desk-team-id>/"):
                target = slot.rsplit("/", 1)[1]
                if target not in desk_slots_:
                    errors.append(f"{row['label']} uses unknown desk slot {target}")
            elif slot.startswith("team/"):
                target = slot.rsplit("/", 1)[1]
                if target not in team_slots and target not in tsw_slots:
                    errors.append(f"{row['label']} uses unknown team slot {target}")
            elif slot.startswith("advisor/"):
                target = slot.split("/", 1)[1]
                if target not in advisor_slots and re.fullmatch(
                    r"<[a-z0-9-]+-advisor-id>", target
                ) is None:
                    errors.append(f"{row['label']} uses unknown advisor slot {target}")
            else:
                errors.append(f"{row['label']} uses unknown slot family {slot}")
    return errors


def crosswalk_role_errors(
    rows: list[dict[str, object]],
    bindings: dict[str, str],
    asw_slots: dict[str, str],
) -> list[str]:
    errors: list[str] = []
    for row in rows:
        seed = row["seed"]
        role = row["role"]
        if seed == "advisor":
            if role not in set(asw_slots.values()):
                errors.append(f"{row['label']} maps advisor to {role}")
        elif seed in bindings:
            if bindings[seed] != role:
                errors.append(
                    f"{row['label']} maps {seed} to {role}, expected {bindings[seed]}"
                )
        elif seed in ("LSA", "TPM"):
            if role != seed:
                errors.append(f"{row['label']} maps {seed} to {role}")
        else:
            errors.append(f"{row['label']} uses unknown seed role {seed}")
    return errors


def crosswalk_label_errors(
    rows: list[dict[str, object]],
    records: dict[str, dict[str, set[str]]],
) -> list[str]:
    errors: list[str] = []
    label_to_cards: dict[str, list[str]] = {}
    for name, record in records.items():
        for label in record["labels"]:
            label_to_cards.setdefault(label, []).append(name)
    for row in rows:
        label = row["label"]
        if is_pending(row):
            if label != "—":
                errors.append(f"pending row {row['seed']} uses label {label}")
            continue
        if label == "—":
            errors.append(f"registered row {row['seed']} has no seat label")
            continue
        names = [
            name
            for name in label_to_cards.get(label, [])
            if row["role"] in records[name]["role_codes"]
            and row["seed"] in records[name]["seed_roles"]
        ]
        if len(names) != 1:
            errors.append(
                f"{label} maps to {len(names)} cards: {', '.join(names)}"
            )
    return errors


def naming_label_errors(
    rows: list[dict[str, object]],
    templates: dict[str, str],
    display_names: dict[str, str],
    desk_labels: dict[str, str],
) -> list[str]:
    errors: list[str] = []
    for row in rows:
        placement = str(row["placement"])
        label = row["label"]
        role = row["role"]
        if is_pending(row):
            if label != "—":
                errors.append(f"pending row {row['seed']} uses label {label}")
            continue
        registered = re.match(r"registered: (ECP|TSW|ASW|CSW)\b", placement)
        if registered:
            kind = registered.group(1)
            token = templates.get(kind)
            if token == "ROLE_CODE":
                expected = role
            elif token == "SLOT_DISPLAY_NAME":
                targets = [slot.rsplit("/", 1)[1] for slot in row["slots"]]
                expected_names = {
                    display_names[target]
                    for target in targets
                    if target in display_names
                }
                if len(expected_names) != 1:
                    errors.append(
                        f"{row['seed']} has no unique {kind} display name"
                    )
                    continue
                expected = expected_names.pop()
            else:
                errors.append(f"{row['seed']} uses unknown {kind} template")
                continue
            if label != expected:
                errors.append(
                    f"{row['seed']} label {label} does not match {kind} {expected}"
                )
        elif placement == "desk seat":
            if desk_labels.get(label) != role:
                errors.append(f"{row['seed']} label {label} is not a desk seat")
        elif placement == "desk persona":
            if label not in DESK_PERSONA_LABELS:
                errors.append(f"{row['seed']} desk persona label is {label}")
        elif is_alternative(row):
            if label != role:
                errors.append(f"{row['seed']} alternative label {label} is not {role}")
        else:
            errors.append(f"{row['seed']} has unknown placement {placement}")
    return errors


def tsw_slot_errors(
    rows: list[dict[str, object]], tsw_slots: dict[str, str]
) -> list[str]:
    errors: list[str] = []
    for slot_id, role in tsw_slots.items():
        matches = [
            row
            for row in rows
            if row["role"] == role
            and not is_alternative(row)
            and not is_pending(row)
            and any(slot.endswith(f"/{slot_id}") for slot in row["slots"])
        ]
        if not matches:
            errors.append(f"TSW slot {slot_id} ({role}) has no current crosswalk row")
    return errors


def tsw_card_errors(
    records: dict[str, dict[str, set[str]]],
    tsw_slots: dict[str, str],
) -> list[str]:
    errors: list[str] = []
    for slot_id, role in tsw_slots.items():
        if not any(role in record["role_codes"] for record in records.values()):
            errors.append(f"TSW slot {slot_id} ({role}) has no card")
    return errors


def csw_slot_errors(
    rows: list[dict[str, object]], display_names: dict[str, str]
) -> list[str]:
    errors: list[str] = []
    for slot_id, display_name in display_names.items():
        matches = [
            row
            for row in rows
            if any(slot.endswith(f"/{slot_id}") for slot in row["slots"])
            and row["label"] == display_name
        ]
        if not matches:
            errors.append(f"CSW slot {slot_id} has no {display_name} row")
    return errors


def fleet_slot_errors(rows: list[dict[str, object]]) -> list[str]:
    errors: list[str] = []
    for slot_id in sorted(fleet_team_slots() - set(NON_SEAT_FLEET_SLOTS)):
        matches = [
            row for row in rows
            if any(slot.endswith(f"/{slot_id}") for slot in row["slots"])
        ]
        if not matches:
            errors.append(f"fleet slot {slot_id} has no crosswalk row")
    return errors


def team_slot_errors(
    rows: list[dict[str, object]], team_slots: dict[str, str]
) -> list[str]:
    errors: list[str] = []
    for slot_id, seed in team_slots.items():
        matches = [
            row
            for row in rows
            if row["seed"] == seed
            and any(slot.endswith(f"/{slot_id}") for slot in row["slots"])
        ]
        if not matches:
            errors.append(f"team slot {slot_id} ({seed}) has no crosswalk row")
    return errors


def team_card_errors(
    records: dict[str, dict[str, set[str]]],
    team_slots: dict[str, str],
    bindings: dict[str, str],
) -> list[str]:
    errors: list[str] = []
    for slot_id, seed in team_slots.items():
        role = bindings[seed]
        matches = [
            name
            for name, record in records.items()
            if role in record["role_codes"] and seed in record["seed_roles"]
        ]
        if not matches:
            errors.append(f"team slot {slot_id} ({seed}, {role}) has no card")
    return errors


def core_slot_errors(
    rows: list[dict[str, object]], core_slots: dict[str, str]
) -> list[str]:
    errors: list[str] = []
    for slot_id, role in core_slots.items():
        matches = [
            row
            for row in rows
            if row["role"] == role
            and any(slot.endswith(f"/{slot_id}") for slot in row["slots"])
        ]
        if not matches:
            errors.append(f"core slot {slot_id} ({role}) has no crosswalk row")
    return errors


def committee_slot_errors(
    rows: list[dict[str, object]], committee_slots: dict[str, str]
) -> list[str]:
    errors: list[str] = []
    for slot_id, seed in committee_slots.items():
        matches = [
            row
            for row in rows
            if row["seed"] == seed
            and any(slot.endswith(f"/{slot_id}") for slot in row["slots"])
        ]
        if not matches:
            errors.append(f"committee slot {slot_id} ({seed}) has no crosswalk row")
    return errors


def desk_slot_errors(
    rows: list[dict[str, object]], desk_slots_: dict[str, str]
) -> list[str]:
    errors: list[str] = []
    for slot_id, role in desk_slots_.items():
        matches = [
            row
            for row in rows
            if row["role"] == role
            and any(slot.endswith(f"/{slot_id}") for slot in row["slots"])
        ]
        if not matches:
            errors.append(f"desk slot {slot_id} ({role}) has no crosswalk row")
    return errors


class SeatCardTests(unittest.TestCase):
    def setUp(self) -> None:
        self.text = read(SEAT_CARDS)
        self.records, self.duplicates = card_records(self.text)
        self.rows = crosswalk_rows(self.text)
        (
            self.bindings,
            self.team_slots,
            self.core_slots,
            self.committee_slots,
            self.asw_slots,
        ) = team_contract()
        self.tsw_slots, self.tsw_template = tsw_contract()
        self.templates = naming_templates()
        self.display_names = csw_display_names()
        self.desk_slots = desk_slots()
        self.desk_labels = desk_label_roles()
        self.advisor_slots = fleet_advisor_slots()

    def slot_errors(self, rows: list[dict[str, object]]) -> list[str]:
        return crosswalk_slot_errors(
            rows,
            self.team_slots,
            self.tsw_slots,
            self.core_slots,
            self.committee_slots,
            self.desk_slots,
            self.advisor_slots,
        )

    def test_no_duplicate_card_sections(self) -> None:
        self.assertEqual(self.duplicates, [])

    def test_a_duplicate_card_section_is_rejected(self) -> None:
        _, duplicates = card_records(
            self.text + "\n## LSA — duplicate\n\n- **Purpose:** duplicate\n"
        )
        self.assertIn("LSA", duplicates)

    def test_every_card_has_the_six_required_fields(self) -> None:
        for name, body in card_sections(self.text)[0].items():
            with self.subTest(card=name):
                for field in SEAT_CARD_FIELDS:
                    self.assertIn(f"- **{field}:**", body, f"{name} lacks {field}")

    def test_every_card_has_one_personality_line(self) -> None:
        for name, body in card_sections(self.text)[0].items():
            with self.subTest(card=name):
                matches = re.findall(r"- \*\*Personality:\*\* (.+)", body)
                self.assertEqual(len(matches), 1)
                self.assertTrue(matches[0].strip())

    def test_every_card_has_a_role_code_seed_role_and_labels(self) -> None:
        for name, record in self.records.items():
            with self.subTest(card=name):
                self.assertTrue(record["role_codes"], name)
                self.assertTrue(record["seed_roles"], name)
                self.assertTrue(record["labels"], name)

    def test_every_template_slot_has_a_crosswalk_row(self) -> None:
        self.assertEqual(team_slot_errors(self.rows, self.team_slots), [])

    def test_every_template_slot_has_a_card(self) -> None:
        self.assertEqual(
            team_card_errors(self.records, self.team_slots, self.bindings), []
        )

    def test_every_current_tsw_slot_has_a_row_and_card(self) -> None:
        self.assertEqual(tsw_slot_errors(self.rows, self.tsw_slots), [])
        self.assertEqual(tsw_card_errors(self.records, self.tsw_slots), [])

    def test_every_core_committee_and_desk_slot_has_a_row(self) -> None:
        self.assertEqual(core_slot_errors(self.rows, self.core_slots), [])
        self.assertEqual(committee_slot_errors(self.rows, self.committee_slots), [])
        self.assertEqual(desk_slot_errors(self.rows, self.desk_slots), [])

    def test_every_neutral_fleet_team_slot_has_a_row_or_is_a_named_route(self) -> None:
        self.assertEqual(fleet_slot_errors(self.rows), [])
        for slot_id in NON_SEAT_FLEET_SLOTS:
            with self.subTest(slot=slot_id):
                self.assertIn(slot_id, fleet_team_slots())
                self.assertIn(slot_id, self.text)

    def test_every_crosswalk_role_matches_the_authoritative_binding(self) -> None:
        self.assertEqual(
            crosswalk_role_errors(self.rows, self.bindings, self.asw_slots), []
        )

    def test_every_crosswalk_slot_exists(self) -> None:
        self.assertEqual(self.slot_errors(self.rows), [])

    def test_every_crosswalk_label_matches_the_naming_template(self) -> None:
        self.assertEqual(
            naming_label_errors(
                self.rows,
                self.templates,
                self.display_names,
                self.desk_labels,
            ),
            [],
        )

    def test_every_csw_slot_uses_its_display_name(self) -> None:
        self.assertEqual(csw_slot_errors(self.rows, self.display_names), [])

    def test_every_crosswalk_label_maps_to_exactly_one_card(self) -> None:
        self.assertEqual(crosswalk_label_errors(self.rows, self.records), [])

    def test_every_card_is_reachable_from_a_crosswalk_row(self) -> None:
        labels = {row["label"] for row in self.rows}
        for name, record in self.records.items():
            with self.subTest(card=name):
                self.assertTrue(record["labels"] & labels, name)

    def test_the_current_tsw_slots_are_the_neutral_fleet_bindings(self) -> None:
        self.assertEqual(self.tsw_template, "ROLE_CODE")
        self.assertEqual(
            set(self.tsw_slots), {"scope", "implement", "verify", "audit"}
        )
        self.assertTrue(set(self.tsw_slots) <= fleet_team_slots())
        for slot_id, role in self.tsw_slots.items():
            rows = [
                row
                for row in self.rows
                if any(slot.endswith(f"/{slot_id}") for slot in row["slots"])
                and row["role"] == role
                and not is_alternative(row)
                and not is_pending(row)
            ]
            with self.subTest(slot=slot_id):
                self.assertEqual(len(rows), 1)
                self.assertEqual(rows[0]["label"], role)

    def test_the_uat_seed_role_is_therapist_verifier(self) -> None:
        uat = [row for row in self.rows if row["role"] == "UAT"]
        self.assertTrue(uat)
        for row in uat:
            self.assertEqual(row["seed"], "therapist-verifier")
            self.assertTrue(is_alternative(row))

    def test_a_wrong_uat_seed_role_is_rejected(self) -> None:
        rows = [dict(row) for row in self.rows]
        uat = next(row for row in rows if row["role"] == "UAT")
        uat["seed"] = "verifier"
        self.assertTrue(crosswalk_role_errors(rows, self.bindings, self.asw_slots))
        self.assertTrue(team_slot_errors(rows, self.team_slots))

    def test_the_final_reviewer_is_an_audit_role(self) -> None:
        final = [
            row
            for row in self.rows
            if any(slot.endswith("/final-reviewer") for slot in row["slots"])
        ]
        self.assertEqual(len(final), 1)
        self.assertEqual(final[0]["role"], "AUD")
        self.assertEqual(final[0]["seed"], "reviewer")
        self.assertTrue(is_pending(final[0]))
        self.assertEqual(final[0]["label"], "—")

    def test_the_research_spike_roles_are_covered_and_pending(self) -> None:
        for slot_id in (
            "researcher-a",
            "researcher-b",
            "judge",
            "synthesizer",
            "final-reviewer",
        ):
            with self.subTest(slot=slot_id):
                self.assertIn(slot_id, self.team_slots)
                rows = [
                    row
                    for row in self.rows
                    if any(slot.endswith(f"/{slot_id}") for slot in row["slots"])
                    and is_pending(row)
                ]
                self.assertEqual(len(rows), 1)
                self.assertEqual(rows[0]["label"], "—")
        self.assertIn("BA", self.records)
        self.assertIn("TW", self.records)

    def test_the_two_adam_personas_use_their_exact_seat_names(self) -> None:
        personas = [row for row in self.rows if row["placement"] == "desk persona"]
        self.assertEqual(len(personas), 2)
        self.assertEqual({row["label"] for row in personas}, DESK_PERSONA_LABELS)
        adam = self.records["Adam"]
        self.assertEqual(adam["labels"], DESK_PERSONA_LABELS)
        self.assertNotIn("ADAM", adam["labels"])

    def test_adam_as_a_desk_persona_label_is_rejected(self) -> None:
        rows = [dict(row) for row in self.rows]
        persona = next(row for row in rows if row["placement"] == "desk persona")
        persona["label"] = "ADAM"
        self.assertTrue(
            naming_label_errors(
                rows,
                self.templates,
                self.display_names,
                self.desk_labels,
            )
        )
        self.assertTrue(crosswalk_label_errors(rows, self.records))

    def test_no_invented_research_label_is_presented_as_native(self) -> None:
        invented = set(INVENTED_RESEARCH_LABELS)
        for name, record in self.records.items():
            with self.subTest(card=name):
                self.assertFalse(record["labels"] & invented)
        for row in self.rows:
            with self.subTest(row=row["seed"]):
                self.assertNotIn(row["label"], invented)

    def test_an_invented_research_label_is_rejected(self) -> None:
        rows = [dict(row) for row in self.rows]
        ba = next(row for row in rows if row["seed"] == "researcher")
        ba["label"] = "RESEARCHER A"
        ba["placement"] = "registered: TSW `ROLE_CODE`"
        self.assertTrue(
            naming_label_errors(
                rows,
                self.templates,
                self.display_names,
                self.desk_labels,
            )
        )
        self.assertTrue(crosswalk_label_errors(rows, self.records))

    def test_the_alternative_template_slots_are_marked(self) -> None:
        for slot_id in ("architect", "builder", "tester", "inspector", "verifier"):
            rows = [
                row
                for row in self.rows
                if any(slot.endswith(f"/{slot_id}") for slot in row["slots"])
            ]
            with self.subTest(slot=slot_id):
                self.assertEqual(len(rows), 1)
                self.assertTrue(is_alternative(rows[0]))

    def test_a_final_reviewer_mapped_to_the_reviewer_role_code_is_rejected(self) -> None:
        rows = [dict(row) for row in self.rows]
        final = next(
            row
            for row in rows
            if any(slot.endswith("/final-reviewer") for slot in row["slots"])
        )
        final["role"] = "reviewer"
        self.assertTrue(crosswalk_role_errors(rows, self.bindings, self.asw_slots))

    def test_a_builder_mapped_to_the_architect_role_is_rejected(self) -> None:
        rows = [dict(row) for row in self.rows]
        builder = next(
            row
            for row in rows
            if row["seed"] == "builder"
            and any(slot.endswith("/builder") for slot in row["slots"])
        )
        builder["role"] = "SA"
        self.assertTrue(crosswalk_role_errors(rows, self.bindings, self.asw_slots))

    def test_a_missing_team_card_is_rejected(self) -> None:
        records = dict(self.records)
        records.pop("BA")
        self.assertTrue(team_card_errors(records, self.team_slots, self.bindings))

    def test_an_unknown_slot_is_rejected(self) -> None:
        rows = [dict(row) for row in self.rows]
        rows[3]["slots"] = ["team/<team-definition-id>/does-not-exist"]
        self.assertTrue(self.slot_errors(rows))

    def test_an_unknown_advisor_slot_is_rejected(self) -> None:
        rows = [dict(row) for row in self.rows]
        advisor = next(row for row in rows if row["seed"] == "advisor")
        advisor["slots"] = ["advisor/does-not-exist"]
        self.assertTrue(self.slot_errors(rows))

    def test_the_crosswalk_keeps_contrasting_reviewer_slots(self) -> None:
        reviewer_slots = [
            slot
            for row in self.rows
            if row["seed"] == "reviewer"
            for slot in row["slots"]
        ]
        self.assertTrue(any("reviewer-a" in slot for slot in reviewer_slots))
        self.assertTrue(any("reviewer-b" in slot for slot in reviewer_slots))
        self.assertTrue(any("rev-sec" in slot for slot in reviewer_slots))
        self.assertTrue(any("rev-db" in slot for slot in reviewer_slots))

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

    def test_a_generic_decision_citation_is_allowed(self) -> None:
        text = "The record uses DEC-001 as a stable citation anchor."
        self.assertNotIn("DEC-0", NEVER_PUBLIC)
        for value in NEVER_PUBLIC:
            self.assertNotIn(value.lower(), text.lower())

    def test_a_private_record_reference_is_rejected(self) -> None:
        text = "ASMA-8562 DEC-015 records the private decision."
        self.assertTrue(
            any(value.lower() in text.lower() for value in NEVER_PUBLIC)
        )


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


def inventory_errors(
    rows: dict[str, list[str]], expected: set[str] | None = None
) -> list[str]:
    errors = []
    if expected is None:
        expected = EXPECTED_INVENTORY_ROWS
    if set(rows) != expected:
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


class Wp04InventoryRowTests(unittest.TestCase):
    def setUp(self) -> None:
        self.readme = read(SKILLS / "README.md")
        table = table_starting_with(self.readme, "WP-04 row")
        self.assertIsNotNone(table)
        self.rows = {row[0]: row for row in table[1:]}

    def test_every_wp04_inventory_row_is_recorded(self) -> None:
        self.assertEqual(set(self.rows), EXPECTED_WP04_ROWS)

    def test_every_wp04_inventory_row_has_a_disposition(self) -> None:
        self.assertEqual(inventory_errors(self.rows, EXPECTED_WP04_ROWS), [])

    def test_wp04_inventory_validation_refuses_missing_rows(self) -> None:
        missing = dict(self.rows)
        missing.pop("WP04-INV-01")
        self.assertTrue(inventory_errors(missing, EXPECTED_WP04_ROWS))

    def test_every_wp04_inventory_source_is_in_the_pack(self) -> None:
        for row_id, row in self.rows.items():
            for name in re.findall(r"`([^`]+)`", row[1]):
                with self.subTest(row=row_id, source=name):
                    matches = list(SKILLS.rglob(f"{name}/SKILL.md"))
                    matches += list(SKILLS.rglob(f"{name}.md"))
                    self.assertTrue(matches, f"{name} is not in the pack")

    def test_wp04_dispositions_only_claim_used_parameters(self) -> None:
        pack_text = "\n".join(
            read(path) for path in SKILLS.rglob("*.md") if path.name != "README.md"
        )
        self.assertEqual(disposition_parameter_errors(self.rows, pack_text), [])

    def test_wp04_inventory_validation_refuses_a_claim_the_pack_does_not_use(self) -> None:
        pack_text = "\n".join(
            read(path) for path in SKILLS.rglob("*.md") if path.name != "README.md"
        )
        mutated = {row_id: list(row) for row_id, row in self.rows.items()}
        mutated["WP04-INV-01"][2] = "Claim `project.tracker.architecture`."
        self.assertEqual(
            disposition_parameter_errors(mutated, pack_text),
            ["WP04-INV-01 claims unused project.tracker.architecture"],
        )


class Wp04OwnershipTests(unittest.TestCase):
    def test_the_review_skill_links_its_rule_owners(self) -> None:
        text = read(SKILLS / "pr-review" / "SKILL.md")
        for owner in (
            "../kontor-communication/SKILL.md",
            "../seat-cards/SKILL.md",
            "../committee-protocol/SKILL.md#independence-rules",
            "../document-organization/SKILL.md#three-homes-for-information",
        ):
            with self.subTest(owner=owner):
                self.assertIn(owner, text)

    def test_the_committee_skill_owns_independence(self) -> None:
        text = read(SKILLS / "committee-protocol" / "SKILL.md")
        self.assertIn("## Independence rules", text)
        for phrase in (
            "Read-only seats",
            "Fleet-bound models",
            "Contrasting reviewers",
            "Fresh judge",
            "No self-review",
        ):
            with self.subTest(phrase=phrase):
                self.assertIn(phrase, text)

    def test_committee_callers_link_the_protocol(self) -> None:
        for path in (
            SKILLS / "epic-orchestration" / "SKILL.md",
            SKILLS / "agent-dispatch" / "SKILL.md",
            SKILLS / "pr-review" / "SKILL.md",
            SKILLS / "epic-orchestration" / "references" / "paseo-direct-native-placement.md",
        ):
            with self.subTest(path=str(path.relative_to(SKILLS))):
                self.assertIn("committee-protocol", read(path))

    def test_the_checklist_owns_eleven_checks_and_done_means(self) -> None:
        text = read(SKILLS / "pr-review" / "references" / "review-checklist.md")
        self.assertIn("The 11 human-judgment checks", text)
        checks = re.findall(r"^\d+\. \*\*", text, re.MULTILINE)
        self.assertEqual(len(checks), 11)
        self.assertIn("**Done means.**", text)
        self.assertIn("check (11)", read(SKILLS / "pr-review" / "SKILL.md"))
        self.assertIn("check (11)", read(SKILLS / "pr-review" / "references" / "desk-messages.md"))

    def test_the_three_homes_have_one_owner(self) -> None:
        occurrences = 0
        for path in SKILLS.rglob("*.md"):
            text = read(path)
            occurrences += len(re.findall(r'<a id="three-homes-for-information"', text))
        self.assertEqual(occurrences, 1)
        self.assertIn(
            '<a id="three-homes-for-information">',
            read(SKILLS / "document-organization" / "SKILL.md"),
        )

    def test_the_pr_desk_configuration_contract_is_exact(self) -> None:
        text = read(SKILLS / "pr-review" / "SKILL.md")
        for phrase in (
            "{{ project.pr_desk.command }}",
            "{{ project.pr_desk.settings }}",
            "{{ project.pr_desk.history_root }}",
            "PR_DESK_DIR",
            "host.paths.pr_desk",
            "<resolved root>/learning.log",
            "poll_start_budget",
            "ready_silence_days",
            "draft_silence_days",
            "fix_round_limit",
            "revise_round_limit",
        ):
            with self.subTest(phrase=phrase):
                self.assertIn(phrase, text)
        self.assertNotIn("Configuration dependency", text)
        self.assertNotIn("partially fixed", text)
        self.assertNotIn("pr-review/learning.log", text)
        readme = read(SKILLS / "README.md")
        self.assertIn("WP04-INV-02", readme)
        self.assertIn("WP04-INV-04", readme)
        self.assertNotIn("Partial:", readme)
        self.assertIn("project.pr_desk", readme)

    def test_the_execution_plan_links_the_mutation_owner(self) -> None:
        self.assertIn(
            "../mutation-testing/SKILL.md",
            read(SKILLS / "create-implementation-plan" / "SKILL.md"),
        )


if __name__ == "__main__":
    unittest.main()
