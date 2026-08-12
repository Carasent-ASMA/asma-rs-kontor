/**
 * Choosing a layout in a headless test.
 *
 * jsdom performs no layout, so the breakpoint cannot be exercised by resizing.
 * What *can* be exercised is the thing that actually matters: which component
 * tree each layout produces, and whether the narrow one is reachable and
 * dismissible from the keyboard.
 */

/** The listeners one stubbed query is holding. */
type Listener = () => void

/** Install a `matchMedia` that answers as the named viewport would. */
export function setViewport(size: 'phone' | 'desktop'): void {
  const narrow = size === 'phone'
  const listeners = new Set<Listener>()
  Object.defineProperty(globalThis, 'matchMedia', {
    configurable: true,
    writable: true,
    value: (query: string) => ({
      // The console asks exactly one question: is this a narrow viewport.
      matches: query.includes('max-width') ? narrow : !narrow,
      media: query,
      onchange: null,
      addEventListener: (_: string, listener: Listener) => listeners.add(listener),
      removeEventListener: (_: string, listener: Listener) => listeners.delete(listener),
      addListener: (listener: Listener) => listeners.add(listener),
      removeListener: (listener: Listener) => listeners.delete(listener),
      dispatchEvent: () => true,
    }),
  })
}
