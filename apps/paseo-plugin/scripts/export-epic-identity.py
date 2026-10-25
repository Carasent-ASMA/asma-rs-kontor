#!/usr/bin/env python3
"""Read-only epic identity export (kontor/epic-identity@1) for the Sessions screen's epic resolver.

    export-epic-identity.py --scope-jql 'project = ASMA AND issuetype = Epic' [KEY ...] > epic-identity.json
    export-epic-identity.py --self-check

Reads from Jira, through the same adapter as collect-responsibility.py, every issue the configured scope
selects and every key a session label declares, following parents up to the epic. Each issue keeps its
ID, key, type and level, parent, labels, status, update time and summary with the time it was observed;
the document adds the scope, provenance, coverage, conflicts and an order that a later export exceeds.
Nothing is written to Jira, to Paseo or to disk. The operator configures the output as a `jira` read
source; it stands in for the shared work catalog until that catalog serves the same answers.

Declared keys are the explicit KEY arguments plus every value of the resolver's labels on the Paseo agent
records under --paseo-home (default $PASEO_HOME, else ~/.paseo), archived ones included, read-only. A key
declared after the export is not covered by it. A key Jira does not return is listed as missing, never as
deleted: Jira answers the same for a deleted issue and one this account may not read.
"""
from __future__ import annotations

import argparse
import glob
import importlib.util
import json
import os
import re
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable

SCHEMA = "kontor/epic-identity@1"
COLLECTOR = "export-epic-identity@1"
FIELDS = "summary,status,issuetype,parent,labels,updated"
# The labels the resolver reads: EPIC_LABELS, TASK_JIRA_LABELS and `ticket` in client/session-organization.ts.
LABEL_KEYS = ("epic", "jira.epic", "origin.epic", "asma.epic", "jira.issue", "task", "asma.task", "ticket")
ISSUE_KEY = re.compile(r"^[A-Z][A-Z0-9_]*-[1-9]\d*$")
PARENT_ROUNDS = 3
BATCH = 50
MAX_ISSUES = 5000
MAX_BYTES = 2 * 1024 * 1024


def adapter():
    """collect-responsibility.py's Jira adapter: asma-cli credentials, read-only GETs, same-origin redirects only."""
    spec = importlib.util.spec_from_file_location("collect_responsibility", Path(__file__).with_name("collect-responsibility.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def stamp(moment: datetime) -> str:
    return moment.astimezone(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")


def instant(value: str) -> str:
    """Jira's `2026-10-24T18:05:12.345+0200` as a UTC instant every reader parses."""
    try:
        return stamp(datetime.strptime(value, "%Y-%m-%dT%H:%M:%S.%f%z"))
    except ValueError:
        return stamp(datetime.fromisoformat(value))


def key_order(key: str) -> tuple[str, int]:
    project, number = key.rsplit("-", 1)
    return project, int(number)


def declared_keys(paseo_home: str) -> tuple[list[str], dict]:
    """Every issue key a resolver label names on a Paseo agent record; values that are not one key are not declarations."""
    keys, read, unreadable = set(), 0, 0
    for path in glob.glob(os.path.join(paseo_home, "agents", "*", "*.json")):
        try:
            with open(path, encoding="utf-8") as handle:
                record = json.load(handle)
        except (OSError, ValueError):
            unreadable += 1
            continue
        labels = record.get("labels") if isinstance(record, dict) else None
        read += 1
        for name in LABEL_KEYS:
            value = labels.get(name) if isinstance(labels, dict) else None
            if isinstance(value, str) and ISSUE_KEY.fullmatch(value.strip().upper()):
                keys.add(value.strip().upper())
    return sorted(keys, key=key_order), {"read": read, "unreadable": unreadable}


def issue(raw: dict, via: str, observed_at: str) -> dict:
    fields = raw["fields"]
    return {"id": raw["id"], "key": raw["key"], "type": fields["issuetype"]["name"], "level": fields["issuetype"]["hierarchyLevel"],
            "parent": (fields.get("parent") or {}).get("key"), "labels": sorted(fields.get("labels") or []),
            "status": fields["status"]["name"], "cat": fields["status"]["statusCategory"]["key"], "updated": instant(fields["updated"]),
            "summary": fields["summary"], "observed_at": observed_at, "via": via}


def collect(jira, rejected: type[BaseException], scope_jql: str, project: str, declared: list[str], sessions: dict,
            now: Callable[[], datetime]) -> dict:
    started = now()
    issues: dict[str, dict] = {}
    missing: list[str] = []
    conflicts: list[dict] = []
    moved: dict[str, str] = {}  # A key Jira returned under another, to that current key.

    def read(key: str) -> bool:
        return key in issues or key in moved

    def take(raws: list[dict], via: str) -> None:
        observed = stamp(now())
        for raw in raws:
            issues.setdefault(raw["key"], issue(raw, via, observed))

    def lookup(keys: list[str], via: str) -> None:
        wanted = [key for key in keys if not read(key)]
        for start in range(0, len(wanted), BATCH):
            try:
                take(jira.search(f"key in ({','.join(wanted[start:start + BATCH])})", FIELDS), via)
            except rejected:
                pass  # A key Jira does not know rejects the whole search; each remaining key is read alone below.
        for key in [key for key in wanted if not read(key)]:
            raw = jira.get(f"/rest/api/3/issue/{key}", {"fields": FIELDS})
            if raw is None:
                missing.append(key)
                continue
            take([raw], via)
            if raw["key"] != key:
                moved[key] = raw["key"]
                conflicts.append({"kind": "moved_key", "key": key, "current": raw["key"], "observed_at": stamp(now())})

    take(jira.search(scope_jql, FIELDS), "scope")
    scope_issues = len(issues)
    own = re.compile(rf"^{re.escape(project)}-[1-9]\d*$")
    in_scope = [key for key in declared if own.fullmatch(key)]
    lookup(in_scope, "declared")
    for _ in range(PARENT_ROUNDS):
        parents = sorted({i["parent"] for i in issues.values() if i["level"] < 1 and i["parent"] and not read(i["parent"])} - set(missing), key=key_order)
        if not parents:
            break
        lookup(parents, "parent")
    unresolved = sorted((i["key"] for i in issues.values() if i["level"] < 1 and i["parent"] and not read(i["parent"])), key=key_order)
    return {"schema": SCHEMA, "order": int(started.timestamp() * 1000), "as_of": stamp(now()),
            "source": {"kind": "jira-rest", "realm": jira.realm},
            "scope": {"jql": scope_jql, "project": project, "declared": in_scope},
            "provenance": {"collector": COLLECTOR, "adapter": "collect-responsibility.py Jira through asma-cli jira_config",
                           "label_keys": list(LABEL_KEYS), "sessions": sessions},
            "coverage": {"scope_issues": scope_issues, "declared": len(in_scope),
                         "found": sum(1 for key in in_scope if key in issues or key in moved), "missing": sorted(missing, key=key_order),
                         "out_of_scope": [key for key in declared if not own.fullmatch(key)], "unresolved": unresolved},
            "conflicts": conflicts, "issues": sorted(issues.values(), key=lambda i: key_order(i["key"]))}


def self_check() -> dict:
    """Synthetic Jira and Paseo records only; prints a document the plugin's schema test parses."""
    module = adapter()
    raw = lambda key, kind, level, parent=None, labels=(): {"id": str(100000 + key_order(key)[1]), "key": key, "fields": {
        "issuetype": {"name": kind, "hierarchyLevel": level}, "summary": f"Synthetic {key}", "labels": list(labels),
        "status": {"name": "In Progress", "statusCategory": {"key": "indeterminate"}}, "updated": "2026-01-01T10:00:00.000+0200",
        **({"parent": {"key": parent}} if parent else {})}}
    known = {i["key"]: i for i in [raw("ASMA-10", "Epic", 1, labels=["ws:alpha"]), raw("ASMA-11", "Story", 0, "ASMA-10"),
                                   raw("ASMA-12", "Sub-task", -1, "ASMA-11"), raw("ASMA-20", "Epic", 1),
                                   raw("ASMA-30", "Task", 0), raw("ASMA-41", "Story", 0, "ASMA-20"), raw("ASMA-50", "Story", 0, "ASMA-51"),
                                   raw("ASMA-60", "Story", 0, "ASMA-2")]}
    calls: list[str] = []

    class Synthetic:
        realm = "tracker.example"

        def search(self, jql: str, fields: str) -> list[dict]:
            assert fields == FIELDS
            calls.append(jql)
            if jql == "project = ASMA AND issuetype = Epic":
                return [known["ASMA-10"]]
            keys = jql.removeprefix("key in (").removesuffix(")").split(",")
            if any(key not in known for key in keys):
                raise module.RejectedQuery("rejected")
            return [known[key] for key in keys]

        def get(self, path: str, query: dict) -> dict | None:
            calls.append(path)
            key = path.rsplit("/", 1)[1]
            return {"ASMA-40": known["ASMA-41"], "ASMA-2": known["ASMA-20"]}.get(key) or known.get(key)

    with tempfile.TemporaryDirectory() as home:
        for name, record in {"a": {"labels": {"task": " asma-12 ", "role": "SWE"}}, "b": {"archivedAt": "x", "labels": {"epic": "ASMA-20"}},
                             "c": {"labels": {"jira.issue": "ASMA-404", "ticket": "ASMA-30", "asma.task": "EX-1"}},
                             "d": {"labels": {"task": "ASMA-40", "epic": "not a key", "origin.epic": "ASMA-11, ASMA-12"}}, "e": {},
                             "g": {"labels": {"task": "ASMA-50", "jira.issue": "ASMA-60"}}}.items():
            os.makedirs(os.path.join(home, "agents", name))
            with open(os.path.join(home, "agents", name, f"{name}.json"), "w", encoding="utf-8") as handle:
                json.dump(record, handle)
        os.makedirs(os.path.join(home, "agents", "f"))
        with open(os.path.join(home, "agents", "f", "f.json"), "w", encoding="utf-8") as handle:
            handle.write("{")
        declared, sessions = declared_keys(home)
    assert declared == ["ASMA-12", "ASMA-20", "ASMA-30", "ASMA-40", "ASMA-50", "ASMA-60", "ASMA-404", "EX-1"], declared
    assert sessions == {"read": 6, "unreadable": 1}, sessions
    ticks = iter(range(1000))
    out = collect(Synthetic(), module.RejectedQuery, "project = ASMA AND issuetype = Epic", "ASMA", declared, sessions,
                  lambda: datetime(2026, 1, 1, 12, 0, next(ticks), tzinfo=timezone.utc))
    by_key = {i["key"]: i for i in out["issues"]}
    assert list(by_key) == ["ASMA-10", "ASMA-11", "ASMA-12", "ASMA-20", "ASMA-30", "ASMA-41", "ASMA-50", "ASMA-60"], list(by_key)
    assert by_key["ASMA-12"]["via"] == "declared" and by_key["ASMA-11"]["via"] == "parent" and by_key["ASMA-10"]["via"] == "scope"
    assert by_key["ASMA-11"]["parent"] == "ASMA-10" and by_key["ASMA-30"]["parent"] is None and by_key["ASMA-10"]["updated"] == "2026-01-01T08:00:00Z"
    # ASMA-60's parent ASMA-2 comes back as epic ASMA-20: one alias, read once, and the story is resolved.
    assert [(c["key"], c["current"]) for c in out["conflicts"]] == [("ASMA-40", "ASMA-41"), ("ASMA-2", "ASMA-20")], out["conflicts"]
    assert by_key["ASMA-60"]["parent"] == "ASMA-2" and calls.count("/rest/api/3/issue/ASMA-2") == 1, calls
    assert out["coverage"] == {"scope_issues": 1, "declared": 7, "found": 6, "missing": ["ASMA-51", "ASMA-404"], "out_of_scope": ["EX-1"],
                               "unresolved": ["ASMA-50"]}, out["coverage"]
    # One rejected batch, then one read per key it held; a parent Jira does not return is read once, not every round.
    assert calls[1].startswith("key in (ASMA-12,") and "/rest/api/3/issue/ASMA-404" in calls and calls[-1] == "/rest/api/3/issue/ASMA-51", calls
    assert calls.count("key in (ASMA-2,ASMA-11,ASMA-51)") == 1 and calls.count("/rest/api/3/issue/ASMA-51") == 1, calls
    assert out["order"] == 1767268800000 and out["as_of"] > out["issues"][0]["observed_at"]
    assert tuple(out["provenance"]["label_keys"]) == LABEL_KEYS
    return out


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("keys", nargs="*", help="explicit keys to read in addition to the declared ones")
    parser.add_argument("--scope-jql", help="the configured scope, for example 'project = ASMA AND issuetype = Epic'")
    parser.add_argument("--project", default="ASMA", help="the project whose declared keys are read (default ASMA)")
    parser.add_argument("--paseo-home", default=os.environ.get("PASEO_HOME") or os.path.join(os.path.expanduser("~"), ".paseo"))
    parser.add_argument("--self-check", action="store_true", help="run the offline check on synthetic data and print its document")
    args = parser.parse_args()
    if args.self_check:
        output = self_check()
    else:
        explicit = [key.strip().upper() for key in args.keys]
        if not args.scope_jql or not ISSUE_KEY.fullmatch(f"{args.project}-1") or not all(ISSUE_KEY.fullmatch(key) for key in explicit):
            parser.error("give --scope-jql, a project key and only issue keys")
        declared, sessions = declared_keys(args.paseo_home)
        module = adapter()
        output = collect(module.Jira.from_asma_cli(), module.RejectedQuery, args.scope_jql, args.project,
                         sorted(set(declared) | set(explicit), key=key_order), sessions, lambda: datetime.now(timezone.utc))
    text = json.dumps(output, ensure_ascii=False, indent=1) + "\n"
    if len(text.encode()) > MAX_BYTES or len(output["issues"]) > MAX_ISSUES:
        raise SystemExit("Export exceeds the plugin's limits (2 MiB, 5000 issues); narrow the scope")
    sys.stdout.write(text)


if __name__ == "__main__":
    main()
