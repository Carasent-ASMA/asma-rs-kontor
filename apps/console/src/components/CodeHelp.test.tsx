import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { CodeHelp } from './CodeHelp'

const HELP = [{
  category: 'role',
  code: 'LSA',
  full_name: 'Lead Software Architect',
  meaning: 'Owns architecture for one epic.',
  lifecycle: 'active',
  source: { id: 'standard-roles', version: 1 },
}]

describe('<CodeHelp>', () => {
  it('keeps a code visible and exposes server help to focus and touch/click', () => {
    render(<CodeHelp code="LSA" category="role" entries={HELP} />)
    const code = screen.getByRole('button', { name: 'LSA' })
    expect(code).toHaveAttribute('aria-describedby')
    expect(screen.getByRole('tooltip')).toHaveTextContent('Lead Software Architect')
    code.focus()
    expect(code).toHaveFocus()
    fireEvent.click(code)
    expect(code).toHaveAttribute('aria-expanded', 'true')
  })

  it('renders an unknown code as unknown instead of hiding it', () => {
    const { container } = render(<CodeHelp code="FUTURE" entries={HELP} />)
    expect(screen.getByRole('button', { name: 'FUTURE' })).toBeInTheDocument()
    expect(screen.getByRole('tooltip')).toHaveTextContent('server returned no definition')
    expect(container.querySelector('[data-known="false"]')).not.toBeNull()
  })
})
