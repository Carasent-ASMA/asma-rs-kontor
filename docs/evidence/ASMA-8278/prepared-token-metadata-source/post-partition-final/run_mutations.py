"""One real isolated defect at a time; no compiler failure counts as a kill."""
from pathlib import Path
import datetime,hashlib,json,os,re,subprocess,time
base=Path(__file__).parent;source=base/'mutation-source';original=Path('/tmp/asma-8278-module-consolidation-20261002.Mcrugq')
expected=json.loads((base/'mutation-baseline-hashes.json').read_text());defs=json.loads((base/'mutation-definitions.json').read_text())
logs=base/'mutation-logs';logs.mkdir(exist_ok=True)
env=dict(os.environ,CARGO_TARGET_DIR=str(base/'mutation-target'),RUST_TEST_THREADS='2')
def hashes(root):return {name:hashlib.sha256((root/name).read_bytes()).hexdigest() for name in expected}
def run(name,target,test=None):
 cmd=['cargo','test','--offline','--jobs','2','-p','kontor-store']
 cmd+=['--lib'] if target=='lib' else ['--test',target]
 if test:cmd+=[test]
 cmd+=['--','--nocapture']
 started=datetime.datetime.now(datetime.timezone.utc).isoformat();clock=time.monotonic();log=logs/(name+'.log')
 with log.open('wb') as output:r=subprocess.run(cmd,cwd=source,env=env,stdout=output,stderr=subprocess.STDOUT)
 return {'command':cmd,'started_utc':started,'elapsed_seconds':round(time.monotonic()-clock,3),'exit_code':r.returncode,'log':str(log),'log_sha256':hashlib.sha256(log.read_bytes()).hexdigest()}
report={'baseline':[],'mutants':[],'limits':'Isolated source/disposable fixture SQLite proof only. SQL guard mutations explicitly assigned by root; no production/install/live effects. No compiler failure counted as kill.'}
def save(): (base/'mutation-report.json').write_text(json.dumps(report,indent=2)+'\n')
for target,test in [('attestation_token_metadata',None),('lib','backup::export::attestation_ledger_tests::token_tables_are_required_at125_and_legacy124_absence_never_skips_keys')]:
 r=run('baseline-'+target,target,test);report['baseline'].append(r);save();assert r['exit_code']==0,r
 print(json.dumps({'baseline':target,'exit':r['exit_code']}),flush=True)
for definition in defs:
 assert hashes(original)==expected,'Original source changed during isolated run'
 assert hashes(source)==expected,'Isolated baseline not restored'
 path=source/definition['path'];before=path.read_bytes();text=before.decode();assert text.count(definition['old'])==1
 try:
  path.write_text(text.replace(definition['old'],definition['new'],1))
  mutant_hash=hashlib.sha256(path.read_bytes()).hexdigest()
  r=run(definition['id'],definition['target'],definition['test']);log=Path(r['log']).read_text()
  compiled='Finished `test` profile' in log and not re.search(r'error\[E\d+\]|error: could not compile',log)
  intended=bool(re.search(r'test [^\n]*'+re.escape(definition['test'])+r'[^\n]*FAILED',log))
  panics=[]
  for hit in re.finditer(r'panicked at ([^\n]+):(\d+):(\d+):',log):
   name,line,col=hit.group(1),int(hit.group(2)),int(hit.group(3));p=source/name
   if p.is_file():
    lines=p.read_text().splitlines();excerpt='\n'.join(f'{n+1}: {lines[n]}' for n in range(max(0,line-4),min(len(lines),line+3)))
    assertion=any(marker in excerpt for marker in ['assert!','assert_eq!','assert_ne!','.expect_err('])
    panics.append({'path':name,'line':line,'column':col,'source_excerpt':excerpt,'assertion_or_expect_err':assertion})
  killed=r['exit_code']!=0 and compiled and intended and any(p['assertion_or_expect_err'] for p in panics)
  record=dict(r,id=definition['id'],path=definition['path'],mutant_sha256=mutant_hash,successful_compilation=compiled,intended_test_failed=intended,panics=panics,result='KILLED' if killed else 'UNRESOLVED')
 finally:
  path.write_bytes(before)
 record['isolated_restored']=hashes(source)==expected;record['original_unchanged']=hashes(original)==expected
 report['mutants'].append(record);save();print(json.dumps({'mutant':definition['id'],'result':record['result'],'restored':record['isolated_restored'],'original_unchanged':record['original_unchanged']}),flush=True)
 if not killed:raise SystemExit('Unresolved mutant; inspect raw log before any classification correction')
report['restored_baseline']=run('restored-baseline','attestation_token_metadata');save();assert report['restored_baseline']['exit_code']==0
report['restored_unit_guard']=run('restored-unit-guard','lib','backup::export::attestation_ledger_tests::token_tables_are_required_at125_and_legacy124_absence_never_skips_keys');save();assert report['restored_unit_guard']['exit_code']==0
report['final_isolated_hashes']=hashes(source);report['final_original_hashes']=hashes(original);report['all_restored']=hashes(source)==hashes(original)==expected;save()
print(json.dumps({'finished':True,'killed':sum(m['result']=='KILLED' for m in report['mutants']),'all_restored':report['all_restored']}),flush=True)
