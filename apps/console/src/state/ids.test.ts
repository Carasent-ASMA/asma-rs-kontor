/**
 * A retry must be a retry.
 *
 * The mutants this file exists to kill: a key minted per attempt instead of per
 * intent, and a UUID the realm will not parse as a v7 message id.
 */
import { describe, expect, it } from 'vitest'
import { KeyLedger, uuidv7 } from './ids'

/** The canonical form the realm parses. */
const CANONICAL = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/

describe('uuidv7', () => {
  it('is canonical, versioned 7 and variant 10', () => {
    for (let attempt = 0; attempt < 200; attempt += 1) {
      expect(uuidv7()).toMatch(CANONICAL)
    }
  })

  it('encodes the millisecond it was minted at, big-endian', () => {
    const at = 0x0192_3456_789a
    const id = uuidv7(at, (length) => new Uint8Array(length))
    expect(id.slice(0, 13).replace('-', '')).toBe('01923456789a')
  })

  it('orders lexicographically by time', () => {
    const early = uuidv7(1_700_000_000_000, (length) => new Uint8Array(length))
    const late = uuidv7(1_700_000_000_001, (length) => new Uint8Array(length))
    expect(early < late).toBe(true)
  })

  it('does not repeat itself', () => {
    const minted = new Set(Array.from({ length: 500 }, () => uuidv7()))
    expect(minted.size).toBe(500)
  })
})

describe('key ledger', () => {
  it('returns one key per intent, however many times it is asked', () => {
    const ledger = new KeyLedger()
    const first = ledger.key('draft-1')
    expect(ledger.key('draft-1')).toBe(first)
    expect(ledger.key('draft-1')).toBe(first)
    expect(ledger.key('draft-2')).not.toBe(first)
  })

  it('holds a key across failed attempts, so a retry commits once', () => {
    let minted = 0
    const ledger = new KeyLedger(() => `key-${(minted += 1)}`)
    const attempts: string[] = []
    for (let attempt = 0; attempt < 3; attempt += 1) {
      // Each attempt fails; none of them mints a second key.
      attempts.push(ledger.key('perm-7'))
    }
    expect(new Set(attempts).size).toBe(1)
    expect(minted).toBe(1)
  })

  it('mints a fresh key only once the intent is released', () => {
    const ledger = new KeyLedger()
    const first = ledger.key('draft')
    expect(ledger.has('draft')).toBe(true)
    ledger.release('draft')
    expect(ledger.has('draft')).toBe(false)
    expect(ledger.key('draft')).not.toBe(first)
  })
})
