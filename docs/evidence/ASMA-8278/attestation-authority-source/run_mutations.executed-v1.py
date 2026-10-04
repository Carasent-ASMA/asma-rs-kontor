#!/usr/bin/env python3
"""Reproducible bounded fixture mutation checks; source checkout is read-only.
SQL guard mutants apply only in this isolated source copy/disposable test DBs.
No installed state, native effect or live qualification is exercised.
"""
import hashlib,io,json,os,pathlib,shutil,subprocess,tarfile,time
ROOT=pathlib.Path(__file__).resolve().parent
SOURCE=pathlib.Path('/tmp/asma-8278-module-consolidation-20261002.Mcrugq')
COPY=ROOT/'isolated-source'
TARGET=pathlib.Path('/var/folders/t3/0tbx772d571_57t01yb9twx00000gn/T/asma-8278-owner-check-validation-o7fe64z3/mutation-target')
OWNED=json.loads((ROOT/'owned-paths.json').read_text())
DEFINITIONS=json.loads((ROOT/'mutation-definitions.json').read_text())
def hashes(base):return {p:hashlib.sha256((base/p).read_bytes()).hexdigest() for p in OWNED}
original=hashes(SOURCE)
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=SOURCE,text=True).strip()
if COPY.exists():raise RuntimeError('Refuse to overwrite existing isolated-source')
COPY.mkdir()
archive=subprocess.check_output(['git','archive','HEAD'],cwd=SOURCE)
with tarfile.open(fileobj=io.BytesIO(archive)) as tar:tar.extractall(COPY,filter='data')
for p in OWNED:
    (COPY/p).parent.mkdir(parents=True,exist_ok=True)
    shutil.copyfile(SOURCE/p,COPY/p)
env=os.environ.copy();env['CARGO_TARGET_DIR']=str(TARGET)
results=[];commands=[]
def run(name,suite,test=None):
    command=['cargo','test','-p','kontor-store','--test',suite,'--offline','--jobs','2']
    if test:command += [test,'--','--exact']
    log=ROOT/(name+'.log');started=time.time()
    with log.open('wb') as out:completed=subprocess.run(command,cwd=COPY,env=env,stdout=out,stderr=subprocess.STDOUT)
    record={'name':name,'command':command,'exit':completed.returncode,'seconds':round(time.time()-started,3),'log':str(log)}
    commands.append(record)
    print(json.dumps(record),flush=True)
    return completed.returncode,log.read_text(errors='replace'),record
# Run each test used for mutation attribution with the restored source first.
for suite,test in dict.fromkeys((d['suite'],d['test']) for d in DEFINITIONS):
    code,output,record=run('baseline-'+test,suite,test)
    if code:raise RuntimeError('Baseline failed: '+record['log'])
for d in DEFINITIONS:
    path=COPY/d['path'];baseline=path.read_bytes();text=baseline.decode()
    if text.count(d['old']) != 1:raise RuntimeError('Mutation match count is not exactly one: '+d['id'])
    try:
        path.write_text(text.replace(d['old'],d['new']))
        code,output,record=run(d['id'],d['suite'],d['test'])
        assertion=('assertion failed' in output or 'assertion `left == right` failed' in output)
        killed=(code==101 and 'test result: FAILED.' in output and assertion and 'error[E' not in output and d['test']+' ... FAILED' in output)
        results.append(dict(record,id=d['id'],verdict='KILLED' if killed else 'UNRESOLVED',assertion_failure=assertion))
    finally:
        path.write_bytes(baseline)
        assert hashes(COPY)==original,'Isolated baseline not restored'
        assert hashes(SOURCE)==original,'Original source changed'
    (ROOT/'mutation-results.json').write_text(json.dumps({'head':head,'source_sha256':original,'results':results,'commands':commands},indent=2)+'\n')
# Re-run target suites after byte restoration, independent of mutant outcomes.
code,output,record=run('restored-ledger','attestation_authority')
if code:raise RuntimeError('Restored ledger baseline failed')
for test in [d['test'] for d in DEFINITIONS if d['suite']=='backup_snapshot']:
    code,output,record=run('restored-'+test,'backup_snapshot',test)
    if code:raise RuntimeError('Restored backup baseline failed')
assert hashes(COPY)==original
assert hashes(SOURCE)==original
summary={'head':head,'source_sha256':original,'restored_copy_sha256':hashes(COPY),'final_source_sha256':hashes(SOURCE),'commands':commands,'results':results,'all_assertion_killed':all(r['verdict']=='KILLED' for r in results)}
(ROOT/'mutation-results.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({'all_assertion_killed':summary['all_assertion_killed'],'count':len(results)}),flush=True)
