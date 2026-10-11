import pathlib,json,hashlib,datetime
R=pathlib.Path('.').resolve();E=R/'.qualification/mcp-rework'
roots=['.qualification/release','.qualification/release-corrected','.qualification/release-complete','.qualification/release-legacy-probe']
files={}
for root in roots:
 for p in sorted((R/root).rglob('*')):
  if p.is_file() and not p.is_symlink():files[str(p.relative_to(R))]={'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()}
protected=json.loads((R/'docs/evidence/ASMA-8454/mcp-repair/protected-original-documents.json').read_text())
assert all(hashlib.sha256((R/p).read_bytes()).hexdigest()==sha for p,sha in protected.items())
receipt={'recorded_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'method':'read-only file bytes; no SQLite open, fixture operation or process restart','held_files':files,'protected_original_documents':protected}
p=E/'preserved-scopes-before.json'
if p.exists():
 old=json.loads(p.read_text());assert old['held_files']==files,'held scope bytes changed';receipt['byte_exact_against_before']=True;(E/'preserved-scopes-after.json').write_text(json.dumps(receipt,indent=2)+'\n')
else:p.write_text(json.dumps(receipt,indent=2)+'\n')
print('preserved held files',len(files),'protected documents',len(protected))
