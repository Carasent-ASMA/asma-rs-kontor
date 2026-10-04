#!/usr/bin/env python3
"""Complete interrupted F3 evidence without replacing predecessor receipts.

Run `mutants`, then `checks`. Existing complete mutant receipts are verified
against the current tree before reuse. Any unexpected failure stops the run.
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

ROOT = Path(__file__).resolve().parents[4]
OUT = Path(__file__).with_name("receipts")
RECORDER = runpy.run_path(str(Path(__file__).with_name("run.py")))
RUN = RECORDER["run"]
MANIFEST = RECORDER["manifest"]
CASES = runpy.run_path(
    str(ROOT / "docs/evidence/ASMA-8159/run-verification.py")
)["mutation_cases"]()


def sha(data: bytes) -> str:
    """Hash the exact bytes used in a receipt."""
    return hashlib.sha256(data).hexdigest()


def save(name: str, value: object) -> None:
    """Write one new additive receipt, refusing an existing name."""
    with (OUT / name).open("x") as stream:
        stream.write(json.dumps(value, indent=2) + "\n")


def receipt(name: str, source: dict, expected_exit: int) -> dict:
    """Verify command, nonempty output, result and exact source provenance."""
    record = json.loads((OUT / (name + ".json")).read_text())
    archive = ROOT / record["log"]
    raw = gzip.decompress(archive.read_bytes())
    text = raw.decode()
    assert raw and sha(raw) == record["raw_sha256"], name
    assert sha(archive.read_bytes()) == record["gzip_sha256"], name
    assert record["command"] == shlex.join(record["argv"]), name
    assert record["cwd"] == str(ROOT), name
    assert record["source_manifest"] == source, name
    assert record["source_manifest_sha256"] == sha(
        json.dumps(source, sort_keys=True).encode()
    ), name
    assert record["exit"] == expected_exit, name
    assert record["compiled"] and not record["compile_failure"], name
    assert record["tests"] == re.findall(r"^test result: .*$", text, re.M)
    assert record["failed_tests"] == re.findall(
        r"^test (.+) \.\.\. FAILED$", text, re.M
    ), name
    if expected_exit == 0:
        assert record["tests"] and not record["failed_tests"], name
        assert all("0 failed;" in row for row in record["tests"]), name
    else:
        assert expected_exit == 101 and len(record["failed_tests"]) == 1
    return record


def complete_mutants() -> None:
    """Reuse five complete triples and complete MUT-005 and MUT-006."""
    frozen = MANIFEST()
    assert frozen == json.loads((OUT / "f3c-mutant-source.json").read_text())
    rows = json.loads((OUT / "f3c-mutants.json").read_text())
    assert {row["mutant"] for row in rows} == {
        "MUT-001", "MUT-002", "MUT-003", "MUT-004", "MUT-007"
    }
    for row in rows:
        name = row["path"]
        original = (ROOT / name).read_bytes()
        transform = next(case[2] for case in CASES
                         if case[0] == row["mutant"])
        mutant = transform(original.decode()).encode()
        assert row["base_sha256"] == row["restored_sha256"] == sha(original)
        assert row["seeded_sha256"] == sha(mutant)
        seeded_source = dict(frozen, **{name: sha(mutant)})
        for phase, source, code in [
            ("baseline", frozen, 0), ("seeded", seeded_source, 101),
            ("restored", frozen, 0),
        ]:
            proof = receipt(row[phase], source, code)
            assert proof["argv"] == row["witness"]
            if code == 0:
                assert any("1 passed; 0 failed" in r for r in proof["tests"])
            else:
                assert proof["failed_tests"] == row["failed_tests"]
        patch = gzip.decompress(
            (OUT / ("f3c-" + row["mutant"].lower() + ".patch.gz"))
            .read_bytes()
        )
        assert sha(patch) == row["patch_sha256"]

    partial = OUT / "f3c-mut-005-restored.raw"
    save("f3c-interrupted-restoration.json", {
        "path": str(partial.relative_to(ROOT)),
        "sha256": sha(partial.read_bytes()),
        "bytes": partial.stat().st_size,
        "status": "INCOMPLETE; compilation line only; no exit or test result",
        "superseded_by": "f3c-mut-005-restored-resumed.json",
    })
    for key, name, transform, argv in CASES:
        if key not in {"MUT-005", "MUT-006"}:
            continue
        assert MANIFEST() == frozen, key
        path = ROOT / name
        original = path.read_bytes()
        mutant = transform(original.decode()).encode()
        seeded_source = dict(frozen, **{name: sha(mutant)})
        baseline_name = "f3c-" + key.lower() + "-baseline"
        seeded_name = "f3c-" + key.lower() + "-seeded"
        restored_name = "f3c-" + key.lower() + "-restored"
        patch = "".join(difflib.unified_diff(
            original.decode().splitlines(True),
            mutant.decode().splitlines(True), fromfile=name, tofile=name,
        )).encode()
        if key == "MUT-005":
            baseline = receipt(baseline_name, frozen, 0)
            seeded = receipt(seeded_name, seeded_source, 101)
            assert gzip.decompress(
                (OUT / "f3c-mut-005.patch.gz").read_bytes()
            ) == patch
            restored_name += "-resumed"
        else:
            RUN(baseline_name, argv)
            baseline = receipt(baseline_name, frozen, 0)
            with (OUT / "f3c-mut-006.patch.gz").open("xb") as stream:
                stream.write(gzip.compress(patch, mtime=0))
            try:
                path.write_bytes(mutant)
                RUN(seeded_name, argv)
                seeded = receipt(seeded_name, seeded_source, 101)
            finally:
                if path.read_bytes() != mutant:
                    raise RuntimeError("Concurrent source change: " + name)
                path.write_bytes(original)
        assert baseline["argv"] == seeded["argv"] == argv
        RUN(restored_name, argv)
        restored = receipt(restored_name, frozen, 0)
        assert restored["argv"] == argv and MANIFEST() == frozen
        for proof in (baseline, restored):
            assert any("1 passed; 0 failed" in r for r in proof["tests"])
        rows.append({
            "mutant": key, "path": name, "verdict": "KILLED",
            "base_sha256": sha(original), "seeded_sha256": sha(mutant),
            "restored_sha256": sha(path.read_bytes()),
            "patch_sha256": sha(patch), "baseline": baseline_name,
            "seeded": seeded_name, "restored": restored_name,
            "witness": argv, "failed_tests": seeded["failed_tests"],
        })
    assert len(rows) == 7 and MANIFEST() == frozen
    save("f3c-completed-mutants.json",
         sorted(rows, key=lambda r: r["mutant"]))
    print("Seven behavioral kills; all baselines/restorations pass.", flush=True)


def complete_checks() -> None:
    """Validate the mechanical pass and rerun synthetic F4 evidence."""
    frozen = MANIFEST()
    mechanical = receipt("f3c-mechanical-after", frozen, 0)
    before_sweep = json.loads(
        (OUT / "f3c-schema-sweep-before.json").read_text()
    )
    sweep = []
    for previous in before_sweep["commands"]:
        argv = previous["argv"]
        result = subprocess.run(argv, cwd=ROOT, text=True, capture_output=True)
        assert result.returncode == 0 and result.stdout, argv
        sweep.append({
            "argv": argv, "command": shlex.join(argv), "cwd": str(ROOT),
            "exit": result.returncode, "stdout": result.stdout,
            "stderr": result.stderr,
        })
    migrations = (ROOT / "crates/kontor-store/src/migrations.rs").read_text()
    version = int(re.search(
        r"pub const SCHEMA_VERSION: i64 = (\d+);", migrations
    ).group(1))
    numbers = sorted(int(p.name[:4]) for p in (
        ROOT / "crates/kontor-store/migrations"
    ).glob("[0-9][0-9][0-9][0-9]_*.sql"))
    assert numbers == list(range(1, version + 1))
    assert "MIGRATIONS.len() == SCHEMA_VERSION as usize" in migrations
    assert "pub use migrations::SCHEMA_VERSION;" in (
        ROOT / "crates/kontor-store/src/lib.rs"
    ).read_text()
    save("f3c-schema-sweep-after.json", {
        "commands": sweep, "schema_version": version,
        "registered_sql_count": len(numbers), "contiguous": True,
        "compile_time_length_assertion_present": True,
        "disposition": {
            "publication_assertion": "Public SCHEMA_VERSION re-export",
            "v115_rewind": "Removes exactly the eight 0120/0121 tables",
            "preserved": "Historical versions and intentional registry pin",
        },
        "source_manifest_sha256": sha(
            json.dumps(frozen, sort_keys=True).encode()
        ),
    })
    commands = [
        ("f3c-f4-forced-rollback", [
            "cargo", "test", "-p", "kontor-store", "--test",
            "memory_migration_rollback", "--locked", "--offline", "--",
            "--nocapture",
        ]),
        ("f3c-f4-v119-upgrade", [
            "cargo", "test", "-p", "kontor-store", "--test", "schema_v1",
            "v120_upgrade_preserves_generic_ledger_and_enforces_immutable_"
            "memory_receipts", "--locked", "--offline", "--", "--exact",
        ]),
        ("f3c-f4-fresh-schema", [
            "cargo", "test", "-p", "kontor-store", "--test", "schema_v1",
            "an_empty_database_migrates_to_the_current_schema_version",
            "--locked", "--offline", "--", "--exact",
        ]),
        ("f3c-f4-synthetic-backup-recovery", [
            "cargo", "test", "-p", "kontor-daemon", "--test",
            "synthetic_census_rehearsal", "--locked", "--offline", "--",
            "--nocapture",
        ]),
    ]
    results = [{
        "receipt": mechanical["name"], "tests": mechanical["tests"],
    }]
    for name, argv in commands:
        assert MANIFEST() == frozen
        RUN(name, argv)
        proof = receipt(name, frozen, 0)
        assert any("1 passed; 0 failed" in row for row in proof["tests"])
        results.append({"receipt": name, "tests": proof["tests"]})
    assert MANIFEST() == frozen
    save("f3c-completed-checks.json", results)
    print("Mechanical 90 tests and four F4 witnesses: zero failures.", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=["mutants", "checks"])
    args = parser.parse_args()
    if args.phase == "mutants":
        complete_mutants()
    else:
        complete_checks()
