/**
 * Laying out a phase graph without knowing what any of it is called.
 *
 * The input is a pinned work-profile revision's own declaration: its phases, the
 * edges between them, where it starts and where it ends. Nothing in this module
 * compares an identifier to a literal, and nothing branches on a profile's name —
 * so a deployment's own profile, with entirely different keys and a different
 * shape, takes exactly the same path through it as the bundled one.
 *
 * That is not a style preference. A console that recognized "the coding phase"
 * would render a profile it recognized and quietly mis-render every other one,
 * and the profile that matters most is the one nobody wrote this code for.
 *
 * # Why breadth-first layers rather than longest path
 *
 * Real profiles route rejections backwards: a gate that fails sends work to an
 * earlier phase, so the graph has cycles and no topological order exists. Layering
 * by breadth-first distance from the entry is defined for every graph, cycles and
 * all, and is deterministic in the declaration order of the input. An edge that
 * lands on its own layer or an earlier one is reported as a *return* edge, which
 * is exactly what a rejection route is.
 */

/** One declared phase, with whatever keys the profile happens to use. */
export interface PhaseNode {
  /** The phase key. Opaque. */
  readonly id: string
  /** Its human label, when the profile carries one. */
  readonly label?: string | null
  /** The gates declared on it. Opaque keys. */
  readonly gates?: readonly string[]
  /** The artifacts it requires before it may run. Opaque keys. */
  readonly requiredArtifacts?: readonly string[]
  /** Where a rejection from this phase routes. Opaque key. */
  readonly rejectionRoute?: string | null
}

/** One declared edge between two phases. */
export interface PhaseEdge {
  /** The phase the edge leaves. */
  readonly from: string
  /** The phase the edge enters. */
  readonly to: string
  /** The role work is handed to across it, when the profile names one. */
  readonly handoffRole?: string | null
}

/** A pinned profile revision's phase graph, as declared. */
export interface PhaseGraphSpec {
  /** The phase the profile enters at, when it declares one. */
  readonly entry?: string | null
  /** The phases that close the profile. */
  readonly terminals?: readonly string[]
  /** Every declared phase. */
  readonly phases: readonly PhaseNode[]
  /** Every declared edge. */
  readonly edges: readonly PhaseEdge[]
}

/** Which direction an edge runs, once the graph is layered. */
export type EdgeDirection =
  /** Into a later layer: the profile's forward flow. */
  | 'forward'
  /** Into the same or an earlier layer: a rejection or a loop. */
  | 'return'
  /** One of its endpoints is not a declared phase. */
  | 'dangling'

/** One edge, placed. */
export interface LaidOutEdge {
  /** The phase the edge leaves. */
  readonly from: string
  /** The phase the edge enters. */
  readonly to: string
  /** The role work is handed to across it. */
  readonly handoffRole: string | null
  /** Which direction it runs. */
  readonly direction: EdgeDirection
}

/** One phase, placed. */
export interface LaidOutNode {
  /** The declared phase. */
  readonly phase: PhaseNode
  /** Its distance from the entry, or `null` when nothing reaches it. */
  readonly layer: number | null
  /** Whether the profile enters here. */
  readonly isEntry: boolean
  /** Whether the profile declares this phase terminal. */
  readonly isTerminal: boolean
  /** Whether anything reaches it from the entry. */
  readonly reachable: boolean
}

/** A phase graph, laid out. */
export interface PhaseGraphLayout {
  /** The placed phases, in declaration order. */
  readonly nodes: readonly LaidOutNode[]
  /** The layers, each holding phase ids in first-discovery order. */
  readonly layers: readonly (readonly string[])[]
  /**
   * Phases nothing reaches from the entry.
   *
   * Shown rather than dropped: a phase the profile declares and never routes to
   * is a fact about the profile, and hiding it would make the rendering prettier
   * than the thing it renders.
   */
  readonly unreachable: readonly string[]
  /** The placed edges, in declaration order. */
  readonly edges: readonly LaidOutEdge[]
  /** Edges naming a phase the profile never declared. */
  readonly dangling: readonly LaidOutEdge[]
}

/**
 * Lay out one declared phase graph.
 *
 * Deterministic: the same declaration always produces the same layout, and the
 * layout depends only on the graph's *shape* and the order things were declared
 * in — never on what anything is called.
 */
export function layoutPhaseGraph(spec: PhaseGraphSpec): PhaseGraphLayout {
  const declared = new Map<string, PhaseNode>()
  for (const phase of spec.phases) {
    // First declaration wins, so a duplicated key cannot silently replace the
    // phase the earlier edges were resolved against.
    if (!declared.has(phase.id)) {
      declared.set(phase.id, phase)
    }
  }

  const outgoing = new Map<string, string[]>()
  const hasIncoming = new Set<string>()
  for (const edge of spec.edges) {
    if (!declared.has(edge.from) || !declared.has(edge.to)) {
      continue
    }
    const list = outgoing.get(edge.from)
    if (list) {
      list.push(edge.to)
    } else {
      outgoing.set(edge.from, [edge.to])
    }
    hasIncoming.add(edge.to)
  }

  const layerOf = new Map<string, number>()
  const queue: string[] = []
  for (const root of roots(spec, declared, hasIncoming)) {
    if (!layerOf.has(root)) {
      layerOf.set(root, 0)
      queue.push(root)
    }
  }
  // Breadth-first, so a phase's layer is its shortest distance from a root and a
  // cycle simply stops expanding when it revisits a phase already placed.
  for (let head = 0; head < queue.length; head += 1) {
    const current = queue[head] as string
    const depth = layerOf.get(current) ?? 0
    for (const next of outgoing.get(current) ?? []) {
      if (!layerOf.has(next)) {
        layerOf.set(next, depth + 1)
        queue.push(next)
      }
    }
  }

  const terminals = new Set(spec.terminals ?? [])
  const nodes: LaidOutNode[] = spec.phases
    .filter((phase) => declared.get(phase.id) === phase)
    .map((phase) => {
      const layer = layerOf.get(phase.id)
      return {
        phase,
        layer: layer ?? null,
        isEntry: spec.entry !== null && spec.entry !== undefined && spec.entry === phase.id,
        isTerminal: terminals.has(phase.id),
        reachable: layer !== undefined,
      }
    })

  const depth = nodes.reduce((most, node) => Math.max(most, node.layer ?? -1), -1)
  const layers: string[][] = Array.from({ length: depth + 1 }, () => [])
  // Declaration order inside a layer, so the layout is stable across reads.
  for (const node of nodes) {
    if (node.layer !== null) {
      layers[node.layer]?.push(node.phase.id)
    }
  }

  const edges: LaidOutEdge[] = spec.edges.map((edge) => {
    const from = layerOf.get(edge.from)
    const to = layerOf.get(edge.to)
    const known = declared.has(edge.from) && declared.has(edge.to)
    let direction: EdgeDirection = 'dangling'
    if (known) {
      direction =
        from !== undefined && to !== undefined && to > from ? 'forward' : 'return'
    }
    return {
      from: edge.from,
      to: edge.to,
      handoffRole: edge.handoffRole ?? null,
      direction,
    }
  })

  return {
    nodes,
    layers,
    unreachable: nodes.filter((node) => !node.reachable).map((node) => node.phase.id),
    edges,
    dangling: edges.filter((edge) => edge.direction === 'dangling'),
  }
}

/**
 * Where the layering starts.
 *
 * The declared entry, when there is one. Otherwise every phase nothing routes to,
 * which is the same answer for any profile that declares its start implicitly.
 * A graph that is one closed cycle has neither, and starts at its first declared
 * phase so that it is laid out rather than reported as entirely unreachable.
 */
function roots(
  spec: PhaseGraphSpec,
  declared: ReadonlyMap<string, PhaseNode>,
  hasIncoming: ReadonlySet<string>,
): readonly string[] {
  if (spec.entry && declared.has(spec.entry)) {
    return [spec.entry]
  }
  const sources = spec.phases
    .filter((phase) => declared.get(phase.id) === phase && !hasIncoming.has(phase.id))
    .map((phase) => phase.id)
  if (sources.length > 0) {
    return sources
  }
  const first = spec.phases[0]
  return first ? [first.id] : []
}
