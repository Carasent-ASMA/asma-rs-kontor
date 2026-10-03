#!/usr/bin/env python3
"""Freeze additive A-01 source/fixture evidence; retain prior ledger bundle."""
import datetime,hashlib,json,pathlib,re,shutil,subprocess
ROOT=pathlib.Path(__file__).resolve().parent
SOURCE=pathlib.Path('/tmp/asma-8278-module-consolidation-20261002.Mcrugq')
PRIOR=pathlib.Path('/var/folders/t3/0tbx772d571_57t01yb9twx00000gn/T/asma-8278-authority-ledger-validation-eevekms_')
OWNED=['crates/kontor-store/src/backup/restore.rs','crates/kontor-store/tests/backup_snapshot.rs']
def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
provenance=json.loads((ROOT/'provenance.json').read_text())
prior=json.loads((PRIOR/'manifest.json').read_text())
assert sha(PRIOR/'manifest.json')=='ce009be367771e8fbe0a323b95106ce041fb1acea4831322013a31ea36b312c6'
prior_artifacts={p:sha(PRIOR/p)==v['sha256'] for p,v in prior['artifacts'].items()}
prior_source={p:sha(PRIOR/'source-candidate'/p)==v for p,v in prior['owned_paths_sha256'].items()}
assert all(prior_artifacts.values()) and all(prior_source.values()),'Prior evidence changed'
mutation=json.loads((ROOT/'mutation-result.json').read_text());hashes={p:sha(SOURCE/p) for p in OWNED}
assert mutation['verdict']=='KILLED'
assert hashes==mutation['source_before_sha256']==mutation['source_after_sha256']==mutation['isolated_after_sha256']
for p in OWNED:
    target=ROOT/'source-candidate'/p;target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(SOURCE/p,target)
red=(ROOT/'red-at-67dc-behavior.log').read_text()
assert 'Finished `test` profile' in red and 'error[E' not in red and 'test result: FAILED. 0 passed; 2 failed;' in red
red_panics=re.findall(r'panicked at (crates/[^:]+):(\d+):(\d+):',red)
assert len(red_panics)==2
red_proof=[]
for p,line,column in red_panics:
    text=(SOURCE/p).read_text().splitlines()[int(line)-1].strip()
    assert text.startswith('assert!(')
    red_proof.append({'path':p,'line':int(line),'column':int(column),'text':text})
checks=json.loads((ROOT/'check-results.json').read_text());assert all(c['exit']==0 for c in checks)
counts={p.name:[{'status':m[1],'passed':int(m[2]),'failed':int(m[3]),'ignored':int(m[4]),'filtered':int(m[5])} for m in re.finditer(r'test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;.*?(\d+) filtered out;',p.read_text())] for p in ROOT.glob('*.log')}
assert counts['backup-full.log'][0]['passed']==23
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=SOURCE,text=True).strip()
(ROOT/'final-status.log').write_text(subprocess.check_output(['git','status','--porcelain'],cwd=SOURCE,text=True))
manifest={
 'schema_version':1,'epic':'ASMA-8278','finding':'A-01 Medium stated-contract defect','result':'bounded source-ready correction; no live qualification or closure',
 'created_at_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
 'source_behavior_base':provenance['source_behavior_base'],'actual_entry_head':provenance['actual_entry_head'],'mutation_copy_base_head':mutation['base_head'],'actual_final_parent_head':head,
 'owned_paths_sha256':hashes,'source_root':str(SOURCE),'source_candidate_root':str(ROOT/'source-candidate'),'exclusive_source_writes_finished':True,
 'change':'Refuse destination nonempty WAL unconditionally before main-file metadata/classification; retain immutable ledger inspection only for nonempty readable main and retain source/nonempty-main guards.',
 'red_proof':{'log':'red-at-67dc-behavior.log','compiled_successfully':True,'passed':0,'failed':2,'original_restore_source_sha256':provenance['source_restore_sha256_before'],'assertion_lines':red_proof,'source_pin':'restore.rs bytes were asserted identical to immutable67dc before tests/fix; docs-only descendants do not change that behavior','command':['cargo','test','-p','kontor-store','--test','backup_snapshot','main_with_committed_wal','--offline','--jobs','2'],'environment':{'RUST_TEST_THREADS':'2'}},
 'final_validation':{'new_case_tests':3,'affected_backup_tests':23,'missing_zero_positive_subcases':4,'clippy':'store all-targets offline jobs2 -Dwarnings','format':'cargo fmt --all -- --check','diff':'git diff --check','unaffected_990_broad_tests_repeated':False,'commands':checks,'per_log_counts':counts},
 'mutation':mutation,
 'prior_evidence_preservation':{'root':str(PRIOR),'manifest_sha256':sha(PRIOR/'manifest.json'),'every_recorded_artifact_hash_unchanged':prior_artifacts,'every_prior_source_candidate_hash_unchanged':prior_source,'no_original_evidence_file_written':True},
 'review_receipt':{'path':provenance['review_receipt'],'sha256':provenance['review_receipt_sha256']},
 'fixture_provenance':{'construction':provenance['fixture'],'independent_commit_check':'A separate read-only SQLite connection reads one actual committed issuer/key/public-DER row in donor before WAL/SHM bytes are copied unchanged. WAL magic and nonempty frame payload/SHM are asserted. Target main is deliberately absent or zero; target before/after main optional bytes, WAL bytes, SHM bytes and directory inventory are compared.','simulation':'Missing/zero main is explicitly simulated, not a captured production crash.','recoverability_limit':'A separate tiny Python SQLite experiment did not recover a copied WAL after manually removing/truncating main. Neither these tests nor this correction establish WAL-alone recovery or live native proof.','experiment':'wal-fixture-experiment.json','production_key_material':'None; existing public DER fixture [1,2,3] is public metadata only and has no RSA-validity/issuance claim.'},
 'boundaries':['Only two owned source paths changed; no Cargo/dependency/schema/authority changes','Existing offline-caller-lock contract remains required','Ordinary missing/zero main with no nonempty WAL retains supported restore; no blind immutable open of absent/empty main','Nonempty ledger/current-target/source refusals, schema123 exact-schema refusal and URI/no-sidecar behavior retained by full affected suite','No stage/commit/push, installed database, signing/keychain/provider/native/Jira effects','Root publishes and re-reviews; live/full qualification and epic closure remain reserved']
}
manifest['artifacts']={p.name:{'sha256':sha(p),'bytes':p.stat().st_size} for p in ROOT.iterdir() if p.is_file() and p.name not in ('manifest.json','manifest.sha256')}
(ROOT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
(ROOT/'manifest.sha256').write_text(sha(ROOT/'manifest.json')+'  manifest.json\n')
print(json.dumps({'manifest':str(ROOT/'manifest.json'),'sha256':sha(ROOT/'manifest.json'),'actual_final_parent_head':head,'owned_paths_sha256':hashes,'backup_passed':23,'mutant':mutation['verdict'],'exclusive_writes_finished':True},indent=2))
