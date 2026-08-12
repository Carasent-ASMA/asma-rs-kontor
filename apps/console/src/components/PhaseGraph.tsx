/**
 * A pinned profile revision's phase graph, rendered from its own declaration.
 *
 * Layers become columns on a wide screen and rows on a narrow one, which is a
 * stylesheet's business rather than this component's. What this component
 * guarantees is that every declared phase appears — including the ones nothing
 * routes to — and that every declared edge appears, including the ones that run
 * backwards, which is what a rejection route is.
 */
import { layoutPhaseGraph, type PhaseGraphSpec } from '../state/graph'

/** Render one profile's phase graph. */
export function PhaseGraph({
  spec,
  currentPhase,
  onSelectPhase,
  selectedPhase,
}: {
  /** The pinned revision's declaration. */
  spec: PhaseGraphSpec
  /** The phase the task's active workflow is in, when there is one. */
  currentPhase?: string | null
  /** Called when an operator picks a phase. */
  onSelectPhase?: (phaseId: string) => void
  /** The phase currently picked. */
  selectedPhase?: string | null
}) {
  const layout = layoutPhaseGraph(spec)
  if (layout.nodes.length === 0) {
    return <p className="empty">this profile revision declares no phases</p>
  }
  const placed = new Map(layout.nodes.map((node) => [node.phase.id, node]))

  return (
    <div className="phase-graph">
      <ol className="phase-layers" aria-label="phases, in order of distance from the entry phase">
        {layout.layers.map((layer, depth) => (
          <li key={depth} className="phase-layer">
            <ol aria-label={`layer ${depth + 1}`}>
              {layer.map((phaseId) => {
                const node = placed.get(phaseId)
                if (!node) {
                  return null
                }
                return (
                  <li key={phaseId}>
                    <button
                      type="button"
                      className="phase-node"
                      data-phase-id={phaseId}
                      data-entry={node.isEntry ? 'true' : undefined}
                      data-terminal={node.isTerminal ? 'true' : undefined}
                      data-current={currentPhase === phaseId ? 'true' : undefined}
                      aria-pressed={selectedPhase === phaseId}
                      aria-current={currentPhase === phaseId ? 'step' : undefined}
                      onClick={() => onSelectPhase?.(phaseId)}
                    >
                      <span className="phase-key">{phaseId}</span>
                      {node.phase.label && node.phase.label !== phaseId ? (
                        <span className="phase-label">{node.phase.label}</span>
                      ) : null}
                      <PhaseKeys label="gates" keys={node.phase.gates} />
                      <PhaseKeys label="requires" keys={node.phase.requiredArtifacts} />
                      {node.phase.rejectionRoute ? (
                        <span className="phase-chip" data-kind="rejects-to">
                          rejects to {node.phase.rejectionRoute}
                        </span>
                      ) : null}
                    </button>
                  </li>
                )
              })}
            </ol>
          </li>
        ))}
      </ol>

      {layout.unreachable.length > 0 ? (
        <section className="phase-unreachable">
          <h4>declared, unreachable from the entry phase</h4>
          <ul>
            {layout.unreachable.map((phaseId) => (
              <li key={phaseId} data-phase-id={phaseId}>
                {phaseId}
              </li>
            ))}
          </ul>
        </section>
      ) : null}

      <section className="phase-edges">
        <h4>transitions</h4>
        <ul>
          {layout.edges.map((edge, index) => (
            <li
              key={`${edge.from}->${edge.to}-${index}`}
              data-direction={edge.direction}
              data-from={edge.from}
              data-to={edge.to}
            >
              <span className="edge-from">{edge.from}</span>
              <span className="edge-arrow" aria-hidden="true">
                →
              </span>
              <span className="edge-to">{edge.to}</span>
              {edge.handoffRole ? (
                <span className="phase-chip" data-kind="handoff">
                  hands to {edge.handoffRole}
                </span>
              ) : null}
              {edge.direction === 'return' ? (
                <span className="phase-chip" data-kind="return">
                  returns
                </span>
              ) : null}
              {edge.direction === 'dangling' ? (
                <span className="phase-chip" data-kind="dangling">
                  names an undeclared phase
                </span>
              ) : null}
            </li>
          ))}
        </ul>
      </section>
    </div>
  )
}

/** A phase's declared keys, whatever they are. */
function PhaseKeys({ label, keys }: { label: string; keys?: readonly string[] }) {
  if (!keys || keys.length === 0) {
    return null
  }
  return (
    <span className="phase-keys">
      <span className="phase-keys-label">{label}</span>
      {keys.map((key) => (
        <span key={key} className="phase-chip" data-kind={label}>
          {key}
        </span>
      ))}
    </span>
  )
}
