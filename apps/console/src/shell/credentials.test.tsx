/**
 * The desktop credential store, against a MOCK of the shell's `credential_*`
 * commands.
 *
 * The mock is an in-memory map keyed by profile, or a fixed refusal. It proves
 * what this module sends and how it treats each answer; it is not OS proof, and
 * nothing here touches a real keychain. Every credential is synthetic.
 */
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { App } from './App'
import { Connect } from './Connect'
import { browserStore, KeychainError, keychainStore, rememberedBaseUrl } from './credentials'
import desktopCommands from '../../../desktop/src-tauri/src/lib.rs?raw'

const { invoke, isTauri } = vi.hoisted(() => ({ invoke: vi.fn(), isTauri: vi.fn(() => true) }))
vi.mock('@tauri-apps/api/core', () => ({ invoke, isTauri }))

/** MOCK shell: entries keyed by profile, or every command refused with `refusal`. */
function mockShell(refusal?: unknown) {
  const entries = new Map<string, { baseUrl: string; token: string }>()
  invoke.mockImplementation(async (command: string, args: { profile: string; baseUrl: string; token: string }) => {
    if (refusal !== undefined) {
      throw refusal
    }
    switch (command) {
      case 'credential_save':
        entries.set(args.profile, { baseUrl: args.baseUrl, token: args.token })
        return null
      case 'credential_load': {
        const saved = entries.get(args.profile)
        if (!saved) throw { kind: 'not_found' }
        return saved
      }
      case 'credential_clear':
        if (!entries.delete(args.profile)) throw { kind: 'not_found' }
        return null
      default:
        throw new Error(`unexpected command ${command}`)
    }
  })
  return entries
}

const ONE = { baseUrl: 'http://127.0.0.1:7717', token: 'synthetic-token-one' }
const TWO = { baseUrl: 'http://127.0.0.1:7718', token: 'synthetic-token-two' }

let webStorageWrites: ReturnType<typeof vi.spyOn>

beforeEach(() => {
  invoke.mockReset()
  isTauri.mockReturnValue(true)
  webStorageWrites = vi.spyOn(Storage.prototype, 'setItem')
})

afterEach(() => {
  // Nothing in this file may reach web storage: not the token, not the URL.
  expect(webStorageWrites).not.toHaveBeenCalled()
  webStorageWrites.mockRestore()
})

describe('keychainStore', () => {
  it('saves, loads, replaces and clears through the shell', async () => {
    const entries = mockShell()
    expect(await keychainStore.load()).toBeNull()

    await keychainStore.save(ONE, '')
    expect(invoke).toHaveBeenLastCalledWith('credential_save', {
      profile: 'default',
      baseUrl: ONE.baseUrl,
      token: ONE.token,
    })
    expect(await keychainStore.load()).toEqual(ONE)

    await keychainStore.save(TWO, '')
    expect(await keychainStore.load()).toEqual(TWO)
    expect(entries.size).toBe(1)

    await keychainStore.clear()
    expect(await keychainStore.load()).toBeNull()
    await expect(keychainStore.clear()).resolves.toBeUndefined()
  })

  it('keeps nothing unless the operator asked to keep it', async () => {
    mockShell()
    await keychainStore.save(ONE)
    expect(invoke).not.toHaveBeenCalled()
  })

  it('touches only its own profile', async () => {
    const entries = mockShell()
    entries.set('other', TWO)
    await keychainStore.save(ONE, '')
    await keychainStore.clear()
    expect([...entries]).toEqual([['other', TWO]])
    for (const [, args] of invoke.mock.calls) {
      expect(args).toMatchObject({ profile: 'default' })
    }
  })

  it.each(['unavailable', 'locked', 'denied', 'invalid'] as const)(
    'throws a %s refusal to the caller without the token',
    async (kind) => {
      mockShell({ kind })
      for (const call of [
        () => keychainStore.save(ONE, ''),
        () => keychainStore.load(),
        () => keychainStore.clear(),
      ]) {
        const error = await call().then(
          () => null,
          (cause: unknown) => cause,
        )
        expect(error).toBeInstanceOf(KeychainError)
        expect((error as KeychainError).kind).toBe(kind)
        expect((error as KeychainError).message).not.toContain(ONE.token)
      }
    },
  )

  it.each([null, {}, { baseUrl: ONE.baseUrl }, { baseUrl: '', token: ONE.token }, { baseUrl: 1, token: 2 }])(
    'refuses a loaded value that is not an endpoint (%j)',
    async (value) => {
      invoke.mockResolvedValue(value)
      const error = await keychainStore.load().catch((cause: unknown) => cause)
      expect(error).toBeInstanceOf(KeychainError)
      expect((error as KeychainError).kind).toBe('invalid')
    },
  )

  it('reports an untyped refusal as unavailable, without its text', async () => {
    mockShell(`command credential_save failed for ${ONE.token}`)
    const error = await keychainStore.save(ONE, '').catch((cause: unknown) => cause)
    expect(error).toBeInstanceOf(KeychainError)
    expect((error as KeychainError).kind).toBe('unavailable')
    expect((error as KeychainError).message).not.toContain(ONE.token)
  })

  it('reports an unknown refusal kind as unavailable', async () => {
    mockShell({ kind: 'exploded' })
    const error = await keychainStore.load().catch((cause: unknown) => cause)
    expect(error).toBeInstanceOf(KeychainError)
    expect((error as KeychainError).kind).toBe('unavailable')
  })
})

describe('browserStore', () => {
  it('keeps the base URL for this session only, never the token', async () => {
    await browserStore.save(ONE)
    expect(webStorageWrites).toHaveBeenCalledTimes(1)
    expect(webStorageWrites.mock.calls.flat()).not.toContain(ONE.token)
    expect(rememberedBaseUrl()).toBe(ONE.baseUrl)
    expect(localStorage.length).toBe(0)
    expect(await browserStore.load()).toBeNull()

    await browserStore.clear()
    expect(rememberedBaseUrl()).toBe('')
    // The one write above is this test's own; the file-wide guard covers the rest.
    webStorageWrites.mockClear()
  })
})

describe('Connect with the keychain store', () => {
  function fill() {
    fireEvent.change(screen.getByLabelText('Realm endpoint'), { target: { value: ONE.baseUrl } })
    fireEvent.change(screen.getByLabelText('Realm bearer'), { target: { value: ONE.token } })
  }

  it('shows a failed keep and stays session-only until the operator unticks it', async () => {
    mockShell({ kind: 'unavailable' })
    const onConnect = vi.fn()
    render(<Connect store={keychainStore} onConnect={onConnect} />)
    expect(screen.queryByLabelText(/password/i)).toBeNull()
    fill()

    fireEvent.click(screen.getByLabelText('Keep this endpoint in the OS keychain'))
    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('the OS keychain is not available')
    expect(onConnect).not.toHaveBeenCalled()

    invoke.mockClear()
    fireEvent.click(screen.getByLabelText('Keep this endpoint in the OS keychain'))
    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(onConnect).toHaveBeenCalledWith(ONE))
    expect(invoke).not.toHaveBeenCalled()
  })

  it('shows a locked keychain on use and forget', async () => {
    mockShell({ kind: 'locked' })
    render(<Connect store={keychainStore} onConnect={vi.fn()} />)
    fireEvent.click(screen.getByRole('button', { name: 'Use the saved endpoint' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('the OS keychain is locked')
    fireEvent.click(screen.getByRole('button', { name: 'Forget the saved endpoint' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('the OS keychain is locked')
  })
})

describe('the store App selects', () => {
  it('is the keychain store inside the desktop shell', async () => {
    mockShell()
    render(<App />)
    expect(await screen.findByLabelText('Keep this endpoint in the OS keychain')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Use the saved endpoint' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('credential_load', { profile: 'default' }))
  })

  it('is the browser store outside it', async () => {
    isTauri.mockReturnValue(false)
    render(<App />)
    expect(await screen.findByText(/running in a browser/)).toBeInTheDocument()
    expect(screen.queryByLabelText('Keep this endpoint in the OS keychain')).toBeNull()
    expect(invoke).not.toHaveBeenCalled()
  })
})

describe('the shell contract', () => {
  /** Each registered `credential_*` command and its arguments as JS names. */
  function rustCommands(): Map<string, string[]> {
    const registered = /generate_handler!\[([^\]]*)\]/.exec(desktopCommands)?.[1] ?? ''
    const commands = new Map<string, string[]>()
    for (const [, name, params] of desktopCommands.matchAll(/async fn (credential_\w+)\(([^)]*)\)/g)) {
      if (!name || !registered.includes(name)) continue
      const args = [...(params ?? '').matchAll(/(\w+):/g)].map(([, arg]) =>
        // Tauri renames a command's snake_case arguments to camelCase.
        (arg ?? '').replace(/_([a-z])/g, (_, letter: string) => letter.toUpperCase()),
      )
      commands.set(name, args.sort())
    }
    return commands
  }

  it('sends exactly the commands and arguments the shell registers', async () => {
    mockShell()
    await keychainStore.save(ONE, '')
    await keychainStore.load()
    await keychainStore.clear()

    const sent = new Map(
      invoke.mock.calls.map((call) => [
        call[0] as string,
        Object.keys(call[1] as Record<string, string>).sort(),
      ]),
    )
    const shell = rustCommands()
    expect(shell.size).toBeGreaterThan(0)
    expect(sent).toEqual(shell)
  })
})
