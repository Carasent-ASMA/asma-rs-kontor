#!/usr/bin/env python3
"""Trusted-base publication branch and title checker (ASMA-8103).

Implements the bounded ASMA-8101 contract, parameterized by the selected
project configuration:

    ordinary branch  <type>/<PROJECT>-<number>-<slug>
    ordinary title   <exact branch key> <summary>
    release branch   releases/v<X>.<Y>.<Z>        (the only keyless form)
    release title    Release v<exact captured version>

The project key, base branch and check name come from the selected YAML config
folder (or a generated projection in CI); the branch vocabulary comes from
``kontor-core``'s ``BranchType`` enum. Nothing deployment-specific is compiled
in.

One exemption: a pull request opened by ``dependabot[bot]`` from a
``dependabot/`` branch carries no Jira key and is accepted with the code
``bot_branch_exempt``. The base and head checks still apply to it. The author
is the event's ``pull_request.user.login``; a ``[bot]`` login belongs to a
GitHub App and no person can register one.

The workflow that runs this module is ``pull_request_target``: the current PR
metadata is read from the event as data, the trusted base revision supplies the
code, and no PR-head or merge code is executed. The release title compares the
captured version text exactly; the producer preserves the numeric spelling, so
``v01.2.3`` demands ``Release v01.2.3`` and is not silently normalized.

This check proves branch/title shape and exact key or version equality plus the
current base policy only. It is not Jira binding proof, it does not contact
Jira, and it cannot prove that the metadata belongs to the asserted head.
"""

from __future__ import annotations

import json
import os
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING

try:  # the workflow fetches this beside the checker
    from kontor_project import (
        ProjectConfigError,
        branch_refusal,
        load_branch_types,
        load_project,
    )
except ImportError:  # running from a checkout: scripts/ is a sibling
    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
    from kontor_project import (
        ProjectConfigError,
        branch_refusal,
        load_branch_types,
        load_project,
    )

if TYPE_CHECKING:
    from collections.abc import Mapping
    from typing import TextIO

CODE_OK = "ok"
CODE_BOT_BRANCH_EXEMPT = "bot_branch_exempt"
CODE_BASE_BRANCH_NOT_DEFAULT = "base_branch_not_default"
CODE_HEAD_SHA_INVALID = "head_sha_invalid"
CODE_BRANCH_SHAPE_INVALID = "branch_shape_invalid"
CODE_BRANCH_TYPE_UNKNOWN = "branch_type_unknown"
CODE_BRANCH_KEY_MISSING = "branch_key_missing"
CODE_BRANCH_KEY_NOT_CANONICAL = "branch_key_not_canonical"
CODE_BRANCH_PROJECT_MISMATCH = "branch_project_mismatch"
CODE_BRANCH_SLUG_INVALID = "branch_slug_invalid"
CODE_RELEASE_BRANCH_INVALID = "release_branch_invalid"
CODE_PR_TITLE_KEY_MISSING = "pr_title_key_missing"
CODE_PR_TITLE_KEY_MISMATCH = "pr_title_key_mismatch"
CODE_RELEASE_TITLE_MISMATCH = "release_title_mismatch"

ACCEPT_CODES: tuple[str, ...] = (CODE_OK, CODE_BOT_BRANCH_EXEMPT)

# (exact pull request author login, branch prefix) pairs exempt from the grammar.
BOT_BRANCHES: tuple[tuple[str, str], ...] = (("dependabot[bot]", "dependabot/"),)

_RELEASE_BRANCH_RE = re.compile(r"^releases/v(?P<version>[0-9]+\.[0-9]+\.[0-9]+)$")
_TITLE_LEAD_KEY_RE = re.compile(r"^(?P<key>[A-Z][A-Z0-9]*-[1-9][0-9]*) ")
_HEAD_SHA_RE = re.compile(r"^[0-9a-f]{40}$")


@dataclass(frozen=True)
class Decision:
    """The checker verdict: outcome, stable reason code and claimed identity."""

    accepted: bool
    code: str
    identity: str | None
    detail: str


def _accept(identity: str | None, detail: str) -> Decision:
    return Decision(True, CODE_OK, identity, detail)


def _refuse(code: str, detail: str) -> Decision:
    return Decision(False, code, None, detail)


def evaluate(
    *,
    base_ref: str,
    head_ref: str,
    title: str,
    head_sha: str,
    pr_author: str = "",
    project_key: str,
    base_branch: str,
    branch_types: tuple[str, ...],
) -> Decision:
    """Decide one current PR metadata snapshot. Pure function, no I/O.

    ``head_sha`` is the asserted ``pull_request.head.sha``; it is required so a
    run that cannot name the exact head never reports success. ``pr_author`` is
    the ``pull_request.user.login``; it matters only for the bot exemption.
    ``project_key``, ``base_branch`` and ``branch_types`` come from the
    selected project configuration and ``kontor-core``; none is compiled in.
    """

    if base_ref != base_branch:
        return _refuse(
            CODE_BASE_BRANCH_NOT_DEFAULT,
            f"the current base ref must be exactly {base_branch!r}",
        )
    if not isinstance(head_sha, str) or _HEAD_SHA_RE.fullmatch(head_sha) is None:
        return _refuse(
            CODE_HEAD_SHA_INVALID,
            "the asserted head must be the lowercase 40-hex pull_request.head.sha",
        )
    if not isinstance(head_ref, str) or not isinstance(title, str):
        return _refuse(CODE_BRANCH_SHAPE_INVALID, "head ref and title must be strings")

    release_match = _RELEASE_BRANCH_RE.fullmatch(head_ref)
    if release_match is not None:
        version = release_match.group("version")
        expected_title = f"Release v{version}"
        if title != expected_title:
            return _refuse(
                CODE_RELEASE_TITLE_MISMATCH,
                f"the title for {head_ref!r} must be exactly {expected_title!r}",
            )
        return _accept(version, f"keyless release branch {head_ref}")

    refusal = branch_refusal(
        head_ref, project_key=project_key, branch_types=branch_types
    )
    if refusal is None:
        key = _branch_key(head_ref)
        return _ordinary_title(key, title)

    if _is_bot_branch(pr_author, head_ref):
        return Decision(
            True, CODE_BOT_BRANCH_EXEMPT, pr_author, f"bot branch {head_ref}"
        )

    return _branch_refusal(project_key, branch_types, refusal)


def _branch_key(head_ref: str) -> str:
    """The canonical key a branch that passed the grammar carries."""

    rest = head_ref.split("/", 1)[1]
    match = re.match(r"^([A-Z][A-Z0-9]*-[0-9]+)", rest)
    if match is None:  # unreachable for a branch that passed the grammar
        raise ValueError(f"canonical branch has no key: {head_ref}")
    return match.group(1)


def _is_bot_branch(pr_author: str, head_ref: str) -> bool:
    return isinstance(pr_author, str) and any(
        pr_author == author
        and head_ref.startswith(prefix)
        and len(head_ref) > len(prefix)
        for author, prefix in BOT_BRANCHES
    )


def _ordinary_title(key: str, title: str) -> Decision:
    leading = _TITLE_LEAD_KEY_RE.match(title)
    if leading is None:
        return _refuse(
            CODE_PR_TITLE_KEY_MISSING,
            f"the title must lead with the branch key {key!r} and a space",
        )
    if leading.group("key") != key:
        return _refuse(
            CODE_PR_TITLE_KEY_MISMATCH,
            f"the title key {leading.group('key')!r} does not equal the branch key {key!r}",
        )
    if re.fullmatch(re.escape(key) + r" \S.*", title) is None:
        return _refuse(
            CODE_PR_TITLE_KEY_MISSING,
            f"the title must be {key!r}, one space and a non-empty summary",
        )
    return _accept(key, f"ordinary branch key {key}")


_REFUSAL_MESSAGES = {
    CODE_BRANCH_SHAPE_INVALID: "the branch must be <type>/<PROJECT>-<number>-<slug> or releases/v<X>.<Y>.<Z>",
    CODE_BRANCH_TYPE_UNKNOWN: "the branch type is not declared by kontor-core",
    CODE_BRANCH_KEY_MISSING: "the branch carries no tracker key after its type",
    CODE_BRANCH_KEY_NOT_CANONICAL: "the branch key is not <PROJECT>-<positive number> without leading zeroes",
    CODE_BRANCH_SLUG_INVALID: "the slug after the key must be non-empty lowercase words joined by single hyphens",
    CODE_RELEASE_BRANCH_INVALID: "a releases/ branch is either releases/<PROJECT>-<number>-<slug> or releases/v<X>.<Y>.<Z>",
}


def _branch_refusal(
    project_key: str,
    branch_types: tuple[str, ...],
    refusal: str,
) -> Decision:
    if refusal == CODE_BRANCH_PROJECT_MISMATCH:
        return _refuse(
            CODE_BRANCH_PROJECT_MISMATCH,
            f"the branch key must belong to project {project_key}",
        )
    if refusal == CODE_BRANCH_TYPE_UNKNOWN:
        return _refuse(
            CODE_BRANCH_TYPE_UNKNOWN,
            "the branch type must be one of: " + ", ".join(branch_types),
        )
    return _refuse(
        refusal,
        _REFUSAL_MESSAGES.get(refusal, "the branch does not satisfy the grammar"),
    )


def _pr_number(value: str) -> int | str | None:
    if isinstance(value, str) and value.isdigit():
        return int(value)
    return value or None


def main(
    env: "Mapping[str, str] | None" = None,
    *,
    out: "TextIO | None" = None,
    err: "TextIO | None" = None,
) -> int:
    """Evaluate the event metadata passed through the environment.

    The workflow supplies the values as environment data; nothing here is
    interpolated into a shell. The JSON record deliberately omits the title
    body and any other free text.
    """

    env = os.environ if env is None else env
    out = sys.stdout if out is None else out
    err = sys.stderr if err is None else err

    base_ref = env.get("BASE_REF", "")
    head_ref = env.get("HEAD_REF", "")
    head_sha = env.get("HEAD_SHA", "")
    pr_author = env.get("PR_AUTHOR", "")

    try:
        project = load_project(env=env)
        branch_types = load_branch_types(env.get("KONTOR_BRANCH_GRAMMAR_SOURCE"))
    except ProjectConfigError as exc:
        print(f"publication-branch-title: {exc}", file=err)
        return 2

    decision = evaluate(
        base_ref=base_ref,
        head_ref=head_ref,
        title=env.get("PR_TITLE", ""),
        head_sha=head_sha,
        pr_author=pr_author,
        project_key=project.key,
        base_branch=project.base_branch,
        branch_types=branch_types,
    )

    record = {
        "action": env.get("PR_ACTION", ""),
        "base_ref": base_ref,
        "check": project.check_name,
        "code": decision.code,
        "config_provenance": dict(project.provenance),
        "config_source": str(project.source),
        "decision": "accept" if decision.accepted else "refuse",
        "head_ref": head_ref,
        "head_sha": head_sha,
        "pr_author": pr_author,
        "pr_number": _pr_number(env.get("PR_NUMBER", "")),
        "project_key": project.key,
    }
    print(json.dumps(record, sort_keys=True), file=out)
    if not decision.accepted:
        print(
            f"{project.check_name}: refused {decision.code}: {decision.detail}",
            file=err,
        )
        return 1
    print(
        f"{project.check_name}: accepted {decision.identity}: {decision.detail}",
        file=err,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
