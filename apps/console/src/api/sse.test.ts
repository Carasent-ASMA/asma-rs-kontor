/**
 * Framing an event stream that arrives in arbitrary pieces.
 *
 * The mutants this file exists to kill: dispatching a half-read frame, losing a
 * frame split across two chunks, and treating a keep-alive comment as content.
 */
import { describe, expect, it } from 'vitest'
import { SseParser } from './sse'

describe('SSE parser', () => {
  it('dispatches a frame on the blank line and not before', () => {
    const parser = new SseParser()
    expect(parser.feed('event: control\ndata: {"a":1}\n')).toEqual([])
    expect(parser.feed('\n')).toEqual([{ id: null, event: 'control', data: '{"a":1}' }])
  })

  it('reassembles a frame split across chunk boundaries', () => {
    const parser = new SseParser()
    const stream = 'id: 42\nevent: content\ndata: {"kind":"message"}\n\n'
    const frames = []
    for (const character of stream) {
      frames.push(...parser.feed(character))
    }
    expect(frames).toEqual([
      { id: '42', event: 'content', data: '{"kind":"message"}' },
    ])
  })

  it('joins multiple data lines with newlines', () => {
    const parser = new SseParser()
    expect(parser.feed('data: one\ndata: two\n\n')).toEqual([
      { id: null, event: 'message', data: 'one\ntwo' },
    ])
  })

  it('ignores keep-alive comments', () => {
    const parser = new SseParser()
    expect(parser.feed(':\n\n: keep-alive\n\n')).toEqual([])
    expect(parser.feed('data: x\n\n')).toHaveLength(1)
  })

  it('defaults the event name and tolerates CRLF', () => {
    const parser = new SseParser()
    expect(parser.feed('data: x\r\n\r\n')).toEqual([
      { id: null, event: 'message', data: 'x' },
    ])
  })

  it('strips exactly one leading space from a value', () => {
    const parser = new SseParser()
    expect(parser.feed('data:  padded\n\n')[0]?.data).toBe(' padded')
  })

  it('does not carry a frame field into the next frame', () => {
    const parser = new SseParser()
    const frames = parser.feed('event: control\ndata: a\n\ndata: b\n\n')
    expect(frames).toEqual([
      { id: null, event: 'control', data: 'a' },
      { id: null, event: 'message', data: 'b' },
    ])
  })

  it('emits several frames from one chunk, in order', () => {
    const parser = new SseParser()
    const frames = parser.feed('data: 1\n\ndata: 2\n\ndata: 3\n\n')
    expect(frames.map((frame) => frame.data)).toEqual(['1', '2', '3'])
  })
})
