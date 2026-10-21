"""Read-only source comparison; exports the exact base and records separate runs."""
import hashlib,importlib.util,json,os,subprocess,tarfile,io
from datetime import datetime,timezone
from pathlib import Path
ROOT=Path.cwd(); OUT=ROOT/'.qualification/reproduction'; OUT.mkdir(parents=True,exist_ok=True)
spec=importlib.util.spec_from_file_location('qualification',ROOT/'evidence/ASMA-8454-qualify-durable-desks.py');q=importlib.util.module_from_spec(spec);spec.loader.exec_module(q)
BASE='e376f4059e47de8fb40956d4dbfbf6920dabc4ec'; CURRENT=q.git('rev-parse','HEAD').strip()
TREE=OUT/'base-e376f405'; TREE.mkdir(exist_ok=True)
archive=subprocess.check_output(['git','archive',BASE],cwd=ROOT)
with tarfile.open(fileobj=io.BytesIO(archive)) as t:t.extractall(TREE,filter='data')
paths=q.git('ls-tree','-r','--name-only',BASE).splitlines(); hashes={}
for p in paths:
 target=TREE/p
 if target.is_file(): hashes[p]=q.digest(target.read_bytes())
(OUT/'base-provenance.json').write_text(json.dumps({'base_commit':BASE,'base_tree':q.git('rev-parse',BASE+'^{tree}').strip(),'export_command':['git','archive',BASE],'archive_sha256':q.digest(archive),'clean_export':str(TREE),'files':hashes,'source_writes':0},indent=2)+'\n')
COMMAND=q.LOOPBACK+['message_delivery_proofs::legacy_message_proof_duplicate_split_across_pages_is_terminal_without_delivery','--','--nocapture','--test-threads=2']

def run(label,tree,target,revision):
 env=q.environment();env['CARGO_TARGET_DIR']=str(target);env['CARGO_NET_OFFLINE']='true';env['RUST_TEST_THREADS']='2'
 log=OUT/(label+'.log');start=datetime.now(timezone.utc).isoformat();print('Running '+label,flush=True)
 with log.open('wb') as stream:r=subprocess.run(COMMAND,cwd=tree,env=env,stdout=stream,stderr=subprocess.STDOUT)
 receipt={'name':label,'source_commit':revision,'cwd':str(tree),'command':COMMAND,'target_dir':str(target),'started_at':start,'exit_code':r.returncode,'log':str(log.relative_to(ROOT)),'log_sha256':q.digest(log.read_bytes()),'source_unmodified':True}
 (OUT/(label+'.json')).write_text(json.dumps(receipt,indent=2)+'\n');print('Finished '+label+': '+str(r.returncode),flush=True)
 return receipt
results=[]
for n in range(1,6):results.append(run('current-'+str(n),ROOT,ROOT/'target',CURRENT))
for n in range(1,6):results.append(run('base-'+str(n),TREE,OUT/'target-base',BASE))
assert all(q.digest((TREE/p).read_bytes())==h for p,h in hashes.items())
frozen=json.loads((ROOT/'.qualification/frozen-source.json').read_text())
assert all(q.digest((ROOT/p).read_bytes())==h for p,h in frozen['inputs'].items())
(OUT/'results.json').write_text(json.dumps(results,indent=2)+'\n')
print('All source bytes unchanged',flush=True)
