import base64
import difflib
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path('/private/tmp/ASMA-8015-mutations-3a35b7c6')
ROOT.mkdir(exist_ok=False)
OWNER = Path('/Users/igor/carasent/asma-modules/.worktrees/asma-8015/asma-rs-kontor')
PIN = '3a35b7c6db8ef65e51aaa14c7ba69df9bf1e1c0f'
TREE = 'ea784274c592100b9154f8c9a1fbd4a297ab7ffc'
assert subprocess.check_output(['git', 'rev-parse', PIN+'^{tree}'], cwd=OWNER, text=True).strip() == TREE
ARCHIVE = ROOT / 'source.tar'
ARCHIVE.write_bytes(subprocess.check_output(['git', 'archive', PIN], cwd=OWNER))
CACHE = Path('/private/tmp/ASMA-8015-credential-qualification/target')
EXPECTED_ARCHIVE = hashlib.sha256(ARCHIVE.read_bytes()).hexdigest()

def digest(data):
    return hashlib.sha256(data).hexdigest()

def now():
    return datetime.now(timezone.utc).isoformat()

def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + '\n')

def snapshot(folder):
    result = {}
    for path in sorted(folder.rglob('*')):
        key = str(path.relative_to(folder))
        if path.is_symlink():
            result[key] = ['symlink', os.readlink(path)]
        elif path.is_dir():
            result[key] = ['directory']
        elif path.is_file():
            result[key] = ['file', digest(path.read_bytes())]
        else:
            raise AssertionError(str(path))
    return result

assert digest(ARCHIVE.read_bytes()) == EXPECTED_ARCHIVE
SOURCE = ROOT / 'source'
SOURCE.mkdir()
with tarfile.open(ARCHIVE) as archive:
    for member in archive.getmembers():
        assert not member.name.startswith('/') and '..' not in Path(member.name).parts
    archive.extractall(SOURCE, filter='data')
before = snapshot(SOURCE)
write_json(ROOT / 'source-entries-before.json', before)
print('Copying the idle QA cache into a new private copy; original cache is read-only.', flush=True)
subprocess.run(['cp', '-cR', str(CACHE), str(ROOT / 'target')], check=True)
env = os.environ.copy()
env.pop('KONTOR_AUTH', None)
env.update(RUSTC_WRAPPER='', CARGO_TARGET_DIR=str(ROOT / 'target'), CARGO_BUILD_JOBS='2', CARGO_TERM_COLOR='never')
LOGS = ROOT / 'logs'
LOGS.mkdir()
PATCHES = ROOT / 'patches'
PATCHES.mkdir()
commands = [
    ('account-unit', ['cargo', 'test', '--offline', '--locked', '-p', 'kontor-accounts', '--lib']),
    ('jira-unit', ['cargo', 'test', '--offline', '--locked', '-p', 'kontor-jira', '--lib']),
    ('daemon-operator', ['cargo', 'test', '--offline', '--locked', '-p', 'kontor-daemon', '--bin', 'kontor-daemon']),
]

receipt = {
    'startedAt': now(), 'sourceCommit': PIN,
    'sourceTree': TREE,
    'sourceArchiveSha256': EXPECTED_ARCHIVE, 'source': str(SOURCE),
    'target': str(ROOT / 'target'), 'initialCacheCopiedFrom': str(CACHE),
    'cacheBoundary': 'Private cloned cache; original QA and primary checkout caches are not modified. Source archive is disposable and isolated.',
    'sourceEntryCount': len(before), 'baseline': [], 'mutants': [], 'restored': [],
    'liveDatabaseOpenedByRunner': False, 'installedBinariesOrPrimarySourceModified': False,
}

def run(label, command):
    path = LOGS / (label + '.txt')
    started = now()
    print('Starting ' + label, flush=True)
    with path.open('wb') as log:
        log.write(('STARTED_AT=' + started + '\nCOMMAND=' + json.dumps(command) + '\n').encode())
        log.flush()
        process = subprocess.run(command, cwd=SOURCE, env=env, stdout=log, stderr=subprocess.STDOUT)
        log.write(('\nPROCESS_EXIT_CODE=' + str(process.returncode) + '\nFINISHED_AT=' + now() + '\n').encode())
    data = path.read_bytes()
    text = data.decode('utf-8')
    summaries = [dict(zip(['passed', 'failed', 'ignored', 'measured', 'filtered'], map(int, values)))
                 for values in re.findall(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out', text)]
    record = {'command': command, 'startedAt': started, 'finishedAt': now(), 'exitCode': process.returncode,
              'log': str(path.relative_to(ROOT)), 'logSha256': digest(data), 'summaries': summaries}
    print(label + ' exit ' + str(process.returncode) + ' ' + json.dumps(summaries), flush=True)
    return record, text

for name, command in commands:
    record, text = run('baseline-' + name, command)
    receipt['baseline'].append(record)
    write_json(ROOT / 'results.json', receipt)
    if record['exitCode'] != 0 or not record['summaries']:
        receipt['failure'] = 'Baseline did not pass; no mutant was applied.'
        write_json(ROOT / 'results.json', receipt)
        sys.exit(2)

def remove_segment(text, start, end):
    assert text.count(start) == 1
    a = text.index(start)
    b = text.index(end, a)
    return text[:a] + text[b:]

def no_lock(text):
    return remove_segment(text, '    let _lock = kontor_daemon::lock::StateRootLock::acquire', '    let secret = kontor_jira::read_credential_document(reader)?;')

def no_equality(text):
    return remove_segment(text, '    if observed.expose_secret() != canonical.expose_secret()', '    parse_credentials(&observed)?;')

def no_validation(text):
    return remove_segment(text, '    if email.trim().is_empty()', '    Ok(credentials)')

def no_stdin_bound(text):
    assert text.count('.take(MAX_DOCUMENT_BYTES + 1)') == 1
    return text.replace('pub fn read_credential_document(reader: impl Read)', 'pub fn read_credential_document(mut reader: impl Read)').replace('        .take(MAX_DOCUMENT_BYTES + 1)\n', '')

def no_line_bound(text):
    return remove_segment(text, '    if input.len() >= 4096', '    Ok(input)')

def no_escaping(text):
    return remove_segment(text, '            if ch == ', '            input.push(ch);')

def jira_test(name):
    return ['cargo', 'test', '--offline', '--locked', '-p', 'kontor-jira', '--lib', 'credentials::tests::'+name, '--', '--exact', '--nocapture']

def accounts_test(name):
    return ['cargo', 'test', '--offline', '--locked', '-p', 'kontor-accounts', '--lib', 'resolver::security_transport_tests::'+name, '--', '--exact', '--nocapture']

mutants = [
    ('MUT-LOCK', 'crates/kontor-daemon/src/main.rs', no_lock,
     ['cargo', 'test', '--offline', '--locked', '-p', 'kontor-daemon', '--bin', 'kontor-daemon', 'tests::a_running_realm_refuses_credential_installation_before_stdin_or_effects', '--', '--exact', '--nocapture'],
     'locked realm must refuse before reading credentials', 1),
    ('MUT-READBACK', 'crates/kontor-jira/src/credentials.rs', no_equality,
     jira_test('a_writer_success_does_not_replace_matching_reader_proof'),
     'a_writer_success_does_not_replace_matching_reader_proof', 1),
    ('MUT-VALIDATE', 'crates/kontor-jira/src/credentials.rs', no_validation,
     jira_test('invalid_documents_and_aliases_have_no_keychain_effect'),
     'invalid_documents_and_aliases_have_no_keychain_effect', 1),
    ('MUT-STDIN', 'crates/kontor-jira/src/credentials.rs', no_stdin_bound,
     jira_test('stdin_is_bounded_and_read_errors_are_redacted'),
     'stdin_is_bounded_and_read_errors_are_redacted', 1),
    ('MUT-LINE', 'crates/kontor-accounts/src/resolver.rs', no_line_bound,
     accounts_test('multiline_or_truncated_commands_are_refused_before_launch'),
     'multiline_or_truncated_commands_are_refused_before_launch', 1),
    ('MUT-QUOTE', 'crates/kontor-accounts/src/resolver.rs', no_escaping,
     accounts_test('the_writer_transports_quoted_secret_only_on_stdin'),
     'the_writer_transports_quoted_secret_only_on_stdin', 1),
]

all_killed = True
for mid, relative, mutate, command, marker, expected_failed in mutants:
    path = SOURCE / relative
    original = path.read_bytes()
    changed = mutate(original.decode('utf-8')).encode('utf-8')
    assert changed != original
    patch = ''.join(difflib.unified_diff(original.decode().splitlines(keepends=True), changed.decode().splitlines(keepends=True),
                                        fromfile='a/' + relative, tofile='b/' + relative)).encode()
    carrier = {'encoding': 'base64', 'decodedSha256': digest(patch), 'decodedBytes': len(patch), 'rawBase64': base64.b64encode(patch).decode()}
    patch_path = PATCHES / (mid + '.diff.json')
    write_json(patch_path, carrier)
    try:
        path.write_bytes(changed)
        record, text = run(mid, command)
        compiled = 'Finished `test` profile' in text and 'Compiling kontor-' in text and 'could not compile' not in text and not re.search(r'^error\[E\d+\]', text, re.M)
        behavioral = 'panicked at' in text and marker in text
        failed = sum(v['failed'] for v in record['summaries'])
        killed = record['exitCode'] == 101 and compiled and behavioral and failed == expected_failed
        record.update(id=mid, file=relative, sourceSha256Before=digest(original), mutatedSourceSha256=digest(changed),
                      patch=str(patch_path.relative_to(ROOT)), patchCarrierSha256=digest(patch_path.read_bytes()),
                      patchDecodedSha256=digest(patch), compiled=compiled, behavioralAssertionMatched=behavioral,
                      expectedFailed=expected_failed, observedFailed=failed, outcome='KILLED' if killed else 'NOT_QUALIFIED', assertionMarker=marker)
        all_killed = all_killed and killed
    finally:
        path.write_bytes(original)
        assert path.read_bytes() == original
    record['sourceSha256PostRestoration'] = digest(path.read_bytes())
    receipt['mutants'].append(record)
    write_json(ROOT / 'results.json', receipt)

restored_green = True
for name, command in commands:
    record, text = run('restored-' + name, command)
    receipt['restored'].append(record)
    restored_green = restored_green and record['exitCode'] == 0 and bool(record['summaries'])
    write_json(ROOT / 'results.json', receipt)

after = snapshot(SOURCE)
write_json(ROOT / 'source-entries-after.json', after)
receipt.update(finishedAt=now(), sourceRestoredByteExact=after == before,
               sourceEntriesBeforeSha256=digest((ROOT / 'source-entries-before.json').read_bytes()),
               sourceEntriesAfterSha256=digest((ROOT / 'source-entries-after.json').read_bytes()),
               allSixCompiledBehavioralKills=all_killed, restoredGreen=restored_green,
               qualificationComplete=all_killed and restored_green and after == before)
write_json(ROOT / 'results.json', receipt)
print('Qualification complete: ' + str(receipt['qualificationComplete']), flush=True)
sys.exit(0 if receipt['qualificationComplete'] else 3)
