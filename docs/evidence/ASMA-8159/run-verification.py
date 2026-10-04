#!/usr/bin/env python3
"""Record offline combined checks and sequential synthetic source mutants.

Run from this checkout. No service, credential, installation or Git mutation.
Each mutant is restored from exact bytes; never restore over another writer.
"""

from __future__ import annotations

import argparse
import difflib
import gzip
import hashlib
import json
from pathlib import Path
import re
import runpy
import shlex
import subprocess
import time

ROOT = Path(__file__).resolve().parents[3]
RECEIPTS = Path(__file__).resolve().parent / "receipts"
PACKAGES = ["kontor-core", "kontor-store", "kontor-api", "kontor-daemon",
            "kontor-cli", "kontor-mcp", "kontor-memory-cognee"]
PACKAGE_ARGS = [arg for name in PACKAGES for arg in ("-p", name)]
OFFLINE = ["--locked", "--offline"]
CHECKS = [
    ("combined-build", ["cargo", "build", *PACKAGE_ARGS, *OFFLINE]),
    ("core", ["cargo", "test", "-p", "kontor-core", "--lib",
              "--test", "experience_memory", *OFFLINE]),
    ("store-memory", ["cargo", "test", "-p", "kontor-store", "--lib",
                      "memory::", *OFFLINE]),
    ("schema", ["cargo", "test", "-p", "kontor-store", "--test",
                "schema_v1", *OFFLINE]),
    ("backup", ["cargo", "test", "-p", "kontor-store", "--test",
                "backup_snapshot", *OFFLINE,
                "memory_ledger_and_import_evidence_restore_"
                "while_fts_is_rebuilt"]),
    ("api-mcp", ["cargo", "test", "-p", "kontor-api", "-p",
                 "kontor-mcp", "--lib", "--tests", *OFFLINE]),
    ("http-cli", ["cargo", "test", "-p", "kontor-daemon", "--test",
                  "experience_memory", "-p", "kontor-cli", "--test",
                  "memory_parity", *OFFLINE]),
    ("registry-parity", ["cargo", "test", "-p", "kontor-tests-contract",
                         "--test", "mcp_parity", "--test", "mcp_cardinality",
                         "--test", "mcp_mutants", *OFFLINE]),
    ("cognee-fixtures", ["cargo", "test", "-p", "kontor-memory-cognee",
                         "--test", "fixtures", *OFFLINE]),
    ("launch-fixtures", ["cargo", "test", "-p", "kontor-daemon",
                         "--test", "loopback_api", "experience_launch::",
                         *OFFLINE]),
    ("context-approved", ["cargo", "test", "-p", "kontor-daemon",
                          "--test", "loopback_api", *OFFLINE, "--",
                          "resolving_a_task_context_tracks_approved_memory_"
                          "and_returns_no_content",
                          "--exact"]),
    ("context-generic", ["cargo", "test", "-p", "kontor-daemon",
                         "--test", "loopback_api", *OFFLINE, "--",
                         "generic_corpus_past_the_general_ceiling_is_excluded_"
                         "by_typed_recall",
                         "--exact"]),
    ("combined-clippy", ["cargo", "clippy", *PACKAGE_ARGS,
                         "--all-targets", *OFFLINE, "--", "-D", "warnings"]),
    ("fmt", ["cargo", "fmt", "--all", "--", "--check"]),
    ("accepted-whitespace", ["git", "diff", "--check", "8acdbd17", "HEAD"]),
    ("working-whitespace", ["git", "diff", "--check"]),
    ("dependency-policy", ["cargo", "deny", "--offline", "check",
                           "licenses", "bans", "sources"]),
]


def sha(data: bytes) -> str:
    """Hash exact source/log bytes."""
    return hashlib.sha256(data).hexdigest()


def save(name: str, value: object) -> None:
    """Persist every command, including failures."""
    (RECEIPTS / name).write_text(json.dumps(value, indent=2) + "\n")


def run(command: list[str], name: str) -> dict:
    """Capture complete local output without printing inherited environment."""
    print(name + ": " + shlex.join(command), flush=True)
    started = time.monotonic()
    raw = RECEIPTS / (name + ".raw")
    with raw.open("wb") as handle:
        result = subprocess.run(command, cwd=ROOT, stdout=handle,
                                stderr=subprocess.STDOUT)
    data = raw.read_bytes()
    output = data.decode(errors="replace")
    archive = RECEIPTS / (name + ".log.gz")
    archive.write_bytes(gzip.compress(data, mtime=0))
    raw.unlink()
    record = {
        "name": name, "command": command, "shell_command": shlex.join(command),
        "cwd": str(ROOT), "exit_code": result.returncode,
        "seconds": round(time.monotonic() - started, 3),
        "log": str(archive.relative_to(ROOT)),
        "log_sha256": sha(archive.read_bytes()), "raw_sha256": sha(data),
        "compiled": "Finished `test` profile" in output,
        "test_results": re.findall(r"^test result: .*$", output, re.M),
        "failed_tests": re.findall(r"^test (.+) \.\.\. FAILED$", output, re.M),
        "compile_failure": "could not compile" in output,
    }
    print(name + ": exit " + str(result.returncode), flush=True)
    return record


def verify_manifest() -> None:
    """Reject source drift against the initial combined tree."""
    frozen = json.loads((RECEIPTS / "source-manifest.json").read_text())
    for name, digest in frozen["files"].items():
        if sha((ROOT / name).read_bytes()) != digest:
            raise RuntimeError("source drift: " + name)


def replace_once(text: str, before: str, after: str) -> str:
    """Require a unique mutation anchor."""
    if text.count(before) != 1:
        raise RuntimeError("mutation anchor is not unique: " + before)
    return text.replace(before, after, 1)


def mutation_cases() -> list:
    """Reuse accepted transforms without changing historical receipts."""
    prior = runpy.run_path(
        str(ROOT / "docs/evidence/ASMA-8158/run-mutations.py"))
    core = "crates/kontor-core/src/memory.rs"
    store = "crates/kontor-store/src/memory.rs"
    specs = [
        ("MUT-001", core,
         "ExperienceMemoryV1::from_document(document).is_ok()",
         'document.json().contains("operational_gap") || '
         "ExperienceMemoryV1::from_document(document).is_ok()",
         "kontor-core", "--test", "experience_memory",
         "strict_experience_roundtrip_and_non_memory_eligibility"),
        ("MUT-002", store,
         "WHERE r.project_id=?1 AND r.project_id=?2 AND r.item_id=?3 "
         "AND r.id=?4 AND r.content_hash=?5",
         "WHERE r.item_id=?3 AND r.id=?4 AND r.content_hash=?5",
         "kontor-store", "--lib", None,
         "memory::experience_tests::malicious_tuple_project_current_approval_"
         "tombstone_hash_policy_and_kind"),
        ("MUT-003", store, "if next_bytes > MAX_RECALL_BYTES {",
         "if next_bytes > MAX_RECALL_BYTES + 1 {",
         "kontor-store", "--lib", None,
         "memory::experience_tests::exact_budget_32768_32769_unicode_escaping_"
         "and_skip_oversized_top"),
        ("MUT-007", store,
         "if !qualification.added || !qualification.cognified "
         "|| !qualification.canary_passed {",
         "if !qualification.added || !qualification.cognified {",
         "kontor-store", "--lib", None,
         "memory::experience_tests::projection_minimal_payload_failures_"
         "freshness_and_compare_and_swap"),
    ]
    cases = []
    for key, path, before, after, package, target, binary, witness in specs:
        def transform(text: str, b=before, a=after) -> str:
            return replace_once(text, b, a)
        command = ["cargo", "test", "-p", package, target]
        if binary:
            command.append(binary)
        command += OFFLINE + ["--", witness, "--exact"]
        cases.append((key, path, transform, command))
    for key, path, transform, command in prior["MUTANTS"]:
        split = command.index("--")
        command = command[:split] + OFFLINE + command[split:]
        cases.append((key, path, transform, command))
    return sorted(cases)


def mutations() -> None:
    """Run baseline, compiled mutant and restored witness sequentially."""
    results = []
    for key, name, transform, command in mutation_cases():
        verify_manifest()
        path = ROOT / name
        original = path.read_bytes()
        baseline = run(command, key.lower() + "-baseline")
        if baseline["exit_code"] or not any(
            "1 passed; 0 failed" in row for row in baseline["test_results"]
        ):
            save("mutations.json", results + [{"mutant": key,
                 "verdict": "BASELINE_FAILED", "baseline": baseline}])
            raise RuntimeError(key + ": baseline failed or witness missing")
        mutant = transform(original.decode()).encode()
        patch = "".join(difflib.unified_diff(
            original.decode().splitlines(True),
            mutant.decode().splitlines(True),
            fromfile=name, tofile=name)).encode()
        archive = RECEIPTS / (key.lower() + ".patch.gz")
        archive.write_bytes(gzip.compress(patch, mtime=0))
        try:
            path.write_bytes(mutant)
            seeded = run(command, key.lower() + "-seeded")
        finally:
            if path.read_bytes() != mutant:
                raise RuntimeError(
                    key + ": concurrent writer; restore withheld")
            path.write_bytes(original)
        verify_manifest()
        restored = run(command, key.lower() + "-restored")
        killed = (seeded["compiled"] and seeded["failed_tests"]
                  and not seeded["compile_failure"] and seeded["exit_code"])
        verdict = "KILLED" if killed else "SURVIVED_OR_INVALID"
        results.append({
            "mutant": key, "path": name, "verdict": verdict,
            "base_sha256": sha(original), "mutant_sha256": sha(mutant),
            "restored_sha256": sha(path.read_bytes()),
            "patch": str(archive.relative_to(ROOT)),
            "patch_sha256": sha(patch),
            "patch_gzip_sha256": sha(archive.read_bytes()),
            "baseline": baseline, "seeded": seeded, "restored": restored,
        })
        save("mutations.json", results)
        if verdict != "KILLED" or restored["exit_code"]:
            raise RuntimeError(key + ": qualification incomplete; STOP")


def main() -> None:
    """Stop at failed checks without repairing accepted source."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mutations", action="store_true")
    args = parser.parse_args()
    RECEIPTS.mkdir(parents=True, exist_ok=True)
    verify_manifest()
    if args.mutations:
        mutations()
        return
    results = []
    for name, command in CHECKS:
        result = run(command, name)
        results.append(result)
        save("verification.json", results)
        if result["exit_code"]:
            raise RuntimeError(name + ": failed; STOP for owning-seat routing")
        if command[:2] == ["cargo", "test"] and not any(
            re.search(r"ok\. [1-9][0-9]* passed", row)
            for row in result["test_results"]
        ):
            raise RuntimeError(name + ": no tests executed")
    verify_manifest()


if __name__ == "__main__":
    main()
