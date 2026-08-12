/**
 * What this realm believes, what the external system reports, and what the
 * pinned workflow revision permits doing about the difference.
 *
 * There is no status field, no assignee picker and no comment box here, and that
 * is deliberate rather than unfinished. The realm owns its own semantic facts;
 * the external system owns its status; the pinned revision decides which
 * transition — if any — connects them. An operator who could type a status
 * directly would be writing into a system this realm does not own, under a
 * revision that never authorized it.
 */
import type { WorkflowInspection } from '../views/projections'
import { Fact, Facts, StateBadge } from './primitives'

/** Render one external workflow's inspection. */
export function WorkflowInspector({ inspection }: { inspection: WorkflowInspection }) {
  return (
    <section className="workflow-inspector" aria-label="external workflow">
      <Facts>
        <Fact label="connector" value={inspection.connector} />
        <Fact label="external item" value={<code>{inspection.externalRef}</code>} />
        <Fact label="pinned workflow" value={inspection.workflowId} />
        <Fact label="workflow revision" value={inspection.workflowVersion} />
        <Fact
          label="assignee ownership"
          value={<StateBadge state={inspection.assigneeOwnership} label="ownership" />}
        />
      </Facts>

      <div className="workflow-columns">
        <section>
          <h4>this realm believes</h4>
          {inspection.internalFacts.length === 0 ? (
            <p className="empty">no semantic facts recorded</p>
          ) : (
            <Facts>
              {inspection.internalFacts.map((fact) => (
                <Fact key={fact.key} label={fact.key} value={fact.value} />
              ))}
            </Facts>
          )}
        </section>

        <section>
          <h4>the external system last reported</h4>
          {inspection.latestObservation === null ? (
            <p className="empty">
              nothing has been observed, which is not the same as nothing having
              happened
            </p>
          ) : (
            <Facts>
              <Fact
                label="status"
                value={
                  <StateBadge state={inspection.latestObservation.status} label="external status" />
                }
              />
              <Fact label="assignee" value={inspection.latestObservation.assignee} />
              <Fact
                label="observed at"
                value={
                  <time dateTime={inspection.latestObservation.observedAt}>
                    {inspection.latestObservation.observedAt}
                  </time>
                }
              />
            </Facts>
          )}
        </section>
      </div>

      <section className="workflow-proposal" data-proposal={inspection.proposal.kind}>
        <h4>proposed</h4>
        <p>
          <strong>{proposalLabel(inspection.proposal)}</strong>
        </p>
        <p className="workflow-because">{inspection.proposal.because}</p>
      </section>

      {inspection.receipt === null ? (
        <p className="empty">no proposal has been acted on</p>
      ) : (
        <Facts>
          <Fact label="receipt" value={<code>{inspection.receipt.receiptId}</code>} />
          <Fact
            label="receipt state"
            value={<StateBadge state={inspection.receipt.state} label="receipt" />}
          />
          <Fact label="attempts" value={inspection.receipt.attempts} />
        </Facts>
      )}
    </section>
  )
}

/** How one proposal reads. */
function proposalLabel(proposal: WorkflowInspection['proposal']): string {
  switch (proposal.kind) {
    case 'noop':
      return 'no change'
    case 'transition':
      return `transition to ${proposal.to}`
    case 'conflict':
      return 'conflict — a human decides'
  }
}
