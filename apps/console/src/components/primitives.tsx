/**
 * The renderers everything else is built from.
 *
 * All of them take *data* and render it. None of them knows what a key means:
 * a gate map, a payload, a set of pinned revisions and a profile's phase keys
 * are all rendered as whatever they turn out to be. A component that recognized
 * one deployment's vocabulary would render that deployment and quietly mislead
 * about every other, and the one that matters is the one nobody wrote this for.
 */
import type { ReactNode } from 'react'
import type { Opaque } from '../api/types'

/** What is shown where a value is genuinely absent. */
const ABSENT = '—'

/** One labelled fact. */
export function Fact({
  label,
  value,
  hint,
}: {
  /** What the value is. */
  label: string
  /** The value, or `null`/`undefined` when the realm sent none. */
  value: ReactNode
  /** An optional note about where the value comes from. */
  hint?: string
}) {
  const absent = value === null || value === undefined || value === ''
  return (
    <div className="fact">
      <dt>
        {label}
        {hint ? <span className="fact-hint"> {hint}</span> : null}
      </dt>
      <dd data-absent={absent ? 'true' : undefined}>
        {absent ? <span aria-label="not reported by the realm">{ABSENT}</span> : value}
      </dd>
    </div>
  )
}

/** A list of labelled facts. */
export function Facts({ children }: { children: ReactNode }) {
  return <dl className="facts">{children}</dl>
}

/**
 * One state value, rendered as itself.
 *
 * The string is put in a data attribute so a stylesheet can pick out the states
 * it has opinions about, and rendered verbatim either way — a value this console
 * has never seen is shown, not swallowed.
 */
export function StateBadge({
  state,
  label,
}: {
  /** The state, exactly as the realm spelled it. */
  state: string | null | undefined
  /** What this state is *of*, for a reader who cannot see the layout. */
  label: string
}) {
  if (!state) {
    return <span className="badge" data-state="unreported">{`${label}: ${ABSENT}`}</span>
  }
  return (
    <span className="badge" data-state={state} title={`${label}: ${state}`}>
      {state}
    </span>
  )
}

/**
 * A map of opaque keys to opaque values.
 *
 * Gate states arrive this way, and so does anything else the contract types as a
 * document. Keys are sorted so the rendering is stable, and nothing is filtered.
 */
export function KeyedStates({
  entries,
  keyLabel,
  valueLabel,
  empty,
}: {
  /** The map, with whatever keys the deployment uses. */
  entries: Opaque
  /** The column heading for the keys. */
  keyLabel: string
  /** The column heading for the values. */
  valueLabel: string
  /** What to say when the map is empty. */
  empty: string
}) {
  const rows = Object.entries(entries).sort(([left], [right]) => left.localeCompare(right))
  if (rows.length === 0) {
    return <p className="empty">{empty}</p>
  }
  return (
    <table className="keyed">
      <thead>
        <tr>
          <th scope="col">{keyLabel}</th>
          <th scope="col">{valueLabel}</th>
        </tr>
      </thead>
      <tbody>
        {rows.map(([key, value]) => (
          <tr key={key}>
            <th scope="row">{key}</th>
            <td>
              <StateBadge state={renderScalar(value)} label={key} />
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}

/**
 * An opaque document, rendered structurally.
 *
 * The contract types several fields as `Object` — a runtime's own payload, the
 * control metadata on an event. Their shape belongs to whoever produced them, so
 * they are walked rather than interpreted.
 */
export function PayloadView({ payload, depth = 0 }: { payload: Opaque; depth?: number }) {
  const rows = Object.entries(payload)
  if (rows.length === 0) {
    return <p className="empty">no payload</p>
  }
  return (
    <dl className="payload" data-depth={depth}>
      {rows.map(([key, value]) => (
        <div key={key} className="payload-row">
          <dt>{key}</dt>
          <dd>
            {isDocument(value) ? (
              // Bounded so a self-referential or pathological document cannot
              // render forever; deeper values are shown as their JSON instead.
              depth < 4 ? (
                <PayloadView payload={value} depth={depth + 1} />
              ) : (
                <code>{safeJson(value)}</code>
              )
            ) : (
              <code>{renderScalar(value)}</code>
            )}
          </dd>
        </div>
      ))}
    </dl>
  )
}

/**
 * A view whose data the public contract does not serve yet.
 *
 * This console renders what `/v1` gives it and says so where it gives nothing.
 * The alternative — a plausible-looking panel filled from somewhere else — is
 * the failure this component exists to make impossible: an operator cannot tell
 * invented data from reported data, so none is invented.
 */
export function PendingProjection({
  subject,
  needs,
}: {
  /** What the view would show. */
  subject: string
  /** The projection the contract has to add before it can. */
  needs: string
}) {
  return (
    <section className="pending" aria-labelledby={`pending-${slug(subject)}`}>
      <h3 id={`pending-${slug(subject)}`}>{subject}</h3>
      <p>
        The realm serves no projection for this yet, so this console shows nothing
        rather than something it cannot source.
      </p>
      <p className="pending-needs">
        Waiting on: <code>{needs}</code>
      </p>
    </section>
  )
}

/** Render a scalar the way it arrived. */
function renderScalar(value: unknown): string {
  if (value === null || value === undefined) {
    return ABSENT
  }
  if (typeof value === 'string') {
    return value
  }
  if (typeof value === 'number' || typeof value === 'boolean') {
    return String(value)
  }
  return safeJson(value)
}

/** Whether a value should be walked rather than printed. */
function isDocument(value: unknown): value is Opaque {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

/** Serialize a value that cannot be walked, without throwing on a cycle. */
function safeJson(value: unknown): string {
  try {
    return JSON.stringify(value) ?? ABSENT
  } catch {
    return '[unserializable]'
  }
}

/** A stable id fragment for one heading. */
function slug(text: string): string {
  return text.toLowerCase().replace(/[^a-z0-9]+/g, '-')
}
