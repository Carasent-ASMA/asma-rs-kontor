/**
 * The phase graph is laid out from shape alone.
 *
 * The mutant this file exists to kill is a renderer that knows what a phase is
 * called — a `switch` on a profile id, a special case for "review", an ordering
 * that happens to be alphabetical. Every assertion is either about a structure
 * the declaration states, or about the layout being unchanged when every name in
 * it changes.
 */
import { describe, expect, it } from 'vitest'
import {
  CHAIN_PROFILE,
  DIAMOND_PROFILE,
  PROFILES,
  RAGGED_PROFILE,
  renameEverything,
} from '../test/fixtures'
import { layoutPhaseGraph, type PhaseGraphLayout } from './graph'

/** The layout's shape, with every identifier replaced by its position. */
function shapeOf(layout: PhaseGraphLayout): unknown {
  const index = new Map<string, number>()
  layout.nodes.forEach((node, position) => index.set(node.phase.id, position))
  const at = (id: string): number => index.get(id) ?? -1
  return {
    layers: layout.layers.map((layer) => layer.map(at)),
    nodes: layout.nodes.map((node) => ({
      layer: node.layer,
      isEntry: node.isEntry,
      isTerminal: node.isTerminal,
      reachable: node.reachable,
      gates: node.phase.gates?.length ?? 0,
      artifacts: node.phase.requiredArtifacts?.length ?? 0,
    })),
    edges: layout.edges.map((edge) => ({
      from: at(edge.from),
      to: at(edge.to),
      direction: edge.direction,
      handoff: edge.handoffRole === null ? null : 'named',
    })),
    unreachable: layout.unreachable.map(at),
  }
}

describe('phase graph layout', () => {
  it('layers a straight chain by distance from the declared entry', () => {
    const layout = layoutPhaseGraph(CHAIN_PROFILE)
    expect(layout.layers).toEqual([['p-9k'], ['p-4d'], ['p-2c']])
    expect(layout.nodes.map((node) => node.isEntry)).toEqual([true, false, false])
    expect(layout.nodes.map((node) => node.isTerminal)).toEqual([false, false, true])
    expect(layout.edges.every((edge) => edge.direction === 'forward')).toBe(true)
  })

  it('rejoins a fan-out and reports a rejection route as a return edge', () => {
    const layout = layoutPhaseGraph(DIAMOND_PROFILE)
    expect(layout.layers).toEqual([['x0'], ['x1', 'x2'], ['x3'], ['x4']])
    const rejection = layout.edges.find((edge) => edge.from === 'x3' && edge.to === 'x1')
    expect(rejection?.direction).toBe('return')
    expect(rejection?.handoffRole).toBe('r-d')
    // A return edge is still an edge: it must not be dropped from the rendering.
    expect(layout.edges).toHaveLength(DIAMOND_PROFILE.edges.length)
  })

  it('keeps a self-loop, an unreachable phase and a dangling edge visible', () => {
    const layout = layoutPhaseGraph(RAGGED_PROFILE)
    // No declared entry, so every phase nothing routes to is a root.
    expect(layout.layers[0]).toEqual(['n-a', 'n-c', 'n-d'])
    expect(layout.unreachable).toEqual([])
    const loop = layout.edges.find((edge) => edge.from === 'n-b' && edge.to === 'n-b')
    expect(loop?.direction).toBe('return')
    expect(layout.dangling.map((edge) => edge.to)).toEqual(['nowhere-at-all'])
  })

  it('places a phase nothing reaches without dropping it', () => {
    const layout = layoutPhaseGraph({
      entry: 'only',
      phases: [{ id: 'only' }, { id: 'stranded' }],
      edges: [],
    })
    expect(layout.unreachable).toEqual(['stranded'])
    expect(layout.nodes).toHaveLength(2)
    expect(layout.nodes[1]?.layer).toBeNull()
  })

  it('lays out a graph that is one closed cycle', () => {
    const layout = layoutPhaseGraph({
      phases: [{ id: 'c1' }, { id: 'c2' }],
      edges: [
        { from: 'c1', to: 'c2' },
        { from: 'c2', to: 'c1' },
      ],
    })
    expect(layout.unreachable).toEqual([])
    expect(layout.layers).toEqual([['c1'], ['c2']])
  })

  it.each(PROFILES.map((spec, index) => [index, spec] as const))(
    'lays profile %i out identically when every identifier in it is renamed',
    (_index, spec) => {
      const before = layoutPhaseGraph(spec)
      const { spec: renamed, rename } = renameEverything(spec)
      const after = layoutPhaseGraph(renamed)

      expect(shapeOf(after)).toEqual(shapeOf(before))
      // And the renamed keys are carried through as data rather than normalized
      // back to something the renderer recognizes.
      expect(after.nodes.map((node) => node.phase.id)).toEqual(
        before.nodes.map((node) => rename(node.phase.id)),
      )
    },
  )

  it('is stable across repeated layouts of the same declaration', () => {
    for (const spec of PROFILES) {
      expect(layoutPhaseGraph(spec)).toEqual(layoutPhaseGraph(spec))
    }
  })

  it('ignores a duplicated phase key rather than replacing the first', () => {
    const layout = layoutPhaseGraph({
      entry: 'dup',
      phases: [
        { id: 'dup', label: 'first' },
        { id: 'dup', label: 'second' },
      ],
      edges: [],
    })
    expect(layout.nodes).toHaveLength(1)
    expect(layout.nodes[0]?.phase.label).toBe('first')
  })

  it('lays out an empty declaration without inventing a phase', () => {
    const layout = layoutPhaseGraph({ phases: [], edges: [] })
    expect(layout.nodes).toEqual([])
    expect(layout.layers).toEqual([])
  })
})
