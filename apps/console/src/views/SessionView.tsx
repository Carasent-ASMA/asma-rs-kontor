/**
 * The session room.
 *
 * Everything the contract offers for a session is here: the canonical transcript,
 * the live continuation, sending a message, and answering a permission request.
 * When the transcript cannot be followed the room says so in the realm's own
 * words and reads it again — and says nothing whatever about the run, because
 * missing content is not a finished session.
 */
import { useRef } from 'react'
import type { KontorClient } from '../api/client'
import { useSessionRoom } from '../shell/useSessionRoom'
import { TimelineItemView } from '../components/TimelineItemView'
import { PermissionPanel } from '../components/PermissionPanel'
import { MasterDetail } from '../shell/MasterDetail'

/** Render the session room for one run. */
export function SessionView({
  client,
  realmId,
  agentRunId,
  onClear,
}: {
  /** The attached client. */
  client: KontorClient | null
  /** The realm the session belongs to. */
  realmId: string | null
  /** The run whose session to open, when one is selected. */
  agentRunId: string | null
  /** Clear the selection. */
  onClear: () => void
}) {
  const room = useSessionRoom(client, realmId, agentRunId)
  const draft = useRef<HTMLTextAreaElement>(null)

  if (!agentRunId) {
    return (
      <p className="empty">
        Select a run on the board to open its session.
      </p>
    )
  }

  const { state } = room

  return (
    <MasterDetail
      detailLabel="session controls"
      open
      onClose={onClear}
      master={
        <div className="transcript">
          <h2>
            Session of <code>{agentRunId}</code>
          </h2>
          <p className="session-phase">
            <span className="badge" data-state={state.phase}>
              {state.phase}
            </span>
            {state.epoch === null ? null : (
              <span className="session-epoch"> epoch {state.epoch}</span>
            )}
          </p>

          {state.phase === 'refetch_required' ? (
            <p className="banner" role="status" data-banner="refetch">
              {/* The realm's own rule where it gave one, this room's reason where
                  the doubt was ours. Either way it is about the transcript, and
                  says nothing about the run. */}
              {state.refetchReason ?? 'this session’s content must be read again'}
              <button type="button" onClick={room.reload}>
                Read it again
              </button>
            </p>
          ) : null}

          {room.error ? (
            <p className="banner" role="alert" data-banner="error">
              {room.error}
            </p>
          ) : null}

          {state.items.length === 0 ? (
            <p className="empty">no content</p>
          ) : (
            <ol className="timeline">
              {state.items.map((item) => (
                <li key={`${item.epoch}:${item.sequence}`}>
                  <TimelineItemView item={item} />
                </li>
              ))}
            </ol>
          )}
        </div>
      }
      detail={
        <div className="session-controls">
          <section>
            <h3>Send a message</h3>
            <form
              onSubmit={(event) => {
                event.preventDefault()
                const body = draft.current?.value ?? ''
                void room.send(body).then(() => {
                  // Cleared only once the realm acknowledged it; a failed attempt
                  // keeps the text so the retry is the same message under the
                  // same key.
                  if (draft.current && !room.error) {
                    draft.current.value = ''
                  }
                })
              }}
            >
              <label htmlFor="message-body">Message</label>
              <textarea id="message-body" name="body" ref={draft} rows={3} />
              <button type="submit" disabled={room.busy}>
                Send
              </button>
            </form>
          </section>

          <section>
            <h3>Permission requests</h3>
            <PermissionPanel
              entries={room.permissions}
              busy={room.busy}
              onDecide={(requestId, decision) => void room.decide(requestId, decision)}
            />
          </section>
        </div>
      }
    />
  )
}
