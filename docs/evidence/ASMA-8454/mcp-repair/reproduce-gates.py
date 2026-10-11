import json,os,subprocess,hashlib,datetime,pathlib,re
R=pathlib.Path('.').resolve();E=R/'.qualification/mcp-repair'
env=os.environ.copy();env.pop('KONTOR_AUTH',None);env.update(RUSTC_WRAPPER='',CARGO_NET_OFFLINE='true',RUST_TEST_THREADS='2',CARGO_TARGET_DIR=str(R/'.qualification/target-carrier-archive'),TMPDIR=str(E/'tmp'),GIT_CEILING_DIRECTORIES=str(E/'tmp'),SWAGGER_UI_DOWNLOAD_URL=(R/'.qualification/swagger-ui.zip').as_uri())
commands=[('paseo', ['cargo','test','--locked','-p','kontor-runtime-paseo','--lib','--test','contract']),('fmt',['cargo','fmt','--all','--check']),('clippy',['cargo','clippy','--locked','-p','kontor-runtime-paseo','--all-targets','--','-D','warnings'])]
for suite in ['durable_desks::','durable_seats::','desk_adams::','pr_reviews::']:
 commands.append((suite[:-2],['cargo','test','--locked','-p','kontor-daemon','--test','loopback_api',suite,'--','--test-threads=2']))
commands.append(('cli_parity',['cargo','test','--offline','--locked','-p','kontor-cli','--test','durable_desk_parity','--','--nocapture']))
receipts=[]
paths=['Cargo.lock','crates/kontor-runtime-paseo/Cargo.toml','crates/kontor-runtime-paseo/src/adapter.rs','crates/kontor-runtime-paseo/src/seat_mcp.rs','crates/kontor-runtime-paseo/src/seat_mcp_directory_tests.rs','crates/kontor-runtime-paseo/tests/contract.rs','crates/kontor-runtime-paseo/tests/contract/hosted_directory.rs']
for name,cmd in commands:
 start=datetime.datetime.now(datetime.timezone.utc).isoformat();log=E/(name+'-gate.log');print('START',name,flush=True)
 inputs={p:hashlib.sha256((R/p).read_bytes()).hexdigest() for p in paths}
 with log.open('wb') as out: result=subprocess.run(cmd,env=env,stdout=out,stderr=subprocess.STDOUT)
 receipt={'name':name,'command':cmd,'cwd':str(R),'started_at':start,'finished_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'exit':result.returncode,'log_sha256':hashlib.sha256(log.read_bytes()).hexdigest(),'runtime_checkout_HEAD':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'tested_input_sha256':inputs,'environment':{k:env[k] for k in ['RUSTC_WRAPPER','CARGO_NET_OFFLINE','RUST_TEST_THREADS','CARGO_TARGET_DIR','TMPDIR','GIT_CEILING_DIRECTORIES','SWAGGER_UI_DOWNLOAD_URL']},'test_summaries':re.findall(r'test result:.*',log.read_text())}
 (E/(name+'-gate.json')).write_text(json.dumps(receipt,indent=2)+'\n');receipts.append(receipt);print('DONE',name,result.returncode,receipt['test_summaries'],flush=True)
 if result.returncode: break
(E/'gates.json').write_text(json.dumps(receipts,indent=2)+'\n')
raise SystemExit(any(r['exit'] for r in receipts))
