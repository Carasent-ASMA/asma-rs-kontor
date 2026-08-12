/**
 * Stable client identifiers, and the ledger that keeps them stable.
 *
 * The session contract keys a message's effect on the `Idempotency-Key` the
 * caller presents, and requires that key to be a canonical UUID v7. A client that
 * mints a fresh key per attempt has turned every retry into a second message —
 * so a key is minted once per *intent* here and held until that intent is
 * acknowledged, however many attempts it takes.
 */

/** Random bytes, as an injectable so tests can pin them. */
export type RandomBytes = (length: number) => Uint8Array

/** The default source of randomness. */
const cryptoBytes: RandomBytes = (length) =>
  globalThis.crypto.getRandomValues(new Uint8Array(length))

/**
 * Mint one canonical UUID v7.
 *
 * Layout per RFC 9562: 48 bits of Unix milliseconds, version `7`, 12 bits of
 * randomness, the `10` variant, then 62 more bits of randomness.
 */
export function uuidv7(
  now: number = Date.now(),
  random: RandomBytes = cryptoBytes,
): string {
  const bytes = new Uint8Array(16)
  const timestamp = BigInt(Math.floor(now))
  for (let index = 0; index < 6; index += 1) {
    // Big-endian: the most significant byte of the 48-bit stamp goes first.
    bytes[index] = Number((timestamp >> BigInt(8 * (5 - index))) & 0xffn)
  }
  const entropy = random(10)
  bytes.set(entropy, 6)
  // Version 7 in the high nibble of octet 6, variant 10 in the top bits of octet 8.
  bytes[6] = ((bytes[6] ?? 0) & 0x0f) | 0x70
  bytes[8] = ((bytes[8] ?? 0) & 0x3f) | 0x80

  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('')
  return [
    hex.slice(0, 8),
    hex.slice(8, 12),
    hex.slice(12, 16),
    hex.slice(16, 20),
    hex.slice(20, 32),
  ].join('-')
}

/**
 * One stable key per intent.
 *
 * The subject is whatever names the intent — a draft, or the runtime's own
 * permission request id. Asking twice for the same subject returns the same key,
 * which is what makes a retry a retry.
 */
export class KeyLedger {
  readonly #keys = new Map<string, string>()
  readonly #mint: () => string

  constructor(mint: () => string = () => uuidv7()) {
    this.#mint = mint
  }

  /** The key for this subject, minted on first ask and stable afterwards. */
  key(subject: string): string {
    const existing = this.#keys.get(subject)
    if (existing !== undefined) {
      return existing
    }
    const minted = this.#mint()
    this.#keys.set(subject, minted)
    return minted
  }

  /** Whether this subject already has a key. */
  has(subject: string): boolean {
    return this.#keys.has(subject)
  }

  /**
   * Drop the key for a subject that will never be retried again.
   *
   * Called once an intent is acknowledged. Dropping it earlier — on a failure,
   * say — would mint a new key for the next attempt and defeat the point.
   */
  release(subject: string): void {
    this.#keys.delete(subject)
  }
}
