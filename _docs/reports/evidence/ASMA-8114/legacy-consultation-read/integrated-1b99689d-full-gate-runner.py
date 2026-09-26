import datetime, hashlib, io, json, os, pathlib, subprocess, tarfile
repo=pathlib.Path('/Users/igor/carasent/asma-modules/.worktrees/asma-8113/asma-rs-kontor')
commit='1b99689d138aa1a4c1e406700f0a6be2316f5554';tree='738d887a11ab19c2e6039ca5dc74d20d5c255267'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo).decode().strip()==commit
assert subprocess.check_output(['git','status','--porcelain'],cwd=repo)==b''
folder=pathlib.Path('/private/tmp/ASMA-8114-integrated-full-gate-1b99689d');assert not folder.exists();folder.mkdir()
target=folder/'target';subprocess.run(['cp','-cR','/private/tmp/ASMA-8114-legacy-read-full-gate-51cfef9c-network/target',str(target)],check=True)
archive=subprocess.check_output(['git','archive',commit],cwd=repo);(folder/'source.tar').write_bytes(archive)
source=folder/'api-source';source.mkdir()
with tarfile.open(fileobj=io.BytesIO(archive)) as tar:tar.extractall(source,filter='data')
env=os.environ.copy();env.update({'RUSTC_WRAPPER':'','CARGO_TARGET_DIR':str(target),'CARGO_BUILD_JOBS':'2','PATH':'/opt/homebrew/bin:'+env.get('PATH','')})
env.pop('KONTOR_AUTH',None);env.pop('JIRA_API_TOKEN',None);env.pop('KONTOR_UPDATE_CONTRACT',None)
command=['python3','-u','scripts/verify-tree.py','--mode','archive'];start=datetime.datetime.now(datetime.timezone.utc).isoformat();log=folder/'full-gate.txt'
with log.open('wb') as out:
 out.write(f'QUALIFIED_SOURCE={commit}\nQUALIFIED_TREE={tree}\nSTARTED_AT={start}\n'.encode());out.flush()
 run=subprocess.run(command,cwd=repo,env=env,stdout=out,stderr=subprocess.STDOUT);finish=datetime.datetime.now(datetime.timezone.utc).isoformat();out.write(f'PROCESS_EXIT_CODE={run.returncode}\nFINISHED_AT={finish}\n'.encode())
receipt={'sourceCommit':commit,'sourceTree':tree,'archiveSha256':hashlib.sha256(archive).hexdigest(),'command':command,'startedAt':start,'finishedAt':finish,'exitCode':run.returncode,'logSha256':hashlib.sha256(log.read_bytes()).hexdigest(),'privateTarget':str(target),'scope':'Integrated exact-candidate archive qualification only. Earlier exact-51 evidence is not relabeled. No merge/deployment or task/epic acceptance.'}
(folder/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt),flush=True)
if run.returncode==0:
 cli='/Users/igor/carasent/asma-modules/_tools/asma-rs-kontor/node_modules/.pnpm/openapi-typescript@7.13.0_typescript@5.9.3/node_modules/openapi-typescript/bin/cli.js'
 commands=[['cargo','test','--locked','-p','kontor-api','--test','openapi_contract'],['node',cli,str(source/'crates/kontor-api/contract/openapi.json'),'-o',str(folder/'schema-verify.d.ts')],['diff','-u',str(source/'apps/console/src/api/schema.d.ts'),str(folder/'schema-verify.d.ts')]]
 api=folder/'frozen-and-api.txt';records=[];api_start=datetime.datetime.now(datetime.timezone.utc).isoformat()
 with api.open('wb') as out:
  for cmd in commands:
   out.write((json.dumps(cmd)+'\n').encode());out.flush();r=subprocess.run(cmd,cwd=source,env=env,stdout=out,stderr=subprocess.STDOUT);records.append({'command':cmd,'exitCode':r.returncode});out.write(f'COMMAND_EXIT_CODE={r.returncode}\n'.encode());out.flush()
   if r.returncode:break
  out.write(f'PROCESS_EXIT_CODE={r.returncode}\n'.encode())
 api_receipt={'sourceCommit':commit,'sourceTree':tree,'archiveSha256':receipt['archiveSha256'],'startedAt':api_start,'finishedAt':datetime.datetime.now(datetime.timezone.utc).isoformat(),'commands':records,'exitCode':r.returncode,'logSha256':hashlib.sha256(api.read_bytes()).hexdigest(),'scope':'Exact integrated-candidate frozen/API verification only; generation tool is read from the installed matching 7.13.0 dependency without modifying primary files.'}
 (folder/'frozen-and-api-receipt.json').write_text(json.dumps(api_receipt,indent=2)+'\n');print(json.dumps(api_receipt),flush=True)
