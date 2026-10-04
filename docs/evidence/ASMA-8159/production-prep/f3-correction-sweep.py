#!/usr/bin/env python3
"""Read-only literal/rewind/registry sweep; no historical receipt rewrites."""
import hashlib,json,re,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[4]
OUT=Path(__file__).with_name("receipts")
commands=[
 ["rg","-n",r"\b119\b|\b120\b|\b121\b","crates","tests","scripts","apps",".github","--glob","*.rs","--glob","*.py","--glob","*.json","--glob","*.toml","--glob","*.ts","--glob","!openapi.json","--glob","!schema.d.ts","--glob","!test-timings.json"],
 ["rg","-n",r"schema_version\(\)|PRAGMA user_version|pragma_update.*user_version|MIGRATIONS.len|include_str!.*(0119|0120|0121)","crates","tests","scripts","--glob","*.rs","--glob","*.py"],
 ["rg","-n",r"pub use migrations::SCHEMA_VERSION|^mod migrations|^pub const SCHEMA_VERSION","crates/kontor-store/src/lib.rs","crates/kontor-store/src/migrations.rs"],
]
results=[]
for argv in commands:
    p=subprocess.run(argv,cwd=ROOT,text=True,capture_output=True)
    results.append({"argv":argv,"cwd":str(ROOT),"exit":p.returncode,"stdout":p.stdout,"stderr":p.stderr})
files=["crates/kontor-store/tests/publication_attestations_immutable.rs","crates/kontor-store/tests/repository_roundtrip.rs","crates/kontor-store/tests/schema_v1.rs","crates/kontor-store/src/lib.rs","crates/kontor-store/src/migrations.rs"]
source={name:hashlib.sha256((ROOT/name).read_bytes()).hexdigest() for name in files}
numbers=sorted(int(p.name[:4]) for p in (ROOT/"crates/kontor-store/migrations").glob("[0-9][0-9][0-9][0-9]_*.sql"))
assert numbers==list(range(1,122))
value={"commands":results,"source_hashes":source,"registry":{"schema_version":121,"registered_sql_count":121,"contiguous":True,"compile_time_length_assertion":True},"disposition":{"publication_assertion":"stale literal 119; change to public root re-export SCHEMA_VERSION, preserving test body","v115_rewind":"remove only 0120/0121 additive memory tables before exact 114 rewind; historical receipt semantics unchanged","schema_v1_current_pin":"121 already current; retain intentional registry pin","historical_version_literals":"preserve installed predecessor/version refusal/backfill thresholds and domain/wire versions; not current-schema assertions"}}
path=OUT/"f3c-schema-sweep-before.json"
assert not path.exists()
path.write_text(json.dumps(value,indent=2)+"\n")
print(json.dumps({"source_hashes":source,"registry":value["registry"],"disposition":value["disposition"]},indent=2))
