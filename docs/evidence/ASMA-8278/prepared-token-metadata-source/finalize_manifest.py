from pathlib import Path
import hashlib,json,subprocess,datetime,re
p=Path(__file__).resolve().parent
src=Path('/tmp/asma-8278-module-consolidation-20261002.Mcrugq')
def sha(q): return hashlib.sha256(q.read_bytes()).hexdigest()
def read(q): return json.loads((p/q).read_text())
def write(q,x): (p/q).write_text(json.dumps(x,indent=2)+'\n')
owned=read('final-owned-hashes.json')
assert len(owned)==13
report=read('post-partition-final/mutation-report.json')
assert report['all_restored'] and report['final_original_hashes']==owned==report['final_isolated_hashes']
assert all(sha(src/q)==h==sha(p/'candidate-files'/q)==sha(p/'post-partition-final/mutation-source'/q) for q,h in owned.items())
assert len(report['mutants'])==10
for group in ['mutation-report.json','post-partition-final/mutation-report.json']:
 r=read(group)
 assert len(r['mutants'])==10
 assert all(z['result']=='KILLED' and z['successful_compilation'] and z['intended_test_failed'] and z['isolated_restored'] and z['original_unchanged'] and any(t['assertion_or_expect_err'] for t in z['panics']) for z in r['mutants'])
 for z in r['baseline']+r['mutants']+[r['restored_baseline']]+([r['restored_unit_guard']] if 'restored_unit_guard' in r else []):
  assert sha(Path(z['log']))==z['log_sha256']
assert not read('post-partition/mutation-report.json')['mutants']
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=src,text=True).strip()
assert head=='ef0b71933d8879f33d963a4ad3b5f1fded4aa52f'
changed=subprocess.check_output(['git','diff','--name-only'],cwd=src,text=True).splitlines()
assert set(changed)<=set(owned)
restore=src/'crates/kontor-store/src/backup/restore.rs'
assert sha(restore)=='14ae5e6ff4cba2ffb0b7c94fdf250279ee88c8b8d7d2f932d2d253b60449d74c'
assert subprocess.check_output(['git','diff','--','Cargo.lock','Cargo.toml','crates/kontor-store/src/backup/restore.rs'],cwd=src,text=True)==''
write('handoff-readback.json',{'observed_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'source_head':head,'owned_count':13,'original_candidate_final_isolated_hashes_equal':True,'all_mutant_compilation_assertion_log_restoration_checks_pass':True,'tracked_changed_paths_within_owned':True,'restore_rs_sha256':sha(restore),'restore_and_root_cargo_unchanged':True,'exclusive_source_writes':'finished; released to root for publication','no_further_source_writes_scheduled':True,'live_effects':'none; no staging/commit/push/installedDB/keychain/native/provider/activation'})
(p/'README.md').write_text('''# ASMA-8278 prepared-token metadata source receipt

Source-ready under root contract `6d58238cdf349fe95acaec7e4d2e921e2fce80de`, spec1.4 section9.7. Module base/final HEAD is `ef0b71933d8879f33d963a4ad3b5f1fded4aa52f`; no source was staged, committed or pushed. Exclusive writes to the13 assigned paths are finished and released to root, the sole publisher. Exact final bytes are pinned in final-owned-hashes.json/tsv and candidate-files/.

## Result and limits

The existing AttestationAuthorityRepository and SqliteStore single writer now support named prepared-token metadata prepare, revoke and coherent-read operations. Migration125 is prospective and was applied only to disposable fixtures. Preparation checks bounds, same-realm/project/application scope, exact independent key/token heads, selected unrevoked key and interval containment, and actual active Hosted or Consultation provenance/current generation/native identity in BEGIN IMMEDIATE before permanent issuer-scoped identity reuse. Stored task=None is exact. Consultation Materializing, Running and AwaitingJudge are eligible; NeedsHuman, Settled and Disposed refuse. Native identities shared with another current seat/container refuse.

Identity, provenance, key commitments and registration revisions are immutable. Revocation is permanent; stale CAS refuses unchanged, while matching-head repeat revocation is unchanged and remains available for expired intended intervals, revoked keys or retired seats. SQL triggers protect registration coherence, permanence, replacement and head monotonicity. Backup continuity guards now inspect both schema124 key and schema125 token ledgers; corrupt125 absence refuses, legitimate124 absence retains prior contracts. Nonempty public history remains unsupported for qualified backup/import. Existing unconditional destination WAL guard and schema123 restore refusal are unchanged.

These are public untrusted metadata only. No issued/active/signature-valid flag, bearer/signature, authenticated actor, grant, permission, receipt, native intent or admission was added. Request expectations and payload digest authenticate nothing. Interval containment has no observed-current-time authority. Detached reads prove no freshness/authenticity; native identifiers prove no actual possession/fencing. Production crypto remains runtime-owned; future re-verification/composition inside the owner transaction is an unresolved source integration decision. Actual issuance/admission, issuer custody, possession, fencing and final live qualification remain Unsupported. This is bounded source/disposable-fixture proof, not epic acceptance or closure.

## Verification and temporal provenance

The deduplicated affected regression total is664 tests across30 suites (core359/store305), plus7 doctests (core5/runtime2/store0). This is an aggregation of observed phases, not one full run at final bytes. verified-suite-counts.json names every suite and its raw log. Relevant daemon/API/CLI/MCP compilation passed. Offline jobs2 scoped core/store all-targets Clippy with warnings denied passed after partition; final fmt/diff checks passed. New Rust files meet hard600-line/file and100-line/function bounds (production525/max63; main test348/max56; fixture396/max87; guards395/max54). Existing large files changed only within assigned seams.

08-affected-regression.log passed223 tests, with prepared19 cases; its compiled binary excluded later history-backup, family-qualified registry/disposed, and false-key-commitment SQL cases. 10-backup-regression.log covered history backup with prepared20 plus store lib18 and backup25. 15-family-registry-fixtures.log covered prepared21. 16-final-focused.log covered prepared22 before partition. Final23-post-partition-final-baseline.log covers all22 under the final test partition, including an added assertion for old-unrevoked key registration1 versus current preparation head2. Root test code was split into approved fixture.rs and guards.rs, and Fixture helpers were extracted; other10 previously owned path hashes equal the pre-partition mutation pins. Failed/intermediate fixture/type/path/target commands are retained, not counted as passes. No unaffected990-suite rerun was claimed.

The user-requested interruption left22-post-partition-baseline.log as a complete compile failure (moved include path and unused imports); its session later expired. Corrected final23 baseline and24 Clippy passed. Exact process readbacks are preserved. Source base and current contract dispositions are separately pinned.

## Mutation evidence

Ten unique seeded defects were assertion-killed before partition and replayed against final13-path pins using a fresh isolated target; these are10 unique mutants, not20. Each compiled successfully, failed the intended existing assertion/expect_err, records panic source path/line/excerpt, and restored original/isolated hashes. Definitions, runners, raw logs, baseline pins and reports are retained in the original and post-partition-final packets. The final replay restored22 integration cases and the targeted125 backup guard unit case, both passing; final13 original/candidate/isolated hashes match.

M01 current key head; M02 token-head CAS; M03 permanent identity/replacement SQL; M04 Hosted occupancy; M05 active binding/node; M06 exact None task; M07 container/native aliasing; M08 schema125 backup token-table guard; M09 permanent SQL revocation; M10 coherent SQL key commitment. No compiler failure counted as a kill. SQL migration mutations were explicitly assigned by root and touched only isolated compiler/disposable fixtures.

The preserved post-partition packet reused the prior mutant target. Its integration baseline passed22, but its unit baseline failed before any mutant; zero mutants were counted there. Original/copy source pins matched. The evidence is consistent with a stale reused mutation artifact, without claiming an established Cargo defect. Fresh post-partition-final target baselines, all10 mutants and restoration passed. The failed packet remains intact.

PlanningPair family fixture proves registry/native metadata selection only, using minimal registry/profile fixtures; it is not qualified placement, profile, protocol or live-native proof. The isolated backup token guard unit uses explicit orphaned synthetic metadata to test the guard independently; real API-created prepared rows also imply key history.

## Artifacts

manifest.json includes final13 hashes, phase outcomes/provenance, every retained raw log/runner/definition artifact hash, counts and limitations. Compiler target caches and copied full source fixture trees are excluded from the small artifact inventory; their assigned source hashes are independently pinned. candidate-files contains exact final13 byte copies. All earlier ledger/A01 review evidence is unchanged.
''')
# These normalized invocation scopes identify commands; older raw logs do not embed
# shell invocation text. Exact mutation argv, timing and environment live in runners.
prepared=['cargo','test','--offline','--jobs','2','-p','kontor-store','--test','attestation_token_metadata']
clippy=['cargo','clippy','--offline','--jobs','2','-p','kontor-core','-p','kontor-store','--all-targets','--','-D','warnings']
phases=[
('01-initial-check.log',0,['cargo','check','--offline','--jobs','2','-p','kontor-store'],'initial source compilation'),
('02-focused-initial.log',101,prepared,'three missing borrowed run-id strings; compile failure'),
('03-focused-fixture-build-fix.log',101,prepared,'13 passed/3 failed: uppercase fixture RoleKey'),
('04-focused-role-fix.log',101,prepared,'13 passed/3 failed: fixture CanonicalDocument schema_version'),
('05-focused-context-fix.log',0,prepared,'16 passed'),
('06-domain-guards.log',101,prepared,'fixture u64 ToSql compile failure'),
('07-affected-initial.log',101,['cargo','test','--offline','--jobs','2','-p','kontor-store','--test','planning_pair'],'incorrect test target'),
('08-affected-regression.log',0,['cargo','test','--offline','--jobs','2','-p','kontor-store','--test','attestation_authority','--test','attestation_token_metadata','--test','consultation_subject','--test','hosted_seat_autonomy','--test','planning_pair_store','--test','repository_roundtrip','--test','schema_v1'],'223 passed; prepared19, later three cases absent'),
('09-focused-backup-lib.log',101,['cargo','test','--offline','--jobs','2','-p','kontor-store','--lib','--test','attestation_token_metadata','--test','backup_snapshot'],'nonexistent export_state fixture method compile failure'),
('10-backup-regression.log',0,['cargo','test','--offline','--jobs','2','-p','kontor-store','--lib','--test','attestation_token_metadata','--test','backup_snapshot'],'lib18/prepared20/backup25'),
('11-core-regression.log',0,['cargo','test','--offline','--jobs','2','-p','kontor-core'],'359 tests/5 docs'),
('12-clippy-initial.log',0,clippy,'initial scoped Clippy'),
('13-backup-port-regression.log',101,['cargo','test','--offline','--jobs','2','-p','kontor-store','--test','backup_import'],'incorrect backup_import target'),
('14-backup-port-regression.log',0,['cargo','test','--offline','--jobs','2','-p','kontor-store','--test','backup_export','--test','consultation_profiles','--test','team_definition_backup_contract'],'25/9/2 passed'),
('15-family-registry-fixtures.log',0,prepared,'21 passed'),
('16-final-focused.log',0,prepared,'22 passed before partition'),
('17-dependent-check.log',0,['cargo','check','--offline','--jobs','2','-p','kontor-daemon','-p','kontor-api','-p','kontor-cli','-p','kontor-mcp'],'dependent compilation passed'),
('18-mutation-runner.log',0,['python3','run_mutations.py'],'pre-partition10 compiled assertion kills and restored baselines; complete report plus expired later session'),
('19-privacy-store-doc.log',0,['cargo','test','--offline','--jobs','2','--doc','-p','kontor-runtime','-p','kontor-store'],'runtime2/store0 docs'),
('20-fmt-check.log',0,['cargo','fmt','--all','--','--check'],'pre-partition formatting'),
('21-final-clippy.log',0,clippy,'pre-partition final source Clippy'),
('22-post-partition-baseline.log',101,prepared,'complete compile-failure log; session expired after user interruption'),
('23-post-partition-final-baseline.log',0,prepared,'final13-path22 passing cases'),
('24-post-partition-clippy.log',0,clippy,'final13-path scoped Clippy'),
('25-post-partition-mutation-setup.log',0,['python3','post-partition/prepare_mutations.py'],'13 source pins match isolated copy'),
('26-post-partition-mutation-runner.log',1,['python3','post-partition/run_mutations.py'],'reused target: integration22 pass, baseline unit guard fail; zero mutant kills counted'),
('27-coding-size-check.log',0,['python3','check_sizes.py'],'all new Rust sizes pass'),
('28-final-fmt.log',0,['cargo','fmt','--all','--','--check'],'formatting, later exact readback repeated'),
('29-final-diff-check.log',0,['git','diff','--check'],'diff check, later exact readback repeated'),
('30-fresh-target-replay-setup.log',0,['python3','post-partition-final/prepare_mutations.py'],'final13 copy matches'),
('31-fresh-target-replay.log',0,['python3','post-partition-final/run_mutations.py'],'fresh target:10 compiled assertion kills; restored integration22/guard1'),
('32-final-fmt-readback.log',0,['cargo','fmt','--all','--','--check'],'exact final process exit0'),
('33-final-diff-readback.log',0,['git','diff','--check'],'exact final process exit0'),
]
phase_records=[]
for name,code,command,note in phases:
 q=p/'logs'/name;t=q.read_text()
 phase_records.append({'log':'logs/'+name,'log_sha256':sha(q),'exit_code':code,'normalized_invocation_scope':command,'outcome':note,'raw_test_results':[{'status':m[0],'passed':int(m[1]),'failed':int(m[2])} for m in re.findall(r'test result: (ok|FAILED)\. (\d+) passed; (\d+) failed;',t)],'content_provenance':'phase-local source; final thirteen-byte pins apply specifically to final23/24 and successful post-partition-final replay, not retrospectively to all earlier phases'})
write('phase-command-outcomes.json',{'command_note':'Normalized cargo invocation scopes reconstructed from executed command context; optional output flags/order are not represented as a byte-exact shell transcript. Mutation reports preserve exact executed argv/timing/log hashes; persisted runners preserve environment/target roots. Process readback packets preserve actual exits, including expired historical sessions.','phases':phase_records})
inventory=[]
for root in [p,p/'logs',p/'mutation-logs',p/'post-partition',p/'post-partition/mutation-logs',p/'post-partition-final',p/'post-partition-final/mutation-logs']:
 for q in sorted(root.iterdir()):
  if q.is_file() and q.name not in ['manifest.json','manifest.sha256']:
   inventory.append({'path':str(q.relative_to(p)),'bytes':q.stat().st_size,'sha256':sha(q)})
counts=read('verified-suite-counts.json')
assert sum(x['count'] for x in counts['suites'])==664 and len(counts['suites'])==30
manifest={'schema_version':1,'epic':'ASMA-8278','worker':'/root/attestation_verifier','status':'SOURCE_READY; exclusive writes finished and released to root','evidence_root':str(p),'source_root':str(src),'source_base':head,'final_source_parent':head,'contract':read('current-contract.json'),'owned_final_sha256':owned,'candidate_files':'candidate-files/<owned path> exact copies; current source and final mutation source byte-checked','checks':{'aggregate_unique_tests':664,'core_tests':359,'store_tests':305,'suites':30,'doctests':7,'scope_counts':counts,'dependent_compilation':['kontor-daemon','kontor-api','kontor-cli','kontor-mcp'],'final_scoped_clippy':'PASS offline jobs2 core/store all-targets warnings denied','final_fmt':'PASS','final_diff':'PASS','new_rust_size_check':'PASS <=600 lines/file, <=100 lines/function','temporal_aggregation':'Not a single all-suite final-byte run; each raw suite log is listed. Later three prepared cases were absent from08, final23 covers22; final replay binds all13 pins. Repeated runs not added.'},'mutation_evidence':{'unique_mutants':10,'pre_partition_report':'mutation-report.json','final_pinned_replay_report':'post-partition-final/mutation-report.json','all_successfully_compiled_intended_assertion_kills':True,'original_and_isolated_restored':True,'final_restored_integration_tests':22,'final_restored_unit_guard_tests':1,'failed_reused_target_packet':'post-partition/mutation-report.json retained; no mutants run/counted; stale artifact is an inference, not an established Cargo bug','definitions_and_runners':'Retained in initial, failed post-partition and successful post-partition-final directories; exact argv/panic excerpts/log hashes in reports','deduplication':'10 unique defects replayed twice; not20 mutants'},'phase_commands':'phase-command-outcomes.json normalized scopes; exact mutant argv in mutation reports','process_readbacks':['phase-process-readback.json','final-process-readback.json','final-static-readback.json'],'preserved_failures':['02','03','04','06','07','09','13','22','26'],'limits':['Public untrusted prepared metadata only; no issuer custody/signing/authenticated actor/grant/receipt/native intent/admission.','No observed-now input or current-time authority; interval containment only.','Detached metadata/native identifiers prove neither transaction-held authority nor actual possession/fencing/finality.','Future runtime crypto/store owner-transaction composition remains an unresolved source integration decision.','Migration125/disposable fixture DBs only; no installed DB, keychain, native, provider or activation effects.','Nonempty ledger backup/import qualified continuity remains Unsupported; restore.rs A01 WAL guard and schema123 refusal unchanged.','PlanningPair registry/native metadata fixture and orphaned synthetic backup unit fixture are not live proof.','No acceptance or closure claim. Root remains sole publisher; no source stage/commit/push.'],'unowned_preservation':read('handoff-readback.json'),'artifact_inventory':inventory,'inventory_exclusions':['compiler target caches','full copied source fixture trees; assigned13 hashes pinned independently','manifest self hash; emitted separately'], 'prior_evidence_unchanged':{'ledger_manifest_sha256':'ce009be367771e8fbe0a323b95106ce041fb1acea4831322013a31ea36b312c6','a01_manifest_sha256':'1297d4b11bee7491fe5544f39344c219ca0681912fdf657d319d4189b5a4169f'}}
write('manifest.json',manifest)
for z in inventory: assert sha(p/z['path'])==z['sha256']
h=sha(p/'manifest.json');(p/'manifest.sha256').write_text(h+'  manifest.json\n')
print(json.dumps({'manifest':str(p/'manifest.json'),'sha256':h,'owned_paths':len(owned),'artifacts':len(inventory),'tests':664,'suites':30,'doctests':7,'unique_mutants':10,'release':'exclusive source writes finished and released to root'}))
