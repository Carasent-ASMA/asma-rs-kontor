#!/usr/bin/env python3
"""Scoped static isolation inventory; does not run account/usage backends."""
import hashlib,json,re,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[4]
FILES=["crates/kontor-daemon/src/usage.rs","crates/kontor-daemon/tests/harness/mod.rs","crates/kontor-daemon/tests/loopback_api.rs","scripts/test-lanes.py","scripts/verify-tree.py","crates/kontor-memory-cognee/src/lib.rs","crates/kontor-daemon/src/applications/memory_projection.rs"]
rows=[]
for name in FILES:
    data=(ROOT/name).read_bytes();lines=data.decode().splitlines()
    rows.append({"path":name,"sha256":hashlib.sha256(data).hexdigest(),"boundary_lines":[[i,line] for i,line in enumerate(lines,1) if any(text in line for text in ["find-generic-password","ProviderHomes::discover","TempDir::new","UsagePoller::discover","with_exact_reporter","provider-homes","exact_reporter","if args.lane == \"full\"","if args.dry_run","pnpm","--workspace"])],"test_names":re.findall(r"(?:async )?fn ([a-zA-Z0-9_]+)\(",(ROOT/name).read_text()) if name.endswith("usage.rs") else []})
argv=["rg","-n","SystemKeychain|provider-homes|ProviderHomes|ExactProviderUsageReporter","crates/kontor-daemon/tests","--glob","*.rs"]
p=subprocess.run(argv,cwd=ROOT,capture_output=True,text=True)
value={"source":rows,"search":{"argv":argv,"exit":p.returncode,"stdout":p.stdout},"executed_scope":"full loopback_api target, all its modules, in TempDir harness; default provider homes empty; exact-provider tests inject ScriptedUsageReporter; Jira credential tests use fixture ports; fake runtime only","excluded":"entire kontor-daemon --lib including usage module on macOS; unisolated blanket workspace/accounts/e2e execution; install/audit/live provider steps of native full lane","console":"full app typecheck/vitest unavailable without dependencies; no install authorized; generated schema parity and standalone TypeScript verified","claim":"static isolation plus exact executed target receipts; ignored cases receive no executed credit"}
Path(__file__).with_name("receipts").joinpath("safety-inventory.json").write_text(json.dumps(value,indent=2)+"\n")
print(json.dumps(value,indent=2))
