from pathlib import Path
import json
root=Path('.');catalog=json.loads(Path('docs/evidence/ASMA-8454/mcp-repair/mutations/catalog.json').read_text())
A='crates/kontor-runtime-paseo/src/adapter.rs';M='crates/kontor-runtime-paseo/src/seat_mcp.rs';a=Path(A).read_text();m=Path(M).read_text()
for item in catalog:
 if item['id']=='MCP-recovered':
  item['before']='''            || (pre_thread_reload
                && matches!(
                    composition_mode,
                    crate::seat_mcp::HostedCompositionMode::OwnedDirectory
                ))''';item['after']=''
 elif item['id']=='MCP-ignore-refusal':
  lo=a.index('        composition.map_err(');hi=a.index('        let (native_id, created)',lo);item['before']=a[lo:hi];item['after']=item['before'].replace('        composition.map_err(', '        let _ = composition.map_err(',1).replace('        })?;', '        });')
 elif item['id']=='MCP-write':
  lo=m.index('fn compose_hosted_directory(');hi=m.index('    let lock = directory_lock(&cwd);',lo);item['before']=m[lo:hi];item['after']=item['before'].replace('    let cwd =','    if !cwd.as_os_str().is_empty() { return Ok(()); }\n    let cwd =',1)
 elif item['id']=='MCP-journal':
  item['before']='''    recover_directory_publication(&mut fs, &record, &cwd, fault)?;
    if crate::client::built_in_provider(provider) != "claude"''';item['after']='''    if crate::client::built_in_provider(provider) != "claude"'''
 elif item['id']=='MCP-rollback':
  item['before']='''publication
            .rollback(&mut fs, fault)
            .and_then''';item['after']='''publication
            .verify_current(&mut fs)
            .and_then'''
 elif item['id']=='MCP-cwd-lock':
  item['before']='''    // A caught interruption may poison this process's lock; the persisted
    // record, rather than in-memory state, decides whether publication is safe.
    let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());''';item['after']='    let _guard = ();'
unit=['cargo','test','--locked','-p','kontor-runtime-paseo','--lib','seat_mcp::directory_tests::ancestry_substitution_between_parent_validation_and_replace_is_refused']
adapter=['cargo','test','--locked','-p','kontor-runtime-paseo','--test','contract','hosted_directory::']
def add(id,file,before,after,command,claim):catalog.append(dict(id=id,file=file,before=before,after=after,command=command,claim=claim))
lo=m.index('        let parent = self.directory(destination.parent().unwrap())?;');hi=m.index('        parent.sync()',lo)
before=m[lo:hi]
after='''        let parent = self.directory(destination.parent().unwrap())?;
        let source = std::fs::File::from(rustix::fs::open(temporary.parent().unwrap(), rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::NOFOLLOW, rustix::fs::Mode::empty())?);
        fault(PublicationStep::ReplaceValidated)?;
        let target = std::fs::File::from(rustix::fs::open(destination.parent().unwrap(), rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::NOFOLLOW, rustix::fs::Mode::empty())?);
        rustix::fs::renameat(&source, temporary.file_name().unwrap(), &target, destination.file_name().unwrap())?;
'''
add('MCP-B1-path-reopen',M,before,after,unit,'Recreate AUD B1: reopen separate parents by pathname across deterministic ancestry substitution; foreign destination changes')
lo=a.index('            crate::seat_mcp::validate_recovered_hosted_directory(');hi=a.index('        } else {',lo);before=a[lo:hi]
after=before.replace('validate_recovered_hosted_directory','compose_for_hosted_seat').replace('                std::path::Path::new(request.cwd.as_str()),\n','                std::path::Path::new(request.cwd.as_str()),\n                composition_mode,\n')
add('MCP-healthy-publish',A,before,after,adapter,'Republish missing/different files on healthy exact-native replay')
before='''pub(crate) fn validate_recovered_hosted_directory(
    seat: Option<&SeatMcp>,
    provider: &str,
    cwd: &Path,
) -> io::Result<()> {'''
add('MCP-healthy-ignore-validation',M,before,before+'\n    if !cwd.as_os_str().is_empty() { return Ok(()); }',adapter,'Skip pending recovery and read-only validation for healthy exact natives')
for item in catalog:assert Path(item['file']).read_text().count(item['before'])==1,(item['id'],'not unique')
E=Path('.qualification/mcp-rework/mutations');E.mkdir(exist_ok=True);(E/'catalog.json').write_text(json.dumps(catalog,indent=2)+'\n')
template=Path('docs/evidence/ASMA-8454/mcp-repair/reproduce-mutations.py').read_text();start=template.index('originals=');body=template[start:]
header='''import pathlib,json,hashlib,subprocess,os,re,signal,datetime
R=pathlib.Path('.').resolve();E=R/'.qualification/mcp-rework/mutations'
env=os.environ.copy();env.pop('KONTOR_AUTH',None);env.update(RUSTC_WRAPPER='',CARGO_NET_OFFLINE='true',RUST_TEST_THREADS='2',CARGO_TARGET_DIR=str(R/'.qualification/target-carrier-archive'),TMPDIR=str(E.parent/'tmp'),GIT_CEILING_DIRECTORIES=str(E.parent/'tmp'),SWAGGER_UI_DOWNLOAD_URL=(R/'.qualification/swagger-ui.zip').as_uri())
base=['cargo','test','--locked'];items=json.loads((E/'catalog.json').read_text())
'''
Path('.qualification/mcp-rework/reproduce-mutations.py').write_text(header+body)
print('prepared',len(catalog),'seeds')
