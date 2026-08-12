/**
 * The phase graph renders shape, not vocabulary.
 *
 * The invariance test is the important one: the rendered DOM, with every
 * identifier substituted, must be *identical* after every identifier in the
 * profile is changed. A component that recognized a phase name — or sorted by
 * one, or styled one specially — fails it.
 */
import { render, screen, within } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { PhaseGraph } from './PhaseGraph'
import {
  CHAIN_PROFILE,
  DIAMOND_PROFILE,
  PROFILES,
  RAGGED_PROFILE,
  renameEverything,
} from '../test/fixtures'

/** Every phase key the rendering placed, in the order it placed them. */
function renderedPhases(container: HTMLElement): string[] {
  return Array.from(container.querySelectorAll('[data-phase-id]')).map(
    (node) => node.getAttribute('data-phase-id') ?? '',
  )
}

describe('<PhaseGraph>', () => {
  it('renders every declared phase and every declared edge', () => {
    const { container } = render(<PhaseGraph spec={DIAMOND_PROFILE} />)
    for (const phase of DIAMOND_PROFILE.phases) {
      expect(container.querySelector(`[data-phase-id="${phase.id}"]`)).not.toBeNull()
    }
    const edges = container.querySelectorAll('.phase-edges li')
    expect(edges).toHaveLength(DIAMOND_PROFILE.edges.length)
  })

  it('renders arbitrary gate and artifact keys as data', () => {
    const { container } = render(<PhaseGraph spec={DIAMOND_PROFILE} />)
    const gated = container.querySelector('[data-phase-id="x3"]') as HTMLElement
    expect(within(gated).getByText('g-88')).toBeInTheDocument()
    expect(within(gated).getByText('g-99')).toBeInTheDocument()
    const requiring = container.querySelector('[data-phase-id="x1"]') as HTMLElement
    expect(within(requiring).getByText('a-1')).toBeInTheDocument()
    expect(within(requiring).getByText('a-2')).toBeInTheDocument()
  })

  it('marks the phase the active workflow is in without knowing which it is', () => {
    const { container } = render(
      <PhaseGraph spec={DIAMOND_PROFILE} currentPhase="x2" />,
    )
    const current = container.querySelector('[data-current="true"]')
    expect(current?.getAttribute('data-phase-id')).toBe('x2')
    expect(current).toHaveAttribute('aria-current', 'step')
  })

  it('shows a rejection route as a returning transition', () => {
    const { container } = render(<PhaseGraph spec={DIAMOND_PROFILE} />)
    const returning = container.querySelector('[data-direction="return"]')
    expect(returning?.getAttribute('data-from')).toBe('x3')
    expect(returning?.getAttribute('data-to')).toBe('x1')
  })

  it('shows an edge naming a phase the profile never declared', () => {
    const { container } = render(<PhaseGraph spec={RAGGED_PROFILE} />)
    const dangling = container.querySelector('[data-direction="dangling"]')
    expect(dangling?.getAttribute('data-to')).toBe('nowhere-at-all')
    expect(within(dangling as HTMLElement).getByText(/undeclared phase/)).toBeInTheDocument()
  })

  it('shows a phase nothing reaches rather than dropping it', () => {
    const { container } = render(
      <PhaseGraph spec={{ entry: 'a', phases: [{ id: 'a' }, { id: 'orphan' }], edges: [] }} />,
    )
    const unreachable = container.querySelector('.phase-unreachable')
    expect(within(unreachable as HTMLElement).getByText('orphan')).toBeInTheDocument()
  })

  it.each(PROFILES.map((spec, index) => [index, spec] as const))(
    'renders profile %i identically when every identifier in it is renamed',
    (_index, spec) => {
      const before = render(<PhaseGraph spec={spec} />)
      const original = before.container.innerHTML
      const placed = renderedPhases(before.container)
      before.unmount()

      const { spec: renamed, rename } = renameEverything(spec)
      const after = render(<PhaseGraph spec={renamed} />)

      // Substituting the new names back gives byte-identical markup, so nothing
      // about the rendering depended on what anything was called.
      let substituted = after.container.innerHTML
      for (const phase of spec.phases) {
        substituted = substituted.split(rename(phase.id)).join(phase.id)
        for (const key of [...(phase.gates ?? []), ...(phase.requiredArtifacts ?? [])]) {
          substituted = substituted.split(rename(key)).join(key)
        }
        if (phase.label) {
          substituted = substituted.split(rename(phase.label)).join(phase.label)
        }
      }
      for (const edge of spec.edges) {
        if (edge.handoffRole) {
          substituted = substituted.split(rename(edge.handoffRole)).join(edge.handoffRole)
        }
        substituted = substituted.split(rename(edge.to)).join(edge.to)
      }

      expect(substituted).toBe(original)
      expect(renderedPhases(after.container).map((id) => id)).toEqual(
        placed.map((id) => rename(id)),
      )
    },
  )

  it('says so when a profile revision declares no phases', () => {
    render(<PhaseGraph spec={{ phases: [], edges: [] }} />)
    expect(screen.getByText(/declares no phases/)).toBeInTheDocument()
  })

  it('renders a chain in entry-to-terminal order', () => {
    const { container } = render(<PhaseGraph spec={CHAIN_PROFILE} />)
    expect(renderedPhases(container)).toEqual(['p-9k', 'p-4d', 'p-2c'])
  })
})
