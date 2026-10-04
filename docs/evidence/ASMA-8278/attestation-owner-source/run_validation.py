from pathlib import Path
import hashlib, io, json, os, shutil, subprocess, tarfile
run = Path(__file__).parent
repo = Path('/tmp/asma-8278-module-consolidation-20261002.Mcrugq')
owned = ['crates/kontor-runtime/src/planning_pair/attestation.rs', 'crates/kontor-runtime/src/planning_pair/attestation/owner_checks.rs', 'crates/kontor-runtime/src/planning_pair/attestation/owner_checks/tests.rs']
manifest = {'source_head': subprocess.run(['git','rev-parse','HEAD'],cwd=repo,check=True,capture_output=True,text=True).stdout.strip(), 'owned_sha256': {path:hashlib.sha256((repo/path).read_bytes()).hexdigest() for path in owned}, 'commands':[], 'mutants':[]}
def execute(name, command, directory, env=None):
    with (run/(name+'.log')).open('w') as log:
        result = subprocess.run(command,cwd=directory,env=env,stdout=log,stderr=subprocess.STDOUT)
    record = {'name':name,'command':command,'cwd':str(directory),'exit':result.returncode,'log':str(run/(name+'.log'))}
    manifest['commands'].append(record)
    (run/'MANIFEST.json').write_text(json.dumps(manifest,indent=2)+'\n')
    print(name+': exit '+str(result.returncode),flush=True)
    return record
checks = [
    ('focused-current',['cargo','test','-p','kontor-runtime','--lib','planning_pair::attestation::owner_checks','--offline','--jobs','2']),
    ('runtime-regression',['cargo','test','-p','kontor-runtime','--offline','--jobs','2']),
    ('fmt',['cargo','fmt','--all','--','--check']),
    ('diff-check',['git','diff','--check']),
    ('clippy',['cargo','clippy','-p','kontor-runtime','--all-targets','--offline','--jobs','2','--','-D','warnings']),
]
for name,command in checks:
    record = execute(name,command,repo)
    if record['exit'] != 0:
        raise SystemExit('failed '+name)
workspace = run/'mutation-source'
workspace.mkdir()
archive = subprocess.run(['git','archive','HEAD'],cwd=repo,check=True,stdout=subprocess.PIPE).stdout
with tarfile.open(fileobj=io.BytesIO(archive)) as package:
    package.extractall(workspace,filter='data')
parent = 'crates/kontor-runtime/src/planning_pair/attestation.rs'
attestation = repo/'crates/kontor-runtime/src/planning_pair/attestation'
for source in [repo/parent,*attestation.rglob('*.rs')]:
    destination = workspace/source.relative_to(repo)
    destination.parent.mkdir(parents=True,exist_ok=True)
    shutil.copy2(source,destination)
manifest['combined_attestation_sha256'] = {str(p.relative_to(repo)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [repo/parent,*attestation.rglob('*.rs')]}
source = workspace/owned[1]
baseline = source.read_text()
base_hash = hashlib.sha256(source.read_bytes()).hexdigest()
env = os.environ.copy()
env['CARGO_TARGET_DIR'] = str(run/'mutation-target')
base_command = ['cargo','test','-p','kontor-runtime','--lib','--offline','--jobs','2']
if execute('mutation-baseline-before',base_command+['planning_pair::attestation::owner_checks'],workspace,env)['exit'] != 0:
    raise SystemExit('red mutation baseline')
mutants = [
    ('skip-current-revision','    if facts.key_set_revision == 0 || facts.key_set_revision != verified.snapshot_revision() {','    if false && (facts.key_set_revision == 0 || facts.key_set_revision != verified.snapshot_revision()) {','zero_older_and_newer_key_set_revisions_are_refused'),
    ('skip-token-revocation','    if facts.token.revoked {','    if facts.token.revoked && false {','key_and_token_revocation_after_verification_are_refused'),
    ('skip-generation','    if facts.seat.occupancy_generation == 0\n        || facts.seat.occupancy_generation != claims.occupancy_generation\n    {','    if false && (facts.seat.occupancy_generation == 0\n        || facts.seat.occupancy_generation != claims.occupancy_generation)\n    {','current_binding_and_nonzero_occupancy_generation_must_match'),
    ('skip-active-seat','    if !facts.seat.active {','    if !facts.seat.active && false {','active_seat_and_active_node_are_both_required'),
    ('skip-token-time','    if !contains_time(claims.not_before, claims.expires_at, facts.now)\n        || !contains_time(facts.token.not_before, facts.token.expires_at, facts.now)\n    {','    if false && (!contains_time(claims.not_before, claims.expires_at, facts.now)\n        || !contains_time(facts.token.not_before, facts.token.expires_at, facts.now))\n    {','signed_token_expiry_between_verification_and_observation_is_refused'),
    ('skip-native-identity','    if facts.seat.native_identity.generation == 0\n        || facts.seat.native_identity != &claims.native_identity\n    {','    if false && (facts.seat.native_identity.generation == 0\n        || facts.seat.native_identity != &claims.native_identity)\n    {','every_native_identity_component_and_positive_generation_must_match'),
    ('skip-current-operation','    if !claims.allowed_operations.contains(&facts.operation) {','    if !claims.allowed_operations.contains(&facts.operation) && false {','the_current_requested_operation_must_be_signed'),
]
for name,old,new,test in mutants:
    if baseline.count(old) != 1:
        raise RuntimeError('mutation target not unique: '+name)
    try:
        source.write_text(baseline.replace(old,new,1))
        full_test = 'planning_pair::attestation::owner_checks::tests::'+test
        record = execute(name,base_command+[full_test],workspace,env)
        log = Path(record['log']).read_text()
        killed = record['exit'] != 0 and 'test result: FAILED' in log and 'error[E' not in log
        manifest['mutants'].append({'name':name,'test':full_test,'result':'KILLED' if killed else 'INVALID_OR_SURVIVED','log':record['log']})
    finally:
        source.write_text(baseline)
        assert hashlib.sha256(source.read_bytes()).hexdigest() == base_hash
    (run/'MANIFEST.json').write_text(json.dumps(manifest,indent=2)+'\n')
    print(name+': '+manifest['mutants'][-1]['result'],flush=True)
if execute('mutation-baseline-after',base_command+['planning_pair::attestation::owner_checks'],workspace,env)['exit'] != 0:
    raise SystemExit('red restored mutation baseline')
manifest['isolated_source_restored'] = hashlib.sha256(source.read_bytes()).hexdigest() == base_hash
manifest['original_owned_sources_preserved'] = all(hashlib.sha256((repo/path).read_bytes()).hexdigest()==digest for path,digest in manifest['owned_sha256'].items())
(run/'MANIFEST.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(str(run/'MANIFEST.json'),flush=True)
if not manifest['original_owned_sources_preserved'] or any(item['result']!='KILLED' for item in manifest['mutants']):
    raise SystemExit(1)
