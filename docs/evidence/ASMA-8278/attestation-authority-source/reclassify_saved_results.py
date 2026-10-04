#!/usr/bin/env python3
"""Correct a report classifier using retained logs and unchanged test assertions.
No test execution, source mutation, compilation failure or exit-code-only kill.
The executed v1 runner/report are retained verbatim; v1 completed after checking
all 12 original and copy hashes after every restore. Its custom assert! messages
were unrecognized by its literal-output classifier only.
"""
import hashlib,json,pathlib,re
ROOT=pathlib.Path(__file__).resolve().parent
SOURCE=pathlib.Path('/tmp/asma-8278-module-consolidation-20261002.Mcrugq')
COPY=ROOT/'isolated-source'
report=json.loads((ROOT/'mutation-results-first-classifier.json').read_text())
definitions={d['id']:d for d in json.loads((ROOT/'mutation-definitions.json').read_text())}
owned=json.loads((ROOT/'owned-paths.json').read_text())
def hashes(base):return {p:hashlib.sha256((base/p).read_bytes()).hexdigest() for p in owned}
assert hashes(SOURCE)==report['source_sha256']==hashes(COPY)==report['final_source_sha256']==report['restored_copy_sha256']
runner=(ROOT/'run_mutations.executed-v1.py').read_text().splitlines()
restoration_lines=[i+1 for i,line in enumerate(runner) if 'assert hashes(COPY)==original' in line or 'assert hashes(SOURCE)==original' in line]
proof=[]
for result in report['results']:
    definition=definitions[result['id']]
    log=pathlib.Path(result['log']).read_text()
    panic=re.search(r'panicked at (crates/[^:]+):(\d+):(\d+):',log)
    assert panic,(result['id'],'no panic path/line')
    file=panic[1];line=int(panic[2]);lines=(COPY/file).read_text().splitlines();text=lines[line-1].strip()
    is_assert=text.startswith(('assert!(', 'assert_eq!(', 'assert_ne!('))
    compiled='Finished `test` profile' in log and 'error[E' not in log
    intended=definition['test']+' ... FAILED' in log
    killed=result['exit']==101 and compiled and intended and is_assert and 'test result: FAILED.' in log
    result['initial_verdict']=result['verdict']
    result['verdict']='KILLED' if killed else 'UNRESOLVED'
    result['assertion_failure']=is_assert
    result['assertion_evidence']={'path':file,'line':line,'text':text,'nearby_source':lines[max(0,line-3):line+4],'compiled_successfully':compiled,'intended_existing_test':intended,'log_sha256':hashlib.sha256(pathlib.Path(result['log']).read_bytes()).hexdigest()}
    result['restoration_verified']=True
    result['restoration_proof']={'executed_runner':'run_mutations.executed-v1.py','hash_assertion_lines':restoration_lines,'hashes_reference':'source_sha256','verification':'v1 checked source/copy hashes after restoring each mutant and completed normally; final hash equality independently rechecked here'}
    baseline=(COPY/definition['path']).read_text()
    assert baseline.count(definition['old'])==1
    result['mutant_sha256_derived_from_exact_definition']=hashlib.sha256(baseline.replace(definition['old'],definition['new']).encode()).hexdigest()
    proof.append({'id':result['id'],'verdict':result['verdict'],'assertion':result['assertion_evidence']})
report['all_assertion_killed']=all(r['verdict']=='KILLED' for r in report['results'])
report['classification_correction']={'source_effects':False,'initial_report':'mutation-results-first-classifier.json','initial_runner':'run_mutations.executed-v1.py','corrected_runner':'run_mutations.py','reason':'v1 literal output matching missed the custom-message assert! panics in M03/M05; exact source macro lines and successful compile/test logs establish the assertion kills'}
(ROOT/'mutation-results.json').write_text(json.dumps(report,indent=2)+'\n')
(ROOT/'mutation-assertion-proof.json').write_text(json.dumps(proof,indent=2)+'\n')
assert report['all_assertion_killed'],'Some mutant remains unresolved'
print(json.dumps({'mutants':len(report['results']),'assertion_killed':sum(r['verdict']=='KILLED' for r in report['results']),'restored_source_equals_copy':hashes(SOURCE)==hashes(COPY)}))
