"""Record the two authorized generators, including logs, hashes and disk use."""

import datetime
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time


ROOT = Path(__file__).resolve().parents[4]
EVIDENCE = Path(__file__).resolve().parent
CONTRACTS = [
    "crates/kontor-api/contract/openapi.json",
    "apps/console/src/api/schema.d.ts",
]
COMMANDS = {
    "02-openapi": [
        "env",
        "CARGO_BUILD_JOBS=1",
        "CARGO_INCREMENTAL=0",
        "CARGO_PROFILE_DEV_DEBUG=0",
        "CARGO_PROFILE_TEST_DEBUG=0",
        "KONTOR_UPDATE_CONTRACT=1",
        "cargo", "test", "-p", "kontor-api", "--test", "openapi_contract",
        "--locked", "--offline", "--",
        "the_committed_contract_document_is_the_one_this_crate_serves", "--exact",
    ],
    "03-console": [
        "node",
        "/Users/igor/.npm/_npx/f1e70922fc87e24a/node_modules/openapi-typescript/bin/cli.js",
        "crates/kontor-api/contract/openapi.json",
        "-o", "apps/console/src/api/schema.d.ts",
    ],
}


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def hashes():
    result = []
    for name in CONTRACTS:
        data = (ROOT / name).read_bytes()
        result.append({"path": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
    return result


def main():
    name = sys.argv[1]
    argv = COMMANDS[name]
    receipt = EVIDENCE / (name + ".json")
    assert not receipt.exists(), "Existing command receipt must be preserved"
    assert Path.cwd() == ROOT, "Only the authorized checkout may run this recorder"
    census = json.loads((EVIDENCE / "01-census.json").read_text())
    assert census["status"] == "CLEAR" and census["exit_code"] == 0
    if name == "03-console":
        previous = json.loads((EVIDENCE / "02-openapi.json").read_text())
        assert previous["exit_code"] == 0
    state = {
        "argv": argv, "cwd": str(ROOT), "started_at_utc": now(),
        "status": "RUNNING", "exit_code": None, "contracts_before": hashes(),
        "stdout_path": str((EVIDENCE / (name + ".stdout.log")).relative_to(ROOT)),
        "stderr_path": str((EVIDENCE / (name + ".stderr.log")).relative_to(ROOT)),
        "target_cap_bytes": 10 * 1024**3, "target_stop_threshold_bytes": 8 * 1024**3,
        "disk_samples": [], "external_processes_signaled": False,
    }

    def save():
        receipt.write_text(json.dumps(state, indent=2) + "\n")

    save()
    with (ROOT / state["stdout_path"]).open("x") as stdout, (ROOT / state["stderr_path"]).open("x") as stderr:
        process = subprocess.Popen(argv, cwd=ROOT, stdout=stdout, stderr=stderr, start_new_session=True)
        state["pid"] = process.pid
        save()
        while True:
            target = ROOT / "target"
            if target.exists():
                started = now()
                sample = subprocess.run(["du", "-sk", str(target)], capture_output=True, text=True)
                disk = {"argv": ["du", "-sk", str(target)], "started_at_utc": started,
                        "completed_at_utc": now(), "exit_code": sample.returncode,
                        "stdout": sample.stdout, "stderr": sample.stderr}
                if sample.returncode == 0:
                    disk["bytes"] = int(sample.stdout.split()[0]) * 1024
                    if disk["bytes"] >= state["target_stop_threshold_bytes"] and process.poll() is None:
                        os.killpg(process.pid, signal.SIGTERM)
                        state["status"] = "OWN_GENERATOR_STOPPED_BEFORE_DISK_CAP"
                state["disk_samples"].append(disk)
            code = process.poll()
            save()
            print(json.dumps({"command": name, "status": state["status"], "timestamp_utc": now(),
                              "target_bytes": state["disk_samples"][-1].get("bytes") if state["disk_samples"] else 0}), flush=True)
            if code is not None:
                break
            time.sleep(20)
    state.update(exit_code=code, completed_at_utc=now(), contracts_after=hashes())
    if state["status"] == "RUNNING":
        state["status"] = "PASS" if code == 0 else "FAIL"
    for stream in ("stdout", "stderr"):
        data = (ROOT / state[stream + "_path"]).read_bytes()
        state[stream + "_bytes"] = len(data)
        state[stream + "_sha256"] = hashlib.sha256(data).hexdigest()
    state["max_recorded_target_bytes"] = max((item.get("bytes", 0) for item in state["disk_samples"]), default=0)
    save()
    print(json.dumps({"command": name, "status": state["status"], "exit_code": code,
                      "receipt": str(receipt.relative_to(ROOT)), "max_recorded_target_bytes": state["max_recorded_target_bytes"]}), flush=True)
    return code


if __name__ == "__main__":
    raise SystemExit(main())
