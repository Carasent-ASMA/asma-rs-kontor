import subprocess,os,json,datetime,pathlib
root=pathlib.Path(__file__).parent
cwd=pathlib.Path('/Users/igor/carasent/asma-modules/.worktrees/asma-8015/asma-rs-kontor')
env=dict(os.environ,RUSTC_WRAPPER='',CARGO_TARGET_DIR=str(root/'target'),CARGO_BUILD_JOBS='2',CARGO_TERM_COLOR='never',SWAGGER_UI_DOWNLOAD_URL=(root/'swagger-ui-v5.17.14.zip').as_uri())
commands=[('account-unit',['cargo','test','--offline','--locked','-p','kontor-accounts','--lib']),('jira-contracts',['cargo','test','--offline','--locked','-p','kontor-jira','--lib','--tests']),('daemon-operator',['cargo','test','--offline','--locked','-p','kontor-daemon','--bin','kontor-daemon']),('realm-preflight',['cargo','test','--offline','--locked','-p','kontor-store','--test','realm_preflight']),('strict-clippy',['cargo','clippy','--offline','--locked','-p','kontor-accounts','-p','kontor-jira','-p','kontor-daemon','--all-targets','--','-D','warnings'])]
results=[]
for name,cmd in commands:
 start=datetime.datetime.now(datetime.timezone.utc).isoformat();print('Starting '+name,flush=True)
 with (root/(name+'.txt')).open('wb') as out:
  code=subprocess.run(cmd,cwd=cwd,env=env,stdout=out,stderr=subprocess.STDOUT).returncode
  out.write(('\nPROCESS_EXIT_CODE='+str(code)+'\n').encode())
 results.append(dict(name=name,command=cmd,startedAt=start,finishedAt=datetime.datetime.now(datetime.timezone.utc).isoformat(),exitCode=code))
 (root/'focused-results.json').write_text(json.dumps(results,indent=2)+'\n');print(name+' exit '+str(code),flush=True)
 if code:break
