import pathlib,json,hashlib,subprocess,os,re,signal,datetime
R=pathlib.Path('.').resolve();E=R/'.qualification/mcp-repair/mutations';E.mkdir(exist_ok=True)
env=os.environ.copy();env.pop('KONTOR_AUTH',None);env.update(RUSTC_WRAPPER='',CARGO_NET_OFFLINE='true',RUST_TEST_THREADS='2',CARGO_TARGET_DIR=str(R/'.qualification/target-carrier-archive'),TMPDIR=str(E.parent/'tmp'),GIT_CEILING_DIRECTORIES=str(E.parent/'tmp'),SWAGGER_UI_DOWNLOAD_URL=(R/'.qualification/swagger-ui.zip').as_uri())
base=['cargo','test','--locked'];unit=base+['-p','kontor-runtime-paseo','--lib','seat_mcp::directory_tests::'];adapter=base+['-p','kontor-runtime-paseo','--test','contract','hosted_directory::']
A='crates/kontor-runtime-paseo/src/adapter.rs';M='crates/kontor-runtime-paseo/src/seat_mcp.rs';a=(R/A).read_text();m=(R/M).read_text()
items=json.loads((R/'scripts/durable-desk-mutants.json').read_text())
def add(id,file,before,after,cmd,claim):
 assert (R/file).read_text().count(before)==1,(id,'seed not unique')
 items.append(dict(id=id,file=file,before=before,after=after,command=cmd,claim=claim))
add('MCP-eligible',A,'=> crate::seat_mcp::HostedCompositionMode::OwnedDirectory,','=> crate::seat_mcp::HostedCompositionMode::Worktree,',adapter,'Recreate the escaped non-Git Claude refusal')
before='''        if recovered_id.is_none()
            || matches!(
                composition_mode,
                crate::seat_mcp::HostedCompositionMode::OwnedDirectory
            )
'''
add('MCP-recovered',A,before,'        if recovered_id.is_none()\n',adapter,'Bypass pending publication before exact-native reload')
lo=a.index('            crate::seat_mcp::compose_for_hosted_seat(');hi=a.index('            })?;',lo)+len('            })?;');before=a[lo:hi]
add('MCP-ignore-refusal',A,before,before.replace('            crate::','            let _ = crate::',1).replace('            })?;','            });'),adapter,'Ignore incomplete composition and create a native')
add('MCP-write',M,'fn compose_hosted_directory(\n','fn compose_hosted_directory(\n',unit,'placeholder')
# Use a compiling early return at the unique directory publication body.
items.pop()
before='''    let cwd = std::fs::canonicalize(cwd)?;
    let lock = directory_lock(&cwd);'''
add('MCP-write',M,before,'    if !cwd.as_os_str().is_empty() { return Ok(()); }\n'+before,unit,'Skip the directory publication')
before='''    let record = publication_record(seat, &cwd)?;
    recover_directory_publication(&record, &cwd, fault)?;'''
add('MCP-journal',M,before,'    let record = publication_record(seat, &cwd)?;',unit,'Ignore interrupted pair originals on next route/subject')
before='''publication
            .rollback(fault)
            .and_then'''
add('MCP-rollback',M,before,'publication\n            .verify_current()\n            .and_then',unit,'Delete journal/temps without restoring the prior bytes')
before='let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());'
add('MCP-cwd-lock',M,before,'let _guard = ();',unit,'Allow two subjects to publish concurrently in the same canonical cwd')
before='''                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "hosted configuration must be a JSON object",
                ));'''
add('MCP-json-guard',M,before,'                return Ok(b"{}\\n".to_vec());',unit,'Accept malformed second JSON and publish an empty object')
(E/'catalog.json').write_text(json.dumps(items,indent=2)+'\n')
originals={i['file']:(R/i['file']).read_bytes() for i in items};status=subprocess.check_output(['git','status','--porcelain'],text=True)
(E/'original-file-sha256.json').write_text(json.dumps({p:hashlib.sha256(v).hexdigest() for p,v in originals.items()},indent=2)+'\n')
def interrupt(sig,frame):raise KeyboardInterrupt
signal.signal(signal.SIGTERM,interrupt);signal.signal(signal.SIGINT,interrupt)
def run(name,cmd):
 log=E/(name+'.log');start=datetime.datetime.now(datetime.timezone.utc).isoformat()
 with log.open('wb') as out:r=subprocess.run(cmd,env=env,stdout=out,stderr=subprocess.STDOUT)
 text=log.read_text();receipt={'name':name,'command':cmd,'started_at':start,'exit':r.returncode,'log_sha256':hashlib.sha256(log.read_bytes()).hexdigest(),'test_summaries':re.findall(r'test result:.*',text),'runtime_checkout_HEAD':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()}
 (E/(name+'.json')).write_text(json.dumps(receipt,indent=2)+'\n');return receipt,text
results=[];checked=set()
try:
 for item in items:
  cmd=item.get('command',base+['-p','kontor-daemon','--test','loopback_api',item['suite'],'--','--test-threads=2']) if 'suite' in item else item['command']
  k=json.dumps(cmd)
  if k not in checked:
   receipt,text=run('baseline-'+item['id'],cmd);assert receipt['exit']==0,(item['id'],'red baseline');checked.add(k)
  path=R/item['file'];before=originals[item['file']];assert path.read_bytes()==before
  seeded=before.decode().replace(item['before'],item['after'],1).encode();line=before.decode()[:before.decode().index(item['before'])].count('\n')+1
  try:
   path.write_bytes(seeded);receipt,text=run(item['id'],cmd)
   verdict='KILLED' if receipt['exit']==101 and 'test result: FAILED' in text else 'SURVIVED' if receipt['exit']==0 else 'INVALID_OR_BLOCKED'
   result={**receipt,'file':item['file'],'line':line,'claim':item['claim'],'original_sha256':hashlib.sha256(before).hexdigest(),'seeded_sha256':hashlib.sha256(seeded).hexdigest(),'result':verdict,'failed_tests':re.findall(r'^test (.+) \.\.\. FAILED$',text,re.M),'failure_excerpts':re.findall(r"thread[^\n]*panicked[^\n]*\n(?:[^\n]*\n){0,6}",text)}
   results.append(result);(E/'results.json').write_text(json.dumps(results,indent=2)+'\n');print(item['id'],verdict,result['failed_tests'],flush=True)
  finally:path.write_bytes(before)
  assert subprocess.check_output(['git','status','--porcelain'],text=True)==status
finally:
 for p,v in originals.items():assert (R/p).read_bytes()==v,p
 (E/'restoration.json').write_text(json.dumps({'all_files_byte_exact':True,'status_exact':subprocess.check_output(['git','status','--porcelain'],text=True)==status,'count':len(results),'distinct_green_targets':len(checked)},indent=2)+'\n')
raise SystemExit(any(r['result']!='KILLED' for r in results))
