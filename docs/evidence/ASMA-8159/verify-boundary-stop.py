#!/usr/bin/env python3
"""Read-only reproduction of the production-prep credential-boundary findings.

Only the named receipt is written. No daemon, database, secret resolver, test,
Git mutation, credential installation or deployment is invoked.
"""

import hashlib
import json
from pathlib import Path
import subprocess
import sys
from datetime import datetime, timezone


ROOT = Path(__file__).resolve().parents[3]
ROOT_REPOSITORY = Path("/Users/igor/carasent/asma-modules")
OUTPUT = ROOT / "docs/evidence/ASMA-8159/receipts/production-prep-boundary-stop.json"
BASE = "fe5373f2d2a2c147a6e2b6398c07b5091eaa1ca0"
MODULE_DEFAULT = "8acdbd17e83d9d7ec545221c4b957e0325916997"
DOCS = "7d19280857ae3c60a870e017da3f733f9ff2625b"
ROOT_DEFAULT = "25de2b541da5d8dd61082e2d150c5c8087b09ac6"


def sha(data):
    return hashlib.sha256(data).hexdigest()


def now():
    return datetime.now(timezone.utc).isoformat()


def main():
    receipt = {
        "schema_version": 1,
        "task": "ASMA-8159",
        "authorization": "44d5d916-d778-46b8-b00d-2cca01cc6553/8155-production-prep",
        "result": "STOP_RESERVED_CREDENTIAL_BOUNDARY",
        "started_utc": now(),
        "runner": {
            "path": str(Path(__file__).resolve()),
            "sha256": sha(Path(__file__).read_bytes()),
            "argv": sys.argv,
        },
        "commands": [],
        "source_excerpts": [],
    }

    def command(argv, cwd=ROOT):
        started = now()
        result = subprocess.run(
            argv, cwd=cwd, capture_output=True, text=True, timeout=45, check=False
        )
        receipt["commands"].append({
            "argv": argv, "cwd": str(cwd), "started_utc": started,
            "finished_utc": now(), "exit": result.returncode,
            "stdout": result.stdout, "stderr": result.stderr,
        })
        if result.returncode != 0:
            raise RuntimeError(f"read-only command failed: {argv}")
        return result.stdout

    try:
        command(["git", "status", "--short"])
        command(["git", "rev-parse", "HEAD", "HEAD^{tree}"])
        command(["git", "branch", "--show-current"])
        for pin in ["655e07ead8b87bebf3a5d43ddccc15c860114e05", "831dd7c1dc95a57258c75f5896bff5b76f3830fa", BASE]:
            command(["git", "merge-base", "--is-ancestor", pin, "HEAD"])
        command(["git", "remote", "get-url", "origin"])
        command(["git", "ls-remote", "--symref", "origin", "HEAD"])
        command(["git", "remote", "get-url", "origin"], ROOT_REPOSITORY)
        command(["git", "ls-remote", "--symref", "origin", "HEAD"], ROOT_REPOSITORY)
        command(["git", "rev-list", "--left-right", "--count", f"{MODULE_DEFAULT}...{BASE}"])
        command(["git", "merge-base", MODULE_DEFAULT, BASE])
        command(["git", "rev-list", "--left-right", "--count", f"{ROOT_DEFAULT}...{DOCS}"], ROOT_REPOSITORY)
        command(["git", "merge-base", ROOT_DEFAULT, DOCS], ROOT_REPOSITORY)
        command(["asma", "git", "--help"])
        command(["git", "diff", "--check"])

        manifest_path = ROOT / "docs/evidence/ASMA-8159/receipts/source-manifest.json"
        manifest = json.loads(manifest_path.read_text())
        files = manifest["files"]
        changed = [path for path, digest in files.items() if sha((ROOT / path).read_bytes()) != digest]
        receipt["accepted_source_integrity"] = {
            "manifest_path": str(manifest_path.relative_to(ROOT)),
            "manifest_file_sha256": sha(manifest_path.read_bytes()),
            "file_count": len(files), "changed": changed,
            "files_map_sha256": sha(json.dumps(files, sort_keys=True).encode()),
        }
        if changed:
            raise RuntimeError("accepted source bytes changed")

        migrations = sorted((ROOT / "crates/kontor-store/migrations").glob("*.sql"))
        numbers = [int(path.name.split("_", 1)[0]) for path in migrations]
        receipt["migrations"] = {
            "count": len(numbers), "maximum": max(numbers),
            "missing": sorted(set(range(1, max(numbers) + 1)) - set(numbers)),
            "duplicates": sorted({n for n in numbers if numbers.count(n) > 1}),
            "memory_0120_sha256": sha((ROOT / "crates/kontor-store/migrations/0120_experience_memory_projection.sql").read_bytes()),
        }

        for name, start, end in [
            ("crates/kontor-daemon/src/runtimes.rs", 422, 435),
            ("crates/kontor-accounts/src/resolver.rs", 1, 34),
            ("crates/kontor-accounts/src/resolver.rs", 896, 913),
            ("crates/kontor-jira/src/credentials.rs", 14, 41),
            ("crates/kontor-daemon/src/github_publication.rs", 10, 17),
            ("crates/kontor-memory-cognee/src/lib.rs", 140, 158),
            ("crates/kontor-daemon/src/lib.rs", 344, 349),
            ("crates/kontor-daemon/src/lib.rs", 572, 586),
            ("crates/kontor-daemon/src/main.rs", 294, 310),
            ("crates/kontor-daemon/src/applications.rs", 19263, 19286),
        ]:
            data = (ROOT / name).read_bytes()
            lines = data.decode().splitlines()
            receipt["source_excerpts"].append({
                "path": name, "sha256": sha(data), "start": start, "end": end,
                "text": "\n".join(f"{i + 1}: {line}" for i, line in enumerate(lines) if start <= i + 1 <= end),
            })
        command(["rg", "-n", "ResolverPolicy::builder|KeychainTarget::new|AccountResolver::new|\\.keychain\\(", "crates", "--glob", "*.rs", "--glob", "!**/tests/**", "--glob", "!**/target/**"])
        receipt["verification"] = "PASS_READ_ONLY_INTEGRITY_AND_FINDING_CAPTURE"
    finally:
        receipt["finished_utc"] = now()
        OUTPUT.write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"receipt": str(OUTPUT.relative_to(ROOT)), "result": receipt["result"], "verification": receipt["verification"]}))


if __name__ == "__main__":
    main()
