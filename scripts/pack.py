#!/usr/bin/env python3
"""Render and check the Kontor workflow pack for one project.

The pack is authored once in ``workflow/skills``. A consuming project selects
one YAML config folder explicitly; this tool renders the pack's skills into
``<project>/.github/skills`` with that folder's ``project:`` parameters, runs
the project's existing skill mirror, and checks the rendered tree.

Commands:

    render <project>            render skills, then mirror them
    check <project>             run the parameterized pack checks
    install-adapters <project>  install native skill adapters in --target-root
    sync <plan>                 regenerate one plan's derived graph block
    validate <plan>...          validate plan graphs

The config folder is selected with ``--config-dir`` or
``KONTOR_CONFIG_DIR``; the flag wins. A relative input is resolved once
against the invocation's working directory. There is no implicit fallback to
the current directory, a repository, or ASMA.

``check`` validates authoring YAML, placeholders, mirror and metadata, byte
budgets and routing, links and anchors, routed-once documentation, ``hold:``
blocks, tracker-neutral plan graphs, and the public-example lint. When
``ASMA_PLAN_GRAPH_VALIDATOR`` is set it must name a readable validator; it is
never silently skipped. CI must set it for the conditional local check.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass
from datetime import date, timedelta
from pathlib import Path, PurePosixPath

try:  # pack.py runs beside public_tree.py in the pack checkout
    from public_tree import file_refusals
except ImportError:  # imported as scripts.pack
    from scripts.public_tree import file_refusals

try:  # DEP-005: the full parser when the environment supplies it
    import yaml as _pyyaml
except ImportError:  # the standard-library subset below is the fallback
    _pyyaml = None

REPO_ROOT = Path(__file__).resolve().parent.parent
PACK_SKILLS = REPO_ROOT / "workflow" / "skills"
PACK_RESOURCES_DIRNAME = ".pack"

PLACEHOLDER_RE = re.compile(r"\{\{(.*?)\}\}", re.DOTALL)
PROJECT_PLACEHOLDER_RE = re.compile(r"project\.[A-Za-z0-9_.]+")
SKILL_FRONTMATTER_RE = re.compile(
    r"\A---[ \t]*\r?\n(.*?)\r?\n---[ \t]*(?:\r?\n|\Z)", re.S
)
LINK_RE = re.compile(r"\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
HEADING_RE = re.compile(r"^\s*(?:>\s*)*#{1,6}\s+(.*?)\s*#*\s*$")
INLINE_CODE_RE = re.compile(r"`[^`]*`")
LINE_ANCHOR_RE = re.compile(r"^L\d+(?:-L\d+)?$")
TASK_ID_RE = re.compile(r"[A-Z][A-Z0-9]*(?:-[A-Z0-9]+)*-[0-9]+\Z")
PLAN_SECTION = "### 2.5 Dependency Graph and Waves"
PLAN_START = "<!-- plan-graph:derived:start -->"
PLAN_END = "<!-- plan-graph:derived:end -->"
PLAN_FENCE_RE = re.compile(r"^ {0,3}(`{3,}|~{3,})([^\n]*)$")

BUDGETS = {
    "AGENTS.md": 8192,
    "CLAUDE.md": 4096,
    ".github/copilot-instructions.md": 4096,
}
HOLD_REASONS = frozenset(
    {
        "waiting_on_dependency",
        "waiting_on_capacity",
        "waiting_on_decision",
        "waiting_on_external_party",
        "deprioritised",
    }
)

ROOT_REQUIRED = ("schema_version", "fleet", "core_team", "project")
PROJECT_REQUIRED = (
    "key",
    "name",
    "owner",
    "resource_roots",
    "decision_authorities",
    "tracker",
    "branch",
    "forge",
    "runtime",
    "cli",
    "team_pack",
    "fleet",
    "supervision",
    "merge_authority",
    "documentation_roots",
    "gap_ledger",
    "attention_channel",
    "glossary",
    "worktree_layout",
    "check_runs",
    "capacity",
    "time_zone",
    "holiday_countries",
    "currency",
)
NESTED_REQUIRED = {
    "decision_authorities": ("platform", "architecture", "decision_records_root"),
    "tracker": (
        "kind",
        "site",
        "project_key",
        "key_pattern",
        "credential_alias",
        "spec",
    ),
    "branch": ("types", "pattern", "commit_pattern"),
    "forge": ("kind", "organization", "repositories", "base_branch"),
    "runtime": ("matrix",),
    "cli": ("command", "verbs"),
    "merge_authority": ("role", "policy"),
    "capacity": ("max_delivery_seats", "max_parallel_tasks", "start_admission"),
}


class PackError(ValueError):
    """A refused input with an actionable message."""


class YamlError(ValueError):
    """The file is outside the YAML subset used by the pack config."""


def _strip_comment(line: str) -> str:
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


def _tokens(text: str) -> list[tuple[int, str]]:
    tokens: list[tuple[int, str]] = []
    for raw in text.splitlines():
        line = _strip_comment(raw).rstrip()
        if not line.strip():
            continue
        indent = len(line) - len(line.lstrip(" "))
        if "\t" in line[:indent]:
            raise YamlError("tab indentation is not supported")
        tokens.append((indent, line.strip()))
    return tokens


def _split_key(content: str) -> tuple[str, bool, str]:
    quote = ""
    depth = 0
    for index, character in enumerate(content):
        if quote:
            if character == quote:
                quote = ""
            continue
        if character in "'\"":
            quote = character
        elif character in "[{":
            depth += 1
        elif character in "]}":
            depth -= 1
        elif character == ":" and depth == 0:
            if index + 1 == len(content) or content[index + 1].isspace():
                return content[:index].strip(), True, content[index + 1 :].strip()
    return content.strip(), False, ""


def _looks_like_mapping(content: str) -> bool:
    if not content or content[0] in "[{\"'":
        return False
    key, found, _ = _split_key(content)
    return found and " " not in key


def _split_flow(text: str) -> list[str]:
    parts: list[str] = []
    quote = ""
    depth = 0
    start = 0
    for index, character in enumerate(text):
        if quote:
            if character == quote:
                quote = ""
            continue
        if character in "'\"":
            quote = character
        elif character in "[{":
            depth += 1
        elif character in "]}":
            depth -= 1
        elif character == "," and depth == 0:
            parts.append(text[start:index])
            start = index + 1
    parts.append(text[start:])
    return [part.strip() for part in parts if part.strip()]


def _parse_scalar(value: str) -> object:
    value = value.strip()
    if not value:
        return ""
    if value[0] in "'\"":
        if len(value) < 2 or value[-1] != value[0]:
            raise YamlError(f"unclosed quoted scalar: {value}")
        if value[0] == "'":
            return value[1:-1].replace("''", "'")
        try:
            return json.loads(value)
        except json.JSONDecodeError as exc:
            raise YamlError(f"invalid double-quoted scalar: {exc}") from exc
    if value.startswith("[") and not value.endswith("]"):
        raise YamlError(f"unclosed flow sequence: {value}")
    if value.startswith("{") and not value.endswith("}"):
        raise YamlError(f"unclosed flow mapping: {value}")
    if value.startswith("[") and value.endswith("]"):
        return [_parse_scalar(part) for part in _split_flow(value[1:-1])]
    if value.startswith("{") and value.endswith("}"):
        result: dict[str, object] = {}
        for part in _split_flow(value[1:-1]):
            key, found, rest = _split_key(part)
            if not found:
                raise YamlError(f"invalid flow mapping entry: {part}")
            result[key] = _parse_scalar(rest)
        return result
    lowered = value.lower()
    if lowered in ("null", "~"):
        return None
    if lowered in ("true", "false"):
        return lowered == "true"
    if re.fullmatch(r"-?[0-9]+", value):
        return int(value)
    if re.fullmatch(r"-?[0-9]+\.[0-9]+", value):
        return float(value)
    return value


def _parse_map(
    tokens: list[tuple[int, str]], index: int, indent: int
) -> tuple[dict[str, object], int]:
    result: dict[str, object] = {}
    while index < len(tokens):
        current_indent, content = tokens[index]
        if current_indent < indent:
            break
        if current_indent > indent:
            raise YamlError(f"unexpected indentation: {content}")
        if content == "-" or content.startswith("- "):
            break
        key, found, rest = _split_key(content)
        if not found:
            raise YamlError(f"expected a mapping entry: {content}")
        if key in result:
            raise YamlError(f"duplicate mapping key: {key}")
        index += 1
        if rest in ("|", ">", "|-", ">-", "|+", ">+"):
            lines = []
            while index < len(tokens) and tokens[index][0] > indent:
                lines.append(tokens[index][1])
                index += 1
            separator = "\n" if rest.startswith("|") else " "
            value = separator.join(lines)
            if not rest.endswith("-"):
                value += "\n"
        elif rest:
            value = _parse_scalar(rest)
        elif index < len(tokens) and tokens[index][0] > indent:
            value, index = _parse_node(tokens, index, tokens[index][0])
        elif (
            index < len(tokens)
            and tokens[index][0] == indent
            and (
                tokens[index][1] == "-"
                or tokens[index][1].startswith("- ")
            )
        ):
            value, index = _parse_seq(tokens, index, indent)
        else:
            value = None
        result[key] = value
    return result, index


def _parse_seq(
    tokens: list[tuple[int, str]], index: int, indent: int
) -> tuple[list[object], int]:
    result: list[object] = []
    while index < len(tokens) and tokens[index][0] == indent:
        content = tokens[index][1]
        if content != "-" and not content.startswith("- "):
            break
        if content == "-":
            index += 1
            if index < len(tokens) and tokens[index][0] > indent:
                value, index = _parse_node(tokens, index, tokens[index][0])
            else:
                value = None
        else:
            rest = content[2:].strip()
            index += 1
            if _looks_like_mapping(rest):
                item = [(indent + 2, rest)]
                while index < len(tokens) and tokens[index][0] > indent:
                    item.append(tokens[index])
                    index += 1
                value, consumed = _parse_map(item, 0, indent + 2)
                if consumed != len(item):
                    raise YamlError(f"unparsed sequence item: {rest}")
            else:
                value = _parse_scalar(rest)
        result.append(value)
    return result, index


def _parse_node(
    tokens: list[tuple[int, str]], index: int, indent: int
) -> tuple[object, int]:
    content = tokens[index][1]
    if content == "-" or content.startswith("- "):
        return _parse_seq(tokens, index, indent)
    if not _looks_like_mapping(content):
        return _parse_scalar(content), index + 1
    return _parse_map(tokens, index, indent)


def _load_yaml_subset(text: str) -> object:
    tokens = _tokens(text)
    if not tokens:
        return None
    value, index = _parse_node(tokens, 0, tokens[0][0])
    if index != len(tokens):
        raise YamlError(f"trailing content at line {index}")
    return value


def load_yaml_text(text: str) -> object:
    if _pyyaml is not None:
        try:
            return _pyyaml.safe_load(text)
        except _pyyaml.YAMLError as exc:
            raise YamlError(str(exc)) from exc
    return _load_yaml_subset(text)


def load_yaml(path: Path) -> object:
    try:
        return load_yaml_text(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError) as exc:
        raise YamlError(f"{path}: {exc}") from exc


def _metadata_text(value: object) -> str:
    if value is None:
        return ""
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (dict, list)):
        return json.dumps(value, ensure_ascii=False, sort_keys=True)
    return str(value)


def _frontmatter(text: str) -> dict[str, str]:
    match = SKILL_FRONTMATTER_RE.match(text)
    if match is None:
        return {}
    try:
        value = load_yaml_text(match.group(1))
    except YamlError as exc:
        raise PackError(f"invalid YAML frontmatter: {exc}") from exc
    if value is None:
        return {}
    if not isinstance(value, dict):
        raise PackError("frontmatter must be a YAML mapping")
    return {str(key): _metadata_text(item) for key, item in value.items()}


def _lookup(project: dict[str, object], dotted: str) -> object:
    current: object = project
    for part in dotted.split("."):
        if not isinstance(current, dict) or part not in current:
            raise KeyError(dotted)
        current = current[part]
    return current


def _format_value(value: object) -> str:
    if value is None:
        raise PackError("unfilled placeholder")
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, list):
        return ", ".join(_format_value(item) for item in value)
    if isinstance(value, dict):
        return json.dumps(value, ensure_ascii=False, sort_keys=True)
    return str(value)


def _is_relative_inside(folder: Path, reference: str) -> Path:
    if not isinstance(reference, str) or not reference.strip():
        raise PackError("config file reference must be a nonempty string")
    pure = PurePosixPath(reference)
    if pure.is_absolute() or ".." in pure.parts:
        raise PackError(f"config file reference escapes the folder: {reference}")
    resolved = (folder / pure).resolve()
    try:
        resolved.relative_to(folder.resolve())
    except ValueError as exc:
        raise PackError(
            f"config file reference escapes the folder: {reference}"
        ) from exc
    return resolved


@dataclass(frozen=True)
class Config:
    """One explicitly selected config folder."""

    folder: Path
    data: dict[str, object]
    project: dict[str, object]
    key_pattern: re.Pattern[str]


def _required_keys(
    folder: Path, data: dict[str, object]
) -> tuple[tuple[str, ...], tuple[str, ...], dict[str, tuple[str, ...]]]:
    schema_path = folder / "orchestration" / "orchestration.schema.yml"
    if not schema_path.is_file():
        return ROOT_REQUIRED, PROJECT_REQUIRED, NESTED_REQUIRED
    try:
        schema = load_yaml(schema_path)
    except YamlError as exc:
        raise PackError(f"{schema_path}: {exc}") from exc
    if not isinstance(schema, dict):
        raise PackError(f"{schema_path}: schema must be a mapping")
    root = schema.get("required")
    project = schema.get("project")
    nested = project.get("nested") if isinstance(project, dict) else None
    if not isinstance(root, list) or not isinstance(project, dict):
        raise PackError(f"{schema_path}: schema is incomplete")
    project_required = project.get("required")
    if not isinstance(project_required, list):
        raise PackError(f"{schema_path}: project.required is missing")
    nested_required: dict[str, tuple[str, ...]] = {}
    if isinstance(nested, dict):
        for name, spec in nested.items():
            values = spec.get("required") if isinstance(spec, dict) else None
            if isinstance(values, list):
                nested_required[str(name)] = tuple(str(value) for value in values)
    return (
        tuple(str(value) for value in root),
        tuple(str(value) for value in project_required),
        nested_required,
    )


def load_config(folder: Path) -> Config:
    """Validate the selected folder and return its project parameters."""

    folder = folder.expanduser().resolve()
    if not folder.exists():
        raise PackError(f"config folder does not exist: {folder}")
    if not folder.is_dir():
        raise PackError(f"config folder is not a directory: {folder}")

    errors: list[str] = []
    for path in sorted(folder.rglob("*")):
        if path.is_file() and path.suffix in (".yml", ".yaml"):
            try:
                load_yaml(path)
            except YamlError as exc:
                errors.append(f"{path.relative_to(folder)}: {exc}")
    if errors:
        raise PackError("; ".join(errors))

    orchestration = folder / "orchestration" / "orchestration.yml"
    if not orchestration.is_file():
        raise PackError(
            f"config folder has no orchestration/orchestration.yml: {folder}"
        )
    try:
        data = load_yaml(orchestration)
    except YamlError as exc:
        raise PackError(f"{orchestration}: {exc}") from exc
    if not isinstance(data, dict):
        raise PackError(f"{orchestration}: expected a mapping")

    root_required, project_required, nested_required = _required_keys(folder, data)
    missing = [key for key in root_required if key not in data]
    if missing:
        raise PackError(f"{orchestration}: missing {', '.join(missing)}")
    project = data.get("project")
    if not isinstance(project, dict):
        raise PackError(f"{orchestration}: project must be a mapping")
    missing = [key for key in project_required if key not in project]
    if missing:
        raise PackError(
            f"{orchestration}: project is missing {', '.join(missing)}"
        )
    for name, required in nested_required.items():
        value = project.get(name)
        if not isinstance(value, dict):
            raise PackError(f"{orchestration}: project.{name} must be a mapping")
        missing = [key for key in required if key not in value]
        if missing:
            raise PackError(
                f"{orchestration}: project.{name} is missing {', '.join(missing)}"
            )

    key = project.get("key")
    if not isinstance(key, str) or not key.strip():
        raise PackError(f"{orchestration}: project.key is required")
    tracker = project["tracker"]
    if not isinstance(tracker, dict):
        raise PackError(f"{orchestration}: project.tracker must be a mapping")
    pattern = tracker.get("key_pattern")
    if not isinstance(pattern, str) or not pattern:
        raise PackError(f"{orchestration}: project.tracker.key_pattern is required")
    try:
        compiled = re.compile(pattern)
    except re.error as exc:
        raise PackError(
            f"{orchestration}: project.tracker.key_pattern is invalid: {exc}"
        ) from exc

    references = [data.get("fleet"), data.get("core_team")]
    references.extend(
        project.get(key) for key in ("team_pack", "fleet", "supervision")
    )
    references.append(tracker.get("spec"))
    for reference in references:
        if reference is None:
            continue
        resolved = _is_relative_inside(folder, str(reference))
        if not resolved.is_file():
            raise PackError(f"config file reference does not exist: {reference}")
    return Config(folder, data, project, compiled)


def resolve_config(explicit: str | None, cwd: Path) -> Path:
    """Resolve the one explicit config-folder selection, never a fallback."""

    selected = explicit if explicit is not None else os.environ.get(
        "KONTOR_CONFIG_DIR"
    )
    if not selected:
        raise PackError(
            "no config folder selected: pass --config-dir PATH or set "
            "KONTOR_CONFIG_DIR"
        )
    path = Path(selected).expanduser()
    if not path.is_absolute():
        path = cwd / path
    return path.resolve()


def _resolve_project(argument: str, cwd: Path) -> Path:
    path = Path(argument).expanduser()
    if not path.is_absolute():
        path = cwd / path
    path = path.resolve()
    if not path.exists():
        raise PackError(f"project does not exist: {path}")
    if not path.is_dir():
        raise PackError(f"project is not a directory: {path}")
    return path


def _skills(root: Path) -> dict[str, Path]:
    if not root.is_dir():
        return {}
    return {
        path.name: path
        for path in sorted(root.iterdir())
        if path.is_dir() and (path / "SKILL.md").is_file()
    }


def _resources(root: Path) -> dict[str, tuple[str, object]]:
    found: dict[str, tuple[str, object]] = {}
    if not root.is_dir():
        return found
    for path in sorted(root.rglob("*")):
        relative = path.relative_to(root).as_posix()
        if path.is_symlink():
            found[relative] = ("link", os.readlink(path))
        elif path.is_file():
            found[relative] = ("file", path.read_bytes())
    return found


def _render_text(text: str, project: dict[str, object], label: str) -> str:
    def replace(match: re.Match[str]) -> str:
        expression = match.group(1).strip()
        if not PROJECT_PLACEHOLDER_RE.fullmatch(expression):
            raise PackError(
                f"{label}: unfilled or unsupported placeholder "
                f"{match.group(0).strip()}"
            )
        dotted = expression[len("project.") :]
        try:
            value = _lookup(project, dotted)
        except KeyError as exc:
            raise PackError(
                f"{label}: missing parameter {expression}"
            ) from exc
        try:
            return _format_value(value)
        except PackError as exc:
            raise PackError(f"{label}: {expression} is unfilled") from exc

    rendered = PLACEHOLDER_RE.sub(replace, text)
    if "{{" in rendered or "}}" in rendered:
        raise PackError(f"{label}: an unfilled placeholder remains")
    return rendered


def _is_rendered_skill_link(relative: Path, skill_names: set[str]) -> bool:
    parts = relative.parts
    return (
        len(parts) >= 4
        and parts[0] == "workflow"
        and parts[1] == "skills"
        and parts[2] in skill_names
    )


def _code_span_ranges(line: str) -> list[tuple[int, int]]:
    ranges: list[tuple[int, int]] = []
    index = 0
    while index < len(line):
        if line[index] != "`":
            index += 1
            continue
        closing = line.find("`", index + 1)
        if closing == -1:
            break
        ranges.append((index, closing + 1))
        index = closing + 1
    return ranges


def _line_links(line: str) -> list[re.Match[str]]:
    ranges = _code_span_ranges(line)
    return [
        match
        for match in LINK_RE.finditer(line)
        if not any(start <= match.start() < end for start, end in ranges)
    ]


def _iter_links(text: str):
    in_fence = False
    for line in text.splitlines():
        stripped = line.lstrip()
        if stripped.startswith(("```", "~~~")):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        for match in _line_links(line):
            yield match.group(1)


def _rewrite_link(
    match: re.Match[str],
    *,
    source_path: Path,
    rendered_path: Path,
    project_root: Path,
    resources: dict[Path, Path],
    skill_names: set[str],
    label: str,
) -> str | None:
    target = match.group(1)
    if target.startswith(
        ("http://", "https://", "mailto:", "skill://", "#")
    ) or "{{" in target:
        return None
    raw_path, separator, fragment = target.partition("#")
    if not raw_path:
        return None
    source_target = (source_path.parent / raw_path).resolve()
    try:
        relative = source_target.relative_to(REPO_ROOT)
    except ValueError as exc:
        raise PackError(f"{label}: link leaves the pack: {target}") from exc
    if _is_rendered_skill_link(relative, skill_names):
        return None
    if not source_target.is_file():
        raise PackError(
            f"{label}: linked pack file does not exist: {target}"
        )
    resources[relative] = source_target
    destination = (
        project_root / ".github" / "skills" / PACK_RESOURCES_DIRNAME / relative
    )
    new_path = Path(
        os.path.relpath(destination, rendered_path.parent)
    ).as_posix()
    return new_path + (separator + fragment if separator else "")


def _rewrite_links(
    text: str,
    *,
    source_path: Path,
    rendered_path: Path,
    project_root: Path,
    resources: dict[Path, Path],
    skill_names: set[str],
    label: str,
) -> str:
    output: list[str] = []
    in_fence = False
    for line in text.splitlines(keepends=True):
        stripped = line.lstrip()
        if stripped.startswith(("```", "~~~")):
            in_fence = not in_fence
            output.append(line)
            continue
        if in_fence:
            output.append(line)
            continue
        matches = _line_links(line)
        for match in reversed(matches):
            replacement = _rewrite_link(
                match,
                source_path=source_path,
                rendered_path=rendered_path,
                project_root=project_root,
                resources=resources,
                skill_names=skill_names,
                label=label,
            )
            if replacement is not None and replacement != match.group(1):
                line = (
                    line[: match.start(1)]
                    + replacement
                    + line[match.end(1) :]
                )
        output.append(line)
    return "".join(output)


def _collect_resources(resources: dict[Path, Path]) -> None:
    queue = list(resources.items())
    while queue:
        relative, source = queue.pop()
        if source.suffix.lower() not in (".md", ".markdown", ".txt"):
            continue
        try:
            text = source.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        for target in _iter_links(text):
            if target.startswith(
                ("http://", "https://", "mailto:", "skill://", "#")
            ) or "{{" in target:
                continue
            raw_path = target.partition("#")[0]
            if not raw_path:
                continue
            linked = (source.parent / raw_path).resolve()
            try:
                linked_relative = linked.relative_to(REPO_ROOT)
            except ValueError as exc:
                raise PackError(
                    f"{relative}: linked pack file leaves the repository: "
                    f"{target}"
                ) from exc
            if not linked.is_file():
                raise PackError(
                    f"{relative}: linked pack file does not exist: {target}"
                )
            if linked_relative not in resources:
                resources[linked_relative] = linked
                queue.append((linked_relative, linked))


def _rendered_skills(
    project: dict[str, object], project_root: Path
) -> tuple[dict[str, dict[str, bytes]], dict[Path, Path]]:
    skills = _skills(PACK_SKILLS)
    if not skills:
        raise PackError(f"pack skills are absent or empty: {PACK_SKILLS}")
    skill_names = set(skills)
    resources: dict[Path, Path] = {}
    rendered: dict[str, dict[str, bytes]] = {}
    for name, directory in skills.items():
        files: dict[str, bytes] = {}
        for path in sorted(directory.rglob("*")):
            if path.is_dir():
                continue
            relative = path.relative_to(directory).as_posix()
            data = path.read_bytes()
            try:
                text = data.decode("utf-8")
            except UnicodeDecodeError:
                files[relative] = data
                continue
            label = f"{name}/{relative}"
            text = _render_text(text, project, label)
            text = _rewrite_links(
                text,
                source_path=path,
                rendered_path=(
                    project_root / ".github" / "skills" / name / relative
                ),
                project_root=project_root,
                resources=resources,
                skill_names=skill_names,
                label=label,
            )
            files[relative] = text.encode("utf-8")
        rendered[name] = files
    _collect_resources(resources)
    return rendered, resources


def _sync_mirror(project_root: Path) -> list[str]:
    source = _skills(project_root / ".github" / "skills")
    mirror_root = project_root / ".agents" / "skills"
    mirror = _skills(mirror_root)
    extra = sorted(mirror.keys() - source.keys())
    if extra:
        raise PackError(
            "refusing to remove .agents-only skills: " + ", ".join(extra)
        )
    if not source:
        return ["no canonical skills to mirror"]
    mirror_root.mkdir(parents=True, exist_ok=True)
    for name, directory in source.items():
        destination = mirror_root / name
        if destination.is_symlink():
            raise PackError(f"refusing to replace symlinked mirror path: {destination}")
        if destination.exists():
            shutil.rmtree(destination)
        shutil.copytree(directory, destination, symlinks=True)
    return [f"mirrored {len(source)} skills into {mirror_root}"]


def _existing_mirror(project_root: Path) -> Path | None:
    for relative in (
        Path("_tools") / "scripts" / "check-codex-agent-config.py",
        Path("scripts") / "check-codex-agent-config.py",
    ):
        candidate = project_root / relative
        if candidate.is_file():
            return candidate
    return None


def _mirror_pack_resources(project_root: Path) -> list[str]:
    source = project_root / ".github" / "skills" / PACK_RESOURCES_DIRNAME
    destination = project_root / ".agents" / "skills" / PACK_RESOURCES_DIRNAME
    if not source.is_dir():
        if destination.is_symlink():
            raise PackError(
                f"refusing to replace symlinked mirror resources: {destination}"
            )
        if destination.exists():
            if not destination.is_dir():
                raise PackError(
                    f"refusing to replace non-directory mirror resources: "
                    f"{destination}"
                )
            shutil.rmtree(destination)
        return []
    if destination.is_symlink():
        raise PackError(
            f"refusing to replace symlinked mirror resources: {destination}"
        )
    if destination.exists():
        if not destination.is_dir():
            raise PackError(
                f"refusing to replace non-directory mirror resources: "
                f"{destination}"
            )
        shutil.rmtree(destination)
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(source, destination, symlinks=True)
    return [f"mirrored pack resources into {destination}"]


def run_mirror(project_root: Path) -> list[str]:
    """Run the project's own mirror when present, otherwise the byte mirror."""

    existing = _existing_mirror(project_root)
    if existing is None:
        messages = _sync_mirror(project_root)
        messages.extend(_mirror_pack_resources(project_root))
        return messages
    result = subprocess.run(
        [
            sys.executable,
            str(existing),
            "--workspace-root",
            str(project_root),
            "--sync-mirrors",
        ],
        cwd=project_root,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        raise PackError(f"project mirror failed: {detail}")
    messages = [f"project mirror passed: {existing.relative_to(project_root)}"]
    messages.extend(_mirror_pack_resources(project_root))
    return messages


def render(project_root: Path, config: Config) -> list[str]:
    rendered, resources = _rendered_skills(config.project, project_root)
    for name, files in rendered.items():
        destination = project_root / ".github" / "skills" / name
        if destination.is_symlink():
            raise PackError(f"refusing to replace symlinked skill: {destination}")
        if destination.exists() and not destination.is_dir():
            raise PackError(f"refusing to replace non-directory skill: {destination}")
        if destination.exists():
            shutil.rmtree(destination)
        for relative, data in files.items():
            path = destination / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)

    pack_root = project_root / ".github" / "skills" / PACK_RESOURCES_DIRNAME
    if pack_root.is_symlink():
        raise PackError(
            f"refusing to replace symlinked pack resources: {pack_root}"
        )
    if pack_root.exists() and not pack_root.is_dir():
        raise PackError(
            f"refusing to replace non-directory pack resources: {pack_root}"
        )
    if pack_root.exists():
        shutil.rmtree(pack_root)
    if resources:
        for relative, source in sorted(resources.items()):
            destination = pack_root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, destination)

    messages = [
        f"rendered {len(rendered)} skills into "
        f"{project_root / '.github' / 'skills'}"
    ]
    if resources:
        messages.append(
            f"installed {len(resources)} pack resource(s) into {pack_root}"
        )
    messages.extend(run_mirror(project_root))
    return messages


def _metadata_errors(path: Path, text: str) -> list[str]:
    try:
        metadata = _frontmatter(text)
    except PackError as exc:
        return [f"{path}: {exc}"]
    errors = []
    name = str(metadata.get("name", "")).strip()
    description = str(metadata.get("description", "")).strip()
    if not name:
        errors.append(f"{path}: missing name")
    elif name != path.parent.name:
        errors.append(
            f"{path}: name {name!r} differs from directory {path.parent.name!r}"
        )
    if not description:
        errors.append(f"{path}: missing description")
    elif len(description) > 1024:
        errors.append(
            f"{path}: description is {len(description)} chars, maximum is 1024"
        )
    return errors


def _rendered_placeholder_errors(project_root: Path) -> list[str]:
    errors: list[str] = []
    pack_skills = set(_skills(PACK_SKILLS))
    rendered = _skills(project_root / ".github" / "skills")
    for name in sorted(pack_skills & rendered.keys()):
        directory = rendered[name]
        for path in sorted(directory.rglob("*.md")):
            text = path.read_text(encoding="utf-8", errors="replace")
            if "{{" in text or "}}" in text:
                errors.append(
                    f"{path.relative_to(project_root)}: unfilled placeholder "
                    "in a rendered skill; run render"
                )
    return errors


def _mirror_errors(project_root: Path) -> list[str]:
    source = _skills(project_root / ".github" / "skills")
    if not source:
        return []
    mirror = _skills(project_root / ".agents" / "skills")
    errors = [
        f"missing .agents mirror skill: {name}"
        for name in sorted(source.keys() - mirror.keys())
    ]
    errors.extend(
        f".agents-only skill has no canonical source: {name}"
        for name in sorted(mirror.keys() - source.keys())
    )
    for name, directory in sorted(source.items()):
        skill = directory / "SKILL.md"
        errors.extend(_metadata_errors(skill, skill.read_text(encoding="utf-8")))
    for name in sorted(source.keys() & mirror.keys()):
        if _resources(source[name]) != _resources(mirror[name]):
            errors.append(
                f"skill mirror differs byte-for-byte, including resources: {name}"
            )

    claude_root = project_root / ".claude" / "skills"
    if claude_root.is_dir():
        for name, directory in sorted(source.items()):
            link = claude_root / name
            if not link.is_symlink():
                errors.append(f"Claude native catalog is missing skill: {name}")
            elif link.resolve() != directory.resolve():
                errors.append(
                    f"Claude native skill must point to canonical source: {name}"
                )

    canonical_pack = project_root / ".github" / "skills" / PACK_RESOURCES_DIRNAME
    mirror_pack = project_root / ".agents" / "skills" / PACK_RESOURCES_DIRNAME
    if canonical_pack.is_dir():
        if not mirror_pack.is_dir():
            errors.append("missing .agents mirror for rendered pack resources")
        elif _resources(canonical_pack) != _resources(mirror_pack):
            errors.append(
                "rendered pack resources differ from the .agents mirror"
            )
    return errors


def _budget_errors(project_root: Path) -> list[str]:
    errors: list[str] = []
    sizes: dict[str, int] = {}
    for relative, limit in BUDGETS.items():
        path = project_root / relative
        size = path.stat().st_size if path.is_file() else 0
        sizes[relative] = size
        if size > limit:
            errors.append(f"{relative}: {size} bytes exceeds {limit}")
    config_path = project_root / "opencode.json"
    if config_path.is_file():
        try:
            config = json.loads(config_path.read_text(encoding="utf-8"))
            inputs = {project_root / "AGENTS.md"}
            for item in config.get("instructions", []):
                path = (project_root / item).resolve()
                if not path.is_file():
                    errors.append(f"OpenCode instruction path is missing: {item}")
                else:
                    inputs.add(path)
            total = sum(path.stat().st_size for path in inputs if path.is_file())
            if total > 16384:
                errors.append(
                    f"OpenCode unconditional instructions are {total} bytes, "
                    "maximum 16384"
                )
        except (json.JSONDecodeError, TypeError) as exc:
            errors.append(f"opencode.json: {exc}")
    if sizes["AGENTS.md"] + sizes["CLAUDE.md"] > 16384:
        errors.append("DeepSeek dual entry inputs exceed 16384 bytes")
    cursor_inputs = sizes["AGENTS.md"] + sizes["CLAUDE.md"] + sizes["AGENTS.md"]
    rules_root = project_root / ".github" / "rules"
    if rules_root.is_dir():
        for rule in rules_root.glob("*.mdc"):
            text = rule.read_text(encoding="utf-8")
            try:
                metadata = _frontmatter(text)
            except PackError:
                continue
            if metadata.get("alwaysApply") == "true":
                cursor_inputs += rule.stat().st_size
                cursor_inputs += (
                    len(re.findall(r"(?m)^\s*@AGENTS\.md\s*$", text))
                    * sizes["AGENTS.md"]
                )
                cursor_inputs += (
                    len(re.findall(r"(?m)^\s*@CLAUDE\.md\s*$", text))
                    * sizes["CLAUDE.md"]
                )
    if cursor_inputs > 16384:
        errors.append(
            f"Cursor unconditional instructions are {cursor_inputs} bytes, "
            "maximum 16384"
        )
    return errors


def _routing_errors(project_root: Path) -> list[str]:
    errors: list[str] = []
    agents = project_root / "AGENTS.md"
    claude = project_root / "CLAUDE.md"
    if agents.is_file() and claude.is_file():
        agents_text = agents.read_text(encoding="utf-8")
        claude_text = claude.read_text(encoding="utf-8")
        imports = re.findall(r"(?m)^\s*@AGENTS\.md\s*$", claude_text)
        if len(imports) != 1:
            errors.append(
                f"CLAUDE.md must import AGENTS.md exactly once; found {len(imports)}"
            )
        if re.search(r"(?im)^\s*@CLAUDE\.md\s*$", agents_text) or re.search(
            r"(?i)read (?:the )?CLAUDE\.md", agents_text
        ):
            errors.append(
                "AGENTS.md routes back to CLAUDE.md and creates an adapter cycle"
            )
        if agents_text.count("@AGENTS.md"):
            errors.append("AGENTS.md contains a duplicate self-import")
    config_path = project_root / "opencode.json"
    if config_path.is_file():
        try:
            config = json.loads(config_path.read_text(encoding="utf-8"))
            for instruction in config.get("instructions", []):
                if (
                    "copilot-instructions.md" in instruction
                    or PurePosixPath(instruction).name == "CLAUDE.md"
                ):
                    errors.append(
                        "OpenCode instruction chain includes a redundant "
                        f"compatibility entry: {instruction}"
                    )
        except (json.JSONDecodeError, TypeError) as exc:
            errors.append(f"opencode.json: {exc}")
    rules_root = project_root / ".github" / "rules"
    if rules_root.is_dir():
        for rule in sorted(rules_root.glob("*.mdc")):
            try:
                metadata = _frontmatter(rule.read_text(encoding="utf-8"))
            except PackError:
                continue
            if metadata.get("alwaysApply") == "true":
                errors.append(
                    f"{rule.relative_to(project_root)} duplicates the native "
                    "Cursor AGENTS.md/CLAUDE.md entries"
                )
    copilot = project_root / ".github" / "copilot-instructions.md"
    if copilot.is_file() and re.search(
        r"(?i)always read.*CLAUDE\.md", copilot.read_text(encoding="utf-8")
    ):
        errors.append(
            "Copilot entry reinstates a compulsory CLAUDE.md bootstrap"
        )
    return errors


def _slug(text: str) -> str:
    text = re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", text)
    text = re.sub(r"\*\*|__|\*|`", "", text).strip().lower()
    text = re.sub(r"[^\w\- ]", "", text)
    return text.replace(" ", "-")


def _anchors(path: Path, cache: dict[Path, set[str]]) -> set[str]:
    if path not in cache:
        text = path.read_text(encoding="utf-8", errors="replace")
        found = set(re.findall(r'<a id="([^"]+)"', text))
        in_fence = False
        for line in text.splitlines():
            if line.lstrip().startswith(("```", "~~~")):
                in_fence = not in_fence
                continue
            if in_fence:
                continue
            match = HEADING_RE.match(line)
            if match:
                found.add(_slug(match.group(1)))
        cache[path] = found
    return cache[path]


def _resource_roots(project: dict[str, object]) -> tuple[dict[str, str], list[str]]:
    raw = project.get("resource_roots")
    if not isinstance(raw, dict) or not raw:
        return {}, ["project.resource_roots must be a nonempty mapping"]
    roots: dict[str, str] = {}
    errors: list[str] = []
    for key, value in raw.items():
        if (
            not isinstance(key, str)
            or not key
            or not isinstance(value, str)
            or not value
        ):
            errors.append(f"project.resource_roots has an invalid entry: {key!r}")
        else:
            roots[key] = value
    return roots, errors


def _documentation_roots(
    project_root: Path,
    project: dict[str, object],
    overrides: dict[str, Path],
) -> tuple[list[tuple[str, Path]], list[str], list[str]]:
    errors: list[str] = []
    notes: list[str] = []
    roots, root_errors = _resource_roots(project)
    errors.extend(root_errors)
    forge = project.get("forge")
    repositories = forge.get("repositories") if isinstance(forge, dict) else None
    if isinstance(repositories, list):
        listed = {str(repository) for repository in repositories}
        for key, repository in roots.items():
            if repository not in listed:
                errors.append(
                    f"project.resource_roots.{key} does not name a "
                    f"project.forge.repositories entry: {repository}"
                )
    for key in overrides:
        if key not in roots:
            errors.append(f"undeclared resource root override: {key}")
    if not overrides and len(roots) == 1:
        overrides = {next(iter(roots)): project_root}
    for key in sorted(roots):
        if key not in overrides:
            notes.append(
                f"resource root {key} was not checked; "
                f"pass --resource-root {key}=PATH"
            )
    for key, path in overrides.items():
        if key in roots and not path.is_dir():
            errors.append(f"resource root {key} is not a directory: {path}")

    entries: list[tuple[str, str, bool]] = []
    documentation = project.get("documentation_roots")
    if isinstance(documentation, list):
        entries.extend(
            (str(value), "documentation_roots", True) for value in documentation
        )
    else:
        errors.append("project.documentation_roots must be a sequence")
    authorities = project.get("decision_authorities")
    if isinstance(authorities, dict) and authorities.get("decision_records_root"):
        entries.append(
            (str(authorities["decision_records_root"]), "decision_records_root", False)
        )
    for key in ("gap_ledger", "glossary"):
        if project.get(key):
            entries.append((str(project[key]), key, False))

    found: list[tuple[str, Path]] = []
    for value, label, required in entries:
        pure = PurePosixPath(value)
        if not value or pure.is_absolute() or ".." in pure.parts:
            errors.append(f"{label} path escapes the project: {value!r}")
            continue
        parts = pure.parts
        if len(parts) < 2:
            errors.append(
                f"{label} path must start with a declared resource root: {value}"
            )
            continue
        if parts[0] not in roots:
            errors.append(
                f"{label} path uses an undeclared resource root "
                f"{parts[0]!r}: {value}"
            )
            continue
        if parts[0] not in overrides:
            if required:
                errors.append(
                    f"{label} path cannot be checked without "
                    f"--resource-root {parts[0]}=PATH: {value}"
                )
            continue
        path = overrides[parts[0]].joinpath(*parts[1:])
        if not path.exists():
            if required:
                errors.append(f"{label} path does not exist: {value}")
            continue
        if required and path.is_file():
            errors.append(f"{label} path is not a directory: {value}")
            continue
        found.append((parts[0], path))

    result: list[tuple[str, Path]] = []
    for key, path in found:
        if any(item[1] == path for item in result):
            continue
        if any(
            other != path and other.is_dir() and path.is_relative_to(other)
            for _, other in found
        ):
            continue
        result.append((key, path))
    return result, errors, notes


def _link_files(
    project_root: Path, roots: list[tuple[str, Path]]
) -> list[Path]:
    files: list[Path] = []
    for _, root in roots:
        if root.is_file() and root.suffix == ".md":
            files.append(root)
        elif root.is_dir():
            files.extend(
                path
                for path in sorted(root.rglob("*.md"))
                if "node_modules" not in path.parts
            )
    for relative in ("AGENTS.md", "CLAUDE.md", "README.md"):
        path = project_root / relative
        if path.is_file():
            files.append(path)
    for relative in (
        ".github/skills",
        ".github/instructions",
        ".github/prompts",
    ):
        root = project_root / relative
        if root.is_dir():
            files.extend(
                path
                for path in sorted(root.rglob("*.md"))
                if "node_modules" not in path.parts
            )
    return sorted({path.resolve() for path in files})


def _link_prefixes(
    roots: dict[str, str], overrides: dict[str, Path]
) -> dict[str, Path]:
    prefixes: dict[str, Path] = {}
    for key, repository in roots.items():
        if key not in overrides:
            continue
        prefixes[key] = overrides[key]
        tail = repository.rsplit("/", 1)[-1]
        if tail:
            prefixes.setdefault(tail, overrides[key])
    return prefixes


def _link_errors(
    files: list[Path],
    project_root: Path,
    prefixes: dict[str, Path],
) -> list[str]:
    errors: list[str] = []
    cache: dict[Path, set[str]] = {}
    skill_tree = (project_root / ".github" / "skills").resolve()
    for path in files:
        in_fence = False
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError as exc:
            errors.append(f"{path}: {exc}")
            continue
        for number, line in enumerate(text.splitlines(), 1):
            if line.lstrip().startswith(("```", "~~~")):
                in_fence = not in_fence
                continue
            if in_fence:
                continue
            for match in LINK_RE.finditer(INLINE_CODE_RE.sub("", line)):
                target = match.group(1)
                if target.startswith(("http://", "https://", "mailto:", "skill://", "#")):
                    continue
                if "{{" in target:
                    continue
                raw_path, _, fragment = target.partition("#")
                if not raw_path:
                    resolved = path
                else:
                    first, separator, rest = raw_path.partition("/")
                    if separator and first in prefixes:
                        resolved = (
                            prefixes[first] / rest
                            if rest
                            else prefixes[first]
                        ).resolve()
                    else:
                        resolved = (path.parent / raw_path).resolve()
                try:
                    relative = path.relative_to(project_root)
                except ValueError:
                    relative = path
                under_skill_tree = skill_tree in path.parents
                if under_skill_tree and (
                    resolved != skill_tree
                    and skill_tree not in resolved.parents
                ):
                    errors.append(
                        f"{relative}:{number}: relative link leaves the "
                        f"skill tree: {target}"
                    )
                    continue
                if not resolved.exists():
                    errors.append(
                        f"{relative}:{number}: broken link: {target}"
                    )
                    continue
                if (
                    fragment
                    and resolved.suffix == ".md"
                    and not LINE_ANCHOR_RE.match(fragment)
                    and fragment not in _anchors(resolved, cache)
                ):
                    errors.append(
                        f"{relative}:{number}: broken anchor: {target}"
                    )
    return errors


def _router_candidates(root: Path) -> list[Path]:
    candidates = []
    for path in sorted(root.rglob("*.md")):
        if "node_modules" in path.parts:
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        if (
            re.search(
                r"(?im)^\|\s*Document\s*\|\s*Summary\s*\|\s*Status\s*\|"
                r"\s*When to load\s*\|",
                text,
            )
            or re.search(r"(?im)^#\s+.*documentation index", text)
        ):
            candidates.append(path)
    return candidates


def _router_links(router: Path) -> dict[Path, int]:
    counts: dict[Path, int] = {}
    for line in router.read_text(encoding="utf-8", errors="replace").splitlines():
        if not line.strip().startswith("|"):
            continue
        for match in LINK_RE.finditer(INLINE_CODE_RE.sub("", line)):
            raw = match.group(1).partition("#")[0]
            if not raw or raw.startswith(
                ("http://", "https://", "mailto:", "skill://")
            ):
                continue
            target = (router.parent / raw).resolve()
            counts[target] = counts.get(target, 0) + 1
    return counts


def _routed_once_errors(
    project_root: Path, roots: list[tuple[str, Path]]
) -> list[str]:
    errors: list[str] = []
    for key, root in roots:
        if not root.is_dir():
            continue
        documents = [
            path.resolve()
            for path in sorted(root.rglob("*.md"))
            if "node_modules" not in path.parts
        ]
        if not documents:
            continue
        routers = [path.resolve() for path in _router_candidates(root)]
        if not routers:
            errors.append(
                f"{key}: documentation index/router missing; every document "
                "must be routed once"
            )
            continue
        by_directory: dict[Path, Path] = {}
        ambiguous = False
        for router in routers:
            if router.parent in by_directory:
                errors.append(
                    "more than one documentation router in "
                    f"{router.parent.relative_to(project_root)}: "
                    f"{by_directory[router.parent].name}, {router.name}"
                )
                ambiguous = True
            else:
                by_directory[router.parent] = router
        if ambiguous:
            continue
        root_router = by_directory.get(root.resolve())
        if root_router is None:
            errors.append(
                f"{key}: documentation index/router missing at the root; "
                "every document must be routed once"
            )
            continue
        links = {router: _router_links(router) for router in routers}
        for document in documents:
            if document == root_router:
                continue
            candidates = [
                router
                for router in routers
                if router != document
                and (
                    router.parent == document.parent
                    or router.parent in document.parent.parents
                )
            ]
            if not candidates:
                errors.append(
                    "document has no owning index: "
                    f"{key}/{document.relative_to(root).as_posix()}"
                )
                continue
            owner = max(candidates, key=lambda router: len(router.parent.parts))
            count = links[owner].get(document, 0)
            relative = f"{key}/{document.relative_to(root).as_posix()}"
            if count == 0:
                errors.append(
                    f"unrouted document: {relative} "
                    f"(owning index {key}/{owner.relative_to(root).as_posix()})"
                )
            elif count > 1:
                errors.append(f"routed more than once: {relative} ({count})")
    return errors


def _parse_hold(text: str) -> dict[str, object] | None:
    matches = list(re.finditer(r"(?m)^hold:\s*$", text))
    if not matches:
        return None
    if len(matches) > 1:
        raise PackError("more than one hold: block")
    lines: list[str] = []
    for line in text[matches[0].end() :].splitlines()[1:]:
        if line.strip() and not line.startswith((" ", "\t")):
            break
        lines.append(line)
    block = "hold:\n" + "\n".join(lines)
    try:
        parsed = load_yaml_text(block)
    except YamlError as exc:
        raise PackError(f"invalid hold: block: {exc}") from exc
    if not isinstance(parsed, dict) or not isinstance(parsed.get("hold"), dict):
        raise PackError("invalid hold: block")
    return parsed["hold"]


def _hold_errors(
    project_root: Path, path: Path, text: str, today: date
) -> list[str]:
    try:
        relative: Path | str = path.relative_to(project_root)
    except ValueError:
        relative = path
    try:
        metadata = _frontmatter(text)
    except PackError as exc:
        status_line = re.search(r"(?m)^status:[ \t]*(.*)$", text)
        status = (
            status_line.group(1).strip().strip("'\"").casefold()
            if status_line
            else ""
        )
        return [f"{relative}: {exc}"] if status == "on hold" else []
    status = str(metadata.get("status", "")).strip().strip("'\"").casefold()
    if status != "on hold":
        return []
    try:
        hold = _parse_hold(text)
    except PackError as exc:
        return [f"{relative}: {exc}"]
    if hold is None:
        return [f"{relative}: status On Hold has no hold: block"]
    errors = []
    reason = str(hold.get("reason", "")).strip()
    owner = str(hold.get("owner", "")).strip()
    lifts_when = str(hold.get("lifts_when", "")).strip()
    review_by = str(hold.get("review_by", "")).strip()
    if reason not in HOLD_REASONS:
        errors.append(
            f"{relative}: hold reason {reason!r} is not one of "
            + ", ".join(sorted(HOLD_REASONS))
        )
    for name, value in (
        ("owner", owner),
        ("lifts_when", lifts_when),
        ("review_by", review_by),
    ):
        if not value:
            errors.append(f"{relative}: hold field {name} is required")
    if review_by:
        try:
            reviewed = date.fromisoformat(review_by)
        except ValueError:
            errors.append(f"{relative}: hold review_by is not an ISO date: {review_by}")
        else:
            if reviewed < today:
                errors.append(
                    f"{relative}: expired hold: review_by {review_by} has passed"
                )
            elif reviewed > today + timedelta(days=30):
                errors.append(
                    f"{relative}: hold review_by {review_by} is more than 30 days ahead"
                )
    return errors


def _markdown_parts(text: str) -> tuple[list[tuple[str, str, int]], str]:
    blocks: list[tuple[str, str, int]] = []
    visible: list[str] = []
    active: tuple[str, str, int] | None = None
    body: list[str] = []
    for number, line in enumerate(text.splitlines(), 1):
        if active is not None:
            marker, info, start = active
            closing = re.fullmatch(
                r" {0,3}" + re.escape(marker[0]) + "{" + str(len(marker)) + r",}\s*",
                line,
            )
            visible.append("")
            if closing:
                blocks.append((info, "\n".join(body), start))
                active, body = None, []
            else:
                body.append(line)
            continue
        match = PLAN_FENCE_RE.match(line)
        if match:
            active = (match.group(1), match.group(2).strip(), number)
            visible.append("")
        else:
            visible.append(line)
    if active is not None:
        raise PackError("unclosed Markdown code fence")
    return blocks, "\n".join(visible)


def _plan_section(visible: str) -> tuple[int, int]:
    lines = visible.splitlines()
    starts = [index for index, line in enumerate(lines) if line == PLAN_SECTION]
    if len(starts) != 1:
        raise PackError(f"expected exactly one '{PLAN_SECTION}' heading")
    start = starts[0]
    end = next(
        (
            index
            for index in range(start + 1, len(lines))
            if re.match(r"^#{1,3} ", lines[index])
        ),
        len(lines),
    )
    return start + 1, end + 1


def _declared_tasks(visible: str) -> list[str]:
    if "## 2. Implementation Steps" not in visible:
        raise PackError("missing '## 2. Implementation Steps'")
    section = visible.split("## 2. Implementation Steps", 1)[1].split(
        PLAN_SECTION, 1
    )[0]
    ids: list[str] = []
    header: list[str] = []
    for line in section.splitlines():
        if not line.startswith("|"):
            header = []
            continue
        cells = [
            cell.strip()
            for cell in re.split(r"(?<!\\)\|", line.strip().strip("|"))
        ]
        if cells[0] == "Task":
            if "Closure condition" not in cells:
                raise PackError("Task table requires a Closure condition column")
            header = cells
        elif header:
            if len(cells) != len(header):
                raise PackError(
                    f"Task table row {cells[0]}: expected {len(header)} cells, "
                    f"found {len(cells)}; escape literal pipes as \\|"
                )
            if all(re.fullmatch(r":?-+:?", cell) for cell in cells):
                continue
            task_id = cells[0].strip("`*")
            if TASK_ID_RE.fullmatch(task_id) is None:
                raise PackError(
                    f"Task cell must contain one local ID: {cells[0]}"
                )
            if not cells[header.index("Closure condition")]:
                raise PackError(
                    f"{task_id}: Closure condition must not be empty"
                )
            ids.append(task_id)
    if not ids:
        raise PackError("no delivery Task rows before §2.5")
    duplicates = sorted({key for key in ids if ids.count(key) > 1})
    if duplicates:
        raise PackError(f"duplicate Task rows: {duplicates}")
    return ids


def _single_line(value: object, where: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise PackError(f"{where}: expected nonempty string")
    if value != value.strip() or any(ord(character) < 32 for character in value):
        raise PackError(f"{where}: expected a trimmed single-line string")
    return value


def _exact_keys(value: object, keys: set[str], where: str) -> dict:
    if not isinstance(value, dict):
        raise PackError(f"{where}: expected object")
    if set(value) != keys:
        raise PackError(
            f"{where}: expected exactly {', '.join(sorted(keys))}"
        )
    return value


def _unique_object(pairs: list[tuple[str, object]]) -> dict:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise PackError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def _validate_binding(
    binding: dict, where: str, key_pattern: re.Pattern[str]
) -> None:
    tracker = binding["jira"]
    kontor = binding["kontor_id"]
    if tracker is not None:
        if not isinstance(tracker, str) or key_pattern.fullmatch(tracker) is None:
            raise PackError(
                f"{where}.jira: expected a key matching the project key_pattern"
            )
    if kontor is not None:
        _single_line(kontor, f"{where}.kontor_id")


def _validate_graph(
    graph: object,
    task_rows: list[str],
    key_pattern: re.Pattern[str],
    project_key: str,
) -> dict[str, int]:
    graph = _exact_keys(
        graph, {"schema_version", "epic", "tasks", "external_fences"}, "graph"
    )
    if type(graph["schema_version"]) is not int or graph["schema_version"] != 1:
        raise PackError("unsupported schema_version")
    epic = _exact_keys(graph["epic"], {"jira", "kontor_id"}, "epic")
    _validate_binding(epic, "epic", key_pattern)
    tasks = graph["tasks"]
    if not isinstance(tasks, list) or not 0 < len(tasks) <= 10000:
        raise PackError("tasks: expected 1–10000 task objects")
    nodes: dict[str, dict] = {}
    for task in tasks:
        task = _exact_keys(
            task,
            {"id", "title", "phase", "jira", "kontor_id", "depends_on"},
            "task",
        )
        task_id = _single_line(task["id"], "task.id")
        if TASK_ID_RE.fullmatch(task_id) is None:
            raise PackError(
                "task.id: expected a local code such as TASK-001"
            )
        if task_id.startswith(project_key + "-"):
            raise PackError(
                "task.id must be local; put the tracker key in jira"
            )
        if task_id in nodes:
            raise PackError(f"duplicate task ID: {task_id}")
        nodes[task_id] = task
        _single_line(task["title"], f"{task_id}.title")
        _single_line(task["phase"], f"{task_id}.phase")
        _validate_binding(task, task_id, key_pattern)
        dependencies = task["depends_on"]
        if not isinstance(dependencies, list) or not all(
            isinstance(dependency, str) for dependency in dependencies
        ):
            raise PackError(f"{task_id}.depends_on: expected array of local IDs")
        if len(dependencies) != len(set(dependencies)):
            raise PackError(f"{task_id}: duplicate dependency")
        if task_id in dependencies:
            raise PackError(f"{task_id}: self-dependency")
    if set(nodes) != set(task_rows):
        raise PackError(
            "graph/task-table mismatch: "
            f"missing={sorted(set(task_rows) - set(nodes))}; "
            f"extra={sorted(set(nodes) - set(task_rows))}"
        )
    for field in ("jira", "kontor_id"):
        values = [
            item[field]
            for item in [epic, *tasks]
            if item[field] is not None
        ]
        if len(values) != len(set(values)):
            raise PackError(f"duplicate {field} binding across epic/tasks")
    children: dict[str, list[str]] = {key: [] for key in nodes}
    remaining: dict[str, int] = {}
    for key, task in nodes.items():
        remaining[key] = len(task["depends_on"])
        for dependency in task["depends_on"]:
            if dependency not in nodes:
                raise PackError(
                    f"{key}: unknown sibling dependency {dependency}; "
                    "record external prerequisites in external_fences"
                )
            children[dependency].append(key)
    waves = {key: 0 for key in nodes if remaining[key] == 0}
    queue = sorted(waves)
    for key in queue:
        for child in sorted(children[key]):
            waves[child] = max(waves.get(child, 0), waves[key] + 1)
            remaining[child] -= 1
            if remaining[child] == 0:
                queue.append(child)
    blocked = sorted(key for key, count in remaining.items() if count)
    if blocked:
        raise PackError(f"cycle blocks these tasks: {blocked}")
    fences = graph["external_fences"]
    if not isinstance(fences, list):
        raise PackError("external_fences: expected array")
    seen: set[tuple[str, str]] = set()
    for fence in fences:
        fence = _exact_keys(
            fence,
            {"task", "prerequisite", "owner", "lift_condition"},
            "external fence",
        )
        for field, value in fence.items():
            _single_line(value, f"external fence.{field}")
        if fence["task"] not in nodes:
            raise PackError("external fence: unknown task")
        if fence["prerequisite"] in nodes:
            raise PackError("sibling prerequisite belongs in depends_on")
        pair = (fence["task"], fence["prerequisite"])
        if pair in seen:
            raise PackError("duplicate external fence")
        seen.add(pair)
    return waves


def _derived_view(graph: dict, waves: dict[str, int]) -> str:
    tasks = sorted(graph["tasks"], key=lambda task: task["id"])
    lines = [PLAN_START, "", "```text"]
    for wave in range(max(waves.values()) + 1):
        ids = [task["id"] for task in tasks if waves[task["id"]] == wave]
        lines.append(f"wave {wave}   {'  '.join(ids)}   ({len(ids)} nodes)")
    lines += ["", "Dependencies (prerequisite -> dependent):"]
    edges = sorted(
        (dependency, task["id"])
        for task in tasks
        for dependency in task["depends_on"]
    )
    lines += [f"  {start} -> {end}" for start, end in edges] or ["  none"]
    lines += ["", f"{graph['epic']['jira'] or 'unbound epic'}"]
    if graph["epic"]["kontor_id"]:
        lines[-1] += f" · {graph['epic']['kontor_id']}"
    for task in tasks:
        binding = task["jira"] or "unbound"
        if task["kontor_id"]:
            binding += f" · {task['kontor_id']}"
        lines.append(
            f"  {task['id']} · {binding} · {task['phase']} · "
            f"wave {waves[task['id']]} · {task['title']}"
        )
    lines += [
        "```",
        "",
        "Structural waves only; external fences, gates and capacity still "
        "govern admission.",
        "",
        PLAN_END,
    ]
    return "\n".join(lines)


def _load_plan(
    text: str,
    key_pattern: re.Pattern[str],
    project_key: str,
    check_derived: bool = True,
) -> tuple[dict, dict[str, int]]:
    blocks, visible = _markdown_parts(text)
    start, end = _plan_section(visible)
    graphs = [
        (body, line)
        for info, body, line in blocks
        if info == "plan-graph"
    ]
    if len(graphs) != 1:
        raise PackError("expected exactly one plan-graph code fence")
    body, line = graphs[0]
    if not start < line < end:
        raise PackError("plan-graph must be inside §2.5")
    try:
        graph = json.loads(
            body,
            object_pairs_hook=_unique_object,
            parse_constant=lambda value: (_ for _ in ()).throw(
                PackError(f"non-JSON constant: {value}")
            ),
        )
    except json.JSONDecodeError as exc:
        raise PackError(
            f"plan-graph line {line + exc.lineno}: {exc.msg}"
        ) from exc
    frontmatter = re.match(
        r"\A---\r?\n(.*?)\r?\n---(?:\r?\n|$)", text, re.DOTALL
    )
    if frontmatter is None:
        raise PackError("missing YAML frontmatter")
    modes = re.findall(
        r"^orchestration:[ \t]*([^\n]*)$", frontmatter.group(1), re.MULTILINE
    )
    modes = [mode.split("#", 1)[0].strip().strip("'\"") for mode in modes]
    if len(modes) != 1 or modes[0] not in ("paseo", "kontor"):
        raise PackError("expected one orchestration: paseo or kontor field")
    waves = _validate_graph(
        graph, _declared_tasks(visible), key_pattern, project_key
    )
    if check_derived:
        if text.count(PLAN_START) != 1 or text.count(PLAN_END) != 1:
            raise PackError("expected one derived view; run sync")
        actual = text[text.index(PLAN_START) : text.index(PLAN_END) + len(PLAN_END)]
        if actual != _derived_view(graph, waves):
            raise PackError("derived graph/binding view is stale; run sync")
        visible_lines = visible.splitlines()
        section_lines = visible_lines[start : end - 1]
        if PLAN_START not in section_lines or PLAN_END not in section_lines:
            raise PackError("derived view must be inside §2.5")
    return graph, waves


def _has_graph_marker(text: str) -> bool:
    return bool(re.search(r"(?m)^ {0,3}(?:`{3,}|~{3,})plan-graph\b", text))


def _is_historical(text: str) -> bool:
    try:
        metadata = _frontmatter(text)
    except PackError:
        return False
    status = metadata.get("status")
    if not isinstance(status, str):
        return False
    return status.strip().strip("'\"").casefold() in {
        "completed",
        "done",
        "released",
        "cancelled",
        "canceled",
        "superseded",
        "archived",
        "historical",
    }


def _is_plan(path: Path, text: str) -> bool:
    lowered = path.as_posix().lower()
    if (
        not lowered.endswith(".md")
        or "/skills/" in lowered
        or path.name.lower() in ("readme.md", "index.md")
    ):
        return False
    return (
        bool(re.search(r"(?:^|/)plan(?:s)?/|(?:^|/)plan[.-]|-plan-", lowered))
        or _has_graph_marker(text)
        or bool(
            re.search(r"^goal:", text, re.MULTILINE)
            and "## 2. Implementation Steps" in text
        )
    )


def _plan_files(
    project_root: Path, roots: list[tuple[str, Path]]
) -> list[Path]:
    files: set[Path] = set()
    for _, root in roots:
        if root.is_dir():
            files.update(
                path
                for path in root.rglob("*.md")
                if "node_modules" not in path.parts
            )
    for name in ("plan", "plans"):
        root = project_root / name
        if root.is_dir():
            files.update(root.rglob("*.md"))
    return sorted(files)


def _validator_path() -> Path | None:
    configured = os.environ.get("ASMA_PLAN_GRAPH_VALIDATOR")
    if not configured:
        return None
    path = Path(configured).expanduser()
    if not path.is_file():
        raise PackError(
            "ASMA_PLAN_GRAPH_VALIDATOR is set but does not name a file: "
            f"{path}; refusing to skip plan-graph validation"
        )
    return path


def _changed_plan_paths(project_root: Path, base: str | None) -> set[Path]:
    if not base:
        raise PackError("--plan-graphs changed requires --base REV")
    result = subprocess.run(
        [
            "git",
            "-C",
            str(project_root),
            "diff",
            "--name-only",
            "--diff-filter=ACMR",
            "-z",
            base,
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        raise PackError(f"cannot read changed plans from {base}: {detail}")
    return {
        (project_root / relative).resolve()
        for relative in result.stdout.split("\0")
        if relative
    }


def _plan_graph_errors(
    project_root: Path,
    config: Config,
    roots: list[Path],
    mode: str = "all",
    base: str | None = None,
) -> list[str]:
    errors: list[str] = []
    validator = _validator_path()
    changed = _changed_plan_paths(project_root, base) if mode == "changed" else set()
    for path in _plan_files(project_root, roots):
        text = path.read_text(encoding="utf-8", errors="replace")
        if not _is_plan(path, text):
            continue
        relative = path.relative_to(project_root)
        marker = _has_graph_marker(text)
        required = mode == "all" or (mode == "changed" and path.resolve() in changed)
        if not marker:
            if required and not _is_historical(text):
                errors.append(
                    f"{relative}: current plan requires a canonical "
                    "plan-graph block in §2.5"
                )
            continue
        try:
            _load_plan(text, config.key_pattern, str(config.project["key"]))
        except PackError as exc:
            errors.append(f"{relative}: {exc}")
            continue
        if validator is not None:
            child_env = os.environ.copy()
            child_env["KONTOR_CONFIG_DIR"] = str(config.folder)
            result = subprocess.run(
                [sys.executable, str(validator), "validate", str(path)],
                env=child_env,
                capture_output=True,
                text=True,
                check=False,
            )
            if result.returncode != 0:
                detail = (result.stderr or result.stdout).strip()
                errors.append(
                    f"{relative}: canonical plan-graph validator failed: {detail}"
                )
    return errors


def _public_example_errors(files: list[Path], project_root: Path) -> list[str]:
    errors: list[str] = []
    for path in files:
        try:
            relative = path.relative_to(project_root).as_posix()
        except ValueError:
            relative = path.as_posix()
        try:
            content = path.read_bytes()
        except OSError:
            continue
        for refusal in file_refusals(relative, content):
            errors.append(refusal.render())
    return errors


def _placeholder_errors(config: Config) -> list[str]:
    errors: list[str] = []
    for name, directory in _skills(PACK_SKILLS).items():
        for path in sorted(directory.rglob("*.md")):
            relative = path.relative_to(PACK_SKILLS)
            text = path.read_text(encoding="utf-8")
            for match in PLACEHOLDER_RE.finditer(text):
                expression = match.group(1).strip()
                if not PROJECT_PLACEHOLDER_RE.fullmatch(expression):
                    errors.append(
                        f"{relative}: unfilled or unsupported placeholder "
                        f"{match.group(0).strip()}"
                    )
                    continue
                dotted = expression[len("project.") :]
                try:
                    value = _lookup(config.project, dotted)
                except KeyError:
                    errors.append(f"{relative}: missing parameter {expression}")
                else:
                    if value is None:
                        errors.append(
                            f"{relative}: parameter {expression} is unfilled"
                        )
    return errors


def check(
    project_root: Path,
    config: Config,
    today: date | None = None,
    *,
    resource_roots: dict[str, Path] | None = None,
    plan_graphs: str = "all",
    plan_base: str | None = None,
    links: bool = True,
    routed_once: bool = True,
    public_lint: bool = True,
    notes: list[str] | None = None,
) -> list[str]:
    today = today or date.today()
    errors = _placeholder_errors(config)
    errors.extend(_rendered_placeholder_errors(project_root))
    errors.extend(_mirror_errors(project_root))
    errors.extend(_budget_errors(project_root))
    errors.extend(_routing_errors(project_root))
    declared, _ = _resource_roots(config.project)
    effective = dict(resource_roots or {})
    if not effective and len(declared) == 1:
        effective = {next(iter(declared)): project_root}
    roots, root_errors, root_notes = _documentation_roots(
        project_root, config.project, effective
    )
    errors.extend(root_errors)
    if notes is not None:
        notes.extend(root_notes)
    files = _link_files(project_root, roots)
    if links:
        errors.extend(
            _link_errors(
                files, project_root, _link_prefixes(declared, effective)
            )
        )
    if routed_once:
        errors.extend(_routed_once_errors(project_root, roots))
    plan_files = _plan_files(project_root, roots)
    for path in plan_files:
        text = path.read_text(encoding="utf-8", errors="replace")
        if _is_plan(path, text):
            errors.extend(_hold_errors(project_root, path, text, today))
    errors.extend(
        _plan_graph_errors(project_root, config, roots, plan_graphs, plan_base)
    )
    lint_files = sorted({*files, *plan_files})
    for name, directory in _skills(project_root / ".github" / "skills").items():
        lint_files.extend(sorted(directory.rglob("*.md")))
    if public_lint:
        errors.extend(
            _public_example_errors(sorted(set(lint_files)), project_root)
        )
    return errors


def sync_plan(
    text: str, key_pattern: re.Pattern[str], project_key: str
) -> str:
    graph, waves = _load_plan(
        text, key_pattern, project_key, check_derived=False
    )
    if text.count(PLAN_START) != 1 or text.count(PLAN_END) != 1:
        raise PackError("add one pair of derived markers in §2.5 before sync")
    start = text.index(PLAN_START)
    end = text.index(PLAN_END) + len(PLAN_END)
    if start >= end:
        raise PackError("derived markers are reversed")
    updated = text[:start] + _derived_view(graph, waves) + text[end:]
    _load_plan(updated, key_pattern, project_key)
    return updated


def _adapter_actions(project_root: Path, target_root: Path) -> tuple[list[Path], list[str]]:
    canonical = _skills(project_root / ".github" / "skills")
    actions: list[Path] = []
    errors: list[str] = []
    if not canonical:
        errors.append(f"project has no canonical skills: {project_root}")
    for name, directory in canonical.items():
        for base in (".agents", ".claude"):
            destination = target_root / base / "skills" / name
            if destination.is_symlink():
                if destination.resolve() != directory.resolve():
                    errors.append(
                        f"adapter points to another source: {destination}"
                    )
            elif destination.exists():
                errors.append(f"adapter path is owned by the target: {destination}")
            else:
                actions.append(destination)
    return actions, errors


def install_adapters(
    project_root: Path, target_root: Path, mode: str
) -> list[str]:
    if not target_root.is_dir():
        raise PackError(f"adapter target root does not exist: {target_root}")
    actions, errors = _adapter_actions(project_root, target_root)
    if mode == "check":
        if actions or errors:
            raise PackError(
                "; ".join(
                    [*errors, f"adapter changes required: {len(actions)}"]
                )
            )
        return ["adapter check passed"]
    if mode == "dry-run":
        if errors:
            raise PackError("; ".join(errors))
        return [f"would install {len(actions)} adapter link(s)"] if actions else [
            "no adapter changes required"
        ]
    if errors:
        raise PackError("; ".join(errors))
    canonical = _skills(project_root / ".github" / "skills")
    for destination in actions:
        source = canonical[destination.name]
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.symlink_to(
            os.path.relpath(source.resolve(), destination.parent.resolve()),
            target_is_directory=True,
        )
    return [f"installed {len(actions)} adapter link(s) into {target_root}"]


def _parse_resource_root_overrides(values: list[str]) -> dict[str, Path]:
    overrides: dict[str, Path] = {}
    for value in values:
        key, separator, raw = value.partition("=")
        if not separator or not key or not raw:
            raise PackError(f"--resource-root must be KEY=PATH: {value}")
        if key in overrides:
            raise PackError(f"duplicate --resource-root for {key}")
        path = Path(raw).expanduser()
        if not path.is_absolute():
            path = Path.cwd() / path
        path = path.resolve()
        if not path.is_dir():
            raise PackError(f"resource root {key} is not a directory: {path}")
        overrides[key] = path
    return overrides


def _config_from_args(args: argparse.Namespace) -> Config:
    cwd = Path.cwd()
    return load_config(resolve_config(getattr(args, "config_dir", None), cwd))


def _command_render(args: argparse.Namespace) -> int:
    config = _config_from_args(args)
    project_root = _resolve_project(args.project, Path.cwd())
    for message in render(project_root, config):
        print(message)
    print(f"config: {config.folder}")
    return 0


def _command_check(args: argparse.Namespace) -> int:
    if args.base and args.plan_graphs != "changed":
        raise PackError("--base requires --plan-graphs changed")
    if args.plan_graphs == "changed" and not args.base:
        raise PackError("--plan-graphs changed requires --base REV")
    config = _config_from_args(args)
    project_root = _resolve_project(args.project, Path.cwd())
    notes: list[str] = []
    errors = check(
        project_root,
        config,
        resource_roots=_parse_resource_root_overrides(args.resource_root),
        plan_graphs=args.plan_graphs,
        plan_base=args.base,
        links=not args.no_links,
        routed_once=not args.no_routed_once,
        public_lint=not args.no_public_lint,
        notes=notes,
    )
    for note in notes:
        print(f"[NOTE] {note}")
    if errors:
        for error in errors:
            print(f"[ERROR] {error}", file=sys.stderr)
        print(
            f"pack check failed: {project_root} (config {config.folder})",
            file=sys.stderr,
        )
        return 1
    print(f"pack check passed: {project_root} (config {config.folder})")
    return 0


def _command_install(args: argparse.Namespace) -> int:
    project_root = _resolve_project(args.project, Path.cwd())
    target = Path(args.target_root).expanduser()
    if not target.is_absolute():
        target = Path.cwd() / target
    target = target.resolve()
    for message in install_adapters(project_root, target, args.mode):
        print(message)
    return 0


def _command_sync(args: argparse.Namespace) -> int:
    config = _config_from_args(args)
    cwd = Path.cwd()
    updates: list[tuple[Path, str]] = []
    for name in args.plans:
        path = Path(name).expanduser()
        if not path.is_absolute():
            path = cwd / path
        text = path.read_text(encoding="utf-8")
        updates.append(
            (
                path,
                sync_plan(
                    text, config.key_pattern, str(config.project["key"])
                ),
            )
        )
    for path, text in updates:
        path.write_text(text, encoding="utf-8")
        print(f"SYNC {path}")
    return 0


def _command_validate(args: argparse.Namespace) -> int:
    config = _config_from_args(args)
    cwd = Path.cwd()
    failures = 0
    for name in args.plans:
        path = Path(name).expanduser()
        if not path.is_absolute():
            path = cwd / path
        text = path.read_text(encoding="utf-8")
        try:
            graph, _ = _load_plan(
                text, config.key_pattern, str(config.project["key"])
            )
        except PackError as exc:
            print(f"FAIL {path}: {exc}", file=sys.stderr)
            failures += 1
            continue
        digest = hashlib.sha256(text.encode()).hexdigest()
        print(f"PASS {path}: {len(graph['tasks'])} tasks; sha256={digest}")
    return 1 if failures else 0


def _add_config(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--config-dir",
        default=argparse.SUPPRESS,
        help="explicit config folder (wins over KONTOR_CONFIG_DIR)",
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    _add_config(parser)
    commands = parser.add_subparsers(dest="command", required=True)

    render_parser = commands.add_parser("render", help="render pack skills")
    _add_config(render_parser)
    render_parser.add_argument("project")
    render_parser.set_defaults(handler=_command_render)

    check_parser = commands.add_parser("check", help="check one project")
    _add_config(check_parser)
    check_parser.add_argument("project")
    check_parser.add_argument(
        "--resource-root",
        action="append",
        default=[],
        metavar="KEY=PATH",
        help="explicit path for one declared project.resource_roots key",
    )
    check_parser.add_argument(
        "--plan-graphs",
        choices=("all", "marker", "changed"),
        default="all",
        help="require graphs on all current plans, marker plans only, or changed plans",
    )
    check_parser.add_argument(
        "--base", help="base revision for --plan-graphs changed"
    )
    check_parser.add_argument(
        "--no-links",
        action="store_true",
        help="disable link and anchor checking for a legacy documentation tree",
    )
    check_parser.add_argument(
        "--no-routed-once",
        action="store_true",
        help="disable routed-once checking for a legacy documentation tree",
    )
    check_parser.add_argument(
        "--no-public-lint",
        action="store_true",
        help="disable the public-example lint for a private documentation tree",
    )
    check_parser.set_defaults(handler=_command_check)

    install_parser = commands.add_parser(
        "install-adapters", help="install native skill adapters"
    )
    install_parser.add_argument("project")
    install_parser.add_argument("--target-root", required=True)
    modes = install_parser.add_mutually_exclusive_group(required=True)
    modes.add_argument("--check", dest="mode", action="store_const", const="check")
    modes.add_argument("--dry-run", dest="mode", action="store_const", const="dry-run")
    modes.add_argument("--apply", dest="mode", action="store_const", const="apply")
    install_parser.set_defaults(handler=_command_install)

    sync_parser = commands.add_parser("sync", help="regenerate a plan derived view")
    _add_config(sync_parser)
    sync_parser.add_argument("plans", nargs="+")
    sync_parser.set_defaults(handler=_command_sync)

    validate_parser = commands.add_parser("validate", help="validate plan graphs")
    _add_config(validate_parser)
    validate_parser.add_argument("plans", nargs="+")
    validate_parser.set_defaults(handler=_command_validate)
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        return args.handler(args)
    except PackError as exc:
        print(f"pack: error: {exc}", file=sys.stderr)
        return 2
    except OSError as exc:
        print(f"pack: error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
