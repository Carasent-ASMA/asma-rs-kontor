import datetime, difflib, hashlib, io, json, os, pathlib, subprocess, tarfile

repo = pathlib.Path('/Users/igor/carasent/asma-modules/.worktrees/asma-8113/asma-rs-kontor')
commit = '51cfef9caaa1e2b12674ad932d0adfb0658d4777'
tree = 'c63a37e6d0b0db8e6cfb495d044e02d33f62a698'
root = pathlib.Path('/private/tmp/ASMA-8114-legacy-read-51cfef9c')
assert not root.exists()
root.mkdir()
source = root / 'source'
source.mkdir()
archive = subprocess.check_output(['git', 'archive', commit], cwd=repo)
(root / 'source.tar').write_bytes(archive)
with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
    tar.extractall(source, filter='data')
target = root / 'target'
subprocess.run(['cp', '-cR', '/private/tmp/ASMA-8114-legacy-read-qualification-20260926/target', str(target)], check=True)
env = os.environ.copy()
env.update({'RUSTC_WRAPPER': '', 'CARGO_TARGET_DIR': str(target), 'CARGO_BUILD_JOBS': '2'})
env.pop('KONTOR_AUTH', None)
env.pop('JIRA_API_TOKEN', None)
path = 'crates/kontor-daemon/src/applications.rs'
file = source / path
original = file.read_bytes()
text = original.decode()
assert hashlib.sha256(original).hexdigest() == '46de8c43aeb8d5619d26c274f9451edffb00f971d0f75aab490d8462d4e7350f'

def inventory():
    return {str(p.relative_to(source)): {'sha256': hashlib.sha256(p.read_bytes()).hexdigest(), 'bytes': p.stat().st_size} for p in sorted(source.rglob('*')) if p.is_file()}

before = inventory()
(root / 'source-entries-before.json').write_text(json.dumps(before, indent=2) + '\n')
selectors = ['legacy_consultation_reads', 'a_consultation_with_no_recorded_subject_refuses_to_be_named', 'committee_containers_follow_their_recorded_subject_not_their_caller', 'consultation_containers_follow_their_recorded_subject_not_their_caller']
results = {'sourceCommit': commit, 'sourceTree': tree, 'archiveSha256': hashlib.sha256(archive).hexdigest(), 'source': str(source), 'target': str(target), 'cacheBoundary': 'Private APFS-cloned cache; original focused target is not written. Source is a disposable exact Git archive.', 'startedAt': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'mutants': []}

def run(label, names):
    log = root / (label + '.txt')
    assert not log.exists()
    calls = []
    with log.open('wb') as out:
        for selector in names:
            command = ['cargo', 'test', '--locked', '-p', 'kontor-daemon', '--test', 'loopback_api', selector, '--', '--nocapture', '--test-threads=1']
            out.write(('COMMAND=' + json.dumps(command) + '\n').encode()); out.flush()
            start = datetime.datetime.now(datetime.timezone.utc).isoformat()
            result = subprocess.run(command, cwd=source, env=env, stdout=out, stderr=subprocess.STDOUT)
            finish = datetime.datetime.now(datetime.timezone.utc).isoformat()
            out.write(f'PROCESS_EXIT_CODE={result.returncode}\n'.encode()); out.flush()
            calls.append({'command': command, 'startedAt': start, 'finishedAt': finish, 'exitCode': result.returncode})
            if result.returncode:
                break
    return {'log': log.name, 'logSha256': hashlib.sha256(log.read_bytes()).hexdigest(), 'exitCode': calls[-1]['exitCode'], 'commands': calls}, log.read_text(errors='replace')

guard = 'if run.subject.is_none() || run.topic.is_none() {'
strict = '''            None => Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "the consultation has no durably recorded subject to name",
            )),'''
assert text.count(guard) == text.count(strict) == 1
faults = [
    ('MUT-LEGACY-READ', text.replace(guard, 'if run.topic.is_none() {'), ['legacy_consultation_reads'], ['legacy archival GET must remain readable:', 'left: 409', 'right: 200']),
    ('MUT-CANONICAL-NAME', text.replace(guard, 'if run.subject.is_some() || run.topic.is_none() {'), ['committee_containers_follow_their_recorded_subject_not_their_caller'], ['a rendered Committee container']),
    ('MUT-NATIVE-SUBJECT', text.replace(strict, '            None => Ok(None),'), ['a_consultation_with_no_recorded_subject_refuses_to_be_named'], ['left: 200', 'right: 409']),
]
try:
    baseline, _ = run('baseline', selectors)
    results['baseline'] = baseline
    assert baseline['exitCode'] == 0
    for label, mutated, names, markers in faults:
        assert file.read_bytes() == original
        patch = ''.join(difflib.unified_diff(text.splitlines(keepends=True), mutated.splitlines(keepends=True), fromfile='a/' + path, tofile='b/' + path))
        (root / (label + '.diff')).write_text(patch)
        file.write_text(mutated)
        try:
            outcome, log = run(label, names)
            compiled = 'Finished `test` profile' in log and 'Running tests/loopback_api.rs' in log
            killed = outcome['exitCode'] == 101 and compiled and all(marker in log for marker in markers) and 'test result: FAILED.' in log
            outcome.update({'id': label, 'patch': label + '.diff', 'patchSha256': hashlib.sha256(patch.encode()).hexdigest(), 'mutatedSourceSha256': hashlib.sha256(mutated.encode()).hexdigest(), 'compiled': compiled, 'expectedBehavioralMarkers': markers, 'killed': killed})
            results['mutants'].append(outcome)
            assert killed, label + ' did not yield its intended compiled behavioral kill'
        finally:
            file.write_bytes(original)
        assert inventory() == before
    restored, _ = run('restored', selectors)
    results['restored'] = restored
    assert restored['exitCode'] == 0
finally:
    file.write_bytes(original)
    after = inventory()
    (root / 'source-entries-after.json').write_text(json.dumps(after, indent=2) + '\n')
    results.update({'finishedAt': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'sourceRestoredByteExact': after == before, 'sourceEntryCount': len(before), 'sourceEntriesBeforeSha256': hashlib.sha256((root / 'source-entries-before.json').read_bytes()).hexdigest(), 'sourceEntriesAfterSha256': hashlib.sha256((root / 'source-entries-after.json').read_bytes()).hexdigest(), 'sourceAndTestSha256': {p: hashlib.sha256((source / p).read_bytes()).hexdigest() for p in [path, 'crates/kontor-daemon/tests/loopback_api.rs', 'crates/kontor-daemon/tests/loopback/legacy_consultation_reads.rs', 'Cargo.lock']}})
    (root / 'results.json').write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps(results))
