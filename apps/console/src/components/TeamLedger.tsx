/**
 * One team run's ledger: the slots it declares and what is in them.
 *
 * A slot with nothing in it is rendered as a declared, unfilled slot rather than
 * omitted — the declaration is what the team promised, and the gap between it
 * and what is running is the whole reason to look at this panel.
 */
import type { TeamLedgerView } from '../views/projections'
import { Fact, Facts, StateBadge } from './primitives'

/** Render one team run's ledger. */
export function TeamLedger({ ledger }: { ledger: TeamLedgerView }) {
  return (
    <section className="team-ledger" aria-label="team ledger">
      <Facts>
        <Fact label="team run" value={<code>{ledger.teamRunId}</code>} />
        <Fact label="pinned template" value={ledger.templateId} />
        <Fact label="template revision" value={ledger.templateVersion} />
      </Facts>
      {ledger.slots.length === 0 ? (
        <p className="empty">this template declares no role slots</p>
      ) : (
        <ul className="role-slots">
          {ledger.slots.map((slot) => (
            <li key={slot.role} data-role={slot.role}>
              <h4>{slot.role}</h4>
              <Facts>
                <Fact
                  label="filled by"
                  value={slot.agentRunId ? <code>{slot.agentRunId}</code> : null}
                />
                <Fact label="run state" value={<StateBadge state={slot.runState} label="run" />} />
                <Fact
                  label="binding"
                  value={<StateBadge state={slot.bindingState} label="binding" />}
                />
                <Fact
                  label="freshness"
                  value={<StateBadge state={slot.freshness} label="freshness" />}
                />
              </Facts>
              <Authority label="may evaluate" keys={slot.mayEvaluate} />
              <Authority label="may waive" keys={slot.mayWaive} />
              <Authority label="skills" keys={slot.skills} />
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}

/** One set of opaque authority keys. */
function Authority({ label, keys }: { label: string; keys: readonly string[] }) {
  return (
    <p className="authority" data-authority={label}>
      <span className="authority-label">{label}</span>
      {keys.length === 0 ? (
        <span className="empty">none declared</span>
      ) : (
        keys.map((key) => (
          <span key={key} className="phase-chip" data-kind={label}>
            {key}
          </span>
        ))
      )}
    </p>
  )
}
