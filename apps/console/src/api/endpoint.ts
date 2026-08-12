/**
 * Which realm this console is pointed at, and what it may honestly say about it.
 *
 * The console is handed a base URL and a realm bearer. It is never handed a
 * runtime endpoint, a runtime credential or a path into the daemon's state root,
 * and there is nowhere here to put one.
 */

/** The loopback endpoint and the realm credential presented to it. */
export interface Endpoint {
  /** The realm's base URL, with no trailing slash. */
  readonly baseUrl: string
  /** One of the realm's tier secrets, read from its 0600 credential file. */
  readonly token: string
}

/**
 * Where the endpoint the console is calling actually lives.
 *
 * This is derived from the URL *this client was configured with* — it is a fact
 * about the console's own configuration, not an assertion the realm made. The
 * contract's `RealmDto` carries no endpoint locality, so a top bar that showed
 * this as realm-reported would be claiming a field the contract does not have.
 * The shell labels it as client-derived for exactly that reason, and swaps to the
 * realm-asserted value when the contract gains one (KON-MVP-16).
 */
export type Locality = 'loopback' | 'not_loopback' | 'unknown'

/** The hosts a loopback realm can answer on. */
const LOOPBACK_HOSTS: ReadonlySet<string> = new Set([
  'localhost',
  '127.0.0.1',
  '[::1]',
  '::1',
])

/**
 * Judge one base URL's locality.
 *
 * An unparseable URL is `unknown` rather than `not_loopback`: the console does
 * not know where it points, and saying "not loopback" would be a claim it cannot
 * support either.
 */
export function localityOf(baseUrl: string): Locality {
  let parsed: URL
  try {
    parsed = new URL(baseUrl)
  } catch {
    return 'unknown'
  }
  // IPv4 loopback is the whole 127/8 block, not only 127.0.0.1.
  const host = parsed.hostname
  if (LOOPBACK_HOSTS.has(host) || /^127\.\d+\.\d+\.\d+$/.test(host)) {
    return 'loopback'
  }
  return 'not_loopback'
}

/**
 * Normalize a base URL a human typed.
 *
 * @throws {Error} when the text is not an absolute http(s) URL.
 */
export function normalizeBaseUrl(raw: string): string {
  const trimmed = raw.trim()
  let parsed: URL
  try {
    parsed = new URL(trimmed)
  } catch {
    throw new Error('the realm endpoint must be an absolute URL')
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    throw new Error('the realm endpoint must be an http or https URL')
  }
  return `${parsed.origin}${parsed.pathname.replace(/\/+$/, '')}`
}
