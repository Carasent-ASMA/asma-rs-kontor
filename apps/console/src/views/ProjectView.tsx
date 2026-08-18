/** Project-scoped Operational topology, teams, consultations and completion. */
import { useEffect, useMemo, useRef, useState } from 'react'
import type { KontorClient } from '../api/client'
import type {
  AdvisorRun,
  CodeHelpEntry,
  CommitteeRun,
  CompletionOutcome,
  CompletionState,
  CoreTeam,
  CoreTeamOutcome,
  CoreTeamPreview,
  CoreTeamSeatSelection,
  EpicProjection,
  MutationReceipt,
  ProfileCatalog,
  ProfileRevision,
  ProjectCapacity,
  PromotedSession,
  PromotionPreview,
  QuickRoles,
  QuickSession,
  RevisionRef,
  RoleCatalogEntry,
  RoleSelection,
  TopologyProjection,
} from '../api/types'
import { CodeHelp } from '../components/CodeHelp'
import { Fact, Facts, StateBadge } from '../components/primitives'

type OperationalClient = Pick<
  KontorClient,
  | 'epic'
  | 'topology'
  | 'coreTeam'
  | 'previewCoreTeam'
  | 'applyCoreTeam'
  | 'quickRoles'
  | 'ensureQuickSession'
  | 'previewPromotion'
  | 'applyPromotion'
  | 'projectCapacity'
  | 'codeHelp'
  | 'advisorProfiles'
  | 'committeeTemplates'
  | 'completionProfiles'
  | 'invokeAdvisor'
  | 'invokeCommittee'
  | 'completion'
  | 'advanceCompletion'
  | 'remediateCompletion'
>

interface Read<T> {
  value: T | null
  error: string | null
}

interface ProjectData {
  projectId: string
  epicId: string
  epic: Read<EpicProjection>
  topology: Read<TopologyProjection>
  coreTeam: Read<CoreTeam>
  roles: Read<QuickRoles>
  capacity: Read<ProjectCapacity>
  help: Read<{ entries: CodeHelpEntry[] }>
  advisors: Read<ProfileCatalog>
  committees: Read<ProfileCatalog>
  completionProfiles: Read<ProfileCatalog>
  completion: Read<CompletionState>
}

/**
 * One idempotency key per intent, held across retries.
 *
 * A retry of an uncertain request is the *same* intent, so it has to present
 * the same key: minting a fresh one at each activation is how a retry becomes a
 * second durable command that the daemon has no way to recognize as a replay.
 *
 * The key is released once the realm confirms one, because after a receipt the
 * next activation is a new intent rather than a replay, and it is replaced
 * whenever the intent itself changes — which is exactly what mutation rule 2
 * asks for.
 */
function useIntentKey(): { keyFor: (intent: unknown) => string; release: () => void } {
  const held = useRef<{ intent: string; key: string } | null>(null)
  return {
    keyFor(intent: unknown): string {
      const fingerprint = JSON.stringify(intent)
      if (held.current?.intent !== fingerprint) {
        held.current = { intent: fingerprint, key: crypto.randomUUID() }
      }
      return held.current.key
    },
    release(): void {
      held.current = null
    },
  }
}

/** Read one project and epic entirely through `/v1`. */
export function ProjectView({ client }: { client: OperationalClient }) {
  const [projectId, setProjectId] = useState('')
  const [epicId, setEpicId] = useState('')
  const [data, setData] = useState<ProjectData | null>(null)
  const [busy, setBusy] = useState(false)

  const read = async (): Promise<void> => {
    const project = projectId.trim()
    const epic = epicId.trim()
    if (!project || !epic) return
    setBusy(true)
    const [epicRead, topology, coreTeam, roles, capacity, help, advisors, committees, profiles, completion] =
      await Promise.all([
        settled(client.epic(project, epic)),
        settled(client.topology(project, epic)),
        settled(client.coreTeam(project)),
        settled(client.quickRoles(project)),
        settled(client.projectCapacity(project)),
        settled(client.codeHelp(project, epic)),
        settled(client.advisorProfiles(project)),
        settled(client.committeeTemplates(project)),
        settled(client.completionProfiles(project)),
        settled(client.completion(project, epic)),
      ])
    setData({
      projectId: project,
      epicId: epic,
      epic: epicRead,
      topology,
      coreTeam,
      roles,
      capacity,
      help,
      advisors,
      committees,
      completionProfiles: profiles,
      completion,
    })
    setBusy(false)
  }

  const help = data?.help.value?.entries ?? []
  const catalogRevision = useMemo(
    () =>
      data?.coreTeam.value?.seats[0]?.role.catalog_revision ??
      help.find((entry) => entry.category === 'role')?.source ??
      null,
    [data?.coreTeam.value, help],
  )

  return (
    <div className="project-view">
      <header className="view-intro">
        <h2>Project Operations</h2>
        <p>
          Server projections only. Logical session lineage is shown beside native placement;
          the browser does not derive lifecycle, identity, capacity or completion.
        </p>
      </header>
      <form
        className="open-by-id"
        aria-label="Open project and epic"
        onSubmit={(event) => {
          event.preventDefault()
          void read()
        }}
      >
        <label htmlFor="operations-project">Project</label>
        <input
          id="operations-project"
          name="project_id"
          required
          value={projectId}
          onChange={(event) => setProjectId(event.target.value)}
        />
        <label htmlFor="operations-epic">Epic</label>
        <input
          id="operations-epic"
          name="epic_id"
          required
          value={epicId}
          onChange={(event) => setEpicId(event.target.value)}
        />
        <button type="submit" disabled={busy}>{busy ? 'Reading…' : 'Read'}</button>
      </form>

      {data ? (
        <div className="project-sections">
          <section aria-labelledby="project-capacity">
            <h3 id="project-capacity">Project capacity</h3>
            {data.capacity.value ? <CapacityPanel capacity={data.capacity.value} /> : <Unavailable read={data.capacity} />}
          </section>

          <section aria-labelledby="project-topology">
            <h3 id="project-topology">Project Session Topology</h3>
            {data.topology.value ? (
              <TopologyPanel topology={data.topology.value} roles={data.roles.value?.roles ?? []} help={help} />
            ) : <Unavailable read={data.topology} />}
          </section>

          <section aria-labelledby="project-core-team">
            <h3 id="project-core-team">Project Core Team</h3>
            {data.coreTeam.value ? (
              <CoreTeamPanel
                client={client}
                projectId={data.projectId}
                team={data.coreTeam.value}
                roles={data.roles.value?.roles ?? []}
                rolesError={data.roles.error}
                catalogRevision={catalogRevision}
                help={help}
              />
            ) : <Unavailable read={data.coreTeam} />}
          </section>

          <section aria-labelledby="quick-sessions">
            <h3 id="quick-sessions">Quick Sessions</h3>
            {data.roles.value ? (
              <QuickSessionPanel
                client={client}
                projectId={data.projectId}
                roles={data.roles.value.roles}
                catalogRevision={catalogRevision}
                help={help}
              />
            ) : <Unavailable read={data.roles} />}
          </section>

          <section aria-labelledby="advisory">
            <h3 id="advisory">Advisory</h3>
            {data.epic.value ? (
              <AdvisoryPanel
                client={client}
                projectId={data.projectId}
                epic={data.epic.value}
                advisors={data.advisors}
                committees={data.committees}
                help={help}
              />
            ) : <Unavailable read={data.epic} />}
          </section>

          <section aria-labelledby="completion-profiles">
            <h3 id="completion-profiles">Completion Profiles</h3>
            {data.completionProfiles.value ? (
              <CompletionProfiles profiles={data.completionProfiles.value} />
            ) : <Unavailable read={data.completionProfiles} />}
            {data.completion.value ? (
              <CompletionPanel
                client={client}
                projectId={data.projectId}
                epicId={data.epicId}
                initial={data.completion.value}
                help={help}
              />
            ) : <Unavailable read={data.completion} />}
          </section>
        </div>
      ) : (
        <p className="empty">Open a project and epic by id; the contract serves no project list.</p>
      )}
    </div>
  )
}

function CapacityPanel({ capacity }: { capacity: ProjectCapacity }) {
  return (
    <>
      <Facts>
        <Fact label="active TeamRuns" value={capacity.active_team_runs} />
        <Fact label="MiniProject concurrency ceiling" value={capacity.mission_ceiling} />
        <Fact label="adaptive admission window" value={capacity.adaptive_width} />
        <Fact label="clean-observation streak" value={capacity.adaptive_streak} />
        <Fact label="snapshot position" value={capacity.snapshot_cursor} />
      </Facts>
      {capacity.last_refusal ? (
        <p className="banner" role="status" data-banner="capacity-refusal">
          Last admission refusal: {capacity.last_refusal}
        </p>
      ) : <p className="empty">No admission refusal reported.</p>}
    </>
  )
}

function TopologyPanel({
  topology,
  roles,
  help,
}: {
  topology: TopologyProjection
  roles: readonly RoleCatalogEntry[]
  help: readonly CodeHelpEntry[]
}) {
  return (
    <>
      <p className="caveat">
        PSW/QSW/ESW/ECP are logical lineage. An ESW is a separate native project;
        its ECP is one ordinary workspace, not a nested native project.
      </p>
      <div className="table-scroll">
        <table>
          <thead><tr><th>Node / kind</th><th>Logical parent</th><th>Placement</th><th>Observed native identity</th><th>Seats</th></tr></thead>
          <tbody>
            {topology.nodes.map((node) => (
              <tr key={node.topology_node_id} data-placement={node.placement}>
                <th scope="row"><code>{node.topology_node_id}</code><br /><CodeHelp code={node.kind_key} entries={help} /></th>
                <td>{node.parent_topology_node_id ? <code>{node.parent_topology_node_id}</code> : 'root'}</td>
                <td>
                  <CodeHelp code={node.placement} entries={help} />
                  <small className="block">desired: {node.desired_binding.runtime_kind}</small>
                </td>
                <td>{node.observed_binding ? <><code>{node.observed_binding.native_id}</code><small className="block">{node.observed_binding.cwd ?? 'cwd not reported'}</small></> : <StateBadge state="unobserved" label="native placement" />}</td>
                <td>
                  {node.seats.length === 0 ? 'none' : (
                    <ul className="compact-list">
                      {node.seats.map((seat) => {
                        const catalog = roles.find((role) => role.role_code === seat.role.role_code)
                        return (
                          <li key={seat.seat_binding_id}>
                            <CodeHelp code={seat.role.role_code} category="role" entries={help} />{' '}
                            {seat.role.custom_display_name ? <span>{seat.role.custom_display_name} · </span> : null}
                            {seat.role.standard_title} · <StateBadge state={seat.lifecycle} label="seat lifecycle" />
                            <small className="block">slot {seat.role_slot_id} · binding {seat.seat_binding_id}</small>
                            <small className="block">declared capabilities: {catalog?.capability_defaults.join(', ') || 'not projected'}</small>
                          </li>
                        )
                      })}
                    </ul>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  )
}

function CoreTeamPanel({
  client,
  projectId,
  team,
  roles,
  rolesError,
  catalogRevision,
  help,
}: {
  client: OperationalClient
  projectId: string
  team: CoreTeam
  roles: readonly RoleCatalogEntry[]
  rolesError: string | null
  catalogRevision: RevisionRef | null
  help: readonly CodeHelpEntry[]
}) {
  const [current, setCurrent] = useState(team)
  const [seats, setSeats] = useState<CoreTeamSeatSelection[]>([])
  const [roleCode, setRoleCode] = useState(roles[0]?.role_code ?? '')
  const [label, setLabel] = useState('')
  const [presence, setPresence] = useState('')
  const [adHocAllowed, setAdHocAllowed] = useState(false)
  const [preview, setPreview] = useState<CoreTeamPreview | null>(null)
  const [receipt, setReceipt] = useState<MutationReceipt | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const apply = useIntentKey()

  useEffect(() => {
    setCurrent(team)
    setSeats(team.seats.map((seat) => ({
      role: {
        catalog_revision: seat.role.catalog_revision,
        role_code: seat.role.role_code,
        ...(seat.role.custom_display_name ? { custom_display_name: seat.role.custom_display_name } : {}),
      },
      presence: seat.presence,
      ad_hoc_allowed: seat.ad_hoc_allowed,
    })))
  }, [team])

  const add = (): void => {
    const selection = roleSelection(catalogRevision, roleCode, label)
    if (!selection || !presence) return
    setSeats((value) => [...value, { role: selection, presence, ad_hoc_allowed: adHocAllowed }])
    setLabel('')
    setPresence('')
    setAdHocAllowed(false)
    setPreview(null)
  }

  return (
    <>
      <p className="caveat">The Core Team persists across epics. It is not a Delivery Team and it is not a running TeamRun.</p>
      <ul className="compact-list" aria-label="current Core Team seats">
        {current.seats.map((seat) => (
          <li key={`${seat.role.role_code}-${seat.seat_binding_id ?? 'unfilled'}`}>
            <CodeHelp code={seat.role.role_code} category="role" entries={help} /> {seat.role.standard_title}
            {seat.role.custom_display_name ? ` · ${seat.role.custom_display_name}` : ''}
            {' · '}presence <CodeHelp code={seat.presence} entries={help} />
            {' · '}Quick sessions {seat.ad_hoc_allowed ? 'allowed' : 'not allowed'}
            {' · '}{seat.seat_binding_id ? <code>{seat.seat_binding_id}</code> : 'not materialized'}
          </li>
        ))}
      </ul>
      <fieldset className="operation-form">
        <legend>New seat / role</legend>
        <RoleFields roles={roles} roleCode={roleCode} onRoleCode={setRoleCode} label={label} onLabel={setLabel} disabled={!catalogRevision} />
        <label className="field">Epic presence<select required value={presence} onChange={(event) => setPresence(event.target.value)}><option value="">Choose…</option><option value="required">required</option><option value="default">default</option><option value="on_demand">on demand</option></select></label>
        <label className="check-field"><input type="checkbox" checked={adHocAllowed} onChange={(event) => setAdHocAllowed(event.target.checked)} /> Quick-session eligible</label>
        <button type="button" onClick={add} disabled={busy || !catalogRevision || !roleCode || !presence}>Add to preview</button>
        {rolesError ? <p className="banner" role="alert">The roster above is the server's. The role catalog is not: {rolesError}</p> : null}
        {!rolesError && !catalogRevision ? <p className="banner" role="alert">The server projected no role-catalog revision, so a valid selection cannot be written.</p> : null}
      </fieldset>
      {seats.length ? (
        <ol className="compact-list" aria-label="proposed Core Team seats">
          {seats.map((seat, index) => (
            <li key={`${seat.role.role_code}-${index}`}>
              <CodeHelp code={seat.role.role_code} category="role" entries={help} /> {seat.role.custom_display_name ?? ''}
              {' · '}presence <CodeHelp code={seat.presence} entries={help} />
              {' · '}Quick sessions {seat.ad_hoc_allowed ? 'allowed' : 'not allowed'}
              <button type="button" onClick={() => { setSeats((value) => value.filter((_, at) => at !== index)); setPreview(null) }}>Remove</button>
            </li>
          ))}
        </ol>
      ) : <p className="empty">No seats in the proposed composition.</p>}
      <div className="operation-actions">
        <button type="button" disabled={busy} onClick={() => void act(() => client.previewCoreTeam(projectId, { seats }), setPreview, setError, setBusy)}>Preview Core Team</button>
        <button
          type="button"
          disabled={busy || !preview}
          onClick={() => {
            if (!preview) return
            const request = { expected_revision: current.revision, preview_hash: preview.preview_hash, seats }
            void act(
              () => client.applyCoreTeam(projectId, request, apply.keyFor(request)),
              (outcome: CoreTeamOutcome) => { apply.release(); setCurrent(outcome.core_team); setReceipt(outcome.receipt); setPreview(null) },
              setError,
              setBusy,
            )
          }}
        >Apply confirmed preview</button>
      </div>
      {preview ? <p className="banner" role="status">Preview <code>{preview.preview_hash}</code> · {preview.effects.length} effects. Apply is now enabled.</p> : null}
      <Problem error={error} />
      <Receipt receipt={receipt} />
    </>
  )
}

function QuickSessionPanel({
  client,
  projectId,
  roles,
  catalogRevision,
  help,
}: {
  client: OperationalClient
  projectId: string
  roles: readonly RoleCatalogEntry[]
  catalogRevision: RevisionRef | null
  help: readonly CodeHelpEntry[]
}) {
  const [roleCode, setRoleCode] = useState(roles[0]?.role_code ?? '')
  const [label, setLabel] = useState('')
  const [purpose, setPurpose] = useState('')
  const [session, setSession] = useState<QuickSession | null>(null)
  const [preview, setPreview] = useState<PromotionPreview | null>(null)
  const [promoted, setPromoted] = useState<PromotedSession | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const ensure = useIntentKey()
  const promote = useIntentKey()

  return (
    <>
      <p className="caveat">A Quick Session (QSW) is one ad-hoc seat. Promotion previews a logical PSW→ESW lineage; it does not nest one native project inside another.</p>
      <form
        className="operation-form"
        aria-label="New Quick Session"
        onSubmit={(event) => {
          event.preventDefault()
          const role = roleSelection(catalogRevision, roleCode, label)
          if (!role) return
          const request = { purpose: purpose.trim(), role }
          void act(
            () => client.ensureQuickSession(projectId, request, ensure.keyFor(request)),
            (opened: QuickSession) => { ensure.release(); setSession(opened) },
            setError,
            setBusy,
          )
        }}
      >
        <RoleFields roles={roles} roleCode={roleCode} onRoleCode={setRoleCode} label={label} onLabel={setLabel} disabled={!catalogRevision} />
        <label className="field grow">Purpose<input required value={purpose} onChange={(event) => setPurpose(event.target.value)} /></label>
        <button type="submit" disabled={busy || !catalogRevision || !roleCode}>Open Quick Session</button>
      </form>
      {session ? (
        <div className="operation-result">
          <p>QSW <code>{session.quick_session_id}</code> · node <code>{session.topology_node_id}</code> · <CodeHelp code={session.role.role_code} category="role" entries={help} /></p>
          <Receipt receipt={session.receipt} />
          <div className="operation-actions">
            <button type="button" disabled={busy} onClick={() => void act(() => client.previewPromotion(projectId, session.quick_session_id), setPreview, setError, setBusy)}>Preview promotion</button>
            <button
              type="button"
              disabled={busy || !preview}
              onClick={() => {
                if (!preview) return
                const request = { expected_revision: session.receipt.revision, preview_hash: preview.preview_hash }
                void act(
                  () => client.applyPromotion(projectId, session.quick_session_id, request, promote.keyFor(request)),
                  (value: PromotedSession) => { promote.release(); setPromoted(value) },
                  setError,
                  setBusy,
                )
              }}
            >Promote confirmed preview</button>
          </div>
        </div>
      ) : null}
      {preview ? <p className="banner" role="status">Promotion preview <code>{preview.preview_hash}</code> · {preview.effects.length} effects.</p> : null}
      {promoted ? <><p>Promoted to epic <code>{promoted.epic_id}</code>.</p><Receipt receipt={promoted.receipt} /></> : null}
      <Problem error={error} />
    </>
  )
}

function AdvisoryPanel({
  client,
  projectId,
  epic,
  advisors,
  committees,
  help,
}: {
  client: OperationalClient
  projectId: string
  epic: EpicProjection
  advisors: Read<ProfileCatalog>
  committees: Read<ProfileCatalog>
  help: readonly CodeHelpEntry[]
}) {
  return (
    <div className="advisory-grid">
      <section>
        <h4>Advisors · ASWs</h4>
        <p className="caveat">An Advisor Session Workspace is one consultation with one pinned Advisor profile.</p>
        {advisors.value ? <ConsultationForm kind="Advisor" profiles={advisors.value.revisions} expectedRevision={epic.revision} invoke={(profile, question, commandId) => client.invokeAdvisor(projectId, epic.epic_id, { expected_revision: epic.revision, profile, question }, commandId)} help={help} /> : <Unavailable read={advisors} />}
      </section>
      <section>
        <h4>Committees · CSWs</h4>
        <p className="caveat">A Committee Session Workspace has a pinned protocol and membership; it is distinct from an ASW and a Delivery Team.</p>
        {committees.value ? <ConsultationForm kind="Committee" profiles={committees.value.revisions} expectedRevision={epic.revision} invoke={(profile, question, commandId) => client.invokeCommittee(projectId, epic.epic_id, { expected_revision: epic.revision, profile, question }, commandId)} help={help} /> : <Unavailable read={committees} />}
      </section>
    </div>
  )
}

function ConsultationForm({
  kind,
  profiles,
  expectedRevision,
  invoke,
  help,
}: {
  kind: 'Advisor' | 'Committee'
  profiles: readonly ProfileRevision[]
  expectedRevision: number
  invoke: (profile: RevisionRef, question: string, commandId: string) => Promise<AdvisorRun | CommitteeRun>
  help: readonly CodeHelpEntry[]
}) {
  const [selected, setSelected] = useState(profileKey(profiles[0]))
  const [question, setQuestion] = useState('')
  const [run, setRun] = useState<AdvisorRun | CommitteeRun | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const consult = useIntentKey()
  const profile = profiles.find((entry) => profileKey(entry) === selected)
  return (
    <>
      <form className="operation-form" aria-label={`Invoke ${kind}`} onSubmit={(event) => {
        event.preventDefault()
        if (!profile) return
        const request = {
          expected_revision: expectedRevision,
          profile: { id: profile.id, version: profile.version },
          question: question.trim(),
        }
        void act(
          () => invoke(request.profile, request.question, consult.keyFor(request)),
          (value: AdvisorRun | CommitteeRun) => { consult.release(); setRun(value) },
          setError,
          setBusy,
        )
      }}>
        <label className="field">Profile<select required value={selected} onChange={(event) => setSelected(event.target.value)}>{profiles.map((entry) => <option key={profileKey(entry)} value={profileKey(entry)}>{entry.name} · {entry.id}@{entry.version}</option>)}</select></label>
        <label className="field grow">Question<input required value={question} onChange={(event) => setQuestion(event.target.value)} /></label>
        <button type="submit" disabled={busy || !profile}>Invoke {kind}</button>
      </form>
      <p className="empty">Member count and protocol are not exposed by this `/v1` profile catalog; the console does not infer them from the profile name or hash.</p>
      {run ? (
        <div className="operation-result">
          <p><CodeHelp code={run.state} entries={help} /> · {kind === 'Advisor' ? <code>{(run as AdvisorRun).advisor_run_id}</code> : <code>{(run as CommitteeRun).committee_run_id}</code>}</p>
          {'findings_recorded' in run ? <p>Findings recorded: {run.findings_recorded}</p> : null}
          <Receipt receipt={run.receipt} />
        </div>
      ) : null}
      <Problem error={error} />
    </>
  )
}

/** The published Completion Profile revisions, read independently of any epic. */
function CompletionProfiles({ profiles }: { profiles: ProfileCatalog }) {
  return (
    <ul className="compact-list" aria-label="Completion profiles">
      {profiles.revisions.map((profile) => <li key={profileKey(profile)}>{profile.name} · <code>{profile.id}@{profile.version}</code> · definition <code>{profile.definition_hash}</code></li>)}
    </ul>
  )
}

function CompletionPanel({
  client,
  projectId,
  epicId,
  initial,
  help,
}: {
  client: OperationalClient
  projectId: string
  epicId: string
  initial: CompletionState
  help: readonly CodeHelpEntry[]
}) {
  const [state, setState] = useState(initial)
  const [receipt, setReceipt] = useState<MutationReceipt | null>(null)
  const [reason, setReason] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const advance = useIntentKey()
  const remediate = useIntentKey()
  const accept = (outcome: CompletionOutcome): void => { setState(outcome.state); setReceipt(outcome.receipt) }
  return (
    <>
      <Facts>
        <Fact label="phase" value={<CodeHelp code={state.phase} entries={help} />} />
        <Fact label="profile" value={<code>{state.profile.id}@{state.profile.version}</code>} />
        <Fact label="revision" value={state.revision} />
        <Fact label="outstanding" value={state.outstanding.length ? state.outstanding.join(', ') : 'none'} />
      </Facts>
      <div className="operation-actions">
        <button type="button" disabled={busy} onClick={() => {
          const request = { expected_revision: state.revision }
          void act(
            () => client.advanceCompletion(projectId, epicId, request, advance.keyFor(request)),
            (outcome: CompletionOutcome) => { advance.release(); accept(outcome) },
            setError,
            setBusy,
          )
        }}>Advance completion</button>
      </div>
      <form className="operation-form" aria-label="Return completion to remediation" onSubmit={(event) => {
        event.preventDefault()
        const request = { expected_revision: state.revision, reason: reason.trim() }
        void act(
          () => client.remediateCompletion(projectId, epicId, request, remediate.keyFor(request)),
          (outcome: CompletionOutcome) => { remediate.release(); accept(outcome) },
          setError,
          setBusy,
        )
      }}>
        <label className="field grow">Remediation reason<input required value={reason} onChange={(event) => setReason(event.target.value)} /></label>
        <button type="submit" disabled={busy}>Return for remediation</button>
      </form>
      <Problem error={error} />
      <Receipt receipt={receipt} />
    </>
  )
}

function RoleFields({
  roles,
  roleCode,
  onRoleCode,
  label,
  onLabel,
  disabled,
}: {
  roles: readonly RoleCatalogEntry[]
  roleCode: string
  onRoleCode: (value: string) => void
  label: string
  onLabel: (value: string) => void
  disabled: boolean
}) {
  const groups = roleGroups(roles)
  return (
    <div className="field-row">
      <label className="field">Role<select required disabled={disabled} value={roleCode} onChange={(event) => onRoleCode(event.target.value)}>{groups.map(([segment, entries]) => <optgroup key={segment} label={segment}>{entries.map((role) => <option key={role.role_code} value={role.role_code}>{role.role_code} · {role.standard_title}</option>)}</optgroup>)}</select></label>
      <label className="field grow">Custom seat label <span className="hint">optional; presentation only</span><input value={label} onChange={(event) => onLabel(event.target.value)} /></label>
    </div>
  )
}

function Receipt({ receipt }: { receipt: MutationReceipt | null }) {
  if (!receipt) return null
  return (
    <p className="receipt" role="status">
      Confirmed receipt <code>{receipt.receipt_id}</code> · {receipt.applied} · revision {receipt.revision} · cursor {receipt.snapshot_cursor}
    </p>
  )
}

function Unavailable<T>({ read }: { read: Read<T> }) {
  return <p className="banner" role="status">{read.error ?? 'The realm returned no projection.'}</p>
}

function Problem({ error }: { error: string | null }) {
  return error ? <p className="banner" role="alert" data-banner="error">{error}</p> : null
}

async function settled<T>(promise: Promise<T>): Promise<Read<T>> {
  try {
    return { value: await promise, error: null }
  } catch (cause) {
    return { value: null, error: cause instanceof Error ? cause.message : 'The request failed.' }
  }
}

async function act<T>(
  run: () => Promise<T>,
  accept: (value: T) => void,
  reject: (message: string | null) => void,
  pending?: (value: boolean) => void,
): Promise<void> {
  pending?.(true)
  try {
    accept(await run())
    reject(null)
  } catch (cause) {
    reject(cause instanceof Error ? cause.message : 'The request failed.')
  } finally {
    pending?.(false)
  }
}

function roleSelection(revision: RevisionRef | null, roleCode: string, label: string): RoleSelection | null {
  if (!revision || !roleCode) return null
  const custom = label.trim()
  return { catalog_revision: revision, role_code: roleCode, ...(custom ? { custom_display_name: custom } : {}) }
}

function roleGroups(roles: readonly RoleCatalogEntry[]): [string, RoleCatalogEntry[]][] {
  const groups = new Map<string, RoleCatalogEntry[]>()
  for (const role of roles) groups.set(role.segment, [...(groups.get(role.segment) ?? []), role])
  return [...groups]
}

function profileKey(profile: ProfileRevision | undefined): string {
  return profile ? `${profile.id}@${profile.version}` : ''
}
