/**
 * Answering a permission, once.
 *
 * The mutants this file exists to kill: offering a decision on a request the
 * transcript already resolved, sending a second answer while one is in flight,
 * and flattening a conflict or an unsupported operation into a generic failure.
 */
import { fireEvent, render, screen, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { PermissionPanel } from './PermissionPanel'
import type { PermissionEntry } from '../state/session'
import { timelineItem, withPayload } from '../test/fixtures'

/** One entry, with whatever the test needs overridden. */
function entry(overrides: Partial<PermissionEntry> = {}): PermissionEntry {
  return {
    requestId: 'perm-1',
    request: withPayload(
      timelineItem(2, { kind: 'permission_request', permission_id: 'perm-1' }),
      { tool: 'write', path: '/tmp/thing' },
    ),
    resolution: null,
    receipt: null,
    ...overrides,
  }
}

describe('<PermissionPanel>', () => {
  it('shows what is being asked for', () => {
    render(<PermissionPanel entries={[entry()]} onDecide={() => {}} busy={false} />)
    expect(screen.getByText('write')).toBeInTheDocument()
    expect(screen.getByText('/tmp/thing')).toBeInTheDocument()
    expect(screen.getByText('awaiting a decision')).toBeInTheDocument()
  })

  it('sends the decision for the request it belongs to', () => {
    const onDecide = vi.fn()
    render(<PermissionPanel entries={[entry()]} onDecide={onDecide} busy={false} />)
    fireEvent.click(screen.getByRole('button', { name: 'allow' }))
    expect(onDecide).toHaveBeenCalledWith('perm-1', 'allow')
  })

  it('disables a request the transcript already resolved, rather than hiding it', () => {
    render(
      <PermissionPanel
        entries={[
          entry({
            resolution: timelineItem(3, {
              kind: 'permission_resolved',
              permission_id: 'perm-1',
            }),
          }),
        ]}
        onDecide={() => {}}
        busy={false}
      />,
    )
    // Visible, so an operator can see the decision exists and was made.
    expect(screen.getByText(/resolved at 1:3/)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'allow' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'deny' })).toBeDisabled()
  })

  it('disables everything while an answer is in flight', () => {
    render(
      <PermissionPanel
        entries={[
          entry({
            receipt: {
              responseId: 'key-1',
              decision: 'allow',
              state: 'sending',
              code: null,
              rule: null,
            },
          }),
        ]}
        onDecide={() => {}}
        busy
      />,
    )
    expect(screen.getByRole('button', { name: 'allow' })).toBeDisabled()
  })

  it('shows the key an answer was sent under, so a retry can be seen to reuse it', () => {
    render(
      <PermissionPanel
        entries={[
          entry({
            receipt: {
              responseId: 'response-key-7',
              decision: 'deny',
              state: 'applied',
              code: null,
              rule: null,
            },
          }),
        ]}
        onDecide={() => {}}
        busy={false}
      />,
    )
    expect(screen.getByText('response-key-7')).toBeInTheDocument()
  })

  it.each([
    ['replayed', null, null],
    ['conflict', 'idempotency_conflict', 'the identifier was already used to commit a different effect'],
    ['unsupported', 'unsupported_capability', 'this session’s runtime never declared that operation'],
    ['refused', 'forbidden', 'the acting authority is not sufficient for this operation'],
  ] as const)('shows a %s receipt as the realm stated it', (state, code, rule) => {
    const { container } = render(
      <PermissionPanel
        entries={[entry({ receipt: { responseId: 'k', decision: 'allow', state, code, rule } })]}
        onDecide={() => {}}
        busy={false}
      />,
    )
    const receipt = container.querySelector(
      `[data-response-state="${state}"]`,
    ) as HTMLElement
    expect(receipt).not.toBeNull()
    if (code) {
      expect(within(receipt).getByText(code)).toBeInTheDocument()
      expect(receipt.textContent).toContain(rule)
    }
  })

  it('says so when nothing has been asked', () => {
    render(<PermissionPanel entries={[]} onDecide={() => {}} busy={false} />)
    expect(screen.getByText(/raised no permission requests/)).toBeInTheDocument()
  })
})
