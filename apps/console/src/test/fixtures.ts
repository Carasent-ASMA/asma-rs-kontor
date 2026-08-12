/**
 * Fixtures for the console's headless suite.
 *
 * Every shape here is built through the generated contract types, so a fixture
 * cannot describe a field the realm does not serve — a suite that passes against
 * invented data is worse than no suite, because it certifies the invention.
 *
 * The profile fixtures are deliberately **anonymous**: their keys spell nothing,
 * and `renameEverything` re-spells them all. A renderer that recognized any of
 * them would fail the invariance test rather than pass three lookalike ones.
 */
import type {
  ControlEvent,
  Run,
  RunSnapshot,
  StreamFrame,
  Task,
  TaskSnapshot,
  TimelineItem,
  TimelinePage,
} from '../api/types'
import type { PhaseGraphSpec } from '../state/graph'

/** A realm id shaped like the ones the realm issues. */
export const REALM = '01920000-0000-7000-8000-000000000001'
/** A second realm, for the cross-realm refusals. */
export const OTHER_REALM = '01920000-0000-7000-8000-0000000000ff'
/** A run id. */
export const RUN = '01920000-0000-7000-8000-000000000010'
/** A project id. */
export const PROJECT = '01920000-0000-7000-8000-000000000020'
/** A task id. */
export const TASK = '01920000-0000-7000-8000-000000000030'

/** Build one run, overriding whatever the test cares about. */
export function run(overrides: Partial<Run> = {}): Run {
  return {
    agent_run_id: RUN,
    project_id: PROJECT,
    team_run_id: '01920000-0000-7000-8000-000000000040',
    role: 'r-7f2',
    applied: {},
    binding: null,
    gaps: [],
    projection: {
      lifecycle: 'running',
      desired: 'run_requested',
      observed: 'running',
      derived: 'confirmed',
      outcome: null,
      last_confirmed_at: '2026-08-10T09:00:00Z',
      freshness: 'fresh',
      last_cursor: 12,
    },
    revision: 3,
    created_at: '2026-08-10T08:00:00Z',
    closed_at: null,
    ...overrides,
  }
}

/** Wrap one run as the snapshot route serves it. */
export function runSnapshot(
  value: Run = run(),
  snapshotCursor = 12,
  realmId: string = REALM,
): RunSnapshot {
  return { realm_id: realmId, snapshot_cursor: snapshotCursor, value }
}

/** Build one task. */
export function task(overrides: Partial<Task> = {}): Task {
  return {
    task_id: TASK,
    project_id: PROJECT,
    title: 'q-41 carry',
    state: 'in_progress',
    revision: 5,
    current_phase: 'p-a1',
    gates: {} as Task['gates'],
    applied: {},
    updated_at: '2026-08-10T09:00:00Z',
    ...overrides,
  }
}

/** Wrap one task as the snapshot route serves it. */
export function taskSnapshot(
  value: Task = task(),
  snapshotCursor = 12,
  realmId: string = REALM,
): TaskSnapshot {
  return { realm_id: realmId, snapshot_cursor: snapshotCursor, value }
}

/** Build one durable control-plane event. */
export function controlEvent(overrides: Partial<ControlEvent> = {}): ControlEvent {
  return {
    realm_id: REALM,
    cursor: 13,
    project_id: PROJECT,
    agent_run_id: RUN,
    runtime_kind: 'k-3',
    generation: 1,
    native_id: 'n-1',
    native_event_id: null,
    native_sequence: 1,
    payload: {} as ControlEvent['payload'],
    observed_at: '2026-08-10T09:05:00Z',
    recorded_at: '2026-08-10T09:05:01Z',
    ...overrides,
  }
}

/** Build one item of session content. */
export function timelineItem(
  sequence: number,
  overrides: Partial<TimelineItem> = {},
): TimelineItem {
  return {
    kind: 'message',
    epoch: 1,
    sequence,
    permission_id: null,
    message_id: null,
    native_event_id: null,
    emitted_at: '2026-08-10T09:00:00Z',
    payload: {} as TimelineItem['payload'],
    ...overrides,
  }
}

/** Give an item a payload the contract types only as an opaque document. */
export function withPayload(
  item: TimelineItem,
  payload: Record<string, unknown>,
): TimelineItem {
  return { ...item, payload: payload as TimelineItem['payload'] }
}

/** Build one page of canonical history. */
export function timelinePage(
  items: readonly TimelineItem[],
  overrides: Partial<TimelinePage> = {},
): TimelinePage {
  const last = items.at(-1)
  const epoch = overrides.epoch ?? last?.epoch ?? 1
  const sequence = last?.sequence ?? 0
  return {
    realm_id: REALM,
    agent_run_id: RUN,
    epoch,
    items: [...items],
    next: null,
    end_epoch: epoch,
    end_sequence: sequence,
    anchor: `anchor-${epoch}-${sequence}`,
    ...overrides,
  }
}

/** Build one live content frame. */
export function streamFrame(
  item: TimelineItem,
  overrides: Partial<StreamFrame> = {},
): StreamFrame {
  return { realm_id: REALM, agent_run_id: RUN, item, ...overrides }
}

/**
 * A straight chain. No gates, no artifacts, no branching.
 */
export const CHAIN_PROFILE: PhaseGraphSpec = {
  entry: 'p-9k',
  terminals: ['p-2c'],
  phases: [
    { id: 'p-9k', label: 'q1' },
    { id: 'p-4d', label: 'q2' },
    { id: 'p-2c', label: 'q3' },
  ],
  edges: [
    { from: 'p-9k', to: 'p-4d', handoffRole: 'r-a' },
    { from: 'p-4d', to: 'p-2c' },
  ],
}

/**
 * A fan-out that rejoins, with a gated rejection routing backwards.
 */
export const DIAMOND_PROFILE: PhaseGraphSpec = {
  entry: 'x0',
  terminals: ['x4'],
  phases: [
    { id: 'x0', label: 'w0', gates: ['g-77'] },
    { id: 'x1', label: 'w1', requiredArtifacts: ['a-1', 'a-2'] },
    { id: 'x2', label: 'w2', requiredArtifacts: ['a-3'] },
    { id: 'x3', label: 'w3', gates: ['g-88', 'g-99'], rejectionRoute: 'x1' },
    { id: 'x4', label: 'w4' },
  ],
  edges: [
    { from: 'x0', to: 'x1' },
    { from: 'x0', to: 'x2', handoffRole: 'r-b' },
    { from: 'x1', to: 'x3' },
    { from: 'x2', to: 'x3' },
    { from: 'x3', to: 'x4', handoffRole: 'r-c' },
    { from: 'x3', to: 'x1', handoffRole: 'r-d' },
  ],
}

/**
 * A graph with everything awkward in it: a self-loop, a phase nothing reaches,
 * an edge naming a phase that was never declared, and no declared entry.
 */
export const RAGGED_PROFILE: PhaseGraphSpec = {
  terminals: ['n-e'],
  phases: [
    { id: 'n-a', label: 'v0' },
    { id: 'n-b', label: 'v1', gates: ['gg-1'] },
    { id: 'n-c', label: 'v2' },
    { id: 'n-d', label: 'v3' },
    { id: 'n-e', label: 'v4' },
  ],
  edges: [
    { from: 'n-a', to: 'n-b' },
    { from: 'n-b', to: 'n-b' },
    { from: 'n-b', to: 'n-e' },
    { from: 'n-c', to: 'n-e' },
    { from: 'n-d', to: 'nowhere-at-all' },
  ],
}

/** The three structurally different profiles the suite lays out. */
export const PROFILES: readonly PhaseGraphSpec[] = [
  CHAIN_PROFILE,
  DIAMOND_PROFILE,
  RAGGED_PROFILE,
]

/**
 * Re-spell every identifier in a profile.
 *
 * Phase keys, gate keys, artifact keys, role keys and labels all change; the
 * shape does not. Anything that renders differently afterwards was reading a
 * name, which is the one thing a profile-driven renderer may never do.
 */
export function renameEverything(spec: PhaseGraphSpec): {
  readonly spec: PhaseGraphSpec
  readonly rename: (id: string) => string
} {
  const minted = new Map<string, string>()
  const rename = (id: string): string => {
    const existing = minted.get(id)
    if (existing !== undefined) {
      return existing
    }
    // Deliberately unlike the originals in length, alphabet and shape.
    const next = `¶ZZ${minted.size.toString(36)}-${'Q'.repeat((minted.size % 4) + 1)}`
    minted.set(id, next)
    return next
  }
  const renameAll = (ids?: readonly string[]): string[] | undefined =>
    ids?.map(rename)

  return {
    rename,
    spec: {
      entry: spec.entry ? rename(spec.entry) : spec.entry,
      terminals: renameAll(spec.terminals),
      phases: spec.phases.map((phase) => ({
        id: rename(phase.id),
        label: phase.label ? rename(phase.label) : phase.label,
        gates: renameAll(phase.gates),
        requiredArtifacts: renameAll(phase.requiredArtifacts),
        rejectionRoute: phase.rejectionRoute ? rename(phase.rejectionRoute) : phase.rejectionRoute,
      })),
      edges: spec.edges.map((edge) => ({
        from: rename(edge.from),
        to: rename(edge.to),
        handoffRole: edge.handoffRole ? rename(edge.handoffRole) : edge.handoffRole,
      })),
    },
  }
}
