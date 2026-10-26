/**
 * Where the endpoint and the realm bearer are kept.
 *
 * Two stores, chosen by where the console is running:
 *
 * * **Desktop** — the operating system's credential store (Keychain, Credential
 *   Manager, Secret Service), through the desktop shell's `credential_*`
 *   commands. The bearer is a realm credential read from a 0600 file, and the
 *   desktop shell is the only place in this application with somewhere durable
 *   to put one.
 * * **Browser** — the base URL only, in `sessionStorage`, and the token nowhere
 *   at all. A bearer in web storage outlives the tab, is readable by anything
 *   that gets script into the page, and buys only the convenience of not
 *   retyping it. Re-entering it after a reload is the correct trade.
 *
 * Neither store holds anything about a *runtime*. The console is never given a
 * runtime endpoint or a runtime credential, and there is nowhere here to put one.
 */
import type { Endpoint } from '../api/endpoint'

/** Where the endpoint this console was pointed at is kept. */
export interface CredentialStore {
  /** Whether this store can hold a credential at all. */
  readonly durable: boolean
  /** The saved endpoint, when there is one and it can be unlocked. */
  load(secret?: string): Promise<Endpoint | null>
  /** Save the endpoint. */
  save(endpoint: Endpoint, secret?: string): Promise<void>
  /** Forget it. */
  clear(secret?: string): Promise<void>
}

/** The key the base URL is remembered under. */
const BASE_URL_KEY = 'kontor.endpoint.base_url'
/** The keychain profile this console keeps its endpoint under. */
const PROFILE = 'default'

/** Whether this console is running inside the desktop shell. */
export async function inDesktop(): Promise<boolean> {
  try {
    const core = await import('@tauri-apps/api/core')
    return core.isTauri()
  } catch {
    return false
  }
}

/**
 * The browser store: the base URL, and deliberately not the token.
 */
export const browserStore: CredentialStore = {
  durable: false,
  async load(): Promise<Endpoint | null> {
    // Never an endpoint: the token is not kept, and an endpoint that cannot
    // authenticate is not a usable one. The remembered base URL is offered to
    // the form through `rememberedBaseUrl` instead, where it is a default the
    // operator can see rather than a connection they did not ask for.
    return null
  },
  async save(endpoint: Endpoint): Promise<void> {
    globalThis.sessionStorage?.setItem(BASE_URL_KEY, endpoint.baseUrl)
  },
  async clear(): Promise<void> {
    globalThis.sessionStorage?.removeItem(BASE_URL_KEY)
  },
}

/** The base URL last used, so it does not have to be retyped. */
export function rememberedBaseUrl(): string {
  return globalThis.sessionStorage?.getItem(BASE_URL_KEY) ?? ''
}

/** Why the desktop shell could not use the OS credential store. */
export type KeychainErrorKind = 'unavailable' | 'locked' | 'denied' | 'not_found' | 'invalid'

/** How to connect anyway when the keychain refuses. */
const SESSION_ONLY = 'untick “Keep this endpoint” to connect for this session only'

const KEYCHAIN_MESSAGES: Record<KeychainErrorKind, string> = {
  unavailable: `the OS keychain is not available on this machine; ${SESSION_ONLY}`,
  locked: `the OS keychain is locked; unlock it and try again, or ${SESSION_ONLY}`,
  denied: `access to the OS keychain was denied; ${SESSION_ONLY}`,
  not_found: 'no endpoint is saved in the OS keychain',
  invalid: `the saved endpoint is not valid; forget it, or ${SESSION_ONLY}`,
}

/** A refusal from the OS credential store. The message never carries a value. */
export class KeychainError extends Error {
  constructor(readonly kind: KeychainErrorKind) {
    super(KEYCHAIN_MESSAGES[kind])
    this.name = 'KeychainError'
  }
}

/**
 * The desktop store: the operating system's credential store.
 *
 * There is no vault password, so `secret`'s value is never read. Its presence
 * is: the form passes it only when the operator asked to keep the endpoint, and
 * without it `save` keeps nothing. Every failure is thrown to the caller; there
 * is no other store to fall back to.
 *
 * The import is dynamic, so a browser build never loads the Tauri API.
 */
export const keychainStore: CredentialStore = {
  durable: true,

  async load(): Promise<Endpoint | null> {
    try {
      const saved = await keychain<unknown>('credential_load', {})
      if (!isEndpoint(saved)) {
        throw new KeychainError('invalid')
      }
      return saved
    } catch (cause) {
      if (cause instanceof KeychainError && cause.kind === 'not_found') {
        return null
      }
      throw cause
    }
  },

  async save(endpoint: Endpoint, secret?: string): Promise<void> {
    if (secret === undefined) {
      return
    }
    await keychain<void>('credential_save', { baseUrl: endpoint.baseUrl, token: endpoint.token })
  },

  async clear(): Promise<void> {
    try {
      await keychain<void>('credential_clear', {})
    } catch (cause) {
      if (!(cause instanceof KeychainError && cause.kind === 'not_found')) {
        throw cause
      }
    }
  },
}

function isEndpoint(value: unknown): value is Endpoint {
  const candidate = value as Partial<Endpoint> | null
  return (
    typeof candidate?.baseUrl === 'string' &&
    candidate.baseUrl !== '' &&
    typeof candidate.token === 'string' &&
    candidate.token !== ''
  )
}

/** Call one `credential_*` command, turning its refusal into a `KeychainError`. */
async function keychain<T>(command: string, args: Record<string, string>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core')
  try {
    return await invoke<T>(command, { profile: PROFILE, ...args })
  } catch (cause) {
    const kind = (cause as { kind?: unknown } | null)?.kind
    throw new KeychainError(
      typeof kind === 'string' && Object.hasOwn(KEYCHAIN_MESSAGES, kind) ? (kind as KeychainErrorKind) : 'unavailable',
    )
  }
}
