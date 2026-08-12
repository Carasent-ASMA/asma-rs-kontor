/**
 * Master and detail, side by side or one at a time.
 *
 * On a wide screen both panels are on the page. On a narrow one the detail is a
 * drawer over the list — a real modal dialog, dismissible with Escape, with the
 * list inert behind it. What it is never is a fixed panel laid over another
 * fixed panel with both still reachable: two overlapping scroll regions is how a
 * console becomes unusable on a phone while looking fine on the machine it was
 * written on.
 */
import { useEffect, useRef, type ReactNode } from 'react'
import { NARROW, useMediaQuery } from './useMediaQuery'

/** Render a master list beside, or under, its detail. */
export function MasterDetail({
  master,
  detail,
  detailLabel,
  open,
  onClose,
}: {
  /** The list. */
  master: ReactNode
  /** The detail, when something is selected. */
  detail: ReactNode
  /** What the detail is, for a reader who cannot see it. */
  detailLabel: string
  /** Whether anything is selected. */
  open: boolean
  /** Close the detail. */
  onClose: () => void
}) {
  const narrow = useMediaQuery(NARROW)
  const drawer = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!narrow || !open) {
      return undefined
    }
    // Focus moves into the drawer, so the next Tab is inside it rather than
    // somewhere behind it.
    drawer.current?.focus()
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') {
        onClose()
      }
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [narrow, open, onClose])

  if (!narrow) {
    return (
      <div className="master-detail" data-layout="wide">
        <section className="master" aria-label="list">
          {master}
        </section>
        <section className="detail" aria-label={detailLabel}>
          {detail}
        </section>
      </div>
    )
  }

  return (
    <div className="master-detail" data-layout="narrow">
      {/* Marked inert while the drawer is up, so nothing behind it takes focus
          or a click through the overlay. */}
      <section className="master" aria-label="list" inert={open ? true : undefined}>
        {master}
      </section>
      {open ? (
        <div
          className="detail drawer"
          role="dialog"
          aria-modal="true"
          aria-label={detailLabel}
          tabIndex={-1}
          ref={drawer}
        >
          <button type="button" className="drawer-close" onClick={onClose}>
            Close
          </button>
          {detail}
        </div>
      ) : null}
    </div>
  )
}
