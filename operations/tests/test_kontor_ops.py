"""Tests for operations/scripts/kontor-ops.py.

The readback tests drive the real HTTP, CLI and stdio MCP paths against local
fakes, so a mismatch in any of the three surfaces is a failure rather than a
mocked assertion.
"""

from __future__ import annotations

import importlib.util
import json
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

REALM = "01a00649-9ee6-73e0-ba1b-6a6c35cfd065"
COMMIT = "9e4524be043056c0b215ab48d592593fe6baa30b"
TREE = "9c278f79a6ba1476fb5e3e0453b4576239e58544"
SCHEMA = 134

SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "kontor-ops.py"
spec = importlib.util.spec_from_file_location("kontor_ops", SCRIPT)
assert spec is not None and spec.loader is not None
kontor_ops = importlib.util.module_from_spec(spec)
spec.loader.exec_module(kontor_ops)


def write_executable(path: Path, body: str) -> None:
    path.write_text(body)
    path.chmod(0o755)


class FakeRealm(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        if self.headers.get("Authorization") != "Bearer test-observer":
            self.send_response(401)
            self.end_headers()
            return
        if self.path == "/v1/health":
            body = {
                "realm_id": REALM,
                "live": True,
                "schema_version": SCHEMA,
                "reconciliation": "open",
                "scheduling_open": True,
                "runtimes": [],
            }
        elif self.path == "/v1/realm":
            body = {
                "realm_id": REALM,
                "schema_version": 1,
                "created_at": "2026-08-15T16:38:03.239349Z",
                "display_label": None,
            }
        else:
            self.send_response(404)
            self.end_headers()
            return
        encoded = json.dumps(body).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)

    def log_message(self, *_args: object) -> None:
        pass


def fake_cli() -> str:
    return f"""#!/usr/bin/env python3
import json
print(json.dumps({{"status": 200, "body": {{"realm_id": "{REALM}", "schema_version": 1}}}}))
"""


def fake_mcp(realm: str = REALM) -> str:
    return f"""#!/usr/bin/env python3
import json
import sys

for line in sys.stdin:
    message = json.loads(line)
    if "id" not in message:
        continue
    method = message["method"]
    if method == "initialize":
        result = {{"protocolVersion": "2025-06-18", "capabilities": {{
            "tools": {{}}}} , "serverInfo": {{"name": "fake", "version": "1"}}}}
    elif method == "tools/list":
        result = {{"tools": [{{"name": "kontor_realm_get"}}]}}
    elif method == "tools/call":
        result = {{"content": [{{"type": "text", "text": json.dumps(
            {{"status": 200, "body": {{"realm_id": "{realm}"}}}})}}],
            "isError": False}}
    else:
        result = {{}}
    print(json.dumps({{"jsonrpc": "2.0", "id": message["id"], "result": result}}), flush=True)
"""


def install_fixture(root: Path, mcp_realm: str = REALM) -> Path:
    install = root / "install"
    install.mkdir()
    for name in kontor_ops.BINARIES:
        (install / name).write_bytes(f"{name} fixture\n".encode())
    write_executable(install / "kontor", fake_cli())
    write_executable(install / "kontor-mcp", fake_mcp(mcp_realm))
    state_root = root / "realm"
    state_root.mkdir()
    (state_root / "credentials.json").write_text(json.dumps({"observer": "test-observer"}))
    (state_root / "credentials.json").chmod(0o600)
    return install


class BuildCommitTests(unittest.TestCase):
    def test_record_and_verify_bind_each_binary_to_its_commit(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            install = install_fixture(Path(directory))
            records = kontor_ops.record_build(install, COMMIT, TREE, None)
            self.assertEqual(set(records), set(kontor_ops.BINARIES))
            verified = kontor_ops.verify_build(install, COMMIT)
            self.assertEqual(verified["kontor-daemon"]["sourceTree"], TREE)

    def test_a_changed_binary_fails_verification(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            install = install_fixture(Path(directory))
            kontor_ops.record_build(install, COMMIT, TREE, None)
            (install / "kontor-daemon").write_bytes(b"different\n")
            with self.assertRaises(kontor_ops.Refused):
                kontor_ops.verify_build(install, None)

    def test_verification_refuses_a_different_commit(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            install = install_fixture(Path(directory))
            kontor_ops.record_build(install, COMMIT, TREE, None)
            with self.assertRaises(kontor_ops.Refused):
                kontor_ops.verify_build(install, "0" * 40)


class ReadbackTests(unittest.TestCase):
    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        self.install = install_fixture(self.root)
        kontor_ops.record_build(self.install, COMMIT, TREE, None)
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), FakeRealm)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.base_url = f"http://127.0.0.1:{self.server.server_address[1]}"

    def tearDown(self) -> None:
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)
        self.directory.cleanup()

    def readback(self) -> dict:
        return kontor_ops.readback(
            self.root / "realm",
            self.base_url,
            REALM,
            SCHEMA,
            self.install,
            {},
            True,
        )

    def test_readback_passes_across_http_cli_and_mcp(self) -> None:
        document = self.readback()
        self.assertEqual(document["health"]["schema_version"], SCHEMA)
        self.assertEqual(document["mcpParity"]["realm"], REALM)
        self.assertTrue(document["binaries"]["kontor-mcp"]["sidecar"])

    def test_mcp_realm_disagreement_fails_the_readback(self) -> None:
        write_executable(self.install / "kontor-mcp", fake_mcp("0" * 8 + "-other"))
        with self.assertRaises(kontor_ops.Refused):
            self.readback()

    def test_expected_schema_mismatch_fails(self) -> None:
        with self.assertRaises(kontor_ops.Refused):
            kontor_ops.readback(
                self.root / "realm",
                self.base_url,
                REALM,
                SCHEMA + 1,
                self.install,
                {},
                True,
            )

    def test_missing_sidecar_fails_when_required(self) -> None:
        (self.install / "kontor.build-commit.json").unlink()
        with self.assertRaises(kontor_ops.Refused):
            self.readback()


if __name__ == "__main__":
    unittest.main()
