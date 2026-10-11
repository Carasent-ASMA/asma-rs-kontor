from pathlib import Path
import os,json,datetime,shutil,stat,hashlib
R=Path('.').resolve();Q=R/'.qualification/mcp-repair';target=Q/'tmp'
assert target.parent==Q and not target.is_symlink()
def inventory(root):
 rows=[]
 if root.exists():
  def visit(directory):
   for e in os.scandir(directory):
    s=e.stat(follow_symlinks=False);rows.append({'path':str(Path(e.path).relative_to(root)),'bytes':s.st_size,'kind':'directory' if stat.S_ISDIR(s.st_mode) else 'symlink' if stat.S_ISLNK(s.st_mode) else 'file'})
    if stat.S_ISDIR(s.st_mode):visit(e.path)
  visit(root)
 return rows
rows=inventory(target)
receipt={'time':datetime.datetime.now(datetime.timezone.utc).isoformat(),'owner':'Codex / ASMA-8454 qualification owner','target':str(target),'command':'python3 .qualification/mcp-repair/cleanup-tmp.py','top_level_entries_before':len(list(target.iterdir())) if target.exists() else 0,'recursive_entries_before':len(rows),'regular_file_bytes_before':sum(x['bytes'] for x in rows if x['kind']=='file'),'inventory_sha256':hashlib.sha256(json.dumps(rows,sort_keys=True).encode()).hexdigest(),'scope':'only repair test TMPDIR; no held discovery realm/package, real native or unrelated Cargo target'}
if target.exists():shutil.rmtree(target)
receipt.update({'absent_after':not target.exists(),'entries_after':0})
(Q/'cleanup.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt,indent=2))
