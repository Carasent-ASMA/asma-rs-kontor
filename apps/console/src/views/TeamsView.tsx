/**
 * Teams: what a team template declares, and what each seat in it may be.
 *
 * # What this view is, and what it is not
 *
 * Every other view in this console renders what `/v1` answered. This one renders
 * a draft held in this tab, against a fixture catalog, because the contract
 * serves neither team-template writes nor a model catalog yet. That is a
 * different kind of screen from the rest of the console, so it says so in a
 * banner rather than looking like the others and quietly meaning something else.
 *
 * Nothing here is saved, nothing is published, and no number below has been
 * verified against a provider.
 *
 * # Why the editor constrains rather than warns
 *
 * A provider select narrows the model select; the model select narrows the
 * effort select and disables it entirely on a route with no effort lever. The
 * rules that cannot be expressed as a narrowed control — rung 2 crossing
 * providers, a pooled provider under itself, a class the model cannot hold, a
 * need band nothing covers — are reported as issues against the slot, blocking
 * or notice, and never silently repaired. An editor that quietly fixed a
 * declaration would be teaching an operator that the declaration did not matter.
 *
 * @see ../state/teams.ts for every rule this file renders
 */
import { useRef, useState } from 'react'
import { MasterDetail } from '../shell/MasterDetail'
import { StateBadge } from '../components/primitives'
import {
  CONTEXT_CLASSES,
  FIXTURE_CATALOG,
  MAX_RUNGS,
  SEED_TEAMS,
  UNVERIFIED,
  blocks,
  modelById,
  modelForRung,
  modelsOf,
  publishTeamRevision,
  reviewSeat,
  validateTeam,
  validateCatalog,
  type ChargingBasis,
  type ContextClass,
  type ContextEnforcement,
  type Issue,
  type ModelCatalog,
  type ModelEntry,
  type ModelRung,
  type Provenance,
  type RankedClassCost,
  type RungEffort,
  type SeatCapabilities,
  type TeamDraft,
  type TeamSlot,
  type TeamRevision,
} from '../state/teams'

/** Render the Teams section. */
export function TeamsView({
  catalog = FIXTURE_CATALOG,
  seed = SEED_TEAMS,
}: {
  /** The catalog every control is constrained against. */
  catalog?: ModelCatalog
  /** The drafts the section opens with. */
  seed?: readonly TeamDraft[]
}) {
  const catalogIssues = validateCatalog(catalog)
  const [drafts, setDrafts] = useState<readonly TeamDraft[]>(seed)
  const [selected, setSelected] = useState<string | null>(null)
  const [revisions, setRevisions] = useState<readonly TeamRevision[]>([])
  if (blocks(catalogIssues)) {
    return (
      <section className="view" aria-label="teams">
        <h2>Teams</h2>
        <p className="banner" role="alert" data-banner="catalog-refused">
          The realm catalog was refused at the <code>/v1/catalog</code> trust boundary.
        </p>
        <IssueList issues={catalogIssues} label="catalog issues" />
      </section>
    )
  }
  const draft = drafts.find((candidate) => candidate.id === selected) ?? null

  /** Replace one draft with an edited copy of itself. */
  const edit = (id: string, change: (draft: TeamDraft) => TeamDraft): void => {
    setDrafts((current) =>
      current.map((candidate) => (candidate.id === id ? change(candidate) : candidate)),
    )
  }

  return (
    <section className="view" aria-label="teams">
      <h2>Teams</h2>
      <p className="banner" role="note" data-banner="fixture">
        Nothing on this screen came from the realm. This is a prototype over a
        fixture catalog, and <strong>every cell states its own provenance</strong>{' '}
        rather than the banner claiming one for all of them: a context ceiling, an
        effort ladder, a charging basis, a price and a need band each read{' '}
        <code>live</code> only where a runtime call returned that value under a
        recorded gate reference, and <code>fixture/needs-verification</code>{' '}
        everywhere else — which is almost everywhere. No price is established, so
        cost figures are withheld rather than printed, and the class
        recommendation rests on coverage instead. Published revisions are
        immutable snapshots; realm persistence is supplied by the API adapter.
      </p>

      <MasterDetail
        detailLabel="team template"
        open={draft !== null}
        onClose={() => setSelected(null)}
        master={
          <TeamList
            drafts={drafts}
            catalog={catalog}
            selected={selected}
            onSelect={setSelected}
          />
        }
        detail={
          draft ? (
            <TeamEditor
              draft={draft}
              catalog={catalog}
              onRename={(name) => edit(draft.id, (current) => ({ ...current, name }))}
              onSlotChange={(slotId, capabilities) =>
                edit(draft.id, (current) => ({
                  ...current,
                  slots: current.slots.map((slot) =>
                    slot.id === slotId ? { ...slot, capabilities } : slot,
                  ),
                }))
              }
              onPublish={() =>
                setRevisions((current) => [...current, publishTeamRevision(current, draft)])
              }
            />
          ) : (
            <p className="empty">Select a team template.</p>
          )
        }
      />
      <section aria-label="published team revisions">
        <h3>Published revisions</h3>
        {revisions.length === 0 ? (
          <p className="empty">No revisions published.</p>
        ) : (
          <ul aria-label="published team revisions">
            {revisions.map((revision) => (
              <li key={`${revision.id}:${revision.version}`}>
                {revision.name} · v{revision.version}
              </li>
            ))}
          </ul>
        )}
      </section>
    </section>
  )
}

/** The drafts this tab holds, with what is wrong with each. */
function TeamList({
  drafts,
  catalog,
  selected,
  onSelect,
}: {
  drafts: readonly TeamDraft[]
  catalog: ModelCatalog
  selected: string | null
  onSelect: (id: string) => void
}) {
  const list = useRef<HTMLUListElement>(null)

  /** Arrow keys move through the list, as they do in the rail and the board. */
  const onKeyDown = (event: React.KeyboardEvent<HTMLUListElement>): void => {
    const step = event.key === 'ArrowDown' ? 1 : event.key === 'ArrowUp' ? -1 : 0
    if (step === 0 || drafts.length === 0) {
      return
    }
    event.preventDefault()
    const index = drafts.findIndex((draft) => draft.id === selected)
    const next = drafts[(Math.max(index, 0) + step + drafts.length) % drafts.length]
    if (!next) {
      return
    }
    onSelect(next.id)
    list.current?.querySelector<HTMLButtonElement>(`[data-draft-id="${next.id}"]`)?.focus()
  }

  return (
    <div className="team-list">
      <h3>Team templates</h3>
      <p className="caveat">
        Prototype drafts, not revisions this realm has published. The contract
        serves no template list, so these are seeded fixtures mirroring the
        chains the fleet runs today.
      </p>
      {drafts.length === 0 ? (
        <p className="empty">No drafts.</p>
      ) : (
        <ul ref={list} onKeyDown={onKeyDown} aria-label="team templates">
          {drafts.map((draft) => {
            const blocking = blockingCount(draft, catalog)
            const isSelected = draft.id === selected
            return (
              <li key={draft.id}>
                <button
                  type="button"
                  data-draft-id={draft.id}
                  aria-current={isSelected ? 'true' : undefined}
                  tabIndex={
                    isSelected || (selected === null && drafts[0]?.id === draft.id) ? 0 : -1
                  }
                  onClick={() => onSelect(draft.id)}
                >
                  <span className="team-name">{draft.name}</span>
                  <span className="badge" data-state="slots">
                    {`${draft.slots.length} slots`}
                  </span>
                  {blocking === 0 ? null : (
                    <span className="badge" data-state="blocking">
                      {`${blocking} blocking`}
                    </span>
                  )}
                </button>
              </li>
            )
          })}
        </ul>
      )}
    </div>
  )
}

/** One draft: its name, what is wrong with it as a template, and its slots. */
function TeamEditor({
  draft,
  catalog,
  onRename,
  onSlotChange,
  onPublish,
}: {
  draft: TeamDraft
  catalog: ModelCatalog
  onRename: (name: string) => void
  onSlotChange: (slotId: string, capabilities: SeatCapabilities) => void
  onPublish: () => void
}) {
  const nameId = `team-name-${slug(draft.id)}`
  return (
    <article className="team-editor" data-draft-id={draft.id}>
      <div className="field">
        <label htmlFor={nameId}>Team name</label>
        <input
          id={nameId}
          value={draft.name}
          onChange={(event) => onRename(event.target.value)}
        />
      </div>

      <IssueList issues={validateTeam(draft)} label="template issues" />

      <button
        type="button"
        disabled={blockingCount(draft, catalog) > 0}
        onClick={onPublish}
      >
        Publish next revision
      </button>

      <h3>Slots</h3>
      {draft.slots.length === 0 ? (
        <p className="empty">This draft declares no slots.</p>
      ) : (
        draft.slots.map((slot) => (
          <SeatCapabilityEditor
            key={slot.id}
            slot={slot}
            catalog={catalog}
            onChange={(capabilities) => onSlotChange(slot.id, capabilities)}
          />
        ))
      )}
    </article>
  )
}

/** One slot's capabilities, constrained against the catalog. */
export function SeatCapabilityEditor({
  slot,
  catalog,
  onChange,
}: {
  /** The slot being edited. */
  slot: TeamSlot
  /** What the controls may offer. */
  catalog: ModelCatalog
  /** Hand back the edited capabilities. */
  onChange: (capabilities: SeatCapabilities) => void
}) {
  const seat = slot.capabilities
  const review = reviewSeat(seat, catalog)
  const key = slug(slot.id)

  /** Replace one rung. */
  const setRung = (index: number, rung: ModelRung): void => {
    onChange({
      ...seat,
      chain: seat.chain.map((current, position) => (position === index ? rung : current)),
    })
  }

  /** Append a rung, preferring a provider the rung above did not use. */
  const addRung = (): void => {
    const last = seat.chain[seat.chain.length - 1]
    const crossing = catalog.models.find((model) => model.provider !== last?.provider)
    const chosen = crossing ?? catalog.models[0]
    if (!chosen) {
      return
    }
    onChange({
      ...seat,
      chain: [
        ...seat.chain,
        {
          provider: chosen.provider,
          model: chosen.id,
          effort: reconcileEffort('unset', chosen),
        },
      ],
    })
  }

  /** Drop one rung. */
  const removeRung = (index: number): void => {
    onChange({ ...seat, chain: seat.chain.filter((_, position) => position !== index) })
  }

  return (
    <article className="seat-editor" data-slot={slot.id}>
      <header>
        <h4>
          <code>{slot.id}</code>
        </h4>
        {/* Derived from rung 1, never set here: a seat that starts on a degraded
            lane does its work and reports it, and something else passes the
            judgement. */}
        <StateBadge
          state={review.canVerdict ? 'verdict_capable' : 'cannot_verdict'}
          label="verdict capability"
        />
      </header>

      <section className="chain" aria-label={`${slot.id} model chain`}>
        <h5>Model chain</h5>
        <ol className="chain-rows">
          {seat.chain.map((rung, index) => (
            <ChainRow
              key={`${key}-rung-${index}`}
              idPrefix={`${key}-rung-${index}`}
              rung={rung}
              index={index}
              catalog={catalog}
              removable={seat.chain.length > 1}
              onChange={(next) => setRung(index, next)}
              onRemove={() => removeRung(index)}
            />
          ))}
        </ol>
        {seat.chain.length < MAX_RUNGS ? (
          <button type="button" onClick={addRung}>
            Add rung
          </button>
        ) : (
          <p className="hint">{`A chain is at most ${MAX_RUNGS} rungs.`}</p>
        )}
      </section>

      <section className="context" aria-label={`${slot.id} context`}>
        <h5>Context</h5>
        <div className="field-row">
          <div className="field">
            <label htmlFor={`${key}-class`}>Context class</label>
            <select
              id={`${key}-class`}
              value={seat.context.class}
              onChange={(event) =>
                onChange({
                  ...seat,
                  context: { ...seat.context, class: event.target.value as ContextClass },
                })
              }
            >
              {CONTEXT_CLASSES.map((entry) => (
                <option key={entry.id} value={entry.id}>
                  {entry.id}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label htmlFor={`${key}-enforcement`}>Enforcement</label>
            <select
              id={`${key}-enforcement`}
              value={seat.context.enforcement}
              onChange={(event) =>
                onChange({
                  ...seat,
                  context: {
                    ...seat.context,
                    enforcement: event.target.value as ContextEnforcement,
                  },
                })
              }
            >
              <option value="best_effort">best_effort</option>
              <option value="required">required</option>
            </select>
          </div>
        </div>

        {review.leadModel ? (
          <ClassTable
            model={review.leadModel}
            basis={review.basis}
            basisProvenance={review.basisProvenance}
            costs={review.costs}
            selected={seat.context.class}
          />
        ) : (
          <p className="empty">
            Rung 1 names no model the catalog serves, so no class can be resolved
            against a ceiling.
          </p>
        )}
        <p className="hint" data-resolved-policy>
          Resolved policy: class {seat.context.class}; source role_slot; effective{' '}
          {review.resolution?.effective ?? 'native'}; enforcement {seat.context.enforcement}; capability{' '}
          {review.resolution?.capability ?? 'unknown'}; latest receipt {seat.latestReceipt ?? 'none'}.
        </p>
      </section>

      <section className="need" aria-label={`${slot.id} need band`}>
        <h5>Need band</h5>
        <div className="field-row">
          <div className="field">
            <label htmlFor={`${key}-need`}>Working set needed (tokens)</label>
            <input
              id={`${key}-need`}
              type="number"
              min={0}
              step={1000}
              value={seat.need.minTokens}
              onChange={(event) =>
                onChange({
                  ...seat,
                  need: { ...seat.need, minTokens: Number(event.target.value) },
                })
              }
            />
            {/* This number drives a blocking rule, so its telemetry provenance
                is shown as plainly as the model ceiling's provenance. */}
            <StateBadge
              state={seat.need.provenance.state}
              label={`${slot.id} need band provenance`}
            />
          </div>
          <div className="field grow">
            <label htmlFor={`${key}-rationale`}>Why this band</label>
            <input
              id={`${key}-rationale`}
              value={seat.need.rationale ?? ''}
              onChange={(event) =>
                onChange({
                  ...seat,
                  need: { ...seat.need, rationale: event.target.value || null },
                })
              }
            />
          </div>
          <div className="field">
            <label htmlFor={`${key}-task-minimum`}>Task minimum (tokens)</label>
            <input
              id={`${key}-task-minimum`}
              type="number"
              min={0}
              step={1000}
              value={seat.taskMinimum?.minTokens ?? ''}
              onChange={(event) =>
                onChange({
                  ...seat,
                  taskMinimum: event.target.value === ''
                    ? undefined
                    : { minTokens: Number(event.target.value), rationale: 'Task-declared minimum', provenance: UNVERIFIED },
                })
              }
            />
          </div>
        </div>
        <p className="recommendation" data-recommended={review.recommended ?? 'none'}>
          {review.recommended === null
            ? 'No class covers this band on the rung 1 model, so there is nothing to recommend. Native is an explicit escape, not an upgrade this editor makes for you.'
            : `Smallest covering class for this band: ${review.recommended}.`}
        </p>
        <p className="hint">Effective need source: {review.needSource}.</p>
      </section>

      <section className="authority" aria-label={`${slot.id} authority`}>
        <h5>Authority</h5>
        <Chips label="skills" keys={seat.skills} />
        <Chips label="may evaluate" keys={seat.mayEvaluate} />
        <Chips label="may waive" keys={seat.mayWaive} />
      </section>

      <IssueList issues={review.issues} label={`${slot.id} issues`} />
    </article>
  )
}

/** One rung: provider, then the models on it, then the efforts that model exposes. */
function ChainRow({
  idPrefix,
  rung,
  index,
  catalog,
  removable,
  onChange,
  onRemove,
}: {
  idPrefix: string
  rung: ModelRung
  index: number
  catalog: ModelCatalog
  removable: boolean
  onChange: (rung: ModelRung) => void
  onRemove: () => void
}) {
  const model = modelForRung(catalog, rung)
  const onProvider = catalog.providers.some((provider) => provider.id === rung.provider)
    ? modelsOf(catalog, rung.provider)
    : []
  // A route with no effort lever is not "effort: low", it is a control that does
  // not apply — so the select is disabled and shows the declaration that says so.
  const efforts: readonly RungEffort[] =
    model && model.efforts.value.length > 0
      ? rung.effort === 'unset'
        ? ['unset', ...model.efforts.value]
        : model.efforts.value
      : ['unset']

  return (
    <li className="chain-row" data-rung={index + 1} data-provider={rung.provider}>
      <span className="rung-number">{`Rung ${index + 1}`}</span>

      <div className="field">
        <label htmlFor={`${idPrefix}-provider`}>Provider</label>
        <select
          id={`${idPrefix}-provider`}
          value={rung.provider}
          onChange={(event) => {
            const provider = event.target.value
            const next = modelsOf(catalog, provider)[0]
            onChange({
              provider,
              model: next?.id ?? '',
              effort: reconcileEffort(rung.effort, next),
            })
          }}
        >
          {catalog.providers.map((provider) => (
            <option key={provider.id} value={provider.id}>
              {provider.label}
            </option>
          ))}
        </select>
      </div>

      <div className="field">
        <label htmlFor={`${idPrefix}-model`}>Model</label>
        <select
          id={`${idPrefix}-model`}
          value={rung.model}
          onChange={(event) => {
            const next = modelById(catalog, rung.provider, event.target.value)
            onChange({
              ...rung,
              model: event.target.value,
              effort: reconcileEffort(rung.effort, next),
            })
          }}
        >
          {onProvider.length === 0 ? <option value={rung.model}>{rung.model}</option> : null}
          {onProvider.map((entry) => (
            <option key={entry.id} value={entry.id}>
              {entry.label}
            </option>
          ))}
        </select>
      </div>

      <div className="field">
        <label htmlFor={`${idPrefix}-effort`}>Effort</label>
        <select
          id={`${idPrefix}-effort`}
          value={rung.effort}
          disabled={!model || model.efforts.value.length === 0}
          onChange={(event) => onChange({ ...rung, effort: event.target.value as RungEffort })}
        >
          {efforts.map((effort) => (
            <option key={effort} value={effort}>
              {effort}
            </option>
          ))}
        </select>
        {/* The ladder decides which pins the editor will accept, so a wrong one
            refuses a real capability. It says where it came from for the same
            reason the ceiling does. */}
        {model ? (
          <StateBadge
            state={model.efforts.provenance.state}
            label={`rung ${index + 1} effort ladder provenance`}
          />
        ) : null}
      </div>

      {/* The ceiling carries its provenance wherever it is shown. It drives the
          clamp state and the blocking need rule, so an unverified number here
          refuses a publish with the authority of a catalog read unless the
          screen says which it is. */}
      <span className="rung-window" data-window-state={model?.contextWindow.provenance.state}>
        {model ? (
          <>
            {model.contextWindow.value === null
              ? 'window not established'
              : `window ${model.contextWindow.value}`}{' '}
            <StateBadge
              state={model.contextWindow.provenance.state}
              label={`rung ${index + 1} ceiling provenance`}
            />
          </>
        ) : (
          'not in catalog'
        )}
      </span>

      {removable ? (
        <button type="button" onClick={onRemove}>
          {`Remove rung ${index + 1}`}
        </button>
      ) : null}
    </li>
  )
}

/**
 * Every class against the rung 1 model: what it asks for, what it gets, and what
 * filling it costs.
 *
 * # Why the cost column is input-only
 *
 * Choosing a class chooses how much context accumulates before the seat
 * compacts. It does not choose how much the model writes back, so no output term
 * belongs in a comparison between classes. See `classCost`.
 *
 * # Why a price can be withheld, for two different reasons
 *
 * A dollar is shown only when both questions come out right.
 *
 * *What does this seat spend?* — on a plan, a wider context costs no money at
 * all; it spends a share of an allowance already bought, and past the allowance
 * it stops being available rather than getting expensive. A provider's published
 * per-token rate can be perfectly accurate and still be the wrong quantity to
 * put in front of an operator sizing a seat. So on anything but a metered
 * provider this column names what is actually consumed instead.
 *
 * *Where did this number come from?* — while a step is still
 * `fixture/needs-verification` the cell says so rather than printing a plausible
 * amount, because nobody can tell a researched number from an invented one by
 * looking at it, and this is the one column in the console where a wrong number
 * becomes a budget. The ratio is withheld on the same grounds twice over: a
 * multiple of an invented figure is invented, and a multiple of the wrong
 * quantity is still the wrong quantity.
 */
function ClassTable({
  model,
  basis,
  basisProvenance,
  costs,
  selected,
}: {
  model: ModelEntry
  basis: ChargingBasis | null
  basisProvenance: Provenance | null
  costs: readonly RankedClassCost[]
  selected: ContextClass
}) {
  const metered = basis === 'metered'
  return (
    <table className="class-table">
      <caption>
        {`Against ${model.label}${
          model.contextWindow.value === null
            ? ', whose ceiling nothing establishes'
            : ` (ceiling ${model.contextWindow.value}, ${model.contextWindow.provenance.state})`
        }. ${
          metered
            ? 'Tokens here deduct from a balance, so the cost of filling the effective threshold is money.'
            : `A wider context here does not cost money — it spends ${basisPhrase(basis)}, so no dollar figure is shown.`
        } Output is a property of the seat's work, not of its context class, so it is not costed either way.`}{' '}
        {basisProvenance ? (
          <>
            {'Charging basis: '}
            <StateBadge state={basis ?? undefined} label="charging basis" />{' '}
            <StateBadge state={basisProvenance.state} label="charging basis provenance" />
          </>
        ) : null}
      </caption>
      <thead>
        <tr>
          <th scope="col">Class</th>
          <th scope="col">Trigger target</th>
          <th scope="col">Effective</th>
          <th scope="col">State</th>
          <th scope="col">Cost per request at threshold</th>
          <th scope="col">Relative</th>
        </tr>
      </thead>
      <tbody>
        {costs.map((row) => {
          const unverified = row.tier?.provenance.state === 'fixture/needs-verification'
          const priced = !metered
            ? 'not-money'
            : row.tier === null
              ? 'none'
              : unverified
                ? 'unverified'
                : 'sourced'
          return (
            <tr
              key={row.class}
              data-class={row.class}
              data-capability={row.resolution.capability}
              data-priced={priced}
              data-selected={row.class === selected ? 'true' : undefined}
            >
              <th scope="row">{row.class}</th>
              <td>
                {row.resolution.requested === null ? 'runtime default' : row.resolution.requested}
              </td>
              <td>{row.resolution.effective === null ? 'not reported' : row.resolution.effective}</td>
              <td>
                <StateBadge state={row.resolution.capability} label={`${row.class} capability`} />
              </td>
              <td>
                {!metered ? (
                  <StateBadge state={basis ?? undefined} label={`${row.class} charging basis`} />
                ) : row.inputUsd === null || row.tier === null ? (
                  <span className="empty">no priced tier</span>
                ) : unverified ? (
                  <>
                    <span className="empty">price not verified</span>{' '}
                    <StateBadge
                      state={row.tier.provenance.state}
                      label={`${row.class} price source`}
                    />
                  </>
                ) : (
                  <>
                    {`$${row.inputUsd.toFixed(2)}`}{' '}
                    <StateBadge
                      state={row.tier.provenance.state}
                      label={`${row.class} price source`}
                    />
                  </>
                )}
              </td>
              <td>
                {row.relative === null || unverified || !metered ? (
                  <span className="empty">—</span>
                ) : (
                  `${row.relative.toFixed(2)}x`
                )}
              </td>
            </tr>
          )
        })}
      </tbody>
    </table>
  )
}

/** Everything wrong with a declaration, blocking first. */
function IssueList({ issues, label }: { issues: readonly Issue[]; label: string }) {
  if (issues.length === 0) {
    return (
      <p className="empty" data-issues={label}>
        Nothing to report.
      </p>
    )
  }
  const ordered = [...issues].sort((left, right) =>
    left.severity === right.severity ? 0 : left.severity === 'blocking' ? -1 : 1,
  )
  return (
    <ul className="issues" aria-label={label}>
      {ordered.map((issue, index) => (
        <li
          key={`${issue.code}-${issue.rung ?? 0}-${index}`}
          data-severity={issue.severity}
          data-code={issue.code}
        >
          <span className="badge" data-state={issue.severity}>
            {issue.severity}
          </span>
          <code>{issue.code}</code>
          {issue.rung === undefined ? null : (
            <span className="issue-rung">{`rung ${issue.rung}`}</span>
          )}
          <span className="issue-message">{issue.message}</span>
        </li>
      ))}
    </ul>
  )
}

/** One set of opaque authority keys, rendered as they arrived. */
function Chips({ label, keys }: { label: string; keys: readonly string[] }) {
  return (
    <p className="authority-row" data-authority={label}>
      <span className="authority-label">{label}</span>
      {keys.length === 0 ? (
        <span className="empty">none declared</span>
      ) : (
        keys.map((entry) => (
          <span key={entry} className="phase-chip" data-kind={label}>
            {entry}
          </span>
        ))
      )}
    </p>
  )
}

/**
 * The effort a rung carries once its model changed.
 *
 * Keep what was pinned when the new route still exposes it; force `unset` when
 * the route exposes nothing; otherwise take the route's own lowest exposed
 * level, which is a visible choice the operator can change rather than a silent
 * escalation of quota spend.
 */
function reconcileEffort(current: RungEffort, model: ModelEntry | undefined): RungEffort {
  if (!model) {
    return current
  }
  if (model.efforts.value.length === 0) {
    return 'unset'
  }
  if (current !== 'unset' && model.efforts.value.includes(current)) {
    return current
  }
  return model.efforts.value[0] ?? 'unset'
}

/** How many blocking issues one draft has, template and slots together. */
function blockingCount(draft: TeamDraft, catalog: ModelCatalog): number {
  const issues = [
    ...validateTeam(draft),
    ...draft.slots.flatMap((slot) =>
      reviewSeat(slot.capabilities, catalog).issues.map((issue) => ({ ...issue, slot: slot.id })),
    ),
  ]
  return new Set(
    issues
      .filter((issue) => issue.severity === 'blocking')
      .map((issue) => `${issue.code}\u0000${issue.slot ?? ''}`),
  ).size
}

/** A stable id fragment for one control. */
function slug(text: string): string {
  return text.toLowerCase().replace(/[^a-z0-9]+/g, '-')
}

/** What a non-metered provider spends, in words a caption can use. */
function basisPhrase(basis: ChargingBasis | null): string {
  switch (basis) {
    case 'plan_allowance':
      return 'a share of a plan allowance that is already paid for'
    case 'included_usage':
      return 'usage included in a plan, at token rates, until that plan runs out'
    case 'request_quota':
      return 'requests out of a capped daily allowance, not tokens'
    default:
      return 'something the catalog does not describe'
  }
}
