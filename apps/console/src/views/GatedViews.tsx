/**
 * The three views whose data the contract does not serve at all yet.
 *
 * Each one is a complete renderer with a prop that is currently never supplied.
 * When KON-MVP-16 adds the projection, the generated types gain it and the only
 * new code is the adapter that fills the prop — the rendering, its rules and its
 * tests are already here.
 *
 * Until then each view says exactly what is missing. The alternative, filling
 * them from a fixture so they look finished, would put data on an operator's
 * screen that no realm reported; a console that does that once cannot be trusted
 * anywhere.
 */
import { IntakeInbox } from '../components/IntakeInbox'
import { WorkflowInspector } from '../components/WorkflowInspector'
import { SchedulingPanel } from '../components/SchedulingPanel'
import { PendingProjection } from '../components/primitives'
import type { IntakeItem, SchedulingView, WorkflowInspection } from './projections'

/** The intake inbox. */
export function IntakeView({ items }: { items?: readonly IntakeItem[] | null }) {
  return (
    <section className="view" aria-label="intake">
      <h2>Intake</h2>
      {items ? (
        <IntakeInbox items={items} />
      ) : (
        <PendingProjection
          subject="The intake inbox"
          needs="a projection of intake receipts — source, dedup key, matched trigger revision, approval state and created-work lineage (KON-MVP-16)"
        />
      )}
    </section>
  )
}

/** The external-workflow inspector. */
export function WorkflowView({
  inspection,
}: {
  inspection?: WorkflowInspection | null
}) {
  return (
    <section className="view" aria-label="external workflow">
      <h2>External workflow</h2>
      {inspection ? (
        <WorkflowInspector inspection={inspection} />
      ) : (
        <PendingProjection
          subject="The external-workflow inspector"
          needs="a projection of this realm's semantic facts, the pinned workflow revision, the latest external observation, assignee ownership and the proposed transition with its receipt (KON-MVP-16)"
        />
      )}
    </section>
  )
}

/** The scheduling view. */
export function ScheduleView({ scheduling }: { scheduling?: SchedulingView | null }) {
  return (
    <section className="view" aria-label="scheduling">
      <h2>Scheduling</h2>
      {scheduling ? (
        <SchedulingPanel scheduling={scheduling} />
      ) : (
        <PendingProjection
          subject="When work may be dispatched"
          needs="a projection stating either that this realm is unrestricted, or its calendar revision, timezone, windows, exceptions, drain state, override and the deterministic explanation of the last rejection (KON-MVP-16)"
        />
      )}
    </section>
  )
}
