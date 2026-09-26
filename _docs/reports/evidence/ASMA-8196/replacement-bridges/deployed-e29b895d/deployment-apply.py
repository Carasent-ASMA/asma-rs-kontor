from pathlib import Path
import datetime, hashlib, json, os, shutil, sqlite3, subprocess, time

work = Path(__file__).parent
plan = json.loads((work / 'plan.json').read_text())
backup = Path(plan['backupDirectory'])
state = Path('/Users/igor/.local/state/kontor/asma')
bin_dir = Path('/Users/igor/.local/bin')
service = f'gui/{os.getuid()}/com.asma.kontor.daemon'
plist = '/Users/igor/Library/LaunchAgents/com.asma.kontor.daemon.plist'
digest = lambda p: hashlib.sha256(Path(p).read_bytes()).hexdigest()
now = lambda: datetime.datetime.now(datetime.timezone.utc).isoformat().replace('+00:00', 'Z')

assert not backup.exists(), 'Deployment must not apply twice'
build_path = Path('/private/tmp/kontor-ASMA-8196-release-e29b895d/build-receipt.json')
qualification_path = Path(plan['qualificationReceipt'])
assert digest(build_path) == plan['buildReceiptSha256']
assert digest(qualification_path) == plan['qualificationReceiptSha256']
build = json.loads(build_path.read_text())
qualification = json.loads(qualification_path.read_text())
assert build['exitCode'] == 0 and build['acceptedExecutableSourceUnchanged']
assert qualification['qualifiedSource'] == plan['qualifiedSource'] and qualification['exitCode'] == 0
for path, h in qualification['sourceAndTestSha256'].items():
    assert digest(Path('/private/tmp/kontor-ASMA-8196-release-e29b895d/source') / path) == h
for row in plan['binaryChecks']:
    assert digest(row['candidate']) == row['candidateSha256']
    assert digest(row['installedPath']) == row['installedSha256']

backup.mkdir(mode=0o700)
shutil.copy2(work / 'plan.json', backup / 'plan.json')
shutil.copy2(build_path, backup / 'build-receipt.json')
shutil.copy2(qualification_path, backup / 'source-qualification.json')
configs = {name: digest(state / name) for name in ['runtimes.json', 'fleet.yml']}
old_mcp_link = os.readlink(bin_dir / 'kontor-mcp')
(backup / 'kontor-mcp.old-link').write_text(old_mcp_link)
for row in plan['binaryChecks']:
    shutil.copy2(row['installedPath'], backup / (row['name'] + '.old'))

def wait_stopped():
    for _ in range(30):
        r = subprocess.run(['lsof', '-tiTCP:7717', '-sTCP:LISTEN'], capture_output=True, text=True)
        if not r.stdout.strip():
            return
        time.sleep(0.5)
    raise RuntimeError('Daemon listener did not stop')

def install_file(source, target):
    staging = target.with_name(target.name + '.ASMA-8196-staging')
    shutil.copy2(source, staging)
    os.chmod(staging, 0o755)
    os.replace(staging, target)

def replace_mcp_link(target):
    staging = bin_dir / 'kontor-mcp.ASMA-8196-link'
    if staging.is_symlink():
        staging.unlink()
    os.symlink(target, staging)
    os.replace(staging, bin_dir / 'kontor-mcp')

def lineage_identity():
    with sqlite3.connect('file:' + str(state / 'kontor.db') + '?mode=ro', uri=True) as db:
        db.row_factory = sqlite3.Row
        team = '01a0b032-fe68-70c2-8cd7-e5bcb76fb56d'
        runs = [dict(r) for r in db.execute('select id,parent_agent_run_id,team_run_id,role_key,account_profile_id from agent_runs where team_run_id=? order by id', (team,))]
        bindings = [dict(r) for r in db.execute('select b.* from runtime_bindings b join agent_runs a on a.id=b.agent_run_id where a.team_run_id=? order by b.id', (team,))]
        seats = [dict(r) for r in db.execute('select id,topology_node_id,role_slot_id,task_id,team_run_id,parent_seat_binding_id,replaced_by_seat_binding_id from seat_bindings where team_run_id=? order by id', (team,))]
        gate = dict(db.execute('select * from task_gate_evaluations where workflow_id=? and gate_key=? and sequence=2', ('01a0aaa3-64a3-7c33-8a0e-cf2d989aeeb7','high-verification-gate')).fetchone())
        return {'runs':runs,'runtimeBindings':bindings,'seatIdentities':seats,'asma8188Gate2':gate}

before_identity = lineage_identity()
(work / 'before-lineage-identity.json').write_text(json.dumps(before_identity, indent=2)+'\n')
stopped = False
replaced = False
bootstrapped = False
try:
    subprocess.run(['launchctl', 'bootout', service], check=True)
    stopped = True
    wait_stopped()
    with sqlite3.connect('file:' + str(state / 'kontor.db') + '?mode=ro', uri=True) as source:
        assert source.execute('pragma user_version').fetchone()[0] == 119
        assert source.execute('pragma quick_check').fetchone()[0] == 'ok'
        assert not source.execute('pragma foreign_key_check').fetchall()
        assert source.execute('select realm_id from realm_metadata').fetchone()[0] == plan['realmId']
        s = json.loads(source.execute('select state from epic_completion where mini_project_id=?', ('01a06e13-878a-7aa3-8a08-bfad07cc8c4c',)).fetchone()[0])
        assert s['phase'] == 'Done' and s['revision'] == 14
        with sqlite3.connect(backup / 'kontor-before.db') as destination:
            source.backup(destination)
    before_db_hash = digest(backup / 'kontor-before.db')
    replaced = True
    for row in plan['binaryChecks']:
        target = bin_dir / row['name']
        if row['name'] == 'kontor-mcp':
            target = bin_dir / 'kontor-mcp.e29b895d'
        install_file(Path(row['candidate']), target)
        assert digest(target) == row['candidateSha256']
    replace_mcp_link(str(bin_dir / 'kontor-mcp.e29b895d'))
    subprocess.run(['launchctl', 'bootstrap', f'gui/{os.getuid()}', plist], check=True)
    bootstrapped = True
    response = None
    for _ in range(30):
        result = subprocess.run([str(bin_dir / 'kontor'), 'realm-get', '--state-root', str(state)], capture_output=True, text=True, timeout=5)
        if result.returncode == 0:
            response = json.loads(result.stdout)
            if response.get('status') == 200 and response['body']['realm_id'] == plan['realmId']:
                break
        time.sleep(0.5)
    else:
        raise RuntimeError('New daemon did not return exact observer realm readback')
    with sqlite3.connect('file:' + str(state / 'kontor.db') + '?mode=ro', uri=True) as db:
        health = {'schema': db.execute('pragma user_version').fetchone()[0], 'quickCheck': db.execute('pragma quick_check').fetchone()[0], 'foreignKeyViolations': len(db.execute('pragma foreign_key_check').fetchall())}
        assert health == {'schema': 119, 'quickCheck': 'ok', 'foreignKeyViolations': 0}
        s = json.loads(db.execute('select state from epic_completion where mini_project_id=?', ('01a06e13-878a-7aa3-8a08-bfad07cc8c4c',)).fetchone()[0])
        assert s['phase'] == 'Done' and s['revision'] == 14
    after_identity = lineage_identity()
    assert after_identity == before_identity, 'Immutable lineage or gate identity changed'
    assert configs == {name: digest(state / name) for name in configs}
    receipt = {'appliedAt': now(), 'sourceCommit': plan['sourceCommit'], 'qualifiedSource': plan['qualifiedSource'], 'sourceTree': build['sourceTree'], 'schema': 119, 'binarySha256': {row['name']: digest(row['installedPath']) for row in plan['binaryChecks']}, 'mcpSymlink': os.readlink(bin_dir / 'kontor-mcp'), 'backup': str(backup / 'kontor-before.db'), 'backupSha256': before_db_hash, 'observerRealmReadback': response, 'health': health, 'configHashesUnchanged': configs, 'asma8098PhasePreserved': 'Done', 'asma8098RevisionPreserved': 14, 'controlledRestart': True, 'manualSQLiteWrites': 0, 'lineageAndGateIdentitiesUnchanged': True, 'beforeLineageIdentitySha256': digest(work / 'before-lineage-identity.json'), 'boundary': 'Actual merged-source lineage-repair deployment and exact realm/DB health only; fresh supported settlement, exact ASMA-8190 preview, all-thirteen census and workflow settlement remain separate. No source or export identity was rewritten.'}
    (backup / 'deployment.json').write_text(json.dumps(receipt, indent=2) + '\n')
    (work / 'applied-receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt, indent=2))
except Exception:
    if bootstrapped:
        subprocess.run(['launchctl', 'bootout', service], check=True)
        wait_stopped()
    if replaced:
        for name in ['kontor-daemon', 'kontor']:
            install_file(backup / (name + '.old'), bin_dir / name)
        replace_mcp_link(old_mcp_link)
    if stopped:
        subprocess.run(['launchctl', 'bootstrap', f'gui/{os.getuid()}', plist], check=True)
    (work / 'rollback.json').write_text(json.dumps({'at': now(), 'oldBinariesRestored': replaced, 'oldServiceRestarted': stopped, 'databaseRestored': False}) + '\n')
    raise
