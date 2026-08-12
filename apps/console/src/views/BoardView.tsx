/**
 * The board: the runs this realm has reported activity about.
 *
 * # Why this is not "all runs"
 *
 * The contract serves no run list. It serves a snapshot *by id* and a durable
 * feed of events that each name their run — so the honest inventory is the set of
 * runs the realm actually told us about, plus whatever an operator opened by
 * hand. The heading says so. Presenting it as the realm's full inventory would be
 * a claim no route supports.
 *
 * # What the detail refuses to conclude
 *
 * A run's lifecycle, what Kontor asked for, what the runtime last reported and
 * what Kontor concluded are four separate facts, and they are rendered as four.
 * `stale`, `diverged`, `runtime_unavailable`, `orphaned` and `lost_contact` are
 * statements about *evidence*; only `terminal` carries an outcome, and only from
 * the contract's own field.
 */
import { useRef } from 'react'
import type { Run } from '../api/types'
import type { CachedRun, ControlState } from '../state/control'
import { cachedRun } from '../state/control'
import { Fact, Facts, StateBadge } from '../components/primitives'
import { MasterDetail } from '../shell/MasterDetail'

/** Render the board. */
export function BoardView({
  control,
  selected,
  onSelect,
  onOpen,
}: {
  /** The control projection. */
  control: ControlState
  /** The run currently selected. */
  selected: string | null
  /** Select one run, or clear the selection. */
  onSelect: (agentRunId: string | null) => void
  /** Read one run by id. */
  onOpen: (agentRunId: string) => void
}) {
  const cached = selected ? cachedRun(control, selected) : undefined
  return (
    <MasterDetail
      detailLabel="run detail"
      open={selected !== null}
      onClose={() => onSelect(null)}
      master={
        <RunList
          control={control}
          selected={selected}
          onSelect={(id) => {
            onSelect(id)
            onOpen(id)
          }}
        />
      }
      detail={
        cached ? (
          <RunDetail cached={cached} />
        ) : selected ? (
          <p className="empty">
            This run has not been read yet. Opening it reads its snapshot from the
            realm.
          </p>
        ) : (
          <p className="empty">Select a run.</p>
        )
      }
    />
  )
}

/** The runs the feed has named, newest activity first. */
function RunList({
  control,
  selected,
  onSelect,
}: {
  control: ControlState
  selected: string | null
  onSelect: (agentRunId: string) => void
}) {
  const list = useRef<HTMLUListElement>(null)
  const entry = useRef<HTMLInputElement>(null)

  /** Arrow keys move through the list, as they do in the rail. */
  const onKeyDown = (event: React.KeyboardEvent<HTMLUListElement>): void => {
    const step =
      event.key === 'ArrowDown' ? 1 : event.key === 'ArrowUp' ? -1 : 0
    if (step === 0 || control.observed.length === 0) {
      return
    }
    event.preventDefault()
    const index = control.observed.findIndex((id) => id === selected)
    const next =
      control.observed[(Math.max(index, 0) + step + control.observed.length) % control.observed.length]
    if (!next) {
      return
    }
    onSelect(next)
    list.current?.querySelector<HTMLButtonElement>(`[data-run-id="${next}"]`)?.focus()
  }

  return (
    <div className="run-list">
      <h2>Runs this realm has reported</h2>
      <p className="caveat">
        The contract serves no run list, so this is what has been opened by hand
        plus what the durable feed has named since — not every run in the realm.
        {control.anchor === null
          ? ' Opening a run also takes the control-plane snapshot the feed is followed from, so nothing is being followed until one is.'
          : null}
      </p>

      <form
        className="open-by-id"
        onSubmit={(event) => {
          event.preventDefault()
          const value = entry.current?.value.trim()
          if (value) {
            onSelect(value)
          }
        }}
      >
        <label htmlFor="open-run">Open a run by id</label>
        <input id="open-run" name="agent_run_id" ref={entry} />
        <button type="submit">Read</button>
      </form>

      {control.observed.length === 0 ? (
        <p className="empty">The feed has not named a run yet.</p>
      ) : (
        <ul ref={list} onKeyDown={onKeyDown} aria-label="runs">
          {control.observed.map((agentRunId) => {
            const known = cachedRun(control, agentRunId)
            const isSelected = agentRunId === selected
            return (
              <li key={agentRunId}>
                <button
                  type="button"
                  data-run-id={agentRunId}
                  aria-current={isSelected ? 'true' : undefined}
                  tabIndex={isSelected || (selected === null && control.observed[0] === agentRunId) ? 0 : -1}
                  onClick={() => onSelect(agentRunId)}
                >
                  <code>{agentRunId}</code>
                  {known ? (
                    <>
                      <StateBadge state={known.value.projection.derived} label="derived state" />
                      <StateBadge state={known.value.projection.freshness} label="freshness" />
                      {known.behind ? (
                        <span className="badge" data-state="behind">
                          behind
                        </span>
                      ) : null}
                    </>
                  ) : (
                    <span className="badge" data-state="unread">
                      not read
                    </span>
                  )}
                </button>
              </li>
            )
          })}
        </ul>
      )}
    </div>
  )
}

/** One run, with every orthogonal fact kept apart from the others. */
export function RunDetail({ cached }: { cached: CachedRun }) {
  const run: Run = cached.value
  const projection = run.projection
  return (
    <article className="run-detail">
      <h2>
        <code>{run.agent_run_id}</code>
      </h2>
      {cached.behind ? (
        <p className="banner" role="status" data-banner="behind">
          The feed has moved past this snapshot. It was current at position{' '}
          <code>{cached.snapshotCursor}</code> and has not been read again since.
        </p>
      ) : null}

      <Facts>
        <Fact label="project" value={<code>{run.project_id}</code>} />
        <Fact label="team run" value={<code>{run.team_run_id}</code>} />
        <Fact label="role" value={run.role} />
        <Fact label="revision" value={run.revision} />
        <Fact label="snapshot position" value={cached.snapshotCursor} />
        <Fact
          label="parent run"
          value={run.parent_agent_run_id ? <code>{run.parent_agent_run_id}</code> : null}
        />
        <Fact
          label="account profile"
          value={run.account_profile_id ? <code>{run.account_profile_id}</code> : null}
        />
      </Facts>

      <section>
        <h3>State</h3>
        <Facts>
          <Fact label="lifecycle" value={<StateBadge state={projection.lifecycle} label="lifecycle" />} />
          <Fact
            label="desired"
            hint="what Kontor asked for"
            value={<StateBadge state={projection.desired} label="desired" />}
          />
          <Fact
            label="observed"
            hint="what the runtime last reported"
            value={<StateBadge state={projection.observed} label="observed" />}
          />
          <Fact
            label="derived"
            hint="what Kontor concluded"
            value={<StateBadge state={projection.derived} label="derived" />}
          />
          {/* Only the contract's own outcome field can say a run finished, and it
              is absent for every state that is merely a statement about evidence. */}
          <Fact label="outcome" value={projection.outcome} />
          <Fact label="freshness" value={<StateBadge state={projection.freshness} label="freshness" />} />
          <Fact label="last confirmed" value={projection.last_confirmed_at} />
          <Fact label="last event position" value={projection.last_cursor} />
          <Fact label="created" value={run.created_at} />
          <Fact label="closed" value={run.closed_at} />
        </Facts>
      </section>

      <section>
        <h3>Binding</h3>
        <BindingFacts binding={run.binding} />
      </section>

      <section>
        <h3>Pinned revisions</h3>
        <AppliedRevisionFacts applied={run.applied} />
      </section>

      <section>
        <h3>Recorded discontinuities</h3>
        {run.gaps.length === 0 ? (
          <p className="empty">none recorded</p>
        ) : (
          <ul className="gaps">
            {run.gaps.map((gap) => (
              <li key={`${gap.detected_cursor}`} data-gap-kind={gap.kind}>
                <StateBadge state={gap.kind} label="gap" /> expected{' '}
                <code>{gap.expected_sequence}</code>, received{' '}
                <code>{gap.received_sequence}</code>
                {gap.content_epoch === null || gap.content_epoch === undefined
                  ? null
                  : ` in epoch ${gap.content_epoch}`}{' '}
                — noticed at position <code>{gap.detected_cursor}</code>,{' '}
                <time dateTime={gap.detected_at}>{gap.detected_at}</time>
              </li>
            ))}
          </ul>
        )}
      </section>
    </article>
  )
}

/**
 * The native session a run is bound to.
 *
 * No binding is not an empty session: it is a run that was never launched, and
 * the two must not read the same.
 */
function BindingFacts({ binding }: { binding: Run['binding'] }) {
  if (!binding) {
    return (
      <p className="empty">
        This run has never been bound to a native session, which is not the same
        as having an empty one.
      </p>
    )
  }
  return (
    <Facts>
      <Fact label="binding" value={<code>{binding.binding_id}</code>} />
      <Fact label="runtime family" value={binding.runtime_kind} />
      <Fact label="host" value={binding.host} />
      <Fact label="generation" value={binding.generation} />
      <Fact label="runtime session id" value={<code>{binding.native_id}</code>} />
      <Fact label="bound at" value={binding.bound_at} />
      <Fact
        label="attached"
        hint="this process holds the frozen capability snapshot"
        value={<StateBadge state={binding.attached ? 'attached' : 'detached'} label="binding" />}
      />
    </Facts>
  )
}

/** The pinned revisions an aggregate is running under, whatever they are. */
export function AppliedRevisionFacts({
  applied,
}: {
  applied: Run['applied']
}) {
  const entries = Object.entries(applied).filter(([, value]) => value !== null && value !== undefined)
  if (entries.length === 0) {
    return <p className="empty">no pinned revisions reported</p>
  }
  return (
    <Facts>
      {entries
        .sort(([left], [right]) => left.localeCompare(right))
        .map(([key, value]) => (
          <Fact key={key} label={key.replace(/_/g, ' ')} value={String(value)} />
        ))}
    </Facts>
  )
}
