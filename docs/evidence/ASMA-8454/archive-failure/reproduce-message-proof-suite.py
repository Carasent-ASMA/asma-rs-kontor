import concurrent.futures,importlib.util,json,os,subprocess
from datetime import datetime,timezone
from pathlib import Path
ROOT=Path.cwd();OUT=ROOT/'.qualification/reproduction'
spec=importlib.util.spec_from_file_location('qualification',ROOT/'scripts/qualify-durable-desks.py');q=importlib.util.module_from_spec(spec);spec.loader.exec_module(q)
BASE='e376f4059e47de8fb40956d4dbfbf6920dabc4ec';CURRENT=q.git('rev-parse','HEAD').strip();COMMAND=q.LOOPBACK+['--','--nocapture','--test-threads=2']
def run(label,tree,target,revision):
 env=q.environment();env['CARGO_TARGET_DIR']=str(target);env['CARGO_NET_OFFLINE']='true';env['RUST_TEST_THREADS']='2'
 log=OUT/(label+'.log');start=datetime.now(timezone.utc).isoformat();print('Running '+label,flush=True)
 with log.open('wb') as stream:r=subprocess.run(COMMAND,cwd=tree,env=env,stdout=stream,stderr=subprocess.STDOUT)
 receipt={'name':label,'source_commit':revision,'cwd':str(tree),'command':COMMAND,'target_dir':str(target),'started_at':start,'exit_code':r.returncode,'log':str(log.relative_to(ROOT)),'log_sha256':q.digest(log.read_bytes()),'source_unmodified':True}
 (OUT/(label+'.json')).write_text(json.dumps(receipt,indent=2)+'\n');print('Finished '+label+': '+str(r.returncode),flush=True);return receipt
with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
 runs=[pool.submit(run,'base-full',OUT/'base-e376f405',OUT/'target-base',BASE),pool.submit(run,'current-full',ROOT,ROOT/'target',CURRENT)]
 results=[future.result() for future in runs]
provenance=json.loads((OUT/'base-provenance.json').read_text());assert all(q.digest((OUT/'base-e376f405'/p).read_bytes())==h for p,h in provenance['files'].items())
frozen=json.loads((ROOT/'.qualification/frozen-source.json').read_text());assert all(q.digest((ROOT/p).read_bytes())==h for p,h in frozen['inputs'].items())
(OUT/'suite-results.json').write_text(json.dumps(results,indent=2)+'\n');print('All source bytes unchanged',flush=True)
