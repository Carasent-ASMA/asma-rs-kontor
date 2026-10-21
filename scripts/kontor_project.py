#!/usr/bin/env python3
"""Read one selected Kontor project configuration and the branch grammar.

The project key, base branch and publication check name are authored once in
the selected YAML config folder (``orchestration/orchestration.yml``). CI
cannot read an external checkout, so it may instead read a generated YAML
projection whose ``generated:`` block records where the values came from and
which repository variables carry them.

The branch grammar is not a second copy here: the conventional branch types
are read from ``crates/kontor-core/src/branch.rs`` at the revision being run.
A missing or unreadable grammar source is refused by name; there is no silent
fallback to a list compiled into this file.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Mapping, Sequence

try:  # scripts/ is on sys.path for the CLI and the local helpers
    from pack import YamlError, load_yaml
except ImportError:  # imported by path, without scripts/ on sys.path
    _PACK_SPEC = importlib.util.spec_from_file_location(
        "pack", Path(__file__).resolve().with_name("pack.py")
    )
    assert _PACK_SPEC is not None and _PACK_SPEC.loader is not None
    _PACK_MODULE = importlib.util.module_from_spec(_PACK_SPEC)
    _PACK_SPEC.loader.exec_module(_PACK_MODULE)
    YamlError = _PACK_MODULE.YamlError
    load_yaml = _PACK_MODULE.load_yaml

PROJECT_CONFIG_RELATIVE = Path("orchestration") / "orchestration.yml"
BRANCH_SOURCE_RELATIVE = Path("crates") / "kontor-core" / "src" / "branch.rs"
PUBLICATION_CHECK_SUFFIX = "publication-branch-title"
CONFIG_PROVENANCE_SOURCE = "selected-config-folder/orchestration/orchestration.yml"
PROJECTION_PROVENANCE_SOURCE = "selected-projection"
WORKING_COPY_REVISION = "working-copy"

BRANCH_SHAPE_INVALID = "branch_shape_invalid"
BRANCH_TYPE_UNKNOWN = "branch_type_unknown"
BRANCH_KEY_MISSING = "branch_key_missing"
BRANCH_KEY_NOT_CANONICAL = "branch_key_not_canonical"
BRANCH_PROJECT_MISMATCH = "branch_project_mismatch"
BRANCH_SLUG_INVALID = "branch_slug_invalid"
RELEASE_BRANCH_INVALID = "release_branch_invalid"

_BRANCH_TYPE_BLOCK_RE = re.compile(
    r'BranchType,\s*"BranchType"\s*\{(?P<body>.*?)\n\s*\}', re.S
)
_BRANCH_TYPE_RE = re.compile(r'=>\s*"([a-z][a-z0-9-]*)"')
_RELEASE_VERSION_RE = re.compile(r"^releases/v([0-9]+)\.([0-9]+)\.([0-9]+)$")


class ProjectConfigError(ValueError):
    """A refused selection or an invalid project configuration."""


@dataclass(frozen=True)
class ProjectConfig:
    """The project values local scripts and the publication check consume."""

    source: Path
    key: str
    base_branch: str
    check_names: tuple[str, ...]
    check_name: str
    provenance: Mapping[str, object]

    def as_record(self) -> dict[str, object]:
        """The stable JSON form written into receipts and check records."""

        return {
            "source": str(self.source),
            "key": self.key,
            "base_branch": self.base_branch,
            "check_names": list(self.check_names),
            "check_name": self.check_name,
            "provenance": dict(self.provenance),
        }


def _resolve_path(value: str, cwd: Path) -> Path:
    path = Path(value).expanduser()
    if not path.is_absolute():
        path = cwd / path
    return path.resolve()


def selected_config_path(
    *,
    config_dir: str | None = None,
    config_file: str | None = None,
    env: Mapping[str, str] | None = None,
    cwd: Path | None = None,
) -> Path:
    """Resolve the one explicit selection; never guess a folder.

    The explicit ``config_file`` or ``config_dir`` argument wins over its
    environment variable. A file is a generated projection (CI); a folder is
    the authored config folder (``KONTOR_CONFIG_DIR``).
    """

    env = os.environ if env is None else env
    cwd = Path.cwd() if cwd is None else cwd

    file_value = config_file
    dir_value = config_dir
    if file_value is None and dir_value is None:
        file_value = env.get("KONTOR_CONFIG_FILE")
        dir_value = env.get("KONTOR_CONFIG_DIR")

    if file_value:
        path = _resolve_path(file_value, cwd)
        if not path.is_file():
            raise ProjectConfigError(f"config file does not exist: {path}")
        return path

    if not dir_value:
        raise ProjectConfigError(
            "no config selected: pass --config-dir/--config-file or set "
            "KONTOR_CONFIG_DIR/KONTOR_CONFIG_FILE"
        )
    folder = _resolve_path(dir_value, cwd)
    if not folder.exists():
        raise ProjectConfigError(f"config folder does not exist: {folder}")
    if not folder.is_dir():
        raise ProjectConfigError(f"config folder is not a directory: {folder}")
    path = folder / PROJECT_CONFIG_RELATIVE
    if not path.is_file():
        raise ProjectConfigError(
            f"config folder has no {PROJECT_CONFIG_RELATIVE.as_posix()}: {folder}"
        )
    return path


def _required_mapping(value: object, label: str) -> Mapping[str, object]:
    if not isinstance(value, dict):
        raise ProjectConfigError(f"{label} must be a mapping")
    return value


def _required_text(value: object, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ProjectConfigError(f"{label} is required")
    return value.strip()


def publication_check_name(key: str, check_names: Sequence[str]) -> str:
    """Pick the publication branch-title check from the project's check runs."""

    expected = f"{key.lower()}/{PUBLICATION_CHECK_SUFFIX}"
    if expected in check_names:
        return expected
    for name in check_names:
        if name == PUBLICATION_CHECK_SUFFIX or name.endswith(
            f"/{PUBLICATION_CHECK_SUFFIX}"
        ):
            return name
    raise ProjectConfigError(
        "project.check_runs must name a publication-branch-title check "
        f"(expected {expected!r} or an entry ending '/{PUBLICATION_CHECK_SUFFIX}')"
    )


def load_project_config(path: Path) -> ProjectConfig:
    """Read and validate the project values the scripts consume."""

    try:
        data = load_yaml(path)
    except YamlError as exc:
        raise ProjectConfigError(f"{path}: {exc}") from exc
    if not isinstance(data, dict):
        raise ProjectConfigError(f"{path}: expected a mapping")

    project = _required_mapping(data.get("project"), f"{path}: project")
    key = _required_text(project.get("key"), f"{path}: project.key")
    tracker = project.get("tracker")
    if isinstance(tracker, dict):
        project_key = tracker.get("project_key")
        if project_key is not None and project_key != key:
            raise ProjectConfigError(
                f"{path}: project.key and project.tracker.project_key disagree"
            )
    forge = _required_mapping(project.get("forge"), f"{path}: project.forge")
    base_branch = _required_text(
        forge.get("base_branch"), f"{path}: project.forge.base_branch"
    )
    raw_check_names = project.get("check_runs")
    if not isinstance(raw_check_names, list) or not all(
        isinstance(name, str) for name in raw_check_names
    ):
        raise ProjectConfigError(f"{path}: project.check_runs must be a list of names")
    check_names = tuple(raw_check_names)
    check_name = publication_check_name(key, check_names)

    generated = data.get("generated")
    provenance: Mapping[str, object]
    if isinstance(generated, dict):
        provenance = dict(generated)
    else:
        source = (
            CONFIG_PROVENANCE_SOURCE
            if path.name == PROJECT_CONFIG_RELATIVE.name
            and path.parent.name == PROJECT_CONFIG_RELATIVE.parent.name
            else PROJECTION_PROVENANCE_SOURCE
        )
        provenance = {"source": source, "source_revision": WORKING_COPY_REVISION}

    return ProjectConfig(
        source=path,
        key=key,
        base_branch=base_branch,
        check_names=check_names,
        check_name=check_name,
        provenance=provenance,
    )


def load_project(
    *,
    config_dir: str | None = None,
    config_file: str | None = None,
    env: Mapping[str, str] | None = None,
    cwd: Path | None = None,
) -> ProjectConfig:
    """Resolve the selection and load its project values."""

    path = selected_config_path(
        config_dir=config_dir, config_file=config_file, env=env, cwd=cwd
    )
    return load_project_config(path)


def _repository_root() -> Path:
    return Path(__file__).resolve().parent.parent


def branch_grammar_path(source: str | None = None) -> Path:
    """The kontor-core source of the conventional branch-type vocabulary."""

    if source:
        return Path(source).expanduser().resolve()
    configured = os.environ.get("KONTOR_BRANCH_GRAMMAR_SOURCE")
    if configured:
        return Path(configured).expanduser().resolve()
    return _repository_root() / BRANCH_SOURCE_RELATIVE


def load_branch_types(source: str | None = None) -> tuple[str, ...]:
    """Read the branch types from ``kontor-core``'s ``BranchType`` enum."""

    path = branch_grammar_path(source)
    if not path.is_file():
        raise ProjectConfigError(f"branch grammar source does not exist: {path}")
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as exc:
        raise ProjectConfigError(f"branch grammar source is unreadable: {path}: {exc}") from exc
    match = _BRANCH_TYPE_BLOCK_RE.search(text)
    if match is None:
        raise ProjectConfigError(
            f"branch grammar source has no BranchType enum: {path}"
        )
    types = tuple(_BRANCH_TYPE_RE.findall(match.group("body")))
    if len(types) < 2 or "releases" not in types:
        raise ProjectConfigError(
            f"branch grammar source declares an incomplete BranchType enum: {path}"
        )
    return types


def _is_canonical_slug(slug: str) -> bool:
    if not slug:
        return False
    return all(
        word
        and all(character.isascii() and (character.islower() or character.isdigit()) for character in word)
        for word in slug.split("-")
    )


def _looks_like_lowercase_key(text: str) -> bool:
    if not text or not ("a" <= text[0] <= "z"):
        return False
    length = 0
    for character in text:
        if ("a" <= character <= "z") or ("0" <= character <= "9"):
            length += 1
        else:
            break
    if text[length : length + 1] != "-":
        return False
    number = text[length + 1 :]
    digits = 0
    for character in number:
        if "0" <= character <= "9":
            digits += 1
        else:
            break
    if digits == 0 or number.startswith("0"):
        return False
    return digits == len(number) or number[digits] == "-"


def branch_refusal(
    branch: str, *, project_key: str, branch_types: Sequence[str]
) -> str | None:
    """Return the stable refusal code for one branch, or ``None``.

    This is the ``kontor-core`` grammar: ``<type>/<PROJECT>-<number>-<slug>``
    plus the keyless ``releases/v<major>.<minor>.<patch>`` form.
    """

    if not isinstance(branch, str) or not branch:
        return BRANCH_SHAPE_INVALID

    if _RELEASE_VERSION_RE.fullmatch(branch):
        if "releases" in branch_types:
            return None
        return BRANCH_TYPE_UNKNOWN

    branch_type, separator, rest = branch.partition("/")
    if not separator:
        return BRANCH_SHAPE_INVALID
    if branch_type not in branch_types:
        return BRANCH_TYPE_UNKNOWN

    first = rest[:1]
    if not first or not ("A" <= first <= "Z"):
        if _looks_like_lowercase_key(rest):
            return BRANCH_KEY_NOT_CANONICAL
        if branch_type == "releases":
            return RELEASE_BRANCH_INVALID
        return BRANCH_KEY_MISSING

    project_length = 0
    for character in rest:
        if ("A" <= character <= "Z") or ("0" <= character <= "9"):
            project_length += 1
        else:
            break
    tail = rest[project_length:]
    if not tail.startswith("-"):
        return BRANCH_KEY_NOT_CANONICAL
    number = tail[1:]
    number_length = 0
    for character in number:
        if "0" <= character <= "9":
            number_length += 1
        else:
            break
    if number_length == 0 or number.startswith("0"):
        return BRANCH_KEY_NOT_CANONICAL

    key = rest[: project_length + 1 + number_length]
    if key.split("-", 1)[0] != project_key:
        return BRANCH_PROJECT_MISMATCH

    remainder = rest[len(key) :]
    if not remainder:
        return BRANCH_SLUG_INVALID
    if remainder[0] != "-":
        return BRANCH_KEY_NOT_CANONICAL
    if not _is_canonical_slug(remainder[1:]):
        return BRANCH_SLUG_INVALID
    return None


def branch_is_canonical(
    branch: str, *, project_key: str, branch_types: Sequence[str]
) -> bool:
    """Whether the branch satisfies the configured project's grammar."""

    return branch_refusal(
        branch, project_key=project_key, branch_types=branch_types
    ) is None


def _project_projection(project: ProjectConfig) -> dict[str, object]:
    return {
        "schema_version": 1,
        "generated": dict(project.provenance),
        "project": {
            "key": project.key,
            "forge": {"base_branch": project.base_branch},
            "check_runs": list(project.check_names),
        },
    }


_YAML_SAFE_KEY = re.compile(r"^[A-Za-z_][A-Za-z0-9_.-]*$")


def _yaml_scalar(value: object) -> str:
    """Render one scalar as YAML 1.2, quoting every string."""

    if value is None:
        return "null"
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return json.dumps(value)
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False)
    raise ProjectConfigError(
        f"projection provenance contains an unsupported value: {type(value).__name__}"
    )


def _yaml_key(value: object) -> str:
    if not isinstance(value, str) or not value:
        raise ProjectConfigError("projection keys must be non-empty strings")
    if _YAML_SAFE_KEY.fullmatch(value):
        return value
    return json.dumps(value, ensure_ascii=False)


def _render_yaml_node(value: object, indent: int, lines: list[str]) -> None:
    pad = " " * indent
    if isinstance(value, dict):
        if not value:
            lines.append(f"{pad}{{}}")
            return
        for key, item in value.items():
            rendered_key = _yaml_key(key)
            if isinstance(item, (dict, list)):
                if item:
                    lines.append(f"{pad}{rendered_key}:")
                    _render_yaml_node(item, indent + 2, lines)
                else:
                    lines.append(
                        f"{pad}{rendered_key}: {'{}' if isinstance(item, dict) else '[]'}"
                    )
            else:
                lines.append(f"{pad}{rendered_key}: {_yaml_scalar(item)}")
        return
    if isinstance(value, list):
        if not value:
            lines.append(f"{pad}[]")
            return
        for item in value:
            if isinstance(item, (dict, list)) and item:
                lines.append(f"{pad}-")
                _render_yaml_node(item, indent + 2, lines)
            elif isinstance(item, (dict, list)):
                lines.append(
                    f"{pad}- {'{}' if isinstance(item, dict) else '[]'}"
                )
            else:
                lines.append(f"{pad}- {_yaml_scalar(item)}")
        return
    raise ProjectConfigError("projection must be a mapping")


def render_project_projection(project: ProjectConfig, *, format: str) -> str:
    """Serialize the complete projection, including its provenance."""

    projection = _project_projection(project)
    if format == "json":
        return json.dumps(projection, indent=2, sort_keys=True) + "\n"
    if format == "yaml":
        lines: list[str] = []
        _render_yaml_node(projection, 0, lines)
        return "\n".join(lines) + "\n"
    raise ProjectConfigError(f"unsupported projection format: {format}")


def main(argv: list[str] | None = None) -> int:
    """Read the selection and report one value or judge one branch."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config-dir", help="explicit config folder")
    parser.add_argument("--config-file", help="explicit generated projection file")
    subcommands = parser.add_subparsers(dest="command", required=True)

    project_command = subcommands.add_parser("project", help="print the project values")
    project_command.add_argument("--format", choices=("json", "yaml"), default="json")

    subcommands.add_parser("key", help="print the project key")
    subcommands.add_parser("base-branch", help="print the base branch")
    subcommands.add_parser("check-name", help="print the publication check name")

    branch_command = subcommands.add_parser(
        "check-branch", help="judge one branch against the configured grammar"
    )
    branch_command.add_argument("--branch", required=True)
    branch_command.add_argument(
        "--grammar-source", help="kontor-core branch.rs (defaults to this checkout)"
    )

    args = parser.parse_args(argv)
    try:
        project = load_project(
            config_dir=args.config_dir, config_file=args.config_file
        )
        if args.command == "project":
            print(
                render_project_projection(project, format=args.format),
                end="",
            )
            return 0
        if args.command == "key":
            print(project.key)
            return 0
        if args.command == "base-branch":
            print(project.base_branch)
            return 0
        if args.command == "check-name":
            print(project.check_name)
            return 0

        branch_types = load_branch_types(args.grammar_source)
        refusal = branch_refusal(
            args.branch,
            project_key=project.key,
            branch_types=branch_types,
        )
        if refusal is None:
            print(f"ok: {args.branch}")
            return 0
        print(f"{refusal}: {args.branch}", file=sys.stderr)
        return 1
    except ProjectConfigError as exc:
        print(f"kontor-project: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
