/**
 * When work may be dispatched, and why the last decision went the way it did.
 *
 * The unrestricted case is stated in words rather than shown as an empty
 * calendar. "This realm places no restriction on when work runs" and "the
 * calendar failed to load" look identical as blank space, and an operator has to
 * be able to tell them apart.
 */
import type { SchedulingView } from '../views/projections'
import { Fact, Facts, StateBadge } from './primitives'

/** Render one realm's scheduling policy. */
export function SchedulingPanel({ scheduling }: { scheduling: SchedulingView }) {
  if (scheduling.kind === 'unrestricted') {
    return (
      <section className="scheduling" data-scheduling="unrestricted" aria-label="scheduling">
        <p>
          This realm places <strong>no calendar restriction</strong> on when work
          runs. Nothing is being withheld for a window.
        </p>
      </section>
    )
  }

  return (
    <section className="scheduling" data-scheduling="calendar" aria-label="scheduling">
      <Facts>
        <Fact label="calendar" value={<code>{scheduling.profileId}</code>} />
        <Fact label="pinned revision" value={scheduling.version} />
        <Fact label="timezone" value={scheduling.timezone} hint="every time below is in it" />
        <Fact
          label="dispatch"
          value={
            <StateBadge
              state={scheduling.draining ? 'draining' : 'accepting'}
              label="dispatch"
            />
          }
        />
      </Facts>

      <section>
        <h4>windows</h4>
        {scheduling.windows.length === 0 ? (
          <p className="empty">
            this calendar opens no windows, so nothing is admitted by it
          </p>
        ) : (
          <ul className="windows">
            {scheduling.windows.map((window) => (
              <li key={`${window.day}-${window.from}-${window.to}`}>
                <span className="window-day">{window.day}</span> {window.from}–{window.to}
              </li>
            ))}
          </ul>
        )}
      </section>

      <section>
        <h4>exceptions</h4>
        {scheduling.exceptions.length === 0 ? (
          <p className="empty">none</p>
        ) : (
          <ul className="exceptions">
            {scheduling.exceptions.map((exception) => (
              <li key={exception.day}>
                <span className="window-day">{exception.day}</span> — {exception.reason}
              </li>
            ))}
          </ul>
        )}
      </section>

      {scheduling.override ? (
        <section className="override" data-override="active">
          <h4>override in force</h4>
          <Facts>
            <Fact label="until" value={scheduling.override.until} />
            <Fact label="approved by" value={scheduling.override.approvedBy} />
          </Facts>
        </section>
      ) : null}

      <section className="decision">
        <h4>last dispatch decision</h4>
        {scheduling.lastDecision === null ? (
          <p className="empty">no decision has been recorded</p>
        ) : (
          <>
            <p>
              <StateBadge
                state={scheduling.lastDecision.admitted ? 'admitted' : 'rejected'}
                label="decision"
              />{' '}
              at{' '}
              <time dateTime={scheduling.lastDecision.evaluatedAt}>
                {scheduling.lastDecision.evaluatedAt}
              </time>
            </p>
            {/* The realm's own explanation, verbatim: an operator asking why
                nothing started is owed the rule, not this console's guess. */}
            <p className="decision-explanation">{scheduling.lastDecision.explanation}</p>
          </>
        )}
      </section>
    </section>
  )
}
