/**
 * One task: where its active workflow stands, and under which pinned revisions.
 *
 * # What the contract serves, and what it does not
 *
 * `GET /v1/projects/{project_id}/tasks/{task_id}` gives the task's state, the
 * phase its active workflow is in, its reduced gate states and the pinned
 * revisions in force. That is what this view renders.
 *
 * It does *not* serve the pinned profile's **contents**, so the phase graph, the
 * artifacts each phase requires and produces, and the evidence references behind
 * each gate evaluation have no source. Those panels say so rather than guess. The
 * renderers themselves are complete and exercised by the suite — when the
 * projection arrives, the adapter is the only new code.
 *
 * There is also no task *list*: a task is opened by id, because that is the only
 * route the contract has.
 */
import { useRef } from 'react'
import type { Task } from '../api/types'
import { opaque } from '../api/types'
import type { ControlState } from '../state/control'
import { cachedTask } from '../state/control'
import type { PhaseGraphSpec } from '../state/graph'
import { Fact, Facts, KeyedStates, PendingProjection, StateBadge } from '../components/primitives'
import { PhaseGraph } from '../components/PhaseGraph'
import { TeamLedger } from '../components/TeamLedger'
import { PersonaEvidence } from '../components/PersonaEvidence'
import { AppliedRevisionFacts } from './BoardView'
import type { PersonaEvidenceView, TeamLedgerView } from './projections'

/** Render the task view. */
export function TaskView({
  control,
  selected,
  onOpen,
  profile,
  team,
  persona,
}: {
  /** The control projection. */
  control: ControlState
  /** The task currently open, as `(project_id, task_id)`. */
  selected: { projectId: string; taskId: string } | null
  /** Read one task by id. */
  onOpen: (projectId: string, taskId: string) => void
  /**
   * The pinned profile revision's declared graph.
   *
   * Absent until the contract serves the pinned profile's contents.
   */
  profile?: PhaseGraphSpec | null
  /** The team ledger, absent for the same reason. */
  team?: TeamLedgerView | null
  /** The persona evidence, absent for the same reason. */
  persona?: PersonaEvidenceView | null
}) {
  const task = selected ? cachedTask(control, selected.taskId)?.value : undefined
  return (
    <div className="task-view">
      <OpenTask onOpen={onOpen} />
      {task ? (
        <TaskDetail task={task} profile={profile ?? null} team={team ?? null} persona={persona ?? null} />
      ) : (
        <p className="empty">
          Open a task by id. The contract serves no task list, so there is nothing
          to browse.
        </p>
      )}
    </div>
  )
}

/** The only way to reach a task the contract has. */
function OpenTask({ onOpen }: { onOpen: (projectId: string, taskId: string) => void }) {
  const project = useRef<HTMLInputElement>(null)
  const task = useRef<HTMLInputElement>(null)
  return (
    <form
      className="open-by-id"
      onSubmit={(event) => {
        event.preventDefault()
        const projectId = project.current?.value.trim()
        const taskId = task.current?.value.trim()
        if (projectId && taskId) {
          onOpen(projectId, taskId)
        }
      }}
    >
      <label htmlFor="open-project">Project</label>
      <input id="open-project" name="project_id" ref={project} />
      <label htmlFor="open-task">Task</label>
      <input id="open-task" name="task_id" ref={task} />
      <button type="submit">Read</button>
    </form>
  )
}

/**
 * The task's reader-facing identity.
 *
 * The confirmed Jira key is what a reader is given, because it is the identifier
 * the rest of the organisation already uses for this work. The UUID does not
 * disappear — every existing route, script and bookmark still accepts it — but it
 * becomes a secondary, debug-level fact rather than the thing the eye lands on.
 *
 * When no binding is confirmed the view says exactly that. Nothing is derived
 * from the title, the legacy backlog code or the UUID's shape: a key that no
 * connector has confirmed is not this task's identity, and guessing one here
 * would be the console inventing identity the server never granted.
 *
 * The state is read straight from the server's `jira_binding` projection. The
 * console performs no lookup of its own — resolution belongs to the one
 * server-side resolver, and a second client-side path is precisely what the
 * identity decision forbids.
 */
function TaskIdentityFacts({ task }: { task: Task }) {
  const binding = task.jira_binding
  const confirmed = binding?.state === 'confirmed' && binding.jira_key ? binding.jira_key : null
  return (
    <>
      <Fact
        label="task"
        value={
          confirmed ? (
            <code className="identity-primary">{confirmed}</code>
          ) : (
            'Awaiting Jira binding'
          )
        }
        hint={confirmed ? 'confirmed Jira key' : 'no confirmed Jira binding yet'}
      />
      <Fact
        label="task uuid"
        value={<code className="identity-internal">{task.task_id}</code>}
        hint="internal identity; still accepted wherever a task is addressed"
      />
    </>
  )
}

/** One task, in full. */
export function TaskDetail({
  task,
  profile,
  team,
  persona,
}: {
  task: Task
  profile: PhaseGraphSpec | null
  team: TeamLedgerView | null
  persona: PersonaEvidenceView | null
}) {
  return (
    <article className="task-detail">
      <h2>{task.title}</h2>
      <Facts>
        <TaskIdentityFacts task={task} />
        <Fact label="project" value={<code>{task.project_id}</code>} />
        <Fact label="state" value={<StateBadge state={task.state} label="task state" />} />
        <Fact label="revision" value={task.revision} />
        <Fact
          label="current phase"
          value={task.current_phase ? <code>{task.current_phase}</code> : null}
        />
        <Fact label="updated" value={task.updated_at} />
      </Facts>

      <section>
        <h3>Pinned revisions</h3>
        <AppliedRevisionFacts applied={task.applied} />
      </section>

      <section>
        <h3>Gates</h3>
        {/* Arbitrary gate keys, rendered as data. */}
        <KeyedStates
          entries={opaque(task.gates)}
          keyLabel="gate"
          valueLabel="state"
          empty="this task's active workflow reduces no gate states"
        />
        <PendingProjection
          subject="Gate evaluations and their evidence"
          needs="a projection of each gate's evaluations, verdicts and evidence references (KON-MVP-16)"
        />
      </section>

      <section>
        <h3>Phases</h3>
        {profile ? (
          <PhaseGraph spec={profile} currentPhase={task.current_phase} />
        ) : (
          <PendingProjection
            subject="The pinned profile's phase graph"
            needs="a projection of the pinned work-profile revision's phases, edges, gates and artifact contracts (KON-MVP-16)"
          />
        )}
      </section>

      <section>
        <h3>Artifacts</h3>
        <PendingProjection
          subject="Required and produced artifacts, with evidence references"
          needs="a projection of the pinned profile's artifact contracts and the task's retained evidence (KON-MVP-16)"
        />
      </section>

      <section>
        <h3>Team</h3>
        {team ? (
          <TeamLedger ledger={team} />
        ) : (
          <PendingProjection
            subject="The team ledger"
            needs="a projection of the team run's declared role slots, role and skill authority, run and binding state (KON-MVP-16)"
          />
        )}
      </section>

      <section>
        <h3>Persona evidence</h3>
        {persona ? (
          <PersonaEvidence evidence={persona} />
        ) : (
          <PendingProjection
            subject="Persona run evidence"
            needs="a projection of the pinned persona scenario, its steps, safety constraints and retained evidence (KON-MVP-16)"
          />
        )}
      </section>
    </article>
  )
}
