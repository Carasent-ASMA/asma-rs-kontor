#!/usr/bin/env python3
"""Record a build commit beside each Kontor binary and read one realm back.

The installed binaries all report version ``0.2.1``, which names no source. A
deploy therefore writes one sidecar per binary, ``<binary>.build-commit.json``,
holding the binary's SHA-256 and the commit and tree it was built from. The
sidecar lives in the same directory as the binary, so it moves with the build
and is read back with it.

``readback`` is the post-deploy health check described by the runbook:

* ``/v1/health`` answers and reports the expected persisted schema generation;
* ``kontor realm-get`` answers for the expected realm;
* the installed binaries match the deploy receipt and their sidecars;
* an MCP ``kontor_realm_get`` call over stdio returns the same realm the HTTP
  and CLI surfaces returned.

Only the observer credential is read, from the realm's own ``credentials.json``;
it is never printed, logged or passed on an argument list.

Commands::

    kontor-ops.py record-build --install-dir DIR --commit SHA [--tree SHA]
    kontor-ops.py verify-build --install-dir DIR [--commit SHA]
    kontor-ops.py readback --state-root DIR --base-url URL \
        --expected-realm REALM --expected-schema N [--install-dir DIR] \
        [--expected-sha256 NAME=SHA256] [--require-build-commit] [--json-out FILE]
"""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import re
import selectors
import signal
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path

BINARIES = ("kontor-daemon", "kontor", "kontor-mcp")
SIDECAR_SUFFIX = ".build-commit.json"
SIDECAR_SCHEMA = 1
COMMIT_PATTERN = re.compile(r"[0-9a-f]{40}")
SHA256_PATTERN = re.compile(r"[0-9a-f]{64}")
MCP_PROTOCOL_VERSION = "2025-06-18"
MCP_TIMEOUT_SECONDS = 20.0


class Refused(Exception):
    """A checked fact is wrong; nothing was installed or changed."""


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _now() -> str:
    return datetime.datetime.now(datetime.timezone.utc).isoformat().replace("+00:00", "Z")


def _write_json(path: Path, document: dict) -> None:
    path.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n")


def sidecar_path(install_dir: Path, name: str) -> Path:
    return install_dir / f"{name}{SIDECAR_SUFFIX}"


def record_build(
    install_dir: Path, commit: str, tree: str | None, recorded_at: str | None
) -> dict:
    """Write one sidecar beside every installed binary."""
    if not install_dir.is_dir():
        raise Refused(f"{install_dir} is not a directory")
    for value, label in ((commit, "commit"), (tree, "tree")):
        if value is not None and COMMIT_PATTERN.fullmatch(value) is None:
            raise Refused(f"{label} {value!r} is not a 40-character lowercase hex object id")
    records = {}
    for name in BINARIES:
        binary = install_dir / name
        if not binary.is_file():
            raise Refused(f"{binary} is not installed")
        record = {
            "schemaVersion": SIDECAR_SCHEMA,
            "binary": name,
            "sha256": digest(binary),
            "sourceCommit": commit,
            "sourceTree": tree,
            "recordedAt": recorded_at or _now(),
        }
        _write_json(sidecar_path(install_dir, name), record)
        records[name] = record
    return records


def read_sidecar(install_dir: Path, name: str) -> dict:
    path = sidecar_path(install_dir, name)
    try:
        document = json.loads(path.read_text())
    except (OSError, ValueError) as error:
        raise Refused(f"{path} is missing or unreadable: {error}") from error
    if not isinstance(document, dict) or document.get("schemaVersion") != SIDECAR_SCHEMA:
        raise Refused(f"{path} is not a build-commit record this tool wrote")
    return document


def verify_build(install_dir: Path, commit: str | None) -> dict:
    """Check every sidecar against the binary beside it."""
    binaries = {}
    for name in BINARIES:
        binary = install_dir / name
        if not binary.is_file():
            raise Refused(f"{binary} is not installed")
        record = read_sidecar(install_dir, name)
        actual = digest(binary)
        if record.get("binary") != name:
            raise Refused(f"{sidecar_path(install_dir, name)} names {record.get('binary')!r}")
        if record.get("sha256") != actual:
            raise Refused(
                f"{binary} has SHA-256 {actual}, not the {record.get('sha256')} "
                "recorded in its sidecar"
            )
        recorded = record.get("sourceCommit")
        if not isinstance(recorded, str) or COMMIT_PATTERN.fullmatch(recorded) is None:
            raise Refused(f"{binary} has no usable source commit beside it")
        if commit is not None and recorded != commit:
            raise Refused(f"{binary} was built from {recorded}, not {commit}")
        binaries[name] = {
            "sha256": actual,
            "sourceCommit": recorded,
            "sourceTree": record.get("sourceTree"),
        }
    return binaries


def _request(url: str, token: str) -> dict:
    request = urllib.request.Request(
        url, headers={"Authorization": f"Bearer {token}", "Accept": "application/json"}
    )
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            return json.loads(response.read())
    except (urllib.error.URLError, TimeoutError, ValueError) as error:
        raise Refused(f"GET {url} failed: {error}") from error


def observer_token(state_root: Path) -> str:
    path = state_root / "credentials.json"
    try:
        document = json.loads(path.read_text())
        token = document["observer"]
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise Refused(f"{path} does not hold a readable observer credential: {error}") from error
    if not isinstance(token, str) or not token:
        raise Refused(f"{path} does not hold a readable observer credential")
    return token


def run_cli(install_dir: Path, state_root: Path, base_url: str) -> dict:
    kontor = install_dir / "kontor"
    if not kontor.is_file():
        raise Refused(f"{kontor} is not installed")
    result = subprocess.run(
        [
            str(kontor),
            "realm-get",
            "--state-root",
            str(state_root),
            "--base-url",
            base_url,
        ],
        capture_output=True,
        text=True,
        timeout=30,
    )
    if result.returncode != 0:
        raise Refused(f"{kontor} realm-get exited {result.returncode}: {result.stderr.strip()}")
    try:
        answer = json.loads(result.stdout)
    except ValueError as error:
        raise Refused(f"{kontor} realm-get did not print JSON: {error}") from error
    if not isinstance(answer, dict):
        raise Refused(f"{kontor} realm-get did not print an answer object")
    return answer


class _McpSession:
    """A minimal JSON-RPC line client for the stdio MCP server."""

    def __init__(self, process: subprocess.Popen[str]) -> None:
        self.process = process
        self.selector = selectors.DefaultSelector()
        assert process.stdout is not None
        self.selector.register(process.stdout, selectors.EVENT_READ)
        self.next_id = 1

    def request(self, method: str, params: dict | None) -> dict:
        request_id = self.next_id
        self.next_id += 1
        message = {"jsonrpc": "2.0", "id": request_id, "method": method}
        if params is not None:
            message["params"] = params
        assert self.process.stdin is not None
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()
        while True:
            if not self.selector.select(MCP_TIMEOUT_SECONDS):
                raise Refused(f"the MCP server did not answer {method} within the timeout")
            assert self.process.stdout is not None
            line = self.process.stdout.readline()
            if not line:
                raise Refused(f"the MCP server closed before answering {method}")
            try:
                answer = json.loads(line)
            except ValueError:
                continue
            if answer.get("id") == request_id:
                if "error" in answer:
                    raise Refused(f"the MCP server refused {method}: {answer['error']}")
                return answer

    def notify(self, method: str) -> None:
        assert self.process.stdin is not None
        self.process.stdin.write(json.dumps({"jsonrpc": "2.0", "method": method}) + "\n")
        self.process.stdin.flush()

    def close(self) -> None:
        self.selector.close()
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            if stream is not None:
                stream.close()


def mcp_parity(install_dir: Path, state_root: Path, base_url: str) -> dict:
    """Prove one observer tool answers the same over MCP as over HTTP."""
    mcp = install_dir / "kontor-mcp"
    if not mcp.is_file():
        raise Refused(f"{mcp} is not installed")
    process = subprocess.Popen(
        [
            str(mcp),
            "--state-root",
            str(state_root),
            "--credential-tier",
            "observer",
            "--base-url",
            base_url,
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    session = _McpSession(process)
    try:
        session.request(
            "initialize",
            {
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "kontor-ops-readback", "version": "1"},
            },
        )
        session.notify("notifications/initialized")
        listed = session.request("tools/list", {})
        tools = listed.get("result", {}).get("tools", [])
        names = [tool.get("name") for tool in tools if isinstance(tool, dict)]
        if "kontor_realm_get" not in names:
            raise Refused("the MCP server does not serve kontor_realm_get")
        called = session.request(
            "tools/call", {"name": "kontor_realm_get", "arguments": {}}
        )
        result = called.get("result", {})
        if result.get("isError") is True:
            raise Refused("the MCP kontor_realm_get call returned an error")
        blocks = result.get("content", [])
        text = next(
            (
                block.get("text")
                for block in blocks
                if isinstance(block, dict) and block.get("type") == "text"
            ),
            None,
        )
        if not isinstance(text, str):
            raise Refused("the MCP kontor_realm_get result carries no text document")
        document = json.loads(text)
        if not isinstance(document, dict) or document.get("status") != 200:
            raise Refused("the MCP kontor_realm_get result is not a successful answer")
        realm = document.get("body", {})
        if not isinstance(realm, dict):
            raise Refused("the MCP kontor_realm_get answer has no body")
        return {"toolCount": len(names), "realm": realm}
    except ValueError as error:
        raise Refused(f"the MCP kontor_realm_get result is not JSON: {error}") from error
    finally:
        session.close()


def readback(
    state_root: Path,
    base_url: str,
    expected_realm: str,
    expected_schema: int,
    install_dir: Path,
    expected_sha256: dict[str, str],
    require_build_commit: bool,
) -> dict:
    """The runbook's readback set, as one JSON document."""
    token = observer_token(state_root)
    health = _request(f"{base_url}/v1/health", token)
    realm_answer = _request(f"{base_url}/v1/realm", token)
    if health.get("realm_id") != expected_realm:
        raise Refused(f"health reports realm {health.get('realm_id')!r}, not {expected_realm!r}")
    if health.get("live") is not True:
        raise Refused("health does not report the realm live")
    if health.get("schema_version") != expected_schema:
        raise Refused(
            f"health reports schema {health.get('schema_version')}, not {expected_schema}"
        )
    if realm_answer.get("realm_id") != expected_realm:
        raise Refused(
            f"/v1/realm reports realm {realm_answer.get('realm_id')!r}, not {expected_realm!r}"
        )

    cli = run_cli(install_dir, state_root, base_url)
    if cli.get("status") != 200:
        raise Refused(f"kontor realm-get reports status {cli.get('status')}")
    cli_realm = cli.get("body", {}).get("realm_id")
    if cli_realm != expected_realm:
        raise Refused(f"kontor realm-get reports realm {cli_realm!r}, not {expected_realm!r}")

    binaries = {}
    for name in BINARIES:
        binary = install_dir / name
        if not binary.is_file():
            raise Refused(f"{binary} is not installed")
        actual = digest(binary)
        expected = expected_sha256.get(name)
        if expected is not None and actual != expected:
            raise Refused(f"{binary} has SHA-256 {actual}, not the deployed {expected}")
        record = None
        try:
            record = read_sidecar(install_dir, name)
        except Refused:
            if require_build_commit:
                raise
        if record is not None and record.get("sha256") != actual:
            raise Refused(
                f"{binary} has SHA-256 {actual}, not the {record.get('sha256')} in its sidecar"
            )
        binaries[name] = {
            "sha256": actual,
            "sourceCommit": record.get("sourceCommit") if record else None,
            "sidecar": record is not None,
        }

    parity = mcp_parity(install_dir, state_root, base_url)
    mcp_realm = parity["realm"].get("realm_id")
    if mcp_realm != expected_realm:
        raise Refused(f"MCP kontor_realm_get reports realm {mcp_realm!r}, not {expected_realm!r}")

    return {
        "checkedAt": _now(),
        "realmId": expected_realm,
        "baseUrl": base_url,
        "health": health,
        "realm": realm_answer,
        "realmGet": cli,
        "schema": {
            "expected": expected_schema,
            "health": health.get("schema_version"),
        },
        "binaries": binaries,
        "mcpParity": {
            "toolCount": parity["toolCount"],
            "realm": mcp_realm,
            "matchesHttp": True,
        },
    }


def _parse_expected(values: list[str]) -> dict[str, str]:
    expected = {}
    for value in values:
        name, separator, checksum = value.partition("=")
        if not separator or name not in BINARIES or SHA256_PATTERN.fullmatch(checksum) is None:
            raise SystemExit(
                "--expected-sha256 needs NAME=SHA256 for one of " + ", ".join(BINARIES)
            )
        expected[name] = checksum
    return expected


def parse_arguments(argv: list[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        prog="kontor-ops.py",
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
        allow_abbrev=False,
    )
    commands = parser.add_subparsers(dest="command", required=True)

    record = commands.add_parser("record-build", help="write build-commit sidecars")
    record.add_argument("--install-dir", type=Path, required=True)
    record.add_argument("--commit", required=True)
    record.add_argument("--tree")
    record.add_argument("--recorded-at")

    verify = commands.add_parser("verify-build", help="check build-commit sidecars")
    verify.add_argument("--install-dir", type=Path, required=True)
    verify.add_argument("--commit")

    read = commands.add_parser("readback", help="run the post-deploy readback set")
    read.add_argument("--state-root", type=Path, required=True)
    read.add_argument("--base-url", required=True)
    read.add_argument("--expected-realm", required=True)
    read.add_argument("--expected-schema", type=int, required=True)
    read.add_argument("--install-dir", type=Path, required=True)
    read.add_argument("--expected-sha256", action="append", default=[], metavar="NAME=SHA256")
    read.add_argument("--require-build-commit", action="store_true")
    read.add_argument("--json-out", type=Path)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    arguments = parse_arguments(argv)
    try:
        if arguments.command == "record-build":
            document = record_build(
                arguments.install_dir,
                arguments.commit,
                arguments.tree,
                arguments.recorded_at,
            )
        elif arguments.command == "verify-build":
            document = verify_build(arguments.install_dir, arguments.commit)
        else:
            document = readback(
                arguments.state_root,
                arguments.base_url.rstrip("/"),
                arguments.expected_realm,
                arguments.expected_schema,
                arguments.install_dir,
                _parse_expected(arguments.expected_sha256),
                arguments.require_build_commit,
            )
            if arguments.json_out is not None:
                _write_json(arguments.json_out, document)
    except Refused as refusal:
        print(f"kontor-ops: refused: {refusal}", file=sys.stderr)
        return 1
    except (OSError, subprocess.SubprocessError) as error:
        print(f"kontor-ops: failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps(document, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    signal.signal(signal.SIGPIPE, signal.SIG_DFL)
    sys.exit(main())
