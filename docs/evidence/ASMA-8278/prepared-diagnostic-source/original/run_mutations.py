from pathlib import Path
import subprocess,hashlib,json,os,re,time,datetime
p=Path(__file__).resolve().parent;a=json.loads((p/'assignment.json').read_text());original=Path(a['source_root']);copy=p/'mutation-source';pins=json.loads((p/'mutation-baseline-hashes.json').read_text());defs=json.loads((p/'mutation-definitions.json').read_text())
env=os.environ.copy();env['CARGO_TARGET_DIR']=str(p/'mutation-target')
def sha(q):return hashlib.sha256(q.read_bytes()).hexdigest()
def hashes(src):return {q:sha(src/q) for q in pins}
def run(name,needle):
 cmd=['cargo','test','--offline','--jobs','2','-p','kontor-runtime','--lib',needle,'--','--nocapture'];log=p/'mutation-logs'/name;t=time.monotonic();started=datetime.datetime.now(datetime.timezone.utc).isoformat()
 with log.open('wb') as f:code=subprocess.run(cmd,cwd=copy,env=env,stdout=f,stderr=subprocess.STDOUT).returncode
 return {'command':cmd,'cwd':str(copy),'cargo_target':env['CARGO_TARGET_DIR'],'started_utc':started,'elapsed_seconds':round(time.monotonic()-t,3),'exit_code':code,'log':str(log.relative_to(p)),'log_sha256':sha(log)}
report={'baseline':None,'mutants':[],'limits':'Four isolated compiler fixture defects, no original source mutation, no live effects. Compile failures never count as kills.'}
def save(): (p/'mutation-report.json').write_text(json.dumps(report,indent=2)+'\n')
needle='planning_pair::attestation::prepared_diagnostic'
assert hashes(original)==pins==hashes(copy)
report['baseline']=run('baseline.log',needle);save();assert report['baseline']['exit_code']==0
print(json.dumps({'baseline':'PASS'}),flush=True)
for d in defs:
 path=copy/d['path'];baseline=path.read_bytes();text=baseline.decode();assert text.count(d['old'])==1
 try:
  path.write_text(text.replace(d['old'],d['new']));mutant_sha=sha(path)
  r=run(d['id']+'.log',needle+'::tests::'+d['test']);raw=(p/r['log']).read_text()
  panics=[]
  for pathstr,line,col in re.findall(r"panicked at (crates/[^:\n]+):(\d+):(\d+):",raw):
   lines=(copy/pathstr).read_text().splitlines();n=int(line);excerpt='\n'.join(f'{i+1}: {lines[i]}' for i in range(max(0,n-4),min(len(lines),n+3)))
   panics.append({'path':pathstr,'line':n,'column':int(col),'source_excerpt':excerpt,'assertion_or_expect_err':any(x in excerpt for x in ['assert_eq!','assert_ne!','assert!','expect_err('])})
  compiled='Finished `test` profile' in raw and 'could not compile' not in raw and 'error[E' not in raw
  intended=bool(re.search(r'test .*'+re.escape(d['test'])+r' \.\.\. FAILED',raw))
  killed=r['exit_code']!=0 and compiled and intended and any(z['assertion_or_expect_err'] for z in panics)
  r.update({'id':d['id'],'defect':d['defect'],'mutated_path':d['path'],'mutant_sha256':mutant_sha,'successful_compilation':compiled,'intended_test_failed':intended,'panics':panics,'result':'KILLED' if killed else 'UNRESOLVED'})
 finally:
  path.write_bytes(baseline)
 r['isolated_restored']=hashes(copy)==pins;r['original_unchanged']=hashes(original)==pins;report['mutants'].append(r);save()
 print(json.dumps({'id':d['id'],'result':r['result'],'restored':r['isolated_restored'],'original_unchanged':r['original_unchanged']}),flush=True)
 assert killed and r['isolated_restored'] and r['original_unchanged'],r
report['restored_baseline']=run('restored-baseline.log',needle);report['final_original_hashes']=hashes(original);report['final_isolated_hashes']=hashes(copy);report['all_restored']=pins==hashes(original)==hashes(copy);save();assert report['restored_baseline']['exit_code']==0 and report['all_restored'];print(json.dumps({'finished':True,'killed':len(report['mutants']),'all_restored':True}),flush=True)
