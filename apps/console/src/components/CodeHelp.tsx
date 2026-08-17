/** Accessible help for one server-controlled code. */
import { useId, useState } from 'react'
import type { CodeHelpEntry } from '../api/types'

/** Keep the code visible while exposing its server-owned name and meaning. */
export function CodeHelp({
  code,
  entries,
  category,
}: {
  code: string
  entries: readonly CodeHelpEntry[]
  category?: string
}) {
  const [open, setOpen] = useState(false)
  const help = entries.find(
    (entry) => entry.code === code && (category === undefined || entry.category === category),
  )
  const description = useId()

  return (
    <span className="code-help" data-open={open} data-known={help ? 'true' : 'false'}>
      <button
        type="button"
        aria-expanded={open}
        aria-describedby={description}
        onClick={() => setOpen((value) => !value)}
      >
        <code>{code}</code>
      </button>
      <span className="code-help-text" id={description} role="tooltip">
        {help ? (
          <>
            <strong>{help.full_name}</strong>
            <span>{help.meaning}</span>
            <small>
              {help.category} · {help.lifecycle} · {help.source.id}@{help.source.version}
            </small>
          </>
        ) : (
          <>
            <strong>Unknown code</strong>
            <span>The server returned no definition for {code}.</span>
          </>
        )}
      </span>
    </span>
  )
}
