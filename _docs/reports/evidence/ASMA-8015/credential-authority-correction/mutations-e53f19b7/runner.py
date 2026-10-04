import pathlib, subprocess, os, datetime, json, hashlib, difflib, sys
OWNER=pathlib.Path('/Users/igor/carasent/asma-modules/.worktrees/asma-8015/asma-rs-kontor')
PIN='e53f19b744b80862b166b613dcb8992bcf371215'
ROOT=pathlib.Path('/private/tmp/ASMA-8015-mutations-e53f19b7');ROOT.mkdir(exist_ok=False)
SRC=ROOT/'source';SRC.mkdir()
archive=subprocess.check_output(['git','archive',PIN],cwd=OWNER);(ROOT/'source.tar').write_bytes(archive)
subprocess.run(['tar','-xf',str(ROOT/'source.tar'),'-C',str(SRC)],check=True)
subprocess.run(['cp','-cR','/private/tmp/ASMA-8015-security-correction/target',str(ROOT/'target')],check=True)
env=dict(os.environ);env.pop('KONTOR_AUTH',None);env.update(RUSTC_WRAPPER='',CARGO_TARGET_DIR=str(ROOT/'target'),CARGO_BUILD_JOBS='2',CARGO_TERM_COLOR='never',SWAGGER_UI_DOWNLOAD_URL='file:///private/tmp/ASMA-8015-security-correction/swagger-ui-v5.17.14.zip')
def inventory():
 return {str(p.relative_to(SRC)):{'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'bytes':p.stat().st_size} for p in sorted(SRC.rglob('*')) if p.is_file()}
before=inventory();(ROOT/'source-entries-before.json').write_text(json.dumps(before,indent=2)+'\n')
results={'sourceCommit':PIN,'sourceTree':subprocess.check_output(['git','rev-parse',PIN+'^{tree}'],cwd=OWNER,text=True).strip(),'sourceArchiveSha256':hashlib.sha256(archive).hexdigest(),'source':str(SRC),'target':str(ROOT/'target'),'scope':'Exact-source synthetic behavioral mutations only; no live OS credential, deployment or closure claim','cacheBoundary':'Separate private clone; no original cache changes','baseline':[],'mutations':[],'restored':[]}
def run(name,args,timeout=None):
 start=datetime.datetime.now(datetime.timezone.utc).isoformat();log=ROOT/(name+'.txt');print('Starting '+name,flush=True)
 with log.open('wb') as out:
  try:code=subprocess.run(['cargo','test','--offline','--locked']+args,cwd=SRC,env=env,stdout=out,stderr=subprocess.STDOUT,timeout=timeout).returncode
  except subprocess.TimeoutExpired:code=124
  out.write(('\nPROCESS_EXIT_CODE='+str(code)+'\n').encode())
 rec={'name':name,'command':['cargo','test','--offline','--locked']+args,'startedAt':start,'finishedAt':datetime.datetime.now(datetime.timezone.utc).isoformat(),'exitCode':code,'log':str(log),'logSha256':hashlib.sha256(log.read_bytes()).hexdigest()};print(name+' exit '+str(code),flush=True);return rec
checks=[('account',['-p','kontor-accounts','--lib']),('jira-unit',['-p','kontor-jira','--lib']),('jira-scope',['-p','kontor-jira','--test','credential_scope']),('daemon',['-p','kontor-daemon','--bin','kontor-daemon']),('realm',['-p','kontor-store','--test','realm_preflight'])]
def save(): (ROOT/'execution-results.json').write_text(json.dumps(results,indent=2)+'\n')
for name,args in checks:
 r=run('baseline-'+name,args);results['baseline'].append(r);save()
 if r['exitCode']:sys.exit(1)
cred='crates/kontor-jira/src/credentials.rs';main='crates/kontor-daemon/src/main.rs';transport='crates/kontor-accounts/src/resolver.rs';store='crates/kontor-store/src/lib.rs'
def replace_once(s,a,b):
 assert s.count(a)==1,(a,s.count(a));return s.replace(a,b,1)
def omit_previous(s):
 start=s.index('    let previous = match keychain.secret(&target) {');end=s.index('    // A failed writer',start)
 return s[:start]+'    let previous: Option<SecretString> = None;\n'+s[end:]
def omit_rollback(s):
 start=s.index('    let restored = match &previous {');end=s.index('    if restored {',start)
 return s[:start]+'    let restored = true;\n'+s[end:]
def block_stdin(s):
 a='fcntl_setfl(pipe, flags | OFlags::NONBLOCK)';assert s.count(a)==2
 return s.replace(a,'fcntl_setfl(pipe, flags)',1)
mutants=[
 ('MUT-SCOPE',cred,lambda s:replace_once(s,'format!("kontor-jira:{realm}:{root_hash}")','format!("kontor-jira:{realm}")'),['-p','kontor-jira','--lib','credentials::tests::duplicate_aliases_in_other_roots_and_copied_realms_cannot_change_the_running_consumers_target','--','--exact']),
 ('MUT-ROLLBACK',cred,omit_rollback,['-p','kontor-jira','--lib','credentials::tests::denied_mismatched_and_malformed_readback_restore_the_exact_previous_entry','--','--exact']),
 ('MUT-PREVIOUS',cred,omit_previous,['-p','kontor-jira','--lib','credentials::tests::an_unreadable_previous_state_refuses_without_mutation','--','--exact']),
 ('MUT-ALIAS',main,lambda s:replace_once(s,'if !connectors.references_alias(alias) {','if false && !connectors.references_alias(alias) {'),['-p','kontor-daemon','--bin','kontor-daemon','tests::unknown_roots_aliases_and_relative_paths_refuse_before_stdin_or_effects','--','--exact']),
 ('MUT-ABSOLUTE',main,lambda s:replace_once(s,'if !state_root.is_absolute() {','if false && !state_root.is_absolute() {'),['-p','kontor-daemon','--bin','kontor-daemon','tests::unknown_roots_aliases_and_relative_paths_refuse_before_stdin_or_effects','--','--exact']),
 ('MUT-DEADLINE',transport,lambda s:replace_once(s,'if Instant::now() >= deadline {','if false && Instant::now() >= deadline {'),['-p','kontor-accounts','--lib','resolver::security_transport_tests::a_stalled_security_process_is_killed_and_reaped','--','--exact']),
 ('MUT-PIPE',transport,block_stdin,['-p','kontor-accounts','--lib','resolver::security_transport_tests::stdin_delivery_is_also_bounded_and_the_child_is_reaped','--','--exact']),
 ('MUT-REAP',transport,lambda s:replace_once(s,'child.wait().map_err(|_| KeychainFailure::Unavailable)?;','let _ = child.id();'),['-p','kontor-accounts','--lib','resolver::security_transport_tests::a_stalled_security_process_is_killed_and_reaped','--','--exact']),
 ('MUT-INITIALIZE',store,lambda s:replace_once(s,'rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX','rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_CREATE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX'),['-p','kontor-store','--test','realm_preflight','realm_preflight_refuses_absent_empty_or_unsupported_databases_without_repair','--','--exact']),
]
for name,path,mutate,args in mutants:
 file=SRC/path;original=file.read_bytes();modified=mutate(original.decode()).encode();assert modified!=original
 patch=''.join(difflib.unified_diff(original.decode().splitlines(keepends=True),modified.decode().splitlines(keepends=True),fromfile='a/'+path,tofile='b/'+path));(ROOT/(name+'.diff')).write_text(patch)
 try:
  file.write_bytes(modified);r=run(name,args,timeout=180);text=(ROOT/(name+'.txt')).read_text(errors='replace')
  r.update(sourcePath=path,originalSourceSha256=hashlib.sha256(original).hexdigest(),mutatedSourceSha256=hashlib.sha256(modified).hexdigest(),patch=str(ROOT/(name+'.diff')),patchSha256=hashlib.sha256(patch.encode()).hexdigest(),compiled='Finished `test` profile' in text and 'Running ' in text,behavioralAssertionFailure='test result: FAILED. 0 passed; 1 failed;' in text)
  r['outcome']='KILLED' if r['exitCode']==101 and r['compiled'] and r['behavioralAssertionFailure'] else 'NOT_QUALIFIED';results['mutations'].append(r);save()
 finally:file.write_bytes(original)
 assert file.read_bytes()==original
 if r['outcome']!='KILLED':sys.exit(1)
for name,args in checks:
 r=run('restored-'+name,args);results['restored'].append(r);save()
 if r['exitCode']:sys.exit(1)
after=inventory();(ROOT/'source-entries-after.json').write_text(json.dumps(after,indent=2)+'\n');results.update(sourceInventoryRestored=before==after,sourceEntryCount=len(after),allNineGenuineBehavioralKills=len(results['mutations'])==9 and all(m['outcome']=='KILLED' for m in results['mutations']),finishedAt=datetime.datetime.now(datetime.timezone.utc).isoformat());save();assert before==after
print('Nine behavioral kills; exact source restoration verified',flush=True)
