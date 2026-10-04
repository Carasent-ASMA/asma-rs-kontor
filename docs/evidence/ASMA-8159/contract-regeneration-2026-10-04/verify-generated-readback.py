"""Compare phase-1 generated artifacts with the authorized static readback."""

import datetime
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[4]
DIRECTORY = Path(__file__).resolve().parent


def main():
    phase = sys.argv[1]
    assert phase in ("api", "complete") and Path.cwd() == ROOT
    receipt = DIRECTORY / ("02a-openapi-readback.json" if phase == "api" else "04-generated-readback.json")
    assert not receipt.exists(), "Existing evidence must be preserved"
    before = json.loads((DIRECTORY / "00-preflight.json").read_text())
    commands = []

    def run(argv):
        start = datetime.datetime.now(datetime.timezone.utc).isoformat()
        result = subprocess.run(argv, cwd=ROOT, capture_output=True, text=True)
        commands.append({"argv": argv, "started_at_utc": start,
                         "completed_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                         "exit_code": result.returncode, "stdout": result.stdout, "stderr": result.stderr})
        assert result.returncode == 0, commands[-1]
        return result.stdout

    for name in (["02-openapi"] if phase == "api" else ["02-openapi", "03-console"]):
        assert json.loads((DIRECTORY / (name + ".json")).read_text())["exit_code"] == 0
    current_head = run(["git", "rev-parse", "HEAD"]).strip()
    changed = run(["git", "diff", "--name-only"]).splitlines()
    staged = run(["git", "diff", "--cached", "--name-only"]).splitlines()
    expected_paths = [item["path"] for item in before["contracts_before"]]
    contract_hashes = []
    for item in before["contracts_before"]:
        data = (ROOT / item["path"]).read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        contract_hashes.append({"path": item["path"], "bytes": len(data), "sha256": digest,
                                "static_sha256": item["sha256"], "byte_identical_to_static_readback": digest == item["sha256"]})
    registry = (ROOT / "crates/kontor-mcp/src/registry.rs").read_text()
    block = re.search(r"pub static REGISTRY: &\[ToolSpec\] = &\[(.*?)\n\];", registry, re.S).group(1)
    names = re.findall(r'\bname:\s*"([^"]+)"', block)
    cli_block = re.search(r"pub static CLI_ONLY: &\[&str\] = &\[(.*?)\];", registry, re.S).group(1)
    cli_only = re.findall(r'"([^"]+)"', cli_block)
    document = json.loads((ROOT / expected_paths[0]).read_bytes())
    methods = {"get", "put", "post", "delete", "options", "head", "patch", "trace"}
    counts = {"mapped_operations": len(names), "advertised_tools": len(names) - len(cli_only),
              "openapi_operations": sum(1 for item in document["paths"].values() for method in item if method in methods)}
    expected_counts = {"mapped_operations": 202, "advertised_tools": 201, "openapi_operations": 203}
    selected_hashes = contract_hashes[:1] if phase == "api" else contract_hashes
    passed = (current_head == before["base_sha"] and not staged
              and set(changed) <= set(expected_paths) and counts == expected_counts
              and len(names) == len(set(names)) and cli_only == ["kontor_account_profile_amend"]
              and all(item["byte_identical_to_static_readback"] for item in selected_hashes))
    if changed:
        run(["git", "diff", "--", *expected_paths])
    state = {"argv": ["python3", str(Path(__file__).relative_to(ROOT)), phase], "cwd": str(ROOT),
             "completed_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
             "exit_code": 0 if passed else 1, "status": "PASS" if passed else "STOP_STATIC_READBACK_DIFF",
             "base_sha": before["base_sha"], "counts": counts, "expected_counts": expected_counts,
             "contracts": contract_hashes, "cli_only": cli_only, "tracked_changed_paths": changed,
             "staged_paths": staged, "commands_and_outputs": commands,
             "scope": "Required generation readback only; QA qualification and self-review not run."}
    receipt.write_text(json.dumps(state, indent=2) + "\n")
    print(json.dumps({"status": state["status"], "exit_code": state["exit_code"], "counts": counts,
                      "contracts_byte_identical_to_static_readback": all(item["byte_identical_to_static_readback"] for item in selected_hashes),
                      "receipt": str(receipt.relative_to(ROOT))}), flush=True)
    return state["exit_code"]


if __name__ == "__main__":
    raise SystemExit(main())
