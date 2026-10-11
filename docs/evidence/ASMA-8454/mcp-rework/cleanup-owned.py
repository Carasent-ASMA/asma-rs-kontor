from pathlib import Path
import json,hashlib,datetime,shutil
R=Path('.').resolve();E=R/'.qualification/mcp-rework';tmp=E/'tmp'
assert tmp.parent==E and not tmp.is_symlink() and tmp.is_dir()
items=list(tmp.rglob('*'));files=[p for p in items if p.is_file() and not p.is_symlink()]
receipt={'owner':'Codex / SWE ASMA-8454','scope':str(tmp),'top_level_entries_before':len(list(tmp.iterdir())),'recursive_entries_before':len(items),'regular_file_bytes_before':sum(p.stat().st_size for p in files),'removed_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'excluded':'held discovery realms/packages and Cargo caches'}
shutil.rmtree(tmp);receipt['confirmed_absent_after']=not tmp.exists();assert receipt['confirmed_absent_after']
(E/'cleanup.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt))
