/**
 * The rail that chooses a view.
 *
 * It is a rail on a wide screen and a row of tabs on a narrow one, which is the
 * stylesheet's business. What matters here is that it is one `<nav>` of real
 * buttons with `aria-current` on the active one, and that arrow keys move
 * between them — a console an operator cannot drive from the keyboard is one
 * they cannot drive while reading something else.
 */
import { useRef } from 'react'

/** The views the console offers. */
export const VIEWS = [
  { id: 'board', label: 'Board' },
  { id: 'task', label: 'Task' },
  { id: 'session', label: 'Session' },
  { id: 'intake', label: 'Intake' },
  { id: 'workflow', label: 'Workflow' },
  { id: 'schedule', label: 'Schedule' },
] as const

/** Which view is on screen. */
export type ViewId = (typeof VIEWS)[number]['id']

/** Render the view chooser. */
export function NavRail({
  current,
  onSelect,
}: {
  /** The view on screen. */
  current: ViewId
  /** Choose another. */
  onSelect: (view: ViewId) => void
}) {
  const rail = useRef<HTMLElement>(null)

  /** Move focus and selection with the arrow keys, wrapping at both ends. */
  const onKeyDown = (event: React.KeyboardEvent<HTMLElement>): void => {
    const step = event.key === 'ArrowDown' || event.key === 'ArrowRight' ? 1 : event.key === 'ArrowUp' || event.key === 'ArrowLeft' ? -1 : 0
    if (step === 0) {
      return
    }
    event.preventDefault()
    const index = VIEWS.findIndex((view) => view.id === current)
    const next = VIEWS[(index + step + VIEWS.length) % VIEWS.length]
    if (!next) {
      return
    }
    onSelect(next.id)
    rail.current?.querySelector<HTMLButtonElement>(`[data-view="${next.id}"]`)?.focus()
  }

  return (
    <nav className="nav-rail" aria-label="console views" ref={rail} onKeyDown={onKeyDown}>
      <ul>
        {VIEWS.map((view) => (
          <li key={view.id}>
            <button
              type="button"
              data-view={view.id}
              aria-current={view.id === current ? 'page' : undefined}
              // One stop for the whole rail: arrow keys move within it, so a
              // reader tabbing through the page does not have to pass six stops.
              tabIndex={view.id === current ? 0 : -1}
              onClick={() => onSelect(view.id)}
            >
              {view.label}
            </button>
          </li>
        ))}
      </ul>
    </nav>
  )
}
