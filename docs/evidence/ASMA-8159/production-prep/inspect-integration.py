#!/usr/bin/env python3
"""Read actual default remote refs, local graph and frozen docs; no fetch/join."""
import hashlib,json,subprocess,re
from pathlib import Path
ROOT=Path(__file__).resolve().parents[4]
SUPER=Path("/Users/igor/carasent/asma-modules")
DOCS=Path("/Users/igor/.paseo/worktrees/0vl4ss0m/docs-asma-8157-experience-memory-skill-and-runbook")
PIN="7d19280857ae3c60a870e017da3f733f9ff2625b"
rows=[]
def command(cwd,argv):
    p=subprocess.run(argv,cwd=cwd,capture_output=True,text=True)
    rows.append({"cwd":str(cwd),"argv":argv,"exit":p.returncode,"stdout":p.stdout,"stderr":p.stderr})
    return p.stdout.strip()
for path in [ROOT,SUPER]:
    remote=command(path,["git","ls-remote","--symref","origin","HEAD"])
    hashes=re.findall(r"^([a-f0-9]{40})\s+HEAD$",remote,re.M)
    if hashes:
        candidate="29c8134646b610234abcb740738979dea84c85fe" if path==ROOT else PIN
        command(path,["git","rev-list","--left-right","--count",hashes[0]+"..."+candidate])
        command(path,["git","merge-base",hashes[0],candidate])
command(ROOT,["git","rev-parse","HEAD","HEAD^{tree}"])
command(ROOT,["git","branch","--show-current"])
command(DOCS,["git","rev-parse","HEAD"])
files=command(DOCS,["git","ls-tree","-r","--name-only",PIN]).splitlines()
joined=[]
for name in files:
    if "experience-memory" in name and (name.endswith(".md") or name.endswith(".json")):
        data=subprocess.check_output(["git","show",PIN+":"+name],cwd=DOCS)
        text=data.decode()
        joined.append({"path":name,"sha256":hashlib.sha256(data).hexdigest(),"names":{n:n in text for n in ["rebuild_memory_projection","kontor_memory_projection_rebuild","credential_alias","memory-cognee.json","projection_unavailable","projection_conflict"]},"rebuild_caveat_lines":[[i,line] for i,line in enumerate(text.splitlines(),1) if "accepted public rebuild" in line or "otherwise `projection_unavailable`" in line]})
numbers=[int(p.name[:4]) for p in (ROOT/"crates/kontor-store/migrations").glob("[0-9][0-9][0-9][0-9]_*.sql")]
assert sorted(numbers)==list(range(1,max(numbers)+1))
value={"commands":rows,"docs_pin":PIN,"docs_files":joined,"migration_count":len(numbers),"migration_max":max(numbers),"missing_or_duplicate_numbers":[],"claim":"read-only remote/default metadata; no fetch/rebase/merge; external owners unchanged"}
Path(__file__).with_name("receipts").joinpath("integration-pins.json").write_text(json.dumps(value,indent=2)+"\n")
print(json.dumps(value,indent=2))
