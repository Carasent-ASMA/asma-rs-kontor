from pathlib import Path
import hashlib,json,re,shutil,subprocess
root=Path('/tmp/asma-8278-module-consolidation-20261002.Mcrugq')
evidence=Path(__file__).parent
paths=['crates/kontor-core/src/repository.rs','crates/kontor-core/src/repository/attestation_authority.rs','crates/kontor-store/src/repository/attestation_authority.rs','crates/kontor-store/src/repository/attestation_authority/token_metadata.rs','crates/kontor-store/src/migrations.rs','crates/kontor-store/migrations/0125_prepared_attestation_tokens.sql','crates/kontor-store/tests/attestation_token_metadata.rs','crates/kontor-store/tests/schema_v1.rs','crates/kontor-store/tests/repository_roundtrip.rs','crates/kontor-store/src/backup/export.rs','crates/kontor-store/tests/backup_snapshot.rs']
source=evidence/'mutation-source'
source.mkdir()
tracked=subprocess.check_output(['git','ls-files','-z'],cwd=root).decode().split('\0')
for name in dict.fromkeys(tracked+paths):
 if not name:continue
 src=root/name;dst=source/name
 if not src.is_file():continue
 dst.parent.mkdir(parents=True,exist_ok=True)
 shutil.copy2(src,dst)
hash_paths=lambda base:{name:hashlib.sha256((base/name).read_bytes()).hexdigest() for name in paths}
(evidence/'mutation-baseline-hashes.json').write_text(json.dumps(hash_paths(root),indent=2)+'\n')
p='crates/kontor-store/src/repository/attestation_authority/token_metadata.rs';s=(source/p).read_text()
sql='crates/kontor-store/migrations/0125_prepared_attestation_tokens.sql';migration=(source/sql).read_text()
backup='crates/kontor-store/src/backup/export.rs';bs=(source/backup).read_text()
def mutation(name,path,old,new,test,target='attestation_token_metadata'):
 assert (source/path).read_text().count(old)==1,(name,old)
 return {'id':name,'path':path,'old':old,'new':new,'test':test,'target':target}
def trigger(name):
 match=re.search(r'CREATE TRIGGER '+name+r'\b.*?END;\n',migration,re.S);assert match
 return match.group()
mutants=[
 mutation('M01-key-head',p,'    require_head(\n        head_in(&transaction, &request.scope)?,\n        Some(request.expected_key_head_revision),\n    )?;','    // Seeded isolated mutation: skip current key-head expectation.','fresh_checks_precede_identity_reuse_and_leave_existing_commitment_unchanged'),
 mutation('M02-token-head',p,'    require_head(head, request.expected_token_head_revision)?;','    // Seeded isolated mutation: skip token-head expectation.','both_heads_use_exact_cas_and_failed_preparation_rolls_back'),
 mutation('M03-permanent-identity',sql,trigger('attestation_tokens_registration_next'),'-- Seeded isolated mutation: no permanent replacement/registration guard.\n','raw_sql_cannot_replace_rewrite_unrevoke_or_delete_metadata_or_heads'),
 mutation('M04-hosted-generation',p,'generation.map(unsigned).transpose()? != Some(request.expected_occupancy_generation)','generation.map(unsigned).transpose()?.is_none()','hosted_generation_is_history_count_plus_one_not_runtime_generation'),
 mutation('M05-active-state',p,'        || binding_state != "active"\n        || node_state != "active"','        || (binding_state.is_empty() && node_state.is_empty())','inactive_binding_and_node_each_refuse_unchanged'),
 mutation('M06-exact-task',p,'member.binding_task != request.task_id || member.node_task != request.task_id','member.binding_task.is_some() && member.node_task.is_some() && request.task_id.is_none()','scope_and_absent_task_are_exact_and_cross_scope_reads_do_not_leak'),
 mutation('M07-native-subject',p,'    if count != 1 {','    if count < 1 {','a_container_native_identity_cannot_masquerade_as_seat_provenance'),
 mutation('M08-backup-token-schema',backup,'        (\n            125,\n            "attestation_token_heads",\n            "prepared_attestation_tokens",\n        ),','        // Seeded isolated mutation: omit the token-ledger continuity guard.','backup::export::attestation_ledger_tests::token_tables_are_required_at125_and_legacy124_absence_never_skips_keys','lib'),
 mutation('M09-unrevocation',sql,'  OR OLD.revoked_revision IS NOT NULL OR NEW.revoked_revision IS NULL\n  OR NEW.revoked_revision IS NOT (SELECT revision + 1 FROM attestation_token_heads\n      WHERE project_id = OLD.project_id AND application = OLD.application)','  OR 0','raw_sql_cannot_replace_rewrite_unrevoke_or_delete_metadata_or_heads'),
 mutation('M10-key-commitment',sql,trigger('attestation_tokens_key_commitment'),'-- Seeded isolated mutation: no coherent key-commitment guard.\n','raw_sql_insert_refuses_false_key_commitments_and_invalid_bounds_atomically'),
]
(evidence/'mutation-definitions.json').write_text(json.dumps(mutants,indent=2)+'\n')
print(json.dumps({'source':str(source),'mutants':len(mutants),'original_matches_copy':hash_paths(root)==hash_paths(source)}))
