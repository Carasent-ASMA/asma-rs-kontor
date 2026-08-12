/**
 * One item of session content.
 *
 * Every normalized kind the contract carries is rendered — messages and model
 * content, tool calls, permission requests and their resolutions, state changes
 * and logs — and so is any kind this console has never heard of. A transcript
 * that silently dropped what it did not recognize would look complete while
 * being exactly as wrong as one with a hole in it.
 *
 * The payload belongs to the runtime, not to this control plane, so it is walked
 * structurally rather than interpreted.
 */
import type { TimelineItem } from '../api/types'
import { opaque } from '../api/types'
import { PayloadView } from './primitives'

/**
 * Payload keys that commonly hold a human-readable body.
 *
 * ponytail: a display heuristic, not domain knowledge — the summary line is a
 * convenience and the full payload is always rendered underneath it, so a
 * runtime that spells its body differently loses nothing but the preview.
 */
const TEXT_KEYS = ['body', 'text', 'content', 'message', 'summary'] as const

/** Render one item of session content. */
export function TimelineItemView({ item }: { item: TimelineItem }) {
  const payload = opaque(item.payload)
  const preview = previewOf(payload)
  return (
    <article
      className="timeline-item"
      data-kind={item.kind}
      data-epoch={item.epoch}
      data-sequence={item.sequence}
      aria-label={`${item.kind} at position ${item.epoch}:${item.sequence}`}
    >
      <header>
        <span className="timeline-kind">{item.kind}</span>
        <span className="timeline-position">
          {item.epoch}:{item.sequence}
        </span>
        <time dateTime={item.emitted_at}>{item.emitted_at}</time>
      </header>
      {preview === null ? null : <p className="timeline-preview">{preview}</p>}
      <details>
        <summary>payload</summary>
        <PayloadView payload={payload} />
        <dl className="facts">
          {item.permission_id ? (
            <div className="fact">
              <dt>permission request</dt>
              <dd>
                <code>{item.permission_id}</code>
              </dd>
            </div>
          ) : null}
          {item.message_id ? (
            <div className="fact">
              <dt>message id</dt>
              <dd>
                <code>{item.message_id}</code>
              </dd>
            </div>
          ) : null}
          {item.native_event_id ? (
            <div className="fact">
              <dt>runtime event id</dt>
              <dd>
                <code>{item.native_event_id}</code>
              </dd>
            </div>
          ) : null}
        </dl>
      </details>
    </article>
  )
}

/** The first readable line the payload happens to offer, if any. */
function previewOf(payload: Record<string, unknown>): string | null {
  for (const key of TEXT_KEYS) {
    const value = payload[key]
    if (typeof value === 'string' && value.trim() !== '') {
      return value
    }
  }
  return null
}
