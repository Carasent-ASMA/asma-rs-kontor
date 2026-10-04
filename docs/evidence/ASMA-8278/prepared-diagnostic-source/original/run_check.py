from pathlib import Path
import json,os,subprocess,datetime,time,sys,hashlib
p=Path(__file__).resolve().parent
name=sys.argv[1]; cmd=sys.argv[2:]
env=os.environ.copy();env['CARGO_TARGET_DIR']=str(p/'cargo-target')
t0=time.monotonic();started=datetime.datetime.now(datetime.timezone.utc).isoformat()
with (p/'logs'/name).open('wb') as log:
 code=subprocess.run(cmd,cwd='/Users/igor/.paseo/worktrees/0n8yzbno/asma-8278-module-restart-20261004',env=env,stdout=log,stderr=subprocess.STDOUT).returncode
result={'command':cmd,'cwd':'/Users/igor/.paseo/worktrees/0n8yzbno/asma-8278-module-restart-20261004','cargo_target':env['CARGO_TARGET_DIR'],'started_utc':started,'elapsed_seconds':round(time.monotonic()-t0,3),'exit_code':code,'log':'logs/'+name,'sha256':hashlib.sha256((p/'logs'/name).read_bytes()).hexdigest()}
(p/(name+'.json')).write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result));sys.exit(code)
