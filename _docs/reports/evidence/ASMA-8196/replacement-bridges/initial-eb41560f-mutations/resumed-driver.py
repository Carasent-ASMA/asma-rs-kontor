from pathlib import Path
import subprocess,os,json,hashlib,datetime,tarfile,io,difflib,re

owner=Path('/Users/igor/carasent/asma-modules/.worktrees/asma-8190/asma-rs-kontor')
root=Path('/private/tmp/ASMA-8196-bridge-mutations-eb41560f')
source=root/'source'
pin='eb41560fe3506eb06b43e5f2ab8f47ece61facaf'
archive=subprocess.check_output(['git','archive',pin],cwd=owner)
if not source.exists():
    source.mkdir(parents=True)
    tarfile.open(fileobj=io.BytesIO(archive)).extractall(source,filter='data')
paths=['crates/kontor-daemon/src/applications.rs','crates/kontor-store/src/repository.rs']
original={p:(source/p).read_text() for p in paths}
assert all((source/p).read_bytes()==(owner/p).read_bytes() for p in paths), 'resume only after exact restoration'
env=dict(os.environ,CARGO_TARGET_DIR='/Users/igor/carasent/asma-modules/_tools/asma-rs-kontor/target')
def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def run(name,args,expected):
    log=root/(name+'.txt')
    reused=log.exists()
    if not reused:
        with log.open('w') as f:
            f.write('QUALIFIED_SOURCE='+pin+'\n');f.flush()
            r=subprocess.run(args,cwd=source,env=env,stdout=f,stderr=subprocess.STDOUT)
            f.write('\nPROCESS_EXIT_CODE='+str(r.returncode)+'\n')
    text=log.read_text()
    assert text.startswith('QUALIFIED_SOURCE='+pin+'\n') and text.endswith('PROCESS_EXIT_CODE='+str(expected)+'\n'),name
    print(name+': exit '+str(expected)+(' (existing exact-pin receipt)' if reused else ''),flush=True)
    if expected==101:
        assert 'Finished `test` profile' in text and 'panicked at' in text and 'test result: FAILED.' in text
        assert 'error[E' not in text
    return {'log':log.name,'logSha256':sha(log),'exitCode':expected,'reusedExistingActualLog':reused}
daemon=['cargo','test','--locked','-p','kontor-daemon','--test','loopback_api']
store=['cargo','test','--locked','-p','kontor-store','--test','team_definition_migration_completeness']
baseline=[run('baseline-daemon',daemon+['replacement_bridges::'],0),run('baseline-store',store,0)]
results=[]
def mutate(name,path,changed,command):
    assert changed!=original[path]
    (source/path).write_text(changed)
    patch=root/(name+'.diff');diff=''.join(difflib.unified_diff(original[path].splitlines(True),changed.splitlines(True),fromfile=path,tofile=path))
    if patch.exists():assert patch.read_text()==diff,'reuse requires identical mutant patch'
    else:patch.write_text(diff)
    try:
        result=run(name,command,101)
        result.update(name=name,compiled=True,behavioralFailure=True,patch=patch.name,patchSha256=sha(patch),mutatedSourceSha256=sha(source/path))
        results.append(result)
    finally:(source/path).write_text(original[path])

p=paths[0];text=original[p]
start=text.index('        // An abandoned, never-bound attempt is not a candidate occupant')
end=text.index('        let mut leaves = runs',start)
old='        let named_parents: BTreeSet<AgentRunId> = runs.iter().filter(|run| !run.is_operator_abandoned_unbound()).filter_map(|run| run.parent_agent_run_id).collect();\n'
mutate('daemon-skip-bridge-ancestry',p,text[:start]+old+text[end:],daemon+['replacement_bridges::cancelled_bound_run_through_abandoned_bridges_names_only_the_current_native'])
matches=list(re.finditer(r'for run in runs\s*\.iter\(\)\s*\.filter\(\|run\| !run\.is_operator_abandoned_unbound\(\)\)\s*\{',text))
assert len(matches)==1
needle=matches[0].group()
mutate('daemon-count-trailing-abandoned',p,text.replace(needle,'for run in &runs {'),daemon+['replacement_bridges::trailing_abandoned_bridge_attempts_keep_the_bound_predecessor_named'])

p=paths[1];text=original[p]
prior=subprocess.check_output(['git','show','dddb72870c6d58d5d6383a69227844dbc8dca45e:'+p],cwd=owner).decode()
method=prior.index('    fn list_live_native_subjects(')
oldstart=prior.index('                    AND NOT EXISTS (',method)
oldend=prior.index('                 ORDER BY 1, 8',oldstart)
start=text.index('                    AND NOT EXISTS (\n                        WITH RECURSIVE descendants')
end=text.index('                 ORDER BY 1, 8',start)
mutate('store-skip-bridge-descendants',p,text[:start]+prior[oldstart:oldend]+text[end:],store+['abandoned_bridges_preserve_current_delivery_migration_record_and_confirmation'])
start=text.index('            if !occupied_seats.insert(seat_binding_id) {')
end=text.index('            let key = (',start)
mutate('store-skip-fork-fence',p,text[:start]+'            let _ = occupied_seats.insert(seat_binding_id);\n'+text[end:],store+['a_genuine_delivery_fork_refuses_the_native_migration_census'])

restored=[run('restored-daemon',daemon+['replacement_bridges::'],0),run('restored-store',store,0)]
sourcehash={p:sha(source/p) for p in paths}
assert all(sha(source/p)==sha(owner/p) for p in paths)
receipt={'recordedAt':datetime.datetime.now(datetime.timezone.utc).isoformat(),'qualifiedSource':pin,'tree':'c76412934c56e1b870defae559fd9a0f3cb97639','archiveSha256':hashlib.sha256(archive).hexdigest(),'baseline':baseline,'mutants':results,'restored':restored,'sourceSha256':sourcehash,'sourceSha256CaptureTime':'post-restoration','buildCache':'Shared _tools/asma-rs-kontor/target; isolated disposable source archive; all source restored and both green suites re-established after mutations','boundary':'Source behavioral mutation qualification only; production source, installed binaries, SQLite and native placement unchanged'}
receipt['driverRecovery']='Initial driver stopped after baseline greens and first compiled kill because its text locator did not accept rustfmt line breaks. Exact source restoration was verified; those actual logs and identical mutant patch were reused, with the initial driver failure preserved separately. No infrastructure failure earns behavioral credit.'
(root/'results.json').write_text(json.dumps(receipt,indent=2)+'\n')
print('MUTATION_QUALIFICATION_COMPLETE',flush=True)
