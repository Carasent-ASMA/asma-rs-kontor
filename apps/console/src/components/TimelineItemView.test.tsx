/**
 * A transcript renders every kind, including the ones it has never seen.
 *
 * The vocabulary is read from `session-kinds.json`, which
 * `kontor-api/tests/openapi_contract.rs` generates and pins to the set the realm
 * actually subscribes to. So a kind added to the contract and forgotten here
 * fails this suite instead of disappearing from an operator's transcript —
 * which is the failure a subscription that filters is not allowed to have.
 */
import { render, screen, within } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { TimelineItemView } from './TimelineItemView'
import { timelineItem, withPayload } from '../test/fixtures'
import KINDS from '../test/session-kinds.json'

describe('<TimelineItemView>', () => {
  it('covers the vocabulary the realm actually subscribes to', () => {
    expect(KINDS).toEqual([
      'message',
      'tool_call',
      'permission_request',
      'permission_resolved',
      'state_change',
      'log',
    ])
  })

  it.each(KINDS)('renders a %s item with its kind and position', (kind) => {
    const { container } = render(
      <TimelineItemView item={timelineItem(4, { kind, epoch: 2 })} />,
    )
    const article = container.querySelector('.timeline-item') as HTMLElement
    expect(article).toHaveAttribute('data-kind', kind)
    expect(article).toHaveAttribute('data-epoch', '2')
    expect(article).toHaveAttribute('data-sequence', '4')
    expect(within(article).getByText(kind)).toBeInTheDocument()
    expect(within(article).getByText('2:4')).toBeInTheDocument()
  })

  it('renders a kind this console has never heard of rather than dropping it', () => {
    const { container } = render(
      <TimelineItemView item={timelineItem(1, { kind: 'something_new_entirely' })} />,
    )
    const article = container.querySelector('.timeline-item') as HTMLElement
    expect(article).toHaveAttribute('data-kind', 'something_new_entirely')
    expect(within(article).getByText('something_new_entirely')).toBeInTheDocument()
  })

  it('shows model content as a readable line and keeps the whole payload', () => {
    render(
      <TimelineItemView
        item={withPayload(timelineItem(1), {
          body: 'the model said this',
          model: 'some-model',
          tokens: 41,
        })}
      />,
    )
    // Twice, and deliberately: once as the preview line, once inside the
    // payload. The preview is a convenience; the payload is the record.
    expect(screen.getAllByText('the model said this')).toHaveLength(2)
    expect(screen.getByText('model')).toBeInTheDocument()
    expect(screen.getByText('some-model')).toBeInTheDocument()
    expect(screen.getByText('41')).toBeInTheDocument()
  })

  it('renders a tool call payload structurally without interpreting it', () => {
    render(
      <TimelineItemView
        item={withPayload(timelineItem(2, { kind: 'tool_call' }), {
          tool: 'whatever-the-runtime-calls-it',
          arguments: { path: '/tmp/x', recursive: true },
        })}
      />,
    )
    expect(screen.getByText('whatever-the-runtime-calls-it')).toBeInTheDocument()
    expect(screen.getByText('path')).toBeInTheDocument()
    expect(screen.getByText('/tmp/x')).toBeInTheDocument()
    expect(screen.getByText('true')).toBeInTheDocument()
  })

  it('shows the runtime request id on a permission item', () => {
    render(
      <TimelineItemView
        item={timelineItem(3, { kind: 'permission_request', permission_id: 'perm-9' })}
      />,
    )
    expect(screen.getByText('perm-9')).toBeInTheDocument()
  })

  it('shows the Kontor message id when the item is about one', () => {
    render(<TimelineItemView item={timelineItem(4, { message_id: 'msg-1' })} />)
    expect(screen.getByText('msg-1')).toBeInTheDocument()
  })

  it('renders an empty payload without pretending there is content', () => {
    render(<TimelineItemView item={timelineItem(5)} />)
    expect(screen.getByText('no payload')).toBeInTheDocument()
  })

  it('does not recurse forever on a deeply nested payload', () => {
    let nested: Record<string, unknown> = { leaf: 'bottom' }
    for (let depth = 0; depth < 12; depth += 1) {
      nested = { down: nested }
    }
    render(<TimelineItemView item={withPayload(timelineItem(6), nested)} />)
    expect(screen.getAllByText('down').length).toBeGreaterThan(0)
  })
})
