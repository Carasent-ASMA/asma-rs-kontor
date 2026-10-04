#!/usr/bin/env python3
"""Trusted-base publication branch and title checker (ASMA-8103).

Implements the bounded ASMA-8101 contract:

    ordinary branch  <type>/ASMA-<number>-<slug>
    ordinary title   <exact branch key> <summary>
    release branch   releases/v<X>.<Y>.<Z>        (the only keyless form)
    release title    Release v<exact captured version>

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
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Mapping
    from typing import TextIO

PROJECT_KEY = "ASMA"
DEFAULT_BASE = "master"
BRANCH_TYPES: tuple[str, ...] = (
    "feat",
    "fix",
    "docs",
    "chore",
    "refactor",
    "test",
    "perf",
    "build",
    "ci",
    "releases",
)

CODE_OK = "ok"
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

CHECK_NAME = "asma/publication-branch-title"

_ORDINARY_BRANCH_RE = re.compile(
    r"^(?P<branch_type>" + "|".join(BRANCH_TYPES) + r")/"
    rf"(?P<key>{PROJECT_KEY}-[1-9][0-9]*)-"
    r"(?P<slug>[a-z0-9]+(?:-[a-z0-9]+)*)$"
)
_RELEASE_BRANCH_RE = re.compile(r"^releases/v(?P<version>[0-9]+\.[0-9]+\.[0-9]+)$")
_KEY_PREFIX_RE = re.compile(r"^(?P<key>[A-Z][A-Z0-9]*-[1-9][0-9]*)(?=-|$)")
_LOWERCASE_KEY_PREFIX_RE = re.compile(r"^[a-z][a-z0-9]*-[1-9][0-9]*(?=-|$)")
_SLUG_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
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


def evaluate(*, base_ref: str, head_ref: str, title: str, head_sha: str) -> Decision:
    """Decide one current PR metadata snapshot. Pure function, no I/O.

    ``head_sha`` is the asserted ``pull_request.head.sha``; it is required so a
    run that cannot name the exact head never reports success.
    """

    if base_ref != DEFAULT_BASE:
        return _refuse(
            CODE_BASE_BRANCH_NOT_DEFAULT,
            f"the current base ref must be exactly {DEFAULT_BASE!r}",
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

    ordinary_match = _ORDINARY_BRANCH_RE.fullmatch(head_ref)
    if ordinary_match is not None:
        return _ordinary_title(ordinary_match.group("key"), title)

    return _branch_refusal(head_ref)


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


def _branch_refusal(head_ref: str) -> Decision:
    prefix, separator, rest = head_ref.partition("/")
    if not separator:
        return _refuse(
            CODE_BRANCH_SHAPE_INVALID,
            "the branch must be <type>/ASMA-<number>-<slug> or releases/v<X>.<Y>.<Z>",
        )
    if prefix not in BRANCH_TYPES:
        return _refuse(
            CODE_BRANCH_TYPE_UNKNOWN,
            "the branch type must be one of: " + ", ".join(BRANCH_TYPES),
        )

    key_match = _KEY_PREFIX_RE.match(rest)
    if key_match is not None:
        key = key_match.group("key")
        if key.split("-", 1)[0] != PROJECT_KEY:
            return _refuse(
                CODE_BRANCH_PROJECT_MISMATCH,
                f"the branch key must belong to project {PROJECT_KEY}",
            )
        slug = rest[key_match.end() :].removeprefix("-")
        if slug == rest[key_match.end() :] or _SLUG_RE.fullmatch(slug) is None:
            return _refuse(
                CODE_BRANCH_SLUG_INVALID,
                "the slug after the key must be non-empty lowercase words "
                "joined by single hyphens",
            )
        return _refuse(
            CODE_BRANCH_SHAPE_INVALID,
            "the branch does not match the canonical grammar",
        )

    if _LOWERCASE_KEY_PREFIX_RE.match(rest) or rest[:1].isupper():
        return _refuse(
            CODE_BRANCH_KEY_NOT_CANONICAL,
            "the branch key must be ASMA-<positive number> without leading zeroes",
        )
    if prefix == "releases":
        return _refuse(
            CODE_RELEASE_BRANCH_INVALID,
            "a releases/ branch is either releases/ASMA-<number>-<slug> or "
            "releases/v<X>.<Y>.<Z>",
        )
    return _refuse(
        CODE_BRANCH_KEY_MISSING,
        "the branch carries no ASMA key after its type",
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
    decision = evaluate(
        base_ref=base_ref,
        head_ref=head_ref,
        title=env.get("PR_TITLE", ""),
        head_sha=head_sha,
    )

    record = {
        "action": env.get("PR_ACTION", ""),
        "base_ref": base_ref,
        "check": CHECK_NAME,
        "code": decision.code,
        "decision": "accept" if decision.accepted else "refuse",
        "head_ref": head_ref,
        "head_sha": head_sha,
        "pr_number": _pr_number(env.get("PR_NUMBER", "")),
    }
    print(json.dumps(record, sort_keys=True), file=out)
    if not decision.accepted:
        print(f"{CHECK_NAME}: refused {decision.code}: {decision.detail}", file=err)
        return 1
    print(f"{CHECK_NAME}: accepted {decision.identity}: {decision.detail}", file=err)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
