from pathlib import Path
import json,re
root=Path('/tmp/asma-8278-module-consolidation-20261002.Mcrugq')
files=['crates/kontor-store/src/repository/attestation_authority/token_metadata.rs','crates/kontor-store/tests/attestation_token_metadata.rs','crates/kontor-store/tests/attestation_token_metadata/fixture.rs','crates/kontor-store/tests/attestation_token_metadata/guards.rs']
def masked(text):
 # Enough Rust lexical masking for these owned files: strings, raw strings and
 # comments, preserving every newline. Lifetimes are left intact.
 pattern=r'(?:br|r)(?P<hashes>#+)".*?"(?P=hashes)|"(?:\\.|[^"\\])*"|//[^\n]*|/\*.*?\*/'
 return re.sub(pattern,lambda m:''.join('\n' if c=='\n' else ' ' for c in m.group()),text,flags=re.S)
result=[]
for name in files:
 text=(root/name).read_text();scan=masked(text);functions=[]
 for m in re.finditer(r'(?m)^[ \t]*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)\b',scan):
  start=scan.find('{',m.end());semicolon=scan.find(';',m.end())
  if start<0 or 0<=semicolon<start:continue
  depth=1;end=start+1
  while depth and end<len(scan):
   depth+=(scan[end]=='{')-(scan[end]=='}');end+=1
  line=scan.count('\n',0,m.start())+1;last=scan.count('\n',0,end)+1
  functions.append({'name':m.group(1),'line':line,'lines':last-line+1})
 result.append({'path':name,'file_lines':len(text.splitlines()),'functions':functions,'pass':len(text.splitlines())<=600 and all(f['lines']<=100 for f in functions)})
print(json.dumps({'scope':'new Rust files/functions only; existing large files changed only inside assigned seams','files':result,'pass':all(f['pass'] for f in result)},indent=2))
raise SystemExit(0 if all(f['pass'] for f in result) else 1)
