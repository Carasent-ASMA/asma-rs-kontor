/**
 * The bar that is always on screen.
 *
 * It answers the four questions an operator should never have to go looking for:
 * which realm this is, where its endpoint lives, whether the realm is open for
 * work, and how current what is on screen actually is.
 *
 * The last one is the reason this component exists. A console whose data has
 * quietly stopped updating looks exactly like one whose realm has gone quiet, and
 * an operator acting on the first while believing the second is the failure this
 * bar is here to prevent.
 */
import type { Health, Realm } from '../api/types'
import type { ControlState } from '../state/control'
import { localityOf, type Endpoint } from '../api/endpoint'
import { StateBadge } from '../components/primitives'

/** How each contact state reads to an operator. */
const CONTACT_WORDING: Readonly<Record<string, string>> = {
  idle: 'nothing snapshotted yet — open a run to anchor the feed',
  live: 'following the realm',
  interrupted: 'not following — what is shown is as of the position below',
  resnapshot_required: 'the realm discarded our position — everything must be read again',
}

/** Render the persistent bar. */
export function TopBar({
  endpoint,
  realm,
  health,
  control,
}: {
  /** The endpoint this console was pointed at. */
  endpoint: Endpoint
  /** The realm's identity, once read. */
  realm: Realm | null
  /** The realm's liveness, once read. */
  health: Health | null
  /** The control projection, once started. */
  control: ControlState | null
}) {
  const locality = localityOf(endpoint.baseUrl)
  const contact = control?.contact ?? 'idle'

  return (
    <header className="top-bar">
      <div className="top-bar-identity">
        <h1>{realm?.display_label ?? 'Kontor'}</h1>
        <p className="realm-id">
          <span className="label">realm</span>{' '}
          <code>{realm?.realm_id ?? 'not yet read'}</code>
        </p>
      </div>

      <dl className="top-bar-facts">
        <div className="fact">
          {/* The contract's `RealmDto` carries no endpoint locality, so this is
              derived from the URL this console was configured with and labelled
              as such. Presenting it as realm-asserted would claim a field the
              contract does not have. */}
          <dt>
            endpoint <span className="fact-hint">(from this console’s configuration)</span>
          </dt>
          <dd>
            <StateBadge state={locality} label="endpoint locality" />{' '}
            <code>{endpoint.baseUrl}</code>
          </dd>
        </div>

        <div className="fact">
          <dt>reconciliation</dt>
          <dd>
            <StateBadge state={health?.reconciliation} label="reconciliation" />
          </dd>
        </div>

        <div className="fact">
          <dt>scheduling</dt>
          <dd>
            <StateBadge
              state={
                health === null ? undefined : health.scheduling_open ? 'open' : 'shut'
              }
              label="scheduling"
            />
          </dd>
        </div>

        <div className="fact">
          <dt>snapshot anchor</dt>
          <dd>
            <code>{control?.anchor ?? '—'}</code>
          </dd>
        </div>

        <div className="fact">
          <dt>newest position</dt>
          <dd>
            <code>{control?.cursor ?? '—'}</code>
          </dd>
        </div>

        <div className="fact">
          <dt>freshness</dt>
          <dd>
            <StateBadge state={contact} label="feed" />
            <span className="contact-wording"> {CONTACT_WORDING[contact] ?? contact}</span>
            {control?.newestRecordedAt ? (
              <>
                {' '}
                <time dateTime={control.newestRecordedAt}>{control.newestRecordedAt}</time>
              </>
            ) : null}
          </dd>
        </div>
      </dl>

      {contact === 'resnapshot_required' ? (
        <p className="banner" role="status" data-banner="resnapshot">
          This realm no longer retains the position this console was reading from.
          Everything shown was current at that position and has not been rechecked
          since.
        </p>
      ) : null}
      {contact === 'interrupted' ? (
        <p className="banner" role="status" data-banner="interrupted">
          The durable feed is not connected. Nothing here is being updated.
        </p>
      ) : null}
    </header>
  )
}
