/**
 * The permission requests one session raised, and how each was answered.
 *
 * Three rules make this panel honest:
 *
 * 1. A request the transcript shows resolved cannot be answered from here —
 *    whoever answered it. The controls are disabled rather than hidden, so an
 *    operator can see that the decision exists and was made.
 * 2. An answer is sent under a stable response key, held across retries. This
 *    panel never mints one; it asks the room's ledger for the key belonging to
 *    that request.
 * 3. Whatever the realm answered is shown as the realm said it — a replay, a
 *    conflict, an unsupported operation — rather than reduced to "failed".
 */
import type { PermissionEntry, ResponseReceipt } from '../state/session'
import { isAnswerable } from '../state/session'
import { opaque } from '../api/types'
import { PayloadView } from './primitives'

/** The decisions the contract accepts. */
const DECISIONS = ['allow', 'deny'] as const

/** Render the permission ledger of one session. */
export function PermissionPanel({
  entries,
  onDecide,
  busy,
}: {
  /** The requests this transcript raised. */
  entries: readonly PermissionEntry[]
  /** Send one decision for one request. */
  onDecide: (requestId: string, decision: string) => void
  /** Whether the room is currently sending anything at all. */
  busy: boolean
}) {
  if (entries.length === 0) {
    return <p className="empty">this session has raised no permission requests</p>
  }
  return (
    <ul className="permissions">
      {entries.map((entry) => {
        const answerable = isAnswerable(entry) && !busy
        return (
          <li key={entry.requestId} data-request-id={entry.requestId}>
            <h4>
              <code>{entry.requestId}</code>
            </h4>
            <PayloadView payload={opaque(entry.request.payload)} />
            <p className="permission-state">
              {entry.resolution
                ? `resolved at ${entry.resolution.epoch}:${entry.resolution.sequence}`
                : 'awaiting a decision'}
            </p>
            <div className="permission-actions">
              {DECISIONS.map((decision) => (
                <button
                  key={decision}
                  type="button"
                  disabled={!answerable}
                  onClick={() => onDecide(entry.requestId, decision)}
                >
                  {decision}
                </button>
              ))}
            </div>
            {entry.receipt ? <Receipt receipt={entry.receipt} /> : null}
          </li>
        )
      })}
    </ul>
  )
}

/** What the realm answered to one decision sent from here. */
function Receipt({ receipt }: { receipt: ResponseReceipt }) {
  return (
    <p className="permission-receipt" data-response-state={receipt.state}>
      <span>
        answered <strong>{receipt.decision}</strong> under key <code>{receipt.responseId}</code>
      </span>
      <span> — {receipt.state}</span>
      {receipt.code ? (
        <span>
          {' '}
          (<code>{receipt.code}</code>: {receipt.rule})
        </span>
      ) : null}
    </p>
  )
}
