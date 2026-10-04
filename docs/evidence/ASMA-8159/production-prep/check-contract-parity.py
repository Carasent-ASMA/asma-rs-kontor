#!/usr/bin/env python3
"""Use the already installed generator; never install or rewrite console source."""
import hashlib,json,subprocess,tempfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[4]
GEN=Path("/Users/igor/.npm/_npx/f1e70922fc87e24a/node_modules/openapi-typescript/bin/cli.js")
contract=ROOT/"crates/kontor-api/contract/openapi.json"
schema=ROOT/"apps/console/src/api/schema.d.ts"
assert GEN.is_file(),"installed generator unavailable; no installation authorized"
with tempfile.TemporaryDirectory(prefix="ASMA-8159-contract-") as directory:
    output=Path(directory)/"schema.d.ts"
    argv=["node",str(GEN),str(contract),"-o",str(output)]
    print(argv,flush=True);subprocess.run(argv,cwd=ROOT,check=True)
    assert output.read_bytes()==schema.read_bytes(),"generated console contract drift"
    argv=["/opt/homebrew/bin/tsc","--noEmit","--strict","--skipLibCheck","--lib","ES2022",str(schema)]
    print(argv,flush=True);subprocess.run(argv,cwd=ROOT,check=True)
    print(json.dumps({"openapi_sha256":hashlib.sha256(contract.read_bytes()).hexdigest(),"schema_sha256":hashlib.sha256(schema.read_bytes()).hexdigest(),"generator_entry_sha256":hashlib.sha256(GEN.read_bytes()).hexdigest(),"generated_equal":True,"standalone_schema_typecheck":True,"full_console_typecheck_vitest":"not executed: console node_modules absent; install unauthorized"}))
