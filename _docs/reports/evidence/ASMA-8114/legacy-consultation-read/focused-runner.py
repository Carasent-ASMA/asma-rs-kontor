import datetime, hashlib, json, os, pathlib, subprocess, sys

root = pathlib.Path('/Users/igor/carasent/asma-modules/.worktrees/asma-8113/asma-rs-kontor')
folder = pathlib.Path('/private/tmp/ASMA-8114-legacy-read-qualification-20260926')
label, *selectors = sys.argv[1:]
assert selectors and '/' not in label
log = folder / (label + '.txt')
receipt = folder / (label + '.json')
assert not log.exists(), 'Preserve previous executions'
paths = ['crates/kontor-daemon/src/applications.rs', 'crates/kontor-daemon/tests/loopback_api.rs', 'crates/kontor-daemon/tests/loopback/legacy_consultation_reads.rs', 'Cargo.lock']
hashes = {p: hashlib.sha256((root / p).read_bytes()).hexdigest() for p in paths}
env = os.environ.copy()
env.update({'RUSTC_WRAPPER': '', 'CARGO_TARGET_DIR': str(folder / 'target'), 'CARGO_BUILD_JOBS': '2'})
env.pop('KONTOR_AUTH', None)
env.pop('JIRA_API_TOKEN', None)
started = datetime.datetime.now(datetime.timezone.utc).isoformat()
runs = []
with log.open('wb') as output:
    for selector in selectors:
        cmd = ['cargo', 'test', '--locked', '-p', 'kontor-daemon', '--test', 'loopback_api', selector, '--', '--nocapture', '--test-threads=1']
        output.write(('COMMAND=' + json.dumps(cmd) + '\n').encode())
        output.flush()
        start = datetime.datetime.now(datetime.timezone.utc).isoformat()
        result = subprocess.run(cmd, cwd=root, env=env, stdout=output, stderr=subprocess.STDOUT)
        finish = datetime.datetime.now(datetime.timezone.utc).isoformat()
        output.write(f'PROCESS_EXIT_CODE={result.returncode}\n'.encode())
        output.flush()
        runs.append({'command': cmd, 'startedAt': start, 'finishedAt': finish, 'exitCode': result.returncode})
        if result.returncode:
            break
assert hashes == {p: hashlib.sha256((root / p).read_bytes()).hexdigest() for p in paths}
data = {'label': label, 'startedAt': started, 'finishedAt': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'cwd': str(root), 'runs': runs, 'exitCode': runs[-1]['exitCode'], 'sourceSha256BeforeAndAfter': hashes, 'sourceUnchanged': True, 'logPath': str(log), 'logSha256': hashlib.sha256(log.read_bytes()).hexdigest(), 'scope': 'Disposable synthetic Realms and a private cloned build cache; no live runtime or database writes.'}
receipt.write_text(json.dumps(data, indent=2) + '\n')
print(json.dumps(data))
print(log.read_text(errors='replace')[-5000:])
