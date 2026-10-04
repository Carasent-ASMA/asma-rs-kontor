# Run from the repository root in an isolated checkout. No git restoration commands.
from pathlib import Path
import hashlib, json, subprocess, time
root = Path.cwd()
receipts = root / 'docs/evidence/ASMA-8156/receipts'
receipts.mkdir(parents=True,exist_ok=True)
cases = [
 ('MUT-001', 'crates/kontor-core/src/memory.rs', 'ExperienceMemoryV1::from_document(document).is_ok()', 'document.json().contains("operational_gap") || ExperienceMemoryV1::from_document(document).is_ok()', ['cargo','test','-p','kontor-core','--test','experience_memory','strict_experience_roundtrip_and_non_memory_eligibility','--offline']),
 ('MUT-002', 'crates/kontor-store/src/memory.rs', 'WHERE r.project_id=?1 AND r.project_id=?2 AND r.item_id=?3 AND r.id=?4 AND r.content_hash=?5', 'WHERE r.item_id=?3 AND r.id=?4 AND r.content_hash=?5', ['cargo','test','-p','kontor-store','--lib','memory::experience_tests::malicious_tuple_project_current_approval_tombstone_hash_policy_and_kind','--offline']),
 ('MUT-003', 'crates/kontor-store/src/memory.rs', 'if next_bytes > MAX_RECALL_BYTES {', 'if next_bytes > MAX_RECALL_BYTES + 1 {', ['cargo','test','-p','kontor-store','--lib','memory::experience_tests::exact_budget_32768_32769_unicode_escaping_and_skip_oversized_top','--offline']),
]
def sha(data): return hashlib.sha256(data).hexdigest()
def status():
 return "\n".join(line for line in subprocess.check_output(["git","status","--porcelain"],text=True).splitlines() if not line[3:].startswith("docs/evidence/ASMA-8156/"))
start_status = status()
summary = []
for ident, filename, needle, replacement, command in cases:
 path = root / filename
 original = path.read_bytes()
 text = original.decode()
 assert text.count(needle) == 1, (ident, text.count(needle))
 record = dict(id=ident,file=filename,command=command,original_sha256=sha(original),needle=needle,replacement=replacement,runs=[])
 def run(phase):
  log = receipts / (ident.lower()+'-'+phase+'.log')
  begin=time.monotonic()
  with log.open('w') as handle:
   result=subprocess.run(command,cwd=root,stdout=handle,stderr=subprocess.STDOUT)
  output=log.read_text()
  entry=dict(phase=phase,exit_code=result.returncode,seconds=round(time.monotonic()-begin,3),log=log.name,log_sha256=sha(log.read_bytes()),source_sha256=sha(path.read_bytes()))
  record['runs'].append(entry)
  print(ident,phase,result.returncode,flush=True)
  return result.returncode,output
 try:
  code, output=run('baseline')
  assert code==0 and 'test result: ok.' in output, (ident,'baseline')
  path.write_text(text.replace(needle,replacement))
  record['mutant_sha256']=sha(path.read_bytes())
  code, output=run('seeded')
  assert code!=0 and 'test result: FAILED.' in output and 'Finished `test`' in output and 'could not compile' not in output, (ident,'invalid or surviving mutant')
  record['verdict']='KILLED'
 finally:
  path.write_bytes(original)
 record['restored_sha256']=sha(path.read_bytes())
 assert record['restored_sha256']==record['original_sha256']
 code, output=run('restored')
 assert code==0 and 'test result: ok.' in output, (ident,'restored')
 (receipts / (ident.lower()+'.json')).write_text(json.dumps(record,indent=2)+'\n')
 summary.append(record)
assert status()==start_status, 'Git status changed during mutation pass'
(receipts / 'mutants.json').write_text(json.dumps(summary,indent=2)+'\n')
print('All mutants behaviorally killed; exact source restored.',flush=True)
