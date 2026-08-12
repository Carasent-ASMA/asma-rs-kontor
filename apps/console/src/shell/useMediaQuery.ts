/**
 * Which layout the viewport is asking for.
 *
 * The breakpoint is a media query rather than a width comparison so the CSS and
 * the component tree agree about what "narrow" means — two sources of that
 * answer is how a drawer ends up open behind a sidebar.
 */
import { useEffect, useState } from 'react'

/** Below this, the console is one column with drawers. */
export const NARROW = '(max-width: 767px)'

/** Follow one media query. */
export function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState(() => match(query))

  useEffect(() => {
    const list = globalThis.matchMedia?.(query)
    if (!list) {
      return undefined
    }
    const update = (): void => setMatches(list.matches)
    update()
    list.addEventListener('change', update)
    return () => list.removeEventListener('change', update)
  }, [query])

  return matches
}

/**
 * Evaluate one query now.
 *
 * An environment without `matchMedia` reports `false`, which is the wide layout:
 * every panel is on the page at once, so nothing is unreachable. Defaulting the
 * other way would hide the detail panel behind a drawer that cannot be opened.
 */
function match(query: string): boolean {
  return globalThis.matchMedia?.(query).matches ?? false
}
