from pathlib import Path
import subprocess,json,hashlib,shutil
p=Path(__file__).resolve().parent;a=json.loads((p/'assignment.json').read_text());src=Path(a['source_root']);copy=p/'mutation-source';copy.mkdir()
archive=subprocess.Popen(['git','archive',a['base']],cwd=src,stdout=subprocess.PIPE)
subprocess.run(['tar','-x','-C',str(copy)],stdin=archive.stdout,check=True);archive.stdout.close();assert archive.wait()==0
hashes={}
for q in a['owned']:
 t=copy/q;t.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(src/q,t);hashes[q]=hashlib.sha256(t.read_bytes()).hexdigest()
path='crates/kontor-runtime/src/planning_pair/attestation/prepared_diagnostic.rs'
defs=[
{'id':'M01-key-material','path':path,'test':'key_material_commitments_are_recomputed_from_selected_der','old':'let material_digest = ContentHash::of(&key.public_key_der);\n    if key.material_digest != material_digest\n        || token.key_material_digest != material_digest\n        || key.issuer != token.issuer','new':'let _material_digest = ContentHash::of(&key.public_key_der);\n    if key.issuer != token.issuer','defect':'omit both recomputed DER commitment comparisons'},
{'id':'M02-original-payload','path':path,'test':'exact_original_payload_digest_precedes_revocation_and_verification','old':'    if token.payload_digest != ContentHash::of(payload) {\n        return Err(Refusal::TokenCommitment);\n    }\n','new':'','defect':'omit exact original payload digest guard'},
{'id':'M03-token-revocation','path':path,'test':'token_revocation_precedes_interval_and_signature_checks','old':'    if token.revoked_revision.is_some() {\n        return Err(Refusal::TokenRevoked);\n    }\n','new':'','defect':'omit selected token revocation guard'},
{'id':'M04-signed-metadata','path':path,'test':'signed_metadata_matches_every_exact_prepared_field_after_verification','old':'    if !matches_metadata(verified.claims(), token) {\n        return Err(Refusal::Metadata);\n    }\n','new':'    let _consistency = matches_metadata(verified.claims(), token);\n','defect':'omit final exact signed/prepared metadata guard'},
]
for d in defs:assert (copy/d['path']).read_text().count(d['old'])==1,d['id']
(p/'mutation-definitions.json').write_text(json.dumps(defs,indent=2)+'\n');(p/'mutation-baseline-hashes.json').write_text(json.dumps(hashes,indent=2)+'\n');(p/'mutation-logs').mkdir()
print(json.dumps({'source':str(copy),'owned':len(hashes),'mutants':len(defs),'source_copies_equal':all(hashlib.sha256((src/q).read_bytes()).hexdigest()==h for q,h in hashes.items())}))
