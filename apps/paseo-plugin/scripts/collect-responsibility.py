#!/usr/bin/env python3
"""Read-only responsibility projection (kontor/responsibility@1) for the Sessions screen's My work.

    collect-responsibility.py EPIC [EPIC ...] [--workstreams FILE] [--ttl-hours 24] > responsibility.json
    collect-responsibility.py --self-check

Reads the selected epics, their children and sub-tasks from Jira (assignee, issue type, parent and
`ws:` labels) and writes the projection to stdout. Nothing is written to Jira or to disk. The operator
places the output at `plugin-config/kontor/responsibility.json` on the host.

Credentials come only from asma-cli's Jira helper (importable, or found under $ASMA_ROOT); they never
enter arguments, output or logs. Workstream responsibility exists only in an operator declaration:

    {"realm": "<tracker host>", "workstreams": [{"key": "<ws key>", "responsible": ["<account id>"], "participants": []}]}

Every declared account is confirmed with a Jira user read. Undecided workstreams are omitted, so the
screen keeps them unknown; nothing is inferred from names, owners, titles or labels.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
import threading
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timedelta, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

COLLECTOR = "collect-responsibility@1"
FIELDS = "assignee,issuetype,parent,labels"
ISSUE_KEY = re.compile(r"^[A-Z][A-Z0-9_]*-[1-9]\d*$")
ACCOUNT_ID = re.compile(r"^[A-Za-z0-9:_-]{1,128}$")
MAX_BYTES = 2 * 1024 * 1024


def issue_record(raw: dict) -> dict:
    """One issue; an absent field stays absent (not read), a null assignee is unassigned."""
    fields = raw["fields"]
    record = {"key": raw["key"], "type": fields["issuetype"]["name"], "level": fields["issuetype"]["hierarchyLevel"],
              "parent": (fields.get("parent") or {}).get("key")}
    if "assignee" in fields:
        record["assignee"] = None if fields["assignee"] is None else fields["assignee"]["accountId"]
    if "labels" in fields:
        record["workstreams"] = sorted({label[3:].strip() for label in fields["labels"] if label.startswith("ws:") and label[3:].strip()})
    return record


def person(user: dict) -> dict:
    return {"accountId": user["accountId"], "displayName": user.get("displayName") or "Unnamed account",
            "active": bool(user.get("active")), "accountType": user.get("accountType") or "unknown"}


def unique_members(pairs: list[tuple[str, object]]) -> dict:
    keys = [key for key, _ in pairs]
    if len(set(keys)) != len(keys):
        raise ValueError("Declaration repeats a member name")
    return dict(pairs)


def account_ids(value: object, limit: int) -> list[str] | None:
    """A list of account ID strings, or None; never a string, an object or a coerced value."""
    if not isinstance(value, list) or len(value) > limit or not all(isinstance(i, str) and ACCOUNT_ID.fullmatch(i) for i in value):
        return None
    return sorted(set(value))


def parse_declaration(raw: bytes, realm: str) -> list[dict]:
    """The operator's declaration, checked strictly; accounts are confirmed against Jira separately.

    Only decided workstreams appear, each naming at least one account: an empty roster would read as
    "nobody here", turning every unknown relation into someone else's.
    """
    data = json.loads(raw, object_pairs_hook=unique_members)
    if not isinstance(data, dict) or set(data) != {"realm", "workstreams"} or data["realm"] != realm:
        raise ValueError(f"Declaration must be {{\"realm\", \"workstreams\"}} for realm {realm}")
    if not isinstance(data["workstreams"], list) or not 1 <= len(data["workstreams"]) <= 256:
        raise ValueError("Declare 1 to 256 workstreams, or leave out --workstreams")
    seen, streams = set(), []
    for stream in data["workstreams"]:
        if not isinstance(stream, dict) or set(stream) != {"key", "responsible", "participants"}:
            raise ValueError("Each workstream declares exactly key, responsible and participants")
        key = stream["key"].strip() if isinstance(stream["key"], str) else ""
        if not key or len(key) > 128 or key.lower() in seen:
            raise ValueError(f"Workstream key {stream['key']!r} must be a unique, non-empty string")
        responsible, participants = account_ids(stream["responsible"], 64), account_ids(stream["participants"], 256)
        if responsible is None or participants is None or not responsible + participants:
            raise ValueError(f"Workstream {key} must name at least one account, as lists of account ID strings")
        seen.add(key.lower())
        streams.append({"key": key, "responsible": responsible, "participants": participants})
    return streams


def project(selection: list[str], realm: str, raw_issues: list[dict], declared_people: list[dict], streams: list[dict] | None,
            declaration: dict | None, now: datetime, ttl_hours: float) -> dict:
    issues = {raw["key"]: issue_record(raw) for raw in raw_issues}
    missing = [key for key in selection if issues.get(key, {}).get("level") != 1]
    if missing:
        raise ValueError(f"Not epics in this realm: {', '.join(missing)}")
    people = {user["accountId"]: person(user) for user in declared_people}
    for raw in raw_issues:
        if raw["fields"].get("assignee"):
            people.setdefault(raw["fields"]["assignee"]["accountId"], person(raw["fields"]["assignee"]))
    stamp = lambda moment: moment.astimezone(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")
    return {"schema": "kontor/responsibility@1", "realm": realm, "generatedAt": stamp(now), "expiresAt": stamp(now + timedelta(hours=ttl_hours)),
            "provenance": {"collector": COLLECTOR, "selection": selection, "complete": True, "workstreams": declaration},
            "people": sorted(people.values(), key=lambda p: p["accountId"]), "issues": sorted(issues.values(), key=lambda i: i["key"]),
            "workstreams": streams or []}


def origin(url: str) -> tuple[str, str, int | None]:
    parts = urllib.parse.urlsplit(url)
    scheme = parts.scheme.lower()
    return scheme, (parts.hostname or "").lower(), parts.port or {"http": 80, "https": 443}.get(scheme)


class CrossOriginRedirect(urllib.error.HTTPError):
    pass


class SameOriginRedirects(urllib.request.HTTPRedirectHandler):
    """The credential goes only to the configured origin: a redirect elsewhere is refused before it is followed."""

    def redirect_request(self, req, fp, code, msg, headers, newurl):  # noqa: ANN001
        if origin(newurl) != origin(req.full_url):
            raise CrossOriginRedirect(req.full_url, code, "cross-origin redirect refused", headers, fp)
        return super().redirect_request(req, fp, code, msg, headers, newurl)


class RejectedQuery(SystemExit):
    """Jira refused the request itself (HTTP 400), for example a search naming a key it does not know."""


class Jira:
    def __init__(self, base: str, headers: dict) -> None:
        self.base, self.headers = base, headers
        self.realm = (urllib.parse.urlparse(base).hostname or "").lower()
        self.opener = urllib.request.build_opener(SameOriginRedirects)

    @classmethod
    def from_asma_cli(cls) -> "Jira":
        try:
            import asma_cli.jira_config  # noqa: F401
        except ImportError:
            root = os.environ.get("ASMA_ROOT")
            if not root:
                raise SystemExit("asma-cli is not importable; set ASMA_ROOT to the workspace that contains _tools/asma-cli")
            sys.path.insert(0, os.path.join(root, "_tools", "asma-cli", "src"))
        environment = dict(os.environ)
        from asma_cli.jira_config import build_jira_headers, normalized_base_url
        base, headers = normalized_base_url(), build_jira_headers()
        # The helper may load its own settings into the environment; nothing else in this process needs them.
        os.environ.clear()
        os.environ.update(environment)
        if not base or not headers:
            raise SystemExit("asma-cli has no Jira configuration on this host")
        return cls(base, headers)

    def get(self, path: str, query: dict) -> dict | None:
        request = urllib.request.Request(f"{self.base}{path}?{urllib.parse.urlencode(query)}", headers=self.headers)
        try:
            with self.opener.open(request, timeout=30) as response:
                return json.load(response)
        except CrossOriginRedirect:
            raise SystemExit(f"Jira read {path} refused a redirect to another origin") from None
        except urllib.error.HTTPError as error:
            if error.code == 404:
                return None
            if error.code == 400:
                raise RejectedQuery(f"Jira read {path} was rejected with HTTP 400") from None
            raise SystemExit(f"Jira read {path} failed with HTTP {error.code}") from None

    def search(self, jql: str, fields: str = FIELDS) -> list[dict]:
        issues, token = [], None
        while True:
            page = self.get("/rest/api/3/search/jql", {"jql": jql, "fields": fields, "maxResults": 100, **({"nextPageToken": token} if token else {})})
            issues += page["issues"]
            token = page.get("nextPageToken")
            if not token or page.get("isLast", False):
                return issues

    def levels(self, clause: str, keys: list[str]) -> list[dict]:
        return [issue for start in range(0, len(keys), 50) for issue in self.search(f"{clause} in ({','.join(keys[start:start + 50])})")]


def collect(selection: list[str], declaration_path: str | None, ttl_hours: float) -> dict:
    jira = Jira.from_asma_cli()
    epics = jira.levels("key", selection)
    children = jira.levels("parent", [issue["key"] for issue in epics])
    subtasks = jira.levels("parent", [issue["key"] for issue in children])
    streams, declared_people, declaration = None, [], None
    if declaration_path:
        path = os.path.realpath(declaration_path)
        with open(path, "rb") as handle:
            raw = handle.read(MAX_BYTES + 1)
        if len(raw) > MAX_BYTES:
            raise SystemExit("Declaration exceeds 2 MiB")
        streams = parse_declaration(raw, jira.realm)
        for account in sorted({i for stream in streams for i in [*stream["responsible"], *stream["participants"]]}):
            user = jira.get("/rest/api/3/user", {"accountId": account})
            if user is None:
                raise SystemExit(f"Declared account {account} does not exist in {jira.realm}")
            declared_people.append(user)
        declaration = {"path": path, "sha256": hashlib.sha256(raw).hexdigest()}
    unique = list({issue["key"]: issue for issue in [*epics, *children, *subtasks]}.values())
    return project(selection, jira.realm, unique, declared_people, streams, declaration, datetime.now(timezone.utc), ttl_hours)


def redirect_check() -> None:
    """The credential is never sent to another origin: a redirect there is refused before the second request."""
    seen: list[str | None] = []

    class Quiet(BaseHTTPRequestHandler):
        def log_message(self, *args) -> None:  # noqa: ANN002
            pass

        def reply(self, code: int, location: str | None = None) -> None:
            self.send_response(code)
            if location:
                self.send_header("Location", location)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b'{"ok": true}')

    class Elsewhere(Quiet):
        def do_GET(self) -> None:  # noqa: N802
            seen.append(self.headers.get("Authorization"))
            self.reply(200)

    other = ThreadingHTTPServer(("127.0.0.1", 0), Elsewhere)

    class Origin(Quiet):
        def do_GET(self) -> None:  # noqa: N802
            self.reply(*{"/away": (302, f"http://127.0.0.1:{other.server_port}/taken"), "/here": (302, "/done")}.get(self.path.split("?")[0], (200,)))

    home = ThreadingHTTPServer(("127.0.0.1", 0), Origin)
    for server in (home, other):
        threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        jira = Jira(f"http://127.0.0.1:{home.server_port}", {"Authorization": "Basic c3ludGhldGlj"})
        jira.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), SameOriginRedirects)
        assert jira.get("/here", {}) == {"ok": True}
        try:
            jira.get("/away", {})
            raise AssertionError("a cross-origin redirect was followed")
        except SystemExit as refusal:
            assert "another origin" in str(refusal)
        assert seen == [], seen
    finally:
        for server in (home, other):
            server.shutdown()
            server.server_close()


def self_check() -> dict:
    """Synthetic data only; prints a projection the plugin's schema test parses."""
    user = lambda i, name: {"accountId": i, "displayName": name, "active": True, "accountType": "atlassian"}
    raw = lambda key, kind, level, parent=None, **fields: {"key": key, "fields": {"issuetype": {"name": kind, "hierarchyLevel": level},
                                                                                   **({"parent": {"key": parent}} if parent else {}), **fields}}
    issues = [raw("ASMA-100", "Epic", 1, assignee=user("account-a", "Alex Example"), labels=["ws:alpha", "other"]),
              raw("ASMA-101", "Story", 0, "ASMA-100", assignee=None, labels=[]),
              raw("ASMA-102", "Sub-task", -1, "ASMA-101")]
    streams = parse_declaration(json.dumps({"realm": "tracker.example", "workstreams": [{"key": "alpha", "responsible": ["account-b"], "participants": []}]}).encode(), "tracker.example")
    out = project(["ASMA-100"], "tracker.example", issues, [user("account-b", "Blair Example")], streams,
                  {"path": "/operator/workstreams.json", "sha256": "0" * 64}, datetime(2026, 1, 1, tzinfo=timezone.utc), 24)
    by_key = {issue["key"]: issue for issue in out["issues"]}
    assert by_key["ASMA-100"] == {"key": "ASMA-100", "type": "Epic", "level": 1, "parent": None, "assignee": "account-a", "workstreams": ["alpha"]}
    assert by_key["ASMA-101"]["assignee"] is None and "assignee" not in by_key["ASMA-102"] and "workstreams" not in by_key["ASMA-102"]
    assert [p["accountId"] for p in out["people"]] == ["account-a", "account-b"] and out["expiresAt"] == "2026-01-02T00:00:00Z"
    stream = lambda key="a", responsible=("account-b",), participants=(): json.dumps({"key": key, "responsible": responsible, "participants": participants})
    for bad in ['{"realm": "other.example", "workstreams": [%s]}' % stream(), '{"realm": "tracker.example", "workstreams": []}',
                *('{"realm": "tracker.example", "workstreams": [%s]}' % entry for entry in [
                    stream(responsible=["x y"]), stream(responsible=["account-b\n"]), stream(key=7), stream(key=None), stream(key=" "),
                    stream(responsible="account-b"), stream(responsible={"account-b": 1}), stream(responsible=[]), stream(participants=[1]),
                    stream(responsible=["account-b", None]), '{"key": "a", "responsible": ["account-b"], "responsible": ["account-c"], "participants": []}',
                    f'{stream("a")}, {stream("A")}']),
                '{"realm": "tracker.example", "realm": "tracker.example", "workstreams": [%s]}' % stream()]:
        try:
            parse_declaration(bad.encode(), "tracker.example")
            raise AssertionError(bad)
        except ValueError:
            pass
    assert parse_declaration(('{"realm": "tracker.example", "workstreams": [%s]}' % stream(responsible=[], participants=["account-c", "account-c"])).encode(),
                             "tracker.example") == [{"key": "a", "responsible": [], "participants": ["account-c"]}]
    redirect_check()
    try:
        project(["ASMA-101"], "tracker.example", issues, [], None, None, datetime(2026, 1, 1, tzinfo=timezone.utc), 24)
        raise AssertionError("a non-epic selection")
    except ValueError:
        pass
    return out


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("epics", nargs="*", help="explicit epic keys, at most 64")
    parser.add_argument("--workstreams", help="operator workstream responsibility declaration (JSON)")
    parser.add_argument("--ttl-hours", type=float, default=24.0, help="hours until the screen stops evaluating this projection (default 24)")
    parser.add_argument("--self-check", action="store_true", help="run the offline check on synthetic data and print its projection")
    args = parser.parse_args()
    if args.self_check:
        output = self_check()
    else:
        selection = list(dict.fromkeys(key.strip().upper() for key in args.epics))
        if not 1 <= len(selection) <= 64 or not all(ISSUE_KEY.fullmatch(key) for key in selection):
            parser.error("give 1 to 64 epic keys")
        if not 0 < args.ttl_hours <= 168:
            parser.error("--ttl-hours must be within (0, 168]")
        try:
            output = collect(selection, args.workstreams, args.ttl_hours)
        except ValueError as error:
            raise SystemExit(str(error)) from None
    text = json.dumps(output, ensure_ascii=False, indent=1) + "\n"
    if len(text.encode()) > MAX_BYTES or len(output["issues"]) > 5000 or len(output["people"]) > 2000:
        raise SystemExit("Projection exceeds the plugin's limits (2 MiB, 5000 issues, 2000 people); select fewer epics")
    sys.stdout.write(text)


if __name__ == "__main__":
    main()
