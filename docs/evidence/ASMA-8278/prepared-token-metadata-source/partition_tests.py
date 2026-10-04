from pathlib import Path
root=Path('/tmp/asma-8278-module-consolidation-20261002.Mcrugq')
p=root/'crates/kontor-store/tests/attestation_token_metadata.rs';s=p.read_text();first=s.index('#[test]');test_start=s.index('#[test]\nfn inactive_binding_and_node_each_refuse_unchanged')
fixture=s[:first].replace('mod support;','use super::support;').replace('struct Fixture {','pub(super) struct Fixture {')
for name in ['id','name','native']:fixture=fixture.replace('fn '+name+'(', 'pub(super) fn '+name+'(')
for name in ['build','sql','request','counts','consultation','consultation_family','refuse_unchanged']:fixture=fixture.replace('    fn '+name+'(', '    pub(super) fn '+name+'(')
# Keep fields visible only to this integration-test module and its children.
start=fixture.index('pub(super) struct Fixture {');end=fixture.index('\n}',start)
fields=fixture[start:end];fields='\n'.join('    pub(super) '+line.strip() if line.startswith('    ') else line for line in fields.splitlines());fixture=fixture[:start]+fields+fixture[end:]
# Extract node creation to keep fixture construction below100 lines.
start=fixture.index('        let root = TopologyNodeId::generate();');end=fixture.index('        let entry = catalog',start);nodes=fixture[start:end]
nodes=nodes.replace('let root = TopologyNodeId::generate();','let root = TopologyNodeId::generate();')
helper='''fn fixture_nodes(store: &SqliteStore, project: ProjectId, epic: MiniProjectId,
    topology: &TopologySnapshot, at: Timestamp) -> (TopologyNodeId, TopologyNodeId) {
'''+nodes+'''    (esw, node)
}

'''
fixture=fixture[:start]+'''        let (esw,node) = fixture_nodes(&store, project, epic, &topology, at);
'''+fixture[end:]
# Extract only fixed public fixture key registration (not a production path).
start=fixture.index('        store\n            .register_attestation_key(');end=fixture.index('        Self {',start);reg=fixture[start:end]
fixture=fixture[:start]+'        fixture_key(&store, &scope);\n'+fixture[end:]
helper+='''fn fixture_key(store: &SqliteStore, scope: &AttestationAuthorityScope) {
'''+reg+'}\n\n'
# Extract the frozen run value without changing its fields or any validator.
start=fixture.index('        let question = BoundedText::parse(');end=fixture.index('        let binding = NewSeatBinding',start);run=fixture[start:end]
run=run.replace('        let run = StoredConsultationRun {','        StoredConsultationRun {').replace('            context_hash: digest,','            context_hash: ContentHash::of(br#"{"schema_version":1}"#),').replace('definition_hash: digest.clone(),','definition_hash: ContentHash::of(br#"{"schema_version":1}"#),')
assert run.rstrip().endswith('};');run=run.rstrip()[:-2]+'}\n'
method='''    fn frozen_run(&self, run_id: ConsultationRunId, profile: String, task: Option<TaskId>,
        node: TopologyNodeId, at: Timestamp) -> StoredConsultationRun {
'''+run+'    }\n\n'
fixture=fixture[:start]+'        let run = self.frozen_run(run_id, profile, task, node, at);\n'+fixture[end:]
where=fixture.index('    pub(super) fn consultation(');fixture=fixture[:where]+method+fixture[where:]
where=fixture.index('pub(super) struct Fixture {');fixture=fixture[:where]+helper+fixture[where:]
root_header='''//! Durable prepared commitments and exact caller expectations; no live proof.
mod support;
#[path = "attestation_token_metadata/fixture.rs"]
mod fixture;
#[path = "attestation_token_metadata/guards.rs"]
mod guards;

use fixture::*;
use kontor_core::id::{AggregateRevision, ContentHash, MiniProjectId, ProjectId, RuntimeKindKey, SeatBindingId};
use kontor_core::repository::{AttestationAuthorityRepository, AttestationSeatProvenance, RegisterAttestationKey, RepositoryError};
use kontor_store::SqliteStore;
'''
folder=p.with_suffix('');folder.mkdir(exist_ok=True)
(folder/'fixture.rs').write_text(fixture)
(folder/'guards.rs').write_text('''//! Stored-domain, schema/SQL and backup boundary regressions, not live proof.
use super::*;
use kontor_core::consultation::ConsultationFamily;
use kontor_core::id::{RoleSlotId, Timestamp};
use rusqlite::{Connection, params};

'''+s[test_start:])
p.write_text(root_header+'\n'+s[first:test_start])
print({'fixture':str(folder/'fixture.rs'),'guards':str(folder/'guards.rs')})
