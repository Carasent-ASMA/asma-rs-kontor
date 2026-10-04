import { fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { App } from './App'
import type { CredentialStore } from './credentials'

const STORE: CredentialStore = {
  durable: false,
  load: async () => null,
  save: async () => {},
  clear: async () => {},
}

afterEach(() => {
  window.history.replaceState({}, '', '/')
  vi.unstubAllGlobals()
})

describe('Tokenomics access', () => {
  it('opens directly without connecting to a realm or fetching realm data', () => {
    const fetch = vi.fn()
    vi.stubGlobal('fetch', fetch)
    window.history.replaceState({}, '', '/?view=tokenomics')
    render(<App store={STORE} />)

    expect(screen.getByTitle('Tokenomics dashboard')).toHaveAttribute('src', '/tokenomics/index.html?theme=light')
    expect(screen.getByRole('button', { name: 'Tokenomics' })).toHaveAttribute('aria-current', 'page')
    expect(screen.queryByRole('heading', { name: 'Connect to a realm' })).toBeNull()
    expect(fetch).not.toHaveBeenCalled()
  })

  it('offers Tokenomics from the connection screen and preserves operational connection access', () => {
    render(<App store={STORE} />)
    fireEvent.click(screen.getByRole('button', { name: 'Open Tokenomics dashboard' }))
    expect(screen.getByTitle('Tokenomics dashboard')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Board' }))
    expect(screen.getByRole('heading', { name: 'Connect to a realm' })).toBeInTheDocument()
  })
})
