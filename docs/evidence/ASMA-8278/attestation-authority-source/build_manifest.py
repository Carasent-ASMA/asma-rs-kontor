#!/usr/bin/env python3
"""Freeze ASMA-8278 bounded ledger source/fixture evidence; no qualification."""
import datetime,hashlib,json,pathlib,re,shutil,subprocess
ROOT=pathlib.Path(__file__).resolve().parent
SOURCE=pathlib.Path('/tmp/asma-8278-module-consolidation-20261002.Mcrugq')
OWNED=json.loads((ROOT/'owned-paths.json').read_text())
checks=json.loads((ROOT/'check-results.json').read_text())
mutations=json.loads((ROOT/'mutation-results.json').read_text())
def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def utc(seconds):return datetime.datetime.fromtimestamp(seconds,datetime.timezone.utc).isoformat()
source_hashes={p:sha(SOURCE/p) for p in OWNED}
assert source_hashes==mutations['source_sha256']==mutations['final_source_sha256']==mutations['restored_copy_sha256']
assert mutations['all_assertion_killed'] and len(mutations['results'])==8
for path in OWNED:
    target=ROOT/'source-candidate'/path;target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(SOURCE/path,target)
assert {p:sha(ROOT/'source-candidate'/p) for p in OWNED}==source_hashes
metadata=json.loads(subprocess.check_output(['cargo','metadata','--offline','--no-deps','--format-version','1'],cwd=SOURCE,text=True))
packages=[p for p in metadata['packages'] if p['name'] in ('kontor-core','kontor-store')]
expected={(p['name'],'unit','lib') for p in packages}
expected|={(p['name'],'integration',t['name']) for p in packages for t in p['targets'] if 'test' in t['kind']}
pattern=re.compile(r'test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;.*?(\d+) filtered out; finished in ([0-9.]+)s')
def suites(name,initial_package=None):
    package=initial_package;target=None;result=[]
    for line in (ROOT/name).read_text().splitlines():
        match=re.search(r'Running unittests src/lib.rs .*deps/(kontor_core|kontor_store)-',line)
        if match:package=match[1].replace('_','-');target=(package,'unit','lib')
        match=re.search(r'Running tests/([^ .]+)\.rs',line)
        if match:target=(package,'integration',match[1])
        match=pattern.search(line)
        if match and target:result.append(dict(package=target[0],kind=target[1],target=target[2],status=match[1],passed=int(match[2]),failed=int(match[3]),ignored=int(match[4]),filtered=int(match[5]),seconds=float(match[6]),log=name))
    return result
first=suites('core-store-regression.log')
remaining=suites('remaining-final-regression.log','kontor-store')
backup=suites('backup-final.log','kontor-store')
coverage={tuple(r[k] for k in ('package','kind','target')):r for r in first if r['status']=='ok'}
for record in remaining+backup:
    assert record['status']=='ok';coverage[tuple(record[k] for k in ('package','kind','target'))]=record
assert set(coverage)==expected,(sorted(expected-set(coverage)),sorted(set(coverage)-expected))
assert sum(r['passed'] for r in coverage.values())==990
all_log_counts={p.name:[dict(status=m[1],passed=int(m[2]),failed=int(m[3]),ignored=int(m[4]),filtered=int(m[5]),seconds=float(m[6])) for m in pattern.finditer(p.read_text(errors='replace'))] for p in ROOT.glob('*.log')}
doc_counts=all_log_counts['core-store-doctests.log'];assert sum(x['passed'] for x in doc_counts)==5
summary=json.loads((ROOT/'source-summary.json').read_text());summary['paths']=source_hashes
(ROOT/'source-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
status=subprocess.check_output(['git','status','--porcelain'],cwd=SOURCE,text=True)
(ROOT/'final-status.log').write_text(status)
with (ROOT/'classifier-correction.log').open('w') as log:
    subprocess.run(['python3',str(ROOT/'reclassify_saved_results.py')],check=True,stdout=log,stderr=subprocess.STDOUT)
mutations=json.loads((ROOT/'mutation-results.json').read_text())
manifest={
 'schema_version':1,'epic':'ASMA-8278','result':'bounded public-metadata source ready; no live or full-formal qualification',
 'created_at_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
 'base_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=SOURCE,text=True).strip(),
 'source_root':str(SOURCE),'evidence_root':str(ROOT),'source_candidate_root':str(ROOT/'source-candidate'),
 'owned_paths_sha256':source_hashes,'all_exclusive_source_writes_finished':True,
 'coverage':{'unique_core_store_tests':990,'doctests':5,'ignored':sum(x['ignored'] for x in coverage.values()),'expected_suites':len(expected),'covered_suites':len(coverage),'successful_suite_records':list(coverage.values()),'aggregation':'714 tests from46 successful initial broad suites +274 from8 final remaining/repaired suites, then replace initial backup18 with final backup20 =990; partial failed suite counts and duplicate reruns not added'},
 'temporal_provenance':{'initial_broad_command_was_not_green':True,'initial_backup_compiled_count':18,'final_backup_compiled_count':20,'initial_backup_binary_record':next(line for line in (ROOT/'core-store-regression.log').read_text().splitlines() if 'Running tests/backup_snapshot.rs' in line),'final_backup_binary_record':next(line for line in (ROOT/'backup-final.log').read_text().splitlines() if 'Running tests/backup_snapshot.rs' in line),'final_backup_source_last_modified_utc':utc((SOURCE/'crates/kontor-store/tests/backup_snapshot.rs').stat().st_mtime),'late_cases':['empty_ledger_restore_handles_uri_delimiters_in_the_target_path','an_uncheckpointed_public_ledger_refusal_leaves_all_target_files_unchanged'],'strengthened_case_after_initial_compile':'a_nonempty_public_authority_snapshot_is_refused_before_destination_creation added source-byte/directory checks','final_proof':'backup-final.log compiles/runs all20 cases; unaffected earlier broad suites are retained, not replayed','timing_limit':'Raw Cargo logs do not timestamp individual lines; recorded command durations and log last-write timestamps are provided, without inferred exact per-case wall-clock times'},
 'commands':checks,'per_log_test_counts':all_log_counts,
 'retained_failures':{'focused.log':'initial fixture reused a unique project root; final focused10/10 green','backup-focused.log':'15/17: historical raw snapshot creation already refuses noncurrent schema; read-only target open created sidecars. Final contract-preserving guards/tests20/20 green','core-store-regression.log':'v115 fixture left124 tables behind;88/89 failed historical suite only','remaining-and-repaired-regression.log':'v115 fixture then queried current-only tables through census while temporarily at115; same outbox assertion retained through direct count, final89/89 green','fmt.log':'initial formatting check before owned-path format'},
 'mutations':{'assertion_killed':8,'unresolved':0,'runner':'run_mutations.py','executed_runner':'run_mutations.executed-v1.py','definitions':'mutation-definitions.json','corrected_report':'mutation-results.json','original_report':'mutation-results-first-classifier.json','correction':'classifier-only; M03 custom material assertion at393 and M05 custom NULL-revocation assertion at412 are evidenced by exact existing assert! lines and successful test compilation','assertion_proof':'mutation-assertion-proof.json','source_and_isolated_hashes_restored':True,'restored_baseline':'restored-ledger.log10/10 plus three restored backup target logs1/1 each','mutant_records':mutations['results']},
 'boundaries':summary['limits']+['No actor/grant/permission or authorizing conversion','No changes to Unsupported native actuation, issuer custody or current generation/possession/fencing claims','Core privacy doctests are retained existing5 cases; no new live/native qualification','RSA DER validity is not asserted by persistence; public configuration projections stay detached','No commit/stage/push, installed database, signing/production keys, provider or Jira effects','Root alone reviews and publishes; independent/live gates and epic closure remain reserved']
}
manifest['artifacts']={p.name:dict(sha256=sha(p),bytes=p.stat().st_size,last_write_utc=utc(p.stat().st_mtime)) for p in ROOT.iterdir() if p.is_file() and p.name not in ('manifest.json','manifest.sha256')}
(ROOT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
(ROOT/'manifest.sha256').write_text(sha(ROOT/'manifest.json')+'  manifest.json\n')
print(json.dumps({'manifest':str(ROOT/'manifest.json'),'sha256':sha(ROOT/'manifest.json'),'tests':manifest['coverage']['unique_core_store_tests'],'doctests':manifest['coverage']['doctests'],'suites':len(coverage),'mutants':8,'owned_paths':len(source_hashes)},indent=2))
