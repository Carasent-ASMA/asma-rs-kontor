/**
 * Where the endpoint and the realm bearer are kept.
 *
 * Two stores, chosen by where the console is running:
 *
 * * **Desktop** — Stronghold. The bearer is a realm credential read from a 0600
 *   file, and the desktop shell is the only place in this application with
 *   somewhere durable to put one.
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
/** The Stronghold record the endpoint is kept in. */
const VAULT_RECORD = 'kontor.endpoint'
/** The Stronghold client this console owns. */
const VAULT_CLIENT = 'kontor-console'
/** The vault file, relative to the application's own data directory. */
const VAULT_FILE = 'kontor-console.stronghold'

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

/**
 * The desktop store: Stronghold, unlocked with the operator's vault password.
 *
 * Every import is dynamic, so a browser build never loads the plugin and a test
 * never needs it present.
 */
export const strongholdStore: CredentialStore = {
  durable: true,

  async load(secret?: string): Promise<Endpoint | null> {
    if (!secret) {
      return null
    }
    const { store } = await openVault(secret)
    const record = await store.get(VAULT_RECORD)
    if (!record) {
      return null
    }
    const text = new TextDecoder().decode(new Uint8Array(record))
    const parsed: unknown = JSON.parse(text)
    if (
      parsed !== null &&
      typeof parsed === 'object' &&
      typeof (parsed as Endpoint).baseUrl === 'string' &&
      typeof (parsed as Endpoint).token === 'string'
    ) {
      return parsed as Endpoint
    }
    return null
  },

  async save(endpoint: Endpoint, secret?: string): Promise<void> {
    if (!secret) {
      return
    }
    const { vault, store } = await openVault(secret)
    const encoded = Array.from(new TextEncoder().encode(JSON.stringify(endpoint)))
    await store.insert(VAULT_RECORD, encoded)
    await vault.save()
  },

  async clear(secret?: string): Promise<void> {
    if (!secret) {
      return
    }
    const { vault, store } = await openVault(secret)
    await store.remove(VAULT_RECORD)
    await vault.save()
  },
}

/** Open — or create — this console's vault and its one client. */
async function openVault(secret: string) {
  const [{ Stronghold }, { appDataDir }] = await Promise.all([
    import('@tauri-apps/plugin-stronghold'),
    import('@tauri-apps/api/path'),
  ])
  const vault = await Stronghold.load(`${await appDataDir()}/${VAULT_FILE}`, secret)
  let client
  try {
    client = await vault.loadClient(VAULT_CLIENT)
  } catch {
    client = await vault.createClient(VAULT_CLIENT)
  }
  return { vault, store: client.getStore() }
}
