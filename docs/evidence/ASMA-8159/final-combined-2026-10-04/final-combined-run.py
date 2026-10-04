#!/usr/bin/env python3
"""QA final-combined execution runner (ASMA-8159, independent seat).

Reuses the recorded in-tree mutation transforms (docs/evidence/ASMA-8159/
run-verification.py mutation_cases) and the recorder shape of
production-prep/run.py, but writes ONLY to this new receipt directory and
refuses any existing name. Never overwrites historical receipts.

Gates executed by this runner are driven from the shell; this module provides
`rec` (command recorder) and `mutants` (baseline/seed/restored triples).
"""
from __future__ import annotations

import argparse
import difflib
import gzip
import hashlib
import json
import re
import runpy
import shlex
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
OUT = Path(__file__).resolve().parent / "receipts"
CASES = runpy.run_path(str(ROOT / "docs/evidence/ASMA-8159/run-verification.py"))[
    "mutation_cases"
]()


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def manifest() -> dict:
    names = subprocess.check_output(
        [
            "git", "ls-files", "--cached", "--others", "--exclude-standard", "--",
            "crates", "tests", "apps", "scripts", "Cargo.toml", "Cargo.lock",
            "deny.toml",
            "docs/evidence/ASMA-8159/production-prep/census.sql",
        ],
        cwd=ROOT,
        text=True,
    ).splitlines()
    return {
        name: sha((ROOT / name).read_bytes())
        for name in sorted(names)
        if (ROOT / name).is_file()
    }


def rec(name: str, argv: list[str], expect: int | None = None) -> dict:
    """Record one command with exact output/hash provenance; refuse reuse."""
    OUT.mkdir(parents=True, exist_ok=True)
    if (OUT / (name + ".json")).exists():
        raise RuntimeError("receipt already exists: " + name)
    before = manifest()
    started = datetime.now(timezone.utc).isoformat()
    clock = time.monotonic()
    raw = OUT / (name + ".raw")
    print(name + ": " + shlex.join(argv), flush=True)
    with raw.open("wb") as stream:
        completed = subprocess.run(argv, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT)
    data = raw.read_bytes()
    text = data.decode(errors="replace")
    archive = OUT / (name + ".log.gz")
    archive.write_bytes(gzip.compress(data, mtime=0))
    raw.unlink()
    record = {
        "name": name,
        "argv": argv,
        "command": shlex.join(argv),
        "cwd": str(ROOT),
        "runner_sha256": sha(Path(__file__).read_bytes()),
        "started_utc": started,
        "finished_utc": datetime.now(timezone.utc).isoformat(),
        "seconds": round(time.monotonic() - clock, 3),
        "exit": completed.returncode,
        "log": str(archive.relative_to(ROOT)),
        "raw_sha256": sha(data),
        "gzip_sha256": sha(archive.read_bytes()),
        "compiled": "Finished `test` profile" in text,
        "compile_failure": "could not compile" in text,
        "tests": re.findall(r"^test result: .*$", text, re.M),
        "failed_tests": re.findall(r"^test (.+) \.\.\. FAILED$", text, re.M),
        "source_manifest_sha256": sha(json.dumps(before, sort_keys=True).encode()),
        "source_manifest": before,
    }
    (OUT / (name + ".json")).write_text(json.dumps(record, indent=2) + "\n")
    if expect is not None and completed.returncode != expect:
        print(
            "EXPECTED exit {} but got {}; failing the gate".format(
                expect, completed.returncode
            ),
            flush=True,
        )
        return record
    return record


def witness_by_mutant(mutant: str) -> list[str]:
    for key, path, transform, command in CASES:
        if key == mutant:
            return command
    raise KeyError(mutant)


# Disposition-obligated composed-path MUT-007 witness (LSA production-prep
# disposition MUT_007_requirements): the same canary-guard mutation must be
# observed through the composed application caller, not only the store target.
COMPOSED_MUT_007 = [
    "cargo", "test", "-p", "kontor-daemon", "--test", "projection_rebuild",
    "--locked", "--offline", "--",
    "composed_adverse_qualification_cannot_activate_before_canary", "--exact",
]


def mutants(mutants: list[str], composed_007: bool = True) -> None:
    results = []
    for key, name, transform, command in CASES:
        if mutants != ["all"] and key not in mutants:
            continue
        path = ROOT / name
        original = path.read_bytes()
        original_sha = sha(original)
        baseline = rec("final-combined-" + key.lower() + "-baseline", command, 0)
        if not any("1 passed; 0 failed" in row for row in baseline["tests"]):
            raise RuntimeError(key + ": baseline failed or witness missing")
        mutant_bytes = transform(original.decode()).encode()
        if mutant_bytes == original:
            raise RuntimeError(key + ": transform produced no change")
        patch = "".join(
            difflib.unified_diff(
                original.decode().splitlines(True),
                mutant_bytes.decode().splitlines(True),
                fromfile=name,
                tofile=name,
            )
        ).encode()
        (OUT / ("final-combined-" + key.lower() + ".patch.gz")).write_bytes(
            gzip.compress(patch, mtime=0)
        )
        seeded = {}
        try:
            path.write_bytes(mutant_bytes)
            if path.read_bytes() != mutant_bytes:
                raise RuntimeError(key + ": seed write not exact")
            seeded["store"] = rec(
                "final-combined-" + key.lower() + "-seeded", command, 101
            )
            if key == "MUT-007" and composed_007:
                seeded["composed"] = rec(
                    "final-combined-mut-007-composed-seeded", COMPOSED_MUT_007, 101
                )
        finally:
            if path.read_bytes() != mutant_bytes:
                raise RuntimeError(key + ": concurrent change while seeded")
            path.write_bytes(original)
        if sha(path.read_bytes()) != original_sha:
            raise RuntimeError(key + ": restoration hash mismatch")
        restored = rec("final-combined-" + key.lower() + "-restored", command, 0)
        if key == "MUT-007" and composed_007:
            rec("final-combined-mut-007-composed-restored", COMPOSED_MUT_007, 0)
        verdict = "KILLED" if (
            seeded["store"]["exit"] == 101
            and seeded["store"]["failed_tests"]
            and restored["exit"] == 0
        ) else "NOT-KILLED"
        results.append(
            {
                "mutant": key,
                "path": name,
                "verdict": verdict,
                "base_sha256": original_sha,
                "seeded_sha256": sha(mutant_bytes),
                "restored_sha256": sha(path.read_bytes()),
                "baseline": baseline["name"],
                "seeded": seeded["store"]["name"],
                "seeded_composed": seeded.get("composed", {}).get("name"),
                "restored": restored["name"],
                "witness": command,
                "failed_tests": seeded["store"]["failed_tests"],
                "composed_failed_tests": seeded.get("composed", {}).get("failed_tests"),
            }
        )
        print("{}: {}".format(key, verdict), flush=True)
    summary = {
        "base": "a04fc2ff0082d6e5650a757bb54a99d14f9d2840",
        "tree": "a3e53f8a8cd24eff6a6bef5897b495dcc11b0870",
        "procedure": "docs/evidence/ASMA-8159/run-verification.py mutation_cases()",
        "composed_mut_007": composed_007,
        "results": results,
    }
    (OUT / "final-combined-mutants-summary.json").write_text(
        json.dumps(summary, indent=2) + "\n"
    )
    print(json.dumps(summary, indent=2), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="mode", required=True)
    m = sub.add_parser("mutants")
    m.add_argument("keys", nargs="+")
    r = sub.add_parser("rec")
    r.add_argument("name")
    r.add_argument("argv", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.mode == "mutants":
        mutants(args.keys)
    else:
        rec(args.name, args.argv)
