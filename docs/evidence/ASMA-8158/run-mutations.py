#!/usr/bin/env python3
"""Replay the three authorized ASMA-8158 source mutants, restoring each file.

Only fixture tests run. No checkout, branch, index, credential or service action.
Patches are evidence; mutated source is never committed. A compilation failure
is recorded as INVALID_COMPILE, never KILLED.
"""
from __future__ import annotations

import difflib
import gzip
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[3]
RECEIPTS = Path(__file__).resolve().parent / "receipts"
ADAPTER = "crates/kontor-memory-cognee/src/lib.rs"
DAEMON = "crates/kontor-daemon/src/applications.rs"
FEATURE_FILES = [
    "Cargo.toml", "Cargo.lock", "crates/kontor-daemon/Cargo.toml",
    DAEMON, "crates/kontor-daemon/src/lib.rs",
    "crates/kontor-daemon/tests/loopback_api.rs",
    "crates/kontor-daemon/tests/loopback/experience_launch.rs",
    "crates/kontor-memory-cognee/Cargo.toml", ADAPTER,
    "crates/kontor-memory-cognee/tests/fixtures.rs", "docs/CONFIGURATION.md",
]


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def manifest() -> dict:
    files = {path: sha((ROOT / path).read_bytes()) for path in FEATURE_FILES}
    return {"files": files, "sha256": sha(json.dumps(files, sort_keys=True).encode())}


def replace_once(text: str, before: str, after: str) -> str:
    if text.count(before) != 1:
        raise RuntimeError(f"mutation anchor is not unique: {before[:80]!r}")
    return text.replace(before, after, 1)


def trusted_text(text: str) -> str:
    text = replace_once(text, "struct Search {\n    candidates: Vec<MemoryCandidate>,\n}",
                        "struct Search {\n    candidates: Vec<MemoryCandidate>,\n    raw_text: Vec<String>,\n}")
    text = replace_once(text, "        let semantic =\n", "        let mut provider_block = None;\n        let semantic =\n")
    text = replace_once(text,
                        """                Ok(Ok(search)) => SemanticRecall::Candidates {
                    projection_digest: snapshot.digest.clone(),
                    candidates: search.candidates,
                },""",
                        """                Ok(Ok(search)) => {
                    provider_block = Some(search.raw_text.join("\\n"));
                    SemanticRecall::Candidates {
                        projection_digest: snapshot.digest.clone(),
                        candidates: search.candidates,
                    }
                },""")
    text = replace_once(text, "        freeze(&semantic)\n",
                        """        let mut recalled = freeze(&semantic)?;
        if let Some(text) = provider_block {
            recalled.canonical_block = text;
        }
        Ok(recalled)
""")
    text = replace_once(text, "    let mut candidates = Vec::new();\n",
                        "    let mut candidates = Vec::new();\n    let mut raw_text = Vec::new();\n")
    text = replace_once(text,
                        "            let entry: ProjectionEntry = serde_json::from_str(text).map_err(|_| malformed())?;",
                        "            raw_text.push(text.to_owned());\n            let entry: ProjectionEntry = serde_json::from_str(text).map_err(|_| malformed())?;")
    return replace_once(text, "    Ok(Search { candidates })", "    Ok(Search { candidates, raw_text })")


def list_fallback(text: str) -> str:
    before = """                .with_store(|store| match run {
                    Some(run) => {
                        store.recall_experiences(project_id, &run.to_string(), &intent, semantic)
                    }
                    None => store.preview_recall(project_id, &intent, semantic),
                })"""
    after = """                .with_store(|store| {
                    let mut recalled = match run {
                        Some(run) => store.recall_experiences(project_id, &run.to_string(), &intent, semantic),
                        None => store.preview_recall(project_id, &intent, semantic),
                    }?;
                    if matches!(semantic, SemanticRecall::Degraded(_)) {
                        let corpus = store.list_memory(project_id)?;
                        recalled.canonical_block = format!("[{}]", corpus.iter()
                            .map(|row| row.document.json()).collect::<Vec<_>>().join(","));
                    }
                    Ok::<_, kontor_store::memory::MemoryError>(recalled)
                })"""
    return replace_once(text, before, after)


MUTANTS = [
    ("MUT-004", ADAPTER, trusted_text, ["cargo", "test", "-p", "kontor-memory-cognee", "--test", "fixtures",
     "authoritative_rehydration_rejects_leakage_and_never_trusts_cognee_text", "--", "--exact"]),
    ("MUT-005", DAEMON, list_fallback, ["cargo", "test", "-p", "kontor-daemon", "--test", "loopback_api",
     "experience_launch::degraded_launch_never_lists_the_corpus_and_timeout_keeps_store_unlocked", "--", "--exact"]),
    ("MUT-006", DAEMON,
     lambda text: replace_once(text, "    if include_block {", "    if false && include_block {"),
     ["cargo", "test", "-p", "kontor-daemon", "--test", "loopback_api",
      "experience_launch::root_launch_contains_exact_frozen_canonical_bytes_and_downstream_cites_same_binding", "--", "--exact"]),
]


def run(command: list[str], name: str) -> dict:
    started = time.monotonic()
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
    output = result.stdout + result.stderr
    path = RECEIPTS / f"{name}.log"
    path.write_text(output)
    return {"command": command, "exit_code": result.returncode, "seconds": round(time.monotonic()-started, 3),
            "log": str(path.relative_to(ROOT)), "log_sha256": sha(output.encode()),
            "compiled": bool(re.search(r"Finished `test` profile", output)),
            "behavioral_failure": bool(re.search(r"test .+ \.\.\. FAILED", output)) and "could not compile" not in output,
            "executed_tests": re.findall(r"test result: .*", output),
            "failed_tests": re.findall(r"test (.+) \.\.\. FAILED", output)}


def main() -> None:
    RECEIPTS.mkdir(parents=True, exist_ok=True)
    frozen = manifest()
    (RECEIPTS / "source-hashes.json").write_text(json.dumps(frozen, indent=2)+"\n")
    before_status = subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)
    (RECEIPTS / "mutation-start-status.log").write_bytes(before_status)
    results = []
    for key, path, mutate, command in MUTANTS:
        print(f"{key}: baseline", flush=True)
        baseline = run(command, key.lower()+"-baseline")
        if baseline["exit_code"] or not any("1 passed; 0 failed" in row for row in baseline["executed_tests"]):
            raise RuntimeError(f"{key}: baseline failed or did not run its witness")
        target = ROOT / path
        original = target.read_bytes()
        mutant = mutate(original.decode()).encode()
        patch = "".join(difflib.unified_diff(original.decode().splitlines(True), mutant.decode().splitlines(True), fromfile=path, tofile=path))
        patch_path = RECEIPTS / (key.lower()+".patch.gz")
        patch_path.write_bytes(gzip.compress(patch.encode(), mtime=0))
        try:
            target.write_bytes(mutant)
            mutated_manifest = manifest()
            print(f"{key}: seeded", flush=True)
            seeded = run(command, key.lower()+"-seeded")
        finally:
            target.write_bytes(original)
        if manifest() != frozen:
            raise RuntimeError(f"{key}: source restoration mismatch")
        print(f"{key}: restored", flush=True)
        restored = run(command, key.lower()+"-restored")
        verdict = "SURVIVED"
        if not seeded["compiled"]:
            verdict = "INVALID_COMPILE"
        elif seeded["exit_code"] and seeded["behavioral_failure"]:
            verdict = "KILLED"
        result = {"mutant": key, "path": path, "verdict": verdict,
                  "base_file_sha256": sha(original), "mutant_file_sha256": sha(mutant),
                  "patch": str(patch_path.relative_to(ROOT)),
                  "patch_sha256": sha(patch.encode()), "patch_gzip_sha256": sha(patch_path.read_bytes()),
                  "base_source_sha256": frozen["sha256"],
                  "mutant_source_sha256": mutated_manifest["sha256"], "restored_source_sha256": manifest()["sha256"],
                  "baseline": baseline, "seeded": seeded, "restored": restored}
        results.append(result)
        (RECEIPTS / "mutations.json").write_text(json.dumps(results, indent=2)+"\n")
        print(f"{key}: {verdict}; restored exit {restored['exit_code']}", flush=True)
        if restored["exit_code"] or verdict != "KILLED":
            raise RuntimeError(f"{key}: mutation qualification incomplete")
    after_status = subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)
    (RECEIPTS / "mutation-end-status.log").write_bytes(after_status)
    if after_status != before_status:
        raise RuntimeError("working status changed outside the expected existing receipts directory")


if __name__ == "__main__":
    main()
