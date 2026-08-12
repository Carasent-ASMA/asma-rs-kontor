/**
 * Renderer inputs for the projections the `/v1` contract does not serve yet.
 *
 * # What these are, and what they are not
 *
 * These are **component props**, not wire types. Nothing here is deserialized
 * from a response, because there is no response to deserialize: the merged
 * KON-MVP-15 surface exposes health, realm, the contract document, run and task
 * snapshots, the durable feed, generic commands and the session routes — and no
 * list or detail projection for pinned profile contents, teams, skills,
 * artifacts, evidence, persona runs, intake receipts, external-workflow
 * inspection or calendar state.
 *
 * Every wire type this console has lives in `api/types.ts` and is generated from
 * the contract document. The rule is unchanged by this file: when KON-MVP-16
 * lands those projections, the generated types gain them, and the only new code
 * is an adapter from the generated type to the prop type below. These shapes are
 * deliberately structural — keys are opaque strings, nothing is enumerated — so
 * that adapter is a mapping and not a translation.
 *
 * # Why they are not populated
 *
 * Every view that needs one of these renders `PendingProjection` until the
 * contract serves it. Filling them from a fixture in the running application
 * would put data on an operator's screen that no realm reported, which is the
 * single thing this console must never do. They are exercised by the headless
 * suite and by nothing else.
 */

/** One role slot a team declares, and what fills it. */
export interface RoleSlot {
  /** The role key. Opaque. */
  readonly role: string
  /** The gates this role may evaluate. Opaque keys. */
  readonly mayEvaluate: readonly string[]
  /** The gates this role may waive. Opaque keys. */
  readonly mayWaive: readonly string[]
  /** The skills the slot declares. Opaque keys. */
  readonly skills: readonly string[]
  /** The run filling the slot, when one is. */
  readonly agentRunId: string | null
  /** That run's derived state, as the realm reported it. */
  readonly runState: string | null
  /** Whether the run is bound to a native session. */
  readonly bindingState: string | null
  /** How old the newest confirmation about it is. */
  readonly freshness: string | null
}

/** One team run's ledger. */
export interface TeamLedgerView {
  /** The team run. */
  readonly teamRunId: string
  /** The pinned template revision the run is executing. */
  readonly templateId: string | null
  /** That template's revision. */
  readonly templateVersion: number | null
  /** The declared slots. */
  readonly slots: readonly RoleSlot[]
}

/** One step a persona scenario declares. */
export interface ScenarioStep {
  /** Its declared order. */
  readonly order: number
  /** What the step instructs. */
  readonly instruction: string
  /** The evidence the step is expected to produce. Opaque keys. */
  readonly expectedEvidence: readonly string[]
  /** Whether that evidence was retained. */
  readonly retained: boolean
}

/** One persona run's evidence. */
export interface PersonaEvidenceView {
  /** The scenario. */
  readonly scenarioId: string
  /** Its pinned revision. */
  readonly version: number | null
  /** The persona the scenario simulates. */
  readonly persona: string
  /** The test identity it runs as. */
  readonly identity: string
  /** Whether that identity was seeded for the test. */
  readonly seeded: boolean
  /** The environment it runs against. */
  readonly environment: string
  /** The steps. */
  readonly steps: readonly ScenarioStep[]
  /** What the scenario is forbidden to do. */
  readonly prohibitedActions: readonly string[]
  /** The gate the scenario exists to exercise. Opaque key. */
  readonly gateUnderTest: string
  /**
   * The role that *performs* the scenario.
   *
   * Rendered apart from the evaluators below, because a persona that graded its
   * own work would be evidence of nothing.
   */
  readonly actorRole: string
  /** The roles with authority over the gate, which the actor is not one of. */
  readonly evaluatorRoles: readonly string[]
}

/** One item in the intake inbox. */
export interface IntakeItem {
  /** The receipt. */
  readonly receiptId: string
  /** Where the item came from. */
  readonly source: string
  /** The key the source was deduplicated on. */
  readonly dedupKey: string
  /** The trigger revision that matched it. */
  readonly triggerId: string | null
  /** That trigger's revision. */
  readonly triggerVersion: number | null
  /** Where approval stands. */
  readonly approvalState: string
  /** When it arrived. */
  readonly receivedAt: string
  /** The work this item created, once it was approved. */
  readonly createdWork: readonly { readonly kind: string; readonly id: string }[]
}

/** One external workflow's inspection. */
export interface WorkflowInspection {
  /** The connector the workflow belongs to. Opaque key. */
  readonly connector: string
  /** The external issue this task is linked to. */
  readonly externalRef: string
  /** The pinned workflow revision in force. */
  readonly workflowId: string | null
  /** That revision. */
  readonly workflowVersion: number | null
  /** What this realm believes, in its own vocabulary. */
  readonly internalFacts: readonly { readonly key: string; readonly value: string }[]
  /** What the external system last reported, and when. */
  readonly latestObservation: {
    readonly status: string
    readonly assignee: string | null
    readonly observedAt: string
  } | null
  /** Who owns the external item, as far as this realm can tell. */
  readonly assigneeOwnership: 'ours' | 'theirs' | 'unassigned' | 'unknown'
  /**
   * What this realm proposes doing about the difference.
   *
   * A proposal, never a control: the inspector shows the transition the pinned
   * revision permits, or that it permits none. There is no free-text status, no
   * assignee picker and no outbound comment box anywhere in this console.
   */
  readonly proposal:
    | { readonly kind: 'noop'; readonly because: string }
    | { readonly kind: 'transition'; readonly to: string; readonly because: string }
    | { readonly kind: 'conflict'; readonly because: string }
  /** The receipt of the last proposal that was acted on. */
  readonly receipt: {
    readonly receiptId: string
    readonly state: string
    readonly attempts: number
  } | null
}

/** One window a calendar opens. */
export interface CalendarWindow {
  /** The day the window falls on, as the calendar spells it. */
  readonly day: string
  /** When it opens, in the calendar's own timezone. */
  readonly from: string
  /** When it closes. */
  readonly to: string
}

/**
 * When work may be dispatched.
 *
 * The unrestricted case is a *variant* rather than an empty calendar, so a view
 * has to render "this realm places no restriction on when work runs" explicitly.
 * An empty window list rendered as blank space says the same thing far too
 * quietly.
 */
export type SchedulingView =
  | { readonly kind: 'unrestricted' }
  | {
      readonly kind: 'calendar'
      /** The calendar profile in force. */
      readonly profileId: string
      /** Its pinned revision. */
      readonly version: number
      /** The timezone every window and exception is stated in. */
      readonly timezone: string
      /** The recurring windows. */
      readonly windows: readonly CalendarWindow[]
      /** The dates the calendar excludes. */
      readonly exceptions: readonly { readonly day: string; readonly reason: string }[]
      /** Whether the realm is draining, and therefore starting nothing new. */
      readonly draining: boolean
      /** An approved override, when one is in force. */
      readonly override: {
        readonly until: string
        readonly approvedBy: string
      } | null
      /**
       * Why the last dispatch decision came out the way it did.
       *
       * Stated by the realm, deterministic, and shown verbatim: an operator
       * asking "why did nothing start" is owed the rule, not a guess.
       */
      readonly lastDecision: {
        readonly admitted: boolean
        readonly explanation: string
        readonly evaluatedAt: string
      } | null
    }
