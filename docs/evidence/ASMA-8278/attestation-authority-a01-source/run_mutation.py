#!/usr/bin/env python3
"""Reproduce exact A-01 guard bypass only in isolated source/disposable DBs."""
import datetime,hashlib,io,json,os,pathlib,re,shutil,subprocess,tarfile,time
ROOT=pathlib.Path(__file__).resolve().parent
SOURCE=pathlib.Path('/tmp/asma-8278-module-consolidation-20261002.Mcrugq')
COPY=ROOT/'isolated-source'
OWNED=['crates/kontor-store/src/backup/restore.rs','crates/kontor-store/tests/backup_snapshot.rs']
DEFINITION=json.loads((ROOT/'mutation-definition.json').read_text())
TARGET='/var/folders/t3/0tbx772d571_57t01yb9twx00000gn/T/asma-8278-owner-check-validation-o7fe64z3/mutation-target'
def hashes(base):return {p:hashlib.sha256((base/p).read_bytes()).hexdigest() for p in OWNED}
original=hashes(SOURCE)
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=SOURCE,text=True).strip()
if COPY.exists():raise RuntimeError('Existing copy must not be overwritten')
COPY.mkdir()
archive=subprocess.check_output(['git','archive','HEAD'],cwd=SOURCE)
with tarfile.open(fileobj=io.BytesIO(archive)) as tar:tar.extractall(COPY,filter='data')
for path in OWNED:shutil.copyfile(SOURCE/path,COPY/path)
env=os.environ.copy();env['CARGO_TARGET_DIR']=TARGET;env['RUST_TEST_THREADS']='2'
records=[]
def run(name,filter):
    command=['cargo','test','-p','kontor-store','--test','backup_snapshot',filter,'--offline','--jobs','2']
    start=time.time();log=ROOT/(name+'.log')
    with log.open('w') as out:result=subprocess.run(command,cwd=COPY,env=env,stdout=out,stderr=subprocess.STDOUT)
    record={'name':name,'command':command,'environment':{'CARGO_TARGET_DIR':TARGET,'RUST_TEST_THREADS':'2'},'started_at_utc':datetime.datetime.fromtimestamp(start,datetime.timezone.utc).isoformat(),'finished_at_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'seconds':round(time.time()-start,3),'exit':result.returncode,'log':str(log)}
    records.append(record);print(json.dumps(record),flush=True)
    return result.returncode,log.read_text(),record
code,output,record=run('isolated-baseline','main_')
assert code==0 and 'test result: ok. 3 passed;' in output,'Baseline must pass'
path=COPY/DEFINITION['path'];bytes_before=path.read_bytes();text=bytes_before.decode()
assert text.count(DEFINITION['old'])==1
proof=[]
try:
    path.write_text(text.replace(DEFINITION['old'],DEFINITION['new']))
    mutant_sha256=hashlib.sha256(path.read_bytes()).hexdigest()
    code,output,record=run('isolated-A01-mutant',DEFINITION['filter'])
    compiled='Finished `test` profile' in output and 'error[E' not in output
    failures=[test for test in DEFINITION['tests'] if test+' ... FAILED' in output]
    panics=re.findall(r'panicked at (crates/[^:]+):(\d+):(\d+):',output)
    for file,line,column in panics:
        assertion_line=(COPY/file).read_text().splitlines()[int(line)-1].strip()
        proof.append({'path':file,'line':int(line),'column':int(column),'text':assertion_line,'is_assertion':assertion_line.startswith(('assert!(', 'assert_eq!(', 'assert_ne!('))})
    killed=code==101 and compiled and len(failures)==2 and len(proof)==2 and all(p['is_assertion'] for p in proof) and 'test result: FAILED. 0 passed; 2 failed;' in output
finally:
    path.write_bytes(bytes_before)
    assert hashes(SOURCE)==original,'Original source changed'
    assert hashes(COPY)==original,'Isolated baseline not restored'
code,output,record=run('isolated-restored-baseline','main_')
assert code==0 and 'test result: ok. 3 passed;' in output,'Restored baseline must pass'
report={'id':DEFINITION['id'],'base_head':head,'definition':'mutation-definition.json','runner':'run_mutation.py','commands':records,'verdict':'KILLED' if killed else 'UNRESOLVED','compiled_successfully':compiled,'intended_failed_tests':failures,'assertion_proof':proof,'mutant_sha256':mutant_sha256,'source_before_sha256':original,'source_after_sha256':hashes(SOURCE),'isolated_after_sha256':hashes(COPY),'restored_baseline_passed':True}
(ROOT/'mutation-result.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({'verdict':report['verdict'],'source_and_copy_restored':hashes(SOURCE)==hashes(COPY)==original}),flush=True)
assert killed,'Mutant was not an intended assertion kill'
