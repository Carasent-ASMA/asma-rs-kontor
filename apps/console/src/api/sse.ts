/**
 * An incremental `text/event-stream` parser.
 *
 * The browser's own `EventSource` cannot carry an `Authorization` header, and
 * every route of this contract requires one — so both SSE routes are read as a
 * `fetch` body and framed here. Keeping the framing pure (text in, frames out)
 * is also what lets the stream rules be tested without a socket.
 */

/** One dispatched server-sent event. */
export interface SseFrame {
  /** The `id:` field, when the server sent one. */
  readonly id: string | null
  /** The `event:` field, defaulting to `message` as the specification requires. */
  readonly event: string
  /** The `data:` field, with multiple data lines joined by newlines. */
  readonly data: string
}

/**
 * Frames a byte-split event stream.
 *
 * Chunks arrive at arbitrary boundaries, so a partial line is held until the
 * rest of it turns up. A frame is dispatched on a blank line and never before —
 * dispatching a half-read `data:` is how a console renders a truncated payload
 * and calls it content.
 */
export class SseParser {
  /** The tail of the last chunk, up to the first unterminated newline. */
  #buffer = ''
  /** The `data:` lines of the frame being assembled. */
  #data: string[] = []
  /** The `event:` field of the frame being assembled. */
  #event: string | null = null
  /** The `id:` field of the frame being assembled. */
  #id: string | null = null

  /** Feed one chunk and take whatever complete frames it finished. */
  feed(chunk: string): SseFrame[] {
    this.#buffer += chunk
    const frames: SseFrame[] = []
    // Normalize the three line terminators the specification allows before
    // splitting, so a CRLF stream does not leave a stray CR on every field.
    const normalized = this.#buffer.replace(/\r\n|\r/g, '\n')
    const lines = normalized.split('\n')
    // The last element is either an unterminated line or the empty string that
    // follows a trailing newline. Either way it is not complete yet.
    this.#buffer = lines.pop() ?? ''

    for (const line of lines) {
      if (line === '') {
        const frame = this.#dispatch()
        if (frame) {
          frames.push(frame)
        }
        continue
      }
      // A line that starts with a colon is a comment. Keep-alives are exactly
      // this, and treating one as a field would inject an empty frame.
      if (line.startsWith(':')) {
        continue
      }
      const colon = line.indexOf(':')
      const field = colon === -1 ? line : line.slice(0, colon)
      const rawValue = colon === -1 ? '' : line.slice(colon + 1)
      const value = rawValue.startsWith(' ') ? rawValue.slice(1) : rawValue
      switch (field) {
        case 'data':
          this.#data.push(value)
          break
        case 'event':
          this.#event = value
          break
        case 'id':
          this.#id = value
          break
        default:
          // `retry:` and anything unknown are ignored, per the specification.
          break
      }
    }
    return frames
  }

  /** Finish the frame in progress, if it has any data. */
  #dispatch(): SseFrame | null {
    if (this.#data.length === 0) {
      this.#event = null
      this.#id = null
      return null
    }
    const frame: SseFrame = {
      id: this.#id,
      event: this.#event ?? 'message',
      data: this.#data.join('\n'),
    }
    this.#data = []
    this.#event = null
    this.#id = null
    return frame
  }
}
