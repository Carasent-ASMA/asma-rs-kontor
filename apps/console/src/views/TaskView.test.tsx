import { render, screen, within } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { TaskDetail } from './TaskView'
import { TASK, task } from '../test/fixtures'

/** The identity row a reader is given, by its label. */
function fact(label: string) {
  const term = screen.getByText(label, { selector: 'dt' })
  const row = term.closest('.fact')
  if (!row) throw new Error(`no fact row for ${label}`)
  return row as HTMLElement
}

function renderDetail(value = task()) {
  return render(<TaskDetail task={value} profile={null} team={null} persona={null} />)
}

describe('task identity', () => {
  it('shows the confirmed Jira key as the task identity', () => {
    renderDetail(task({ jira_binding: { state: 'confirmed', jira_key: 'ASMA-8119' } }))
    expect(within(fact('task')).getByText('ASMA-8119')).toBeTruthy()
  })

  it('keeps the uuid as a secondary identity rather than the primary one', () => {
    renderDetail(task({ jira_binding: { state: 'confirmed', jira_key: 'ASMA-8119' } }))
    // Still on the page: every route and bookmark still accepts it.
    expect(within(fact('task uuid')).getByText(TASK)).toBeTruthy()
    // But it is not what the reader is told the task *is*.
    expect(within(fact('task')).queryByText(TASK)).toBeNull()
  })

  it('says a task is awaiting its binding rather than showing a key', () => {
    renderDetail(task({ jira_binding: { state: 'awaiting_jira_binding' } }))
    expect(within(fact('task')).getByText('Awaiting Jira binding')).toBeTruthy()
    // The uuid remains reachable even with no confirmed key.
    expect(within(fact('task uuid')).getByText(TASK)).toBeTruthy()
  })

  it('never invents an identity when a confirmed binding carries no key', () => {
    // A confirmed state with no key is malformed; the console must not fall back
    // to the uuid, the title or anything derived as though it were the key.
    renderDetail(task({ jira_binding: { state: 'confirmed' } }))
    const identity = fact('task')
    expect(within(identity).getByText('Awaiting Jira binding')).toBeTruthy()
    expect(within(identity).queryByText(TASK)).toBeNull()
    expect(within(identity).queryByText('q-41 carry')).toBeNull()
  })

  it('does not derive a key from the title', () => {
    renderDetail(task({ title: 'ASMA-9999 looks like a key', jira_binding: { state: 'awaiting_jira_binding' } }))
    expect(within(fact('task')).queryByText(/ASMA-9999/)).toBeNull()
  })
})
