from pathlib import Path
import json,re,hashlib
p=Path(__file__).resolve().parent;a=json.loads((p/'assignment.json').read_text());root=Path(a['source_root']);records=[]
for name in a['owned'][1:]:
 s=(root/name).read_text();mask=list(s)
 pattern=r'//[^\n]*|/\*[\s\S]*?\*/|"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])\''
 for m in re.finditer(pattern,s):
  for i in range(m.start(),m.end()):
   if mask[i]!='\n':mask[i]=' '
 clean=''.join(mask);funcs=[]
 for m in re.finditer(r'(?m)^\s*(?:pub )?fn (\w+)\(',clean):
  start=clean.index('{',m.end());depth=0
  for end in range(start,len(clean)):
   depth+=(clean[end]=='{')-(clean[end]=='}')
   if depth==0:break
  # Start at the fn keyword, rather than preceding blank lines.
  line=clean[:m.start()].count('\n')+1;bodylines=clean[start:end+1].count('\n')+1
  funcs.append({'name':m[1],'start_line':line,'body_lines':bodylines,'total_lines':clean[m.start():end+1].count('\n')+1})
 records.append({'path':name,'lines':len(s.splitlines()),'sha256':hashlib.sha256(s.encode()).hexdigest(),'functions':funcs,'pass':len(s.splitlines())<=600 and all(z['total_lines']<=100 for z in funcs)})
result={'files':records,'pass':all(x['pass'] for x in records),'limits':'hard600 file/100 function lines; Rust strings/comments masked before brace counting'};print(json.dumps(result,indent=2));assert result['pass']
