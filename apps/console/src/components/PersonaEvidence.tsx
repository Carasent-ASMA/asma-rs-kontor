/**
 * One persona run's evidence.
 *
 * The panel's job is to make one distinction impossible to miss: the role that
 * *performed* the scenario is not the authority that judges the gate it exercises.
 * A simulated user that graded its own session would be evidence of nothing, so
 * the executor and the gate's evaluators are rendered as separate, labelled
 * facts rather than as one list of participants.
 */
import type { PersonaEvidenceView } from '../views/projections'
import { Fact, Facts } from './primitives'

/** Render one persona run's evidence. */
export function PersonaEvidence({ evidence }: { evidence: PersonaEvidenceView }) {
  const executorIsEvaluator = evidence.evaluatorRoles.includes(evidence.actorRole)
  return (
    <section className="persona-evidence" aria-label="persona evidence">
      <Facts>
        <Fact label="scenario" value={<code>{evidence.scenarioId}</code>} />
        <Fact label="pinned revision" value={evidence.version} />
        <Fact label="persona" value={evidence.persona} />
        <Fact
          label="test identity"
          value={`${evidence.identity}${evidence.seeded ? ' (seeded)' : ''}`}
        />
        <Fact label="environment" value={evidence.environment} />
        <Fact label="gate under test" value={<code>{evidence.gateUnderTest}</code>} />
      </Facts>

      <section className="persona-authority">
        <h4>authority</h4>
        <Facts>
          <Fact
            label="executed by"
            value={<span data-role="executor">{evidence.actorRole}</span>}
            hint="performs the scenario"
          />
          <Fact
            label="judged by"
            hint="independent authority over the gate"
            value={
              evidence.evaluatorRoles.length === 0 ? null : (
                <span data-role="evaluators">{evidence.evaluatorRoles.join(', ')}</span>
              )
            }
          />
        </Facts>
        {executorIsEvaluator ? (
          <p className="warning" role="note" data-warning="self-evaluated">
            The role that performed this scenario also holds authority over the gate
            it exercises, so this evidence is not independent.
          </p>
        ) : null}
      </section>

      <section>
        <h4>steps</h4>
        {evidence.steps.length === 0 ? (
          <p className="empty">this scenario declares no steps</p>
        ) : (
          <ol className="persona-steps">
            {[...evidence.steps]
              .sort((left, right) => left.order - right.order)
              .map((step) => (
                <li key={step.order} data-retained={step.retained ? 'true' : 'false'}>
                  <p>{step.instruction}</p>
                  <p className="persona-step-evidence">
                    {step.expectedEvidence.length === 0
                      ? 'no evidence expected'
                      : `expects ${step.expectedEvidence.join(', ')}`}
                    {' — '}
                    {step.retained ? 'retained' : 'not retained'}
                  </p>
                </li>
              ))}
          </ol>
        )}
      </section>

      <section>
        <h4>safety constraints</h4>
        {evidence.prohibitedActions.length === 0 ? (
          <p className="empty">this scenario declares no prohibited actions</p>
        ) : (
          <ul className="persona-prohibited">
            {evidence.prohibitedActions.map((action) => (
              <li key={action}>{action}</li>
            ))}
          </ul>
        )}
      </section>
    </section>
  )
}
