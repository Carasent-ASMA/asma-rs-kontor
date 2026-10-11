#!/usr/bin/env python3
"""Record ASMA-8454 checks; seed exactly one mutant and always restore it.

Runs only local test fixtures. It never starts a live daemon, calls Jira,
selects a live pin, or publishes a release. Logs contain test output only.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / ".qualification"
CARGO = ["cargo", "test", "--offline", "--locked"]
LOOPBACK = CARGO + ["-p", "kontor-daemon", "--test", "loopback_api"]


def digest(data: bytes) -> str:
    """Hash exact bytes, including intentional whitespace."""
    return hashlib.sha256(data).hexdigest()


def git(*args: str) -> str:
    """Read this checkout without changing Git state."""
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True)


def environment() -> dict[str, str]:
    """Keep build and temporary fixtures inside the owning checkout."""
    env = os.environ.copy()
    env["RUSTC_WRAPPER"] = ""
    env["TMPDIR"] = str(OUTPUT / "tmp")
    # Nested disposable repositories must not discover this worktree's .git.
    env["GIT_CEILING_DIRECTORIES"] = str(OUTPUT / "tmp")
    env["CARGO_TARGET_DIR"] = str(ROOT / "target")
    archive = OUTPUT / "swagger-ui.zip"
    if archive.exists():
        env["SWAGGER_UI_DOWNLOAD_URL"] = archive.as_uri()
    env.pop("KONTOR_AUTH", None)
    return env


def run(name: str, command: list[str]) -> dict[str, object]:
    """Record command, time, exit status and complete output digest."""
    started = datetime.now(timezone.utc).isoformat()
    started_head = git("rev-parse", "HEAD").strip()
    log = OUTPUT / f"{name}.log"
    print(f"Running {name}", flush=True)
    with log.open("wb") as stream:
        result = subprocess.run(command, cwd=ROOT, env=environment(),
                                stdout=stream, stderr=subprocess.STDOUT)
    output = log.read_bytes()
    receipt = {
        "name": name, "head": git("rev-parse", "HEAD").strip(),
        "started_head": started_head,
        "started_at": started, "command": command,
        "exit_code": result.returncode, "log": str(log.relative_to(ROOT)),
        "log_sha256": digest(output),
    }
    (OUTPUT / f"{name}.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(f"Finished {name}: {result.returncode}", flush=True)
    return receipt


def baseline() -> None:
    """Run the four shipped daemon contracts and integrated rehearsal."""
    for suite in ["durable_desks::", "durable_seats::",
                  "desk_adams::", "pr_reviews::"]:
        receipt = run(f"baseline-{suite[:-2]}", LOOPBACK + [
            suite, "--", "--nocapture", "--test-threads=2"])
        if receipt["exit_code"] != 0:
            raise SystemExit("Red baseline; mutations are forbidden")


def mutations() -> None:
    """Require green baselines; compile and test each isolated fault."""
    catalog = json.loads((ROOT / "scripts/durable-desk-mutants.json").read_text())
    # Every exact target suite earns a fresh green baseline before it is used.
    checked: set[str] = set()
    results = []
    for mutant in catalog:
        command = mutant.get("command", LOOPBACK + [mutant["suite"], "--", "--test-threads=2"])
        target = json.dumps(command)
        if target not in checked:
            result = run(f"green-{mutant['id']}", command)
            if result["exit_code"] != 0:
                raise SystemExit("Red baseline; no mutant seeded")
            checked.add(target)
        path = ROOT / mutant["file"]
        before = path.read_bytes()
        status = git("status", "--porcelain", "--", mutant["file"])
        text = before.decode()
        if text.count(mutant["before"]) != 1:
            raise SystemExit(f"{mutant['id']}: seed is not unique")
        line = text[:text.index(mutant["before"])].count("\n") + 1
        seeded = text.replace(mutant["before"], mutant["after"], 1).encode()
        try:
            path.write_bytes(seeded)
            receipt = run(mutant["id"], command)
            log = (OUTPUT / f"{mutant['id']}.log").read_text()
            if "test result: FAILED" in log and receipt["exit_code"] == 101:
                verdict = "KILLED"
            elif receipt["exit_code"] == 0:
                verdict = "SURVIVED"
            else:
                verdict = "INVALID_OR_BLOCKED"
            results.append({**mutant, **receipt, "line": line,
                            "source_sha256": digest(before),
                            "seeded_sha256": digest(seeded), "result": verdict,
                            "failed_tests": re.findall(r"^test (.+) \.\.\. FAILED$", log, re.M),
                            "failure_excerpts": re.findall(r"thread[^\n]*panicked[^\n]*\n(?:[^\n]*\n){0,5}", log)})
        finally:
            path.write_bytes(before)
            if (path.read_bytes() != before
                    or git("status", "--porcelain", "--", mutant["file"]) != status):
                raise SystemExit("Restoration mismatch; stop immediately")
        (OUTPUT / "mutation-results.json").write_text(
            json.dumps(results, indent=2) + "\n")
    if any(row["result"] != "KILLED" for row in results):
        raise SystemExit("Unresolved mutation result blocks completion")
    # Prove the restored production source is green again.
    baseline()


def declarations(source: Path) -> None:
    """Compare a read-only, byte-pinned dated decision with fixture readback."""
    expected = "2297aba99eb830c35d9a5f386635162a008056dcdcd1bc1d3c019db33ea9cb05"
    content = source.read_bytes()
    if digest(content) != expected:
        raise SystemExit("Declaration source changed; Igor's newer decision is required")
    blocks = re.findall(r"  - name: '([^']+)'\n(.*?)(?=  - name:|\Z)",
                        content.decode().split("durable_seats:\n", 1)[1], re.S)
    rows = []
    log = (OUTPUT / "baseline-pr_reviews.log").read_text()
    marker = "ASMA-8454 integrated evidence: "
    evidence = json.loads(next(line.split(marker, 1)[1] for line in log.splitlines()
                               if marker in line))
    seats = evidence["durable_seats_readback"]["seats"]
    for title, block in blocks:
        fields = dict(re.findall(r"^    (\w+):\s*'?([^'\n]+)'?$", block, re.M))
        matching = [seat for seat in seats if seat["title"] == title]
        assert len(matching) == 1, f"Exact declaration missing: {title}"
        seat = matching[0]
        assert fields["workspace"] == "ADAM"
        assert fields["model_chain"] == "premium"
        assert seat["role_code"] == fields["role"] == "LSA"
        assert seat["decided_by"] == fields["decided_by"] == "Igor"
        assert seat["decided_on"] == fields["date"] == "2026-10-08"
        assert seat["procedure"] == fields["skill"]
        assert seat["recovery_owner"] == "paseo_direct"
        assert seat["status"] == "unchecked" and seat.get("native_id") is None
        assert seat.get("seat_binding_id") is None and seat.get("model_route") is None
        rows.append({"source_title": title, "source_fields": fields,
                     "fixture_readback": seat})
    assert len(rows) == 2
    receipt = {"source_file": str(source), "source_sha256": expected,
               "source_status": "uncommitted dated decision, branch feat/ASMA-8419-pr-draft-status-checker",
               "reported_source_head": "c4f9957c", "dated_owner": "Igor, 2026-10-08",
               "authority": "coordinator Q-002 disposition supplied 2026-10-11T13:16Z",
               "read_at": datetime.now(timezone.utc).isoformat(), "comparison": rows,
               "premium_chain": "preserved in source; not resolved, adopted or launched",
               "real_seat_effects": 0}
    (OUTPUT / "asma-8419-comparison.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print("Pinned declarations match; no runtime adoption or launch", flush=True)


def main() -> None:
    """Choose one reproducible phase; never perform publication."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=["baseline", "mutations", "declarations"])
    parser.add_argument("--declaration-source", type=Path)
    args = parser.parse_args()
    (OUTPUT / "tmp").mkdir(parents=True, exist_ok=True)
    if args.phase == "baseline":
        baseline()
    elif args.phase == "mutations":
        mutations()
    else:
        if args.declaration_source is None:
            parser.error("declarations requires --declaration-source")
        declarations(args.declaration_source)


if __name__ == "__main__":
    main()
