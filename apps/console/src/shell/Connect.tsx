/**
 * Pointing the console at a realm.
 *
 * The operator supplies the loopback endpoint and one of the realm's tier
 * secrets, which they read from its 0600 credential file. Nothing is discovered:
 * this console does not scan for daemons, does not read a state root and is never
 * handed a runtime endpoint.
 *
 * On the desktop the pair can be kept in Stronghold, unlocked with a vault
 * password. In the browser it cannot, and the form says so rather than quietly
 * putting a bearer token in web storage.
 */
import { useState, type FormEvent } from 'react'
import { localityOf, normalizeBaseUrl, type Endpoint } from '../api/endpoint'
import { rememberedBaseUrl, type CredentialStore } from './credentials'

/** Render the connection form. */
export function Connect({
  store,
  onConnect,
}: {
  /** Where a credential may be kept, if anywhere. */
  store: CredentialStore
  /** Called with the endpoint to attach to. */
  onConnect: (endpoint: Endpoint) => void
}) {
  const [baseUrl, setBaseUrl] = useState(rememberedBaseUrl)
  const [token, setToken] = useState('')
  const [secret, setSecret] = useState('')
  const [remember, setRemember] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const locality = baseUrl.trim() === '' ? null : localityOf(baseUrl)

  const connect = async (event: FormEvent): Promise<void> => {
    event.preventDefault()
    let endpoint: Endpoint
    try {
      endpoint = { baseUrl: normalizeBaseUrl(baseUrl), token: token.trim() }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'the endpoint is not a URL')
      return
    }
    if (endpoint.token === '') {
      setError('a realm bearer is required: every route of this contract is authenticated')
      return
    }
    try {
      await store.save(endpoint, remember ? secret : undefined)
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'the credential could not be saved')
      return
    }
    onConnect(endpoint)
  }

  const unlock = async (): Promise<void> => {
    try {
      const saved = await store.load(secret)
      if (saved) {
        onConnect(saved)
      } else {
        setError('no saved endpoint could be unlocked with that password')
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'the vault could not be opened')
    }
  }

  const forget = async (): Promise<void> => {
    try {
      await store.clear(secret)
      setError('the saved endpoint was forgotten')
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'the vault could not be opened')
    }
  }

  return (
    <main className="connect">
      <h1>Connect to a realm</h1>
      <form onSubmit={(event) => void connect(event)}>
        <label htmlFor="base-url">Realm endpoint</label>
        <input
          id="base-url"
          name="base_url"
          value={baseUrl}
          placeholder="http://127.0.0.1:7777"
          onChange={(event) => setBaseUrl(event.target.value)}
        />
        {locality === 'not_loopback' ? (
          <p className="warning" role="note">
            That endpoint is not a loopback address. A realm answers only to a
            loopback host, so this will be refused.
          </p>
        ) : null}

        <label htmlFor="token">Realm bearer</label>
        <input
          id="token"
          name="token"
          type="password"
          autoComplete="off"
          value={token}
          onChange={(event) => setToken(event.target.value)}
        />
        <p className="hint">One of the realm’s tier secrets, from its 0600 credential file.</p>

        {store.durable ? (
          <>
            <label htmlFor="vault-secret">Vault password</label>
            <input
              id="vault-secret"
              name="vault_secret"
              type="password"
              autoComplete="off"
              value={secret}
              onChange={(event) => setSecret(event.target.value)}
            />
            <label className="checkbox">
              <input
                type="checkbox"
                checked={remember}
                onChange={(event) => setRemember(event.target.checked)}
              />
              Keep this endpoint in Stronghold
            </label>
            <button type="button" onClick={() => void unlock()}>
              Unlock a saved endpoint
            </button>
            {/* A credential that can be kept and never dropped is one an
                operator has no way to revoke from here. */}
            <button type="button" onClick={() => void forget()}>
              Forget the saved endpoint
            </button>
          </>
        ) : (
          <p className="hint">
            This console is running in a browser, so the bearer is not kept
            anywhere and has to be entered each time.
          </p>
        )}

        {error ? (
          <p className="banner" role="alert">
            {error}
          </p>
        ) : null}
        <button type="submit">Connect</button>
      </form>
    </main>
  )
}
