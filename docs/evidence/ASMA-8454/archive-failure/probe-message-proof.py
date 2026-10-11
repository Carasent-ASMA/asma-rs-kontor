"""Scratch-only observability: preserve original behavior and print failed response."""
import concurrent.futures,importlib.util,io,json,subprocess,tarfile
from datetime import datetime,timezone
from pathlib import Path
ROOT=Path.cwd();OUT=ROOT/'.qualification/reproduction'
spec=importlib.util.spec_from_file_location('qualification',ROOT/'scripts/qualify-durable-desks.py');q=importlib.util.module_from_spec(spec);spec.loader.exec_module(q)
BASE='e376f4059e47de8fb40956d4dbfbf6920dabc4ec';CURRENT=q.git('rev-parse','HEAD').strip()
REL='crates/kontor-daemon/tests/loopback/message_delivery_proofs.rs';NAME='legacy_message_proof_duplicate_split_across_pages_is_terminal_without_delivery'
COMMAND=q.LOOPBACK+['message_delivery_proofs::qualification_readonly_repeat_probe','--','--nocapture','--test-threads=1']
def prepare(label,revision):
 tree=OUT/(label+'-probe');tree.mkdir(exist_ok=True)
 archive=subprocess.check_output(['git','archive',revision],cwd=ROOT)
 with tarfile.open(fileobj=io.BytesIO(archive)) as t:t.extractall(tree,filter='data')
 p=tree/REL;original=p.read_text()
 start=original.index('async fn '+NAME);end=original.index('\n#[tokio::test]',start)
 section=original[start:end];old='    assert_eq!(step(&world, run, message, 0, "start").await.status, 200);'
 assert section.count(old)==1
 new='''    let answer = step(&world, run, message, 0, "start").await;
    if answer.status != 200 {
        let issued = world.daemon.state().message_issuance(message).unwrap().unwrap();
        let anchor = world.daemon.state().with_store(|store| store.message_delivery_proof_anchor(&issued));
        let prior: Vec<_> = world.fake.content(&held).iter().filter_map(|event| {
            if let kontor_runtime::timeline::EventSubject::Message(id) = &event.subject {
                if *id != message { return Some(world.daemon.state().message_issuance(*id)); }
            }
            None
        }).collect();
        eprintln!("READONLY_PROBE status={} body={} issued={:?} anchor={:?} prior={:?}", answer.status, answer.body, issued, anchor, prior);
    }
    assert_eq!(answer.status, 200, "{}", answer.body);'''
 changed=original[:start]+section.replace(old,new)+original[end:]
 changed+='''\n#[test]
fn qualification_readonly_repeat_probe() {
    for round in 1..=100 {
        eprintln!("READONLY_PROBE round={round}");
        legacy_message_proof_duplicate_split_across_pages_is_terminal_without_delivery();
    }
}
'''
 p.write_text(changed)
 (OUT/(label+'-probe-provenance.json')).write_text(json.dumps({'source_commit':revision,'scratch_tree':str(tree),'archive_sha256':q.digest(archive),'diagnostic_file':REL,'original_file_sha256':q.digest(original.encode()),'diagnostic_file_sha256':q.digest(changed.encode()),'only_changes':'Capture/print refusal and read-only issuance/anchor fields; repeat original test without changing status or assertions. No production code changes.','original_test':NAME,'maximum_repeats':100},indent=2)+'\n')
 return tree

def run(label,tree,target,revision):
 env=q.environment();env['CARGO_TARGET_DIR']=str(target);env['CARGO_NET_OFFLINE']='true';env['RUST_TEST_THREADS']='1'
 log=OUT/(label+'-probe.log');start=datetime.now(timezone.utc).isoformat();print('Running '+label+'-probe',flush=True)
 with log.open('wb') as stream:r=subprocess.run(COMMAND,cwd=tree,env=env,stdout=stream,stderr=subprocess.STDOUT)
 receipt={'name':label+'-probe','source_commit':revision,'cwd':str(tree),'command':COMMAND,'target_dir':str(target),'started_at':start,'exit_code':r.returncode,'log':str(log.relative_to(ROOT)),'log_sha256':q.digest(log.read_bytes()),'diagnostic_only':True,'production_source_unmodified':True}
 (OUT/(label+'-probe.json')).write_text(json.dumps(receipt,indent=2)+'\n');print('Finished '+label+'-probe: '+str(r.returncode),flush=True);return receipt
base=prepare('base',BASE);current=prepare('current',CURRENT)
with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
 runs=[pool.submit(run,'base',base,OUT/'target-base',BASE),pool.submit(run,'current',current,ROOT/'target',CURRENT)]
 results=[future.result() for future in runs]
frozen=json.loads((ROOT/'.qualification/frozen-source.json').read_text());assert all(q.digest((ROOT/p).read_bytes())==h for p,h in frozen['inputs'].items())
(OUT/'probe-results.json').write_text(json.dumps(results,indent=2)+'\n')
print('Main source bytes unchanged',flush=True)
