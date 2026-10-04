import pathlib,subprocess,os,datetime,json,hashlib,sys
owner=pathlib.Path('/Users/igor/carasent/asma-modules/.worktrees/asma-8015/asma-rs-kontor')
pin='e53f19b744b80862b166b613dcb8992bcf371215';root=pathlib.Path('/private/tmp/ASMA-8015-full-gate-e53f19b7');root.mkdir(exist_ok=False)
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=owner,text=True).strip()==pin
assert not subprocess.check_output(['git','status','--porcelain'],cwd=owner)
tree=subprocess.check_output(['git','rev-parse',pin+'^{tree}'],cwd=owner,text=True).strip();archive=subprocess.check_output(['git','archive',pin],cwd=owner);(root/'source.tar').write_bytes(archive)
print('Cloning completed isolated cache for exact-source gate',flush=True)
subprocess.run(['cp','-cR','/private/tmp/ASMA-8015-security-correction/target',str(root/'target')],check=True)
env=dict(os.environ);env.pop('KONTOR_AUTH',None);env.update(RUSTC_WRAPPER='',CARGO_TARGET_DIR=str(root/'target'),CARGO_BUILD_JOBS='2',CARGO_TERM_COLOR='never',SWAGGER_UI_DOWNLOAD_URL='file:///private/tmp/ASMA-8015-security-correction/swagger-ui-v5.17.14.zip',PYTHONUNBUFFERED='1')
started=datetime.datetime.now(datetime.timezone.utc).isoformat();log=root/'full-gate.txt'
with log.open('wb') as out:
 out.write(('QUALIFIED_SOURCE='+pin+'\nQUALIFIED_TREE='+tree+'\nSTARTED_AT='+started+'\n').encode());out.flush()
 code=subprocess.run([sys.executable,'scripts/verify-tree.py','--mode','archive'],cwd=owner,env=env,stdout=out,stderr=subprocess.STDOUT).returncode
 finished=datetime.datetime.now(datetime.timezone.utc).isoformat();out.write(('\nPROCESS_EXIT_CODE='+str(code)+'\nFINISHED_AT='+finished+'\n').encode())
record=dict(sourceCommit=pin,sourceTree=tree,sourceArchiveSha256=hashlib.sha256(archive).hexdigest(),startedAt=started,finishedAt=finished,exitCode=code,log=str(log),logSha256=hashlib.sha256(log.read_bytes()).hexdigest(),scope='Exact integrated source archive qualification only. No live Keychain, installation, deployment, Jira, workflow or closure credit.',cacheBoundary='Distinct private clone of completed synthetic working-tree cache; no original cache mutation; every gate runs anew')
(root/'receipt.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps(record),flush=True);sys.exit(code)
