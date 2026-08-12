/**
 * The shell: two layouts, one keyboard, and a bar that never lies about how
 * current the screen is.
 *
 * The mutants this file exists to kill: a narrow layout that leaves two fixed
 * panels over each other, a drawer that cannot be dismissed, a rail that only
 * works with a mouse, and a top bar that shows an interrupted feed as a healthy
 * one.
 */
import { fireEvent, render, screen, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { NavRail, VIEWS } from './NavRail'
import { MasterDetail } from './MasterDetail'
import { TopBar } from './TopBar'
import { setViewport } from '../test/viewport'
import { REALM, controlEvent } from '../test/fixtures'
import { applyEvent, feedInterrupted, initialControlState, resnapshotRequired } from '../state/control'
import type { Health, Realm } from '../api/types'

/** The endpoint the bar is told about. */
const ENDPOINT = { baseUrl: 'http://127.0.0.1:7777', token: 'secret' }

/** One realm identity. */
const IDENTITY: Realm = {
  realm_id: REALM,
  schema_version: 1,
  created_at: '2026-08-01T00:00:00Z',
  display_label: 'a realm',
}

/** One health answer. */
function health(overrides: Partial<Health> = {}): Health {
  return {
    realm_id: REALM,
    live: true,
    schema_version: 1,
    reconciliation: 'open' as Health['reconciliation'],
    scheduling_open: true,
    runtimes: ['k-1'],
    ...overrides,
  }
}

describe('<NavRail>', () => {
  it('marks the view on screen and moves between views with the arrow keys', () => {
    const onSelect = vi.fn()
    render(<NavRail current="board" onSelect={onSelect} />)

    const current = screen.getByRole('button', { name: 'Board' })
    expect(current).toHaveAttribute('aria-current', 'page')
    // One tab stop for the whole rail; the arrows move within it.
    expect(current).toHaveAttribute('tabindex', '0')
    expect(screen.getByRole('button', { name: 'Task' })).toHaveAttribute('tabindex', '-1')

    fireEvent.keyDown(screen.getByRole('navigation'), { key: 'ArrowDown' })
    expect(onSelect).toHaveBeenCalledWith('Task'.toLowerCase())
  })

  it('wraps at both ends rather than dead-ending', () => {
    const onSelect = vi.fn()
    render(<NavRail current="board" onSelect={onSelect} />)
    fireEvent.keyDown(screen.getByRole('navigation'), { key: 'ArrowUp' })
    expect(onSelect).toHaveBeenCalledWith(VIEWS[VIEWS.length - 1]?.id)
  })

  it('offers every view', () => {
    render(<NavRail current="board" onSelect={() => {}} />)
    for (const view of VIEWS) {
      expect(screen.getByRole('button', { name: view.label })).toBeInTheDocument()
    }
  })
})

describe('<MasterDetail>', () => {
  it('puts both panels on the page on a wide viewport', () => {
    setViewport('desktop')
    const { container } = render(
      <MasterDetail
        master={<p>the list</p>}
        detail={<p>the detail</p>}
        detailLabel="detail"
        open
        onClose={() => {}}
      />,
    )
    expect(container.querySelector('[data-layout="wide"]')).not.toBeNull()
    expect(screen.getByText('the list')).toBeInTheDocument()
    expect(screen.getByText('the detail')).toBeInTheDocument()
    // No dialog on a wide screen: nothing is covering anything.
    expect(screen.queryByRole('dialog')).toBeNull()
  })

  it('shows only the list on a narrow viewport until something is selected', () => {
    setViewport('phone')
    const { container } = render(
      <MasterDetail
        master={<p>the list</p>}
        detail={<p>the detail</p>}
        detailLabel="detail"
        open={false}
        onClose={() => {}}
      />,
    )
    expect(container.querySelector('[data-layout="narrow"]')).not.toBeNull()
    expect(screen.getByText('the list')).toBeInTheDocument()
    expect(screen.queryByText('the detail')).toBeNull()
  })

  it('opens the detail as a modal drawer with the list behind it inert', () => {
    setViewport('phone')
    const { container } = render(
      <MasterDetail
        master={<p>the list</p>}
        detail={<p>the detail</p>}
        detailLabel="run detail"
        open
        onClose={() => {}}
      />,
    )
    const drawer = screen.getByRole('dialog', { name: 'run detail' })
    expect(drawer).toHaveAttribute('aria-modal', 'true')
    expect(within(drawer).getByText('the detail')).toBeInTheDocument()
    // Exactly one fixed panel at a time: the list underneath takes neither focus
    // nor a click while the drawer is up.
    expect(container.querySelector('.master')).toHaveAttribute('inert')
  })

  it('dismisses the drawer with Escape and with the close control', () => {
    setViewport('phone')
    const onClose = vi.fn()
    render(
      <MasterDetail
        master={<p>the list</p>}
        detail={<p>the detail</p>}
        detailLabel="detail"
        open
        onClose={onClose}
      />,
    )
    fireEvent.keyDown(document, { key: 'Escape' })
    expect(onClose).toHaveBeenCalledTimes(1)
    fireEvent.click(screen.getByRole('button', { name: 'Close' }))
    expect(onClose).toHaveBeenCalledTimes(2)
  })

  it('moves focus into the drawer when it opens', () => {
    setViewport('phone')
    render(
      <MasterDetail
        master={<p>the list</p>}
        detail={<p>the detail</p>}
        detailLabel="detail"
        open
        onClose={() => {}}
      />,
    )
    expect(document.activeElement).toBe(screen.getByRole('dialog'))
  })
})

describe('<TopBar>', () => {
  beforeEach(() => setViewport('desktop'))

  it('names the realm, the endpoint and where the endpoint lives', () => {
    render(<TopBar endpoint={ENDPOINT} realm={IDENTITY} health={health()} control={null} />)
    expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('a realm')
    expect(screen.getByText(REALM)).toBeInTheDocument()
    expect(screen.getByText('loopback')).toBeInTheDocument()
    expect(screen.getByText(ENDPOINT.baseUrl)).toBeInTheDocument()
  })

  it('labels the endpoint locality as this console’s own, not the realm’s claim', () => {
    // `RealmDto` carries no locality, so presenting it as realm-reported would
    // claim a field the contract does not have.
    render(<TopBar endpoint={ENDPOINT} realm={IDENTITY} health={health()} control={null} />)
    expect(screen.getByText(/from this console’s configuration/)).toBeInTheDocument()
  })

  it('warns about an endpoint that is not loopback', () => {
    render(
      <TopBar
        endpoint={{ baseUrl: 'https://example.test', token: 't' }}
        realm={IDENTITY}
        health={health()}
        control={null}
      />,
    )
    expect(screen.getByText('not_loopback')).toBeInTheDocument()
  })

  it('shows reconciliation and whether scheduling is open', () => {
    render(
      <TopBar
        endpoint={ENDPOINT}
        realm={IDENTITY}
        health={health({
          reconciliation: 'pending' as Health['reconciliation'],
          scheduling_open: false,
        })}
        control={null}
      />,
    )
    expect(screen.getByText('pending')).toBeInTheDocument()
    expect(screen.getByText('shut')).toBeInTheDocument()
  })

  it('shows the newest position and that the feed is being followed', () => {
    const control = applyEvent(initialControlState(REALM), controlEvent({ cursor: 77 })).state
    render(<TopBar endpoint={ENDPOINT} realm={IDENTITY} health={health()} control={control} />)
    expect(screen.getByText('77')).toBeInTheDocument()
    expect(screen.getByText(/following the realm/)).toBeInTheDocument()
  })

  it('says plainly when nothing is being updated', () => {
    const control = feedInterrupted(
      applyEvent(initialControlState(REALM), controlEvent({ cursor: 5 })).state,
    )
    const { container } = render(
      <TopBar endpoint={ENDPOINT} realm={IDENTITY} health={health()} control={control} />,
    )
    expect(container.querySelector('[data-banner="interrupted"]')).not.toBeNull()
    expect(screen.getByText(/Nothing here is being updated/)).toBeInTheDocument()
  })

  it('says plainly when the realm discarded the position it was reading from', () => {
    const control = resnapshotRequired(initialControlState(REALM))
    const { container } = render(
      <TopBar endpoint={ENDPOINT} realm={IDENTITY} health={health()} control={control} />,
    )
    expect(container.querySelector('[data-banner="resnapshot"]')).not.toBeNull()
    expect(screen.getByText(/no longer retains the position/)).toBeInTheDocument()
  })
})
