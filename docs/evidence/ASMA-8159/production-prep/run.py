#!/usr/bin/env python3
"""Record exact source/fake commands and sequential compiled mutation witnesses.

Use run NAME COMMAND... or mutants. No historical receipt is overwritten.
"""
import argparse
from datetime import datetime, timezone
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

ROOT = Path(__file__).resolve().parents[4]
OUT = Path(__file__).resolve().parent / "receipts"

def sha(data):
    return hashlib.sha256(data).hexdigest()

def save(name, data):
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / name).write_text(json.dumps(data, indent=2) + "\n")

def manifest():
    names = subprocess.check_output(["git", "ls-files", "--cached", "--others", "--exclude-standard", "--", "crates", "tests", "apps", "scripts", "Cargo.toml", "Cargo.lock", "deny.toml", "docs/evidence/ASMA-8159/production-prep/census.sql"], cwd=ROOT, text=True).splitlines()
    return {name: sha((ROOT / name).read_bytes()) for name in sorted(names) if (ROOT / name).is_file()}

def run(name, argv):
    OUT.mkdir(parents=True, exist_ok=True)
    if (OUT / (name + ".json")).exists():
        raise RuntimeError("receipt already exists; choose a new explicit name")
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
        "name": name, "argv": argv, "command": shlex.join(argv), "cwd": str(ROOT),
        "runner_sha256": sha(Path(__file__).read_bytes()), "started_utc": started,
        "finished_utc": datetime.now(timezone.utc).isoformat(), "seconds": round(time.monotonic()-clock,3),
        "exit": completed.returncode, "log": str(archive.relative_to(ROOT)),
        "raw_sha256": sha(data), "gzip_sha256": sha(archive.read_bytes()),
        "compiled": "Finished `test` profile" in text, "compile_failure": "could not compile" in text,
        "tests": re.findall(r"^test result: .*$", text, re.M),
        "failed_tests": re.findall(r"^test (.+) \.\.\. FAILED$", text, re.M),
        "source_manifest_sha256": sha(json.dumps(before,sort_keys=True).encode()),
        "source_manifest": before,
    }
    save(name + ".json", record)
    print(name + ": exit " + str(completed.returncode), flush=True)
    return record

def mutants(prefix=""):
    old = runpy.run_path(str(ROOT / "docs/evidence/ASMA-8159/run-verification.py"))
    cases = old["mutation_cases"]()
    # Required order: composed MUT-007 first, then the complete remaining set.
    cases.sort(key=lambda case: (case[0] != "MUT-007", case[0]))
    baseline_tree = manifest()
    results = []
    save(prefix + "mutant-source.json", baseline_tree)
    for key, name, transform, argv in cases:
        if key == "MUT-007":
            argv = ["cargo", "test", "-p", "kontor-daemon", "--test", "projection_rebuild", "--locked", "--offline", "--", "composed_adverse_qualification_cannot_activate_before_canary", "--exact"]
        if manifest() != baseline_tree:
            raise RuntimeError("source changed before mutant: " + key)
        path = ROOT / name
        original = path.read_bytes()
        baseline = run(prefix + key.lower() + "-baseline", argv)
        if baseline["exit"] or not any("1 passed; 0 failed" in row for row in baseline["tests"]):
            raise RuntimeError("baseline failed or witness missing: " + key)
        mutant = transform(original.decode()).encode()
        patch = "".join(difflib.unified_diff(original.decode().splitlines(True), mutant.decode().splitlines(True), fromfile=name, tofile=name)).encode()
        (OUT / (prefix + key.lower() + ".patch.gz")).write_bytes(gzip.compress(patch,mtime=0))
        try:
            path.write_bytes(mutant)
            seeded = run(prefix + key.lower() + "-seeded", argv)
        finally:
            if path.read_bytes() != mutant:
                raise RuntimeError("concurrent source change; restoration withheld: " + name)
            path.write_bytes(original)
        restored = run(prefix + key.lower() + "-restored", argv)
        if manifest() != baseline_tree:
            raise RuntimeError("restoration mismatch: " + key)
        killed = seeded["exit"] == 101 and seeded["compiled"] and seeded["failed_tests"] and not seeded["compile_failure"]
        row = {"mutant":key,"path":name,"verdict":"KILLED" if killed else "SURVIVED_OR_INVALID",
            "base_sha256":sha(original),"seeded_sha256":sha(mutant),"restored_sha256":sha(path.read_bytes()),
            "patch_sha256":sha(patch),"baseline":baseline["name"],"seeded":seeded["name"],"restored":restored["name"],
            "witness":argv,"failed_tests":seeded["failed_tests"]}
        results.append(row);save(prefix + "mutants.json",results)
        if not killed or restored["exit"]:
            raise RuntimeError("mutant not qualified: " + key)

if __name__ == "__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation",choices=["run","mutants"])
    parser.add_argument("name",nargs="?")
    parser.add_argument("command",nargs=argparse.REMAINDER)
    args=parser.parse_args()
    if args.operation=="mutants": mutants(args.name or "")
    else:
        if not args.name or not args.command: parser.error("run requires NAME COMMAND...")
        raise SystemExit(run(args.name,args.command)["exit"])
