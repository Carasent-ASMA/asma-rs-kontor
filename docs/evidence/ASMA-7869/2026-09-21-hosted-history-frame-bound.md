# ASMA-7869 hosted message history recovery

Observed on 2026-09-21. This is operator repair evidence, not a producer verdict
or an epic completion claim. The watchdog remains stopped.

## Operational gap

`kontor_topology_seat_message_send` refused the same supported resumption twice
with HTTP 503 and `the session's runtime could not be reached`. The session was
idle, unarchived and readable through Paseo. No new activity was observed.

- Realm: `01a00649-9ee6-73e0-ba1b-6a6c35cfd065`
- Project: `01a0064a-e056-7603-9968-ef64fdaacb75`
- Epic: `01a0074f-6719-7570-adf7-95ee3ec69875` (ASMA-7869)
- Seat: `01a02b8e-8f63-7161-b043-cf8cc6d1297e`
- Native: `0836cd42-5ea6-47b3-8577-becf085ce0b8`
- Key: `direct-repair-20260921-7869-residual-source-qualification-v1`
- Completion checkpoint: generation 3, revision 9, ticket gate.
- Owner: root. Status: source qualified; deployment and original-key replay pending.

The bounded fallback was a read-only native canonical timeline request. A
five-entry page was readable. A 500-entry request returned all 125 entries,
sequences 1–125 in epoch `1cfa3d66-4d23-430b-b65f-0ab739723654`, in a
9,629,998-byte frame. Kontor's existing 8 MiB frame bound rejected that response
before it could determine whether the message existed. No direct native send,
credential substitution, topology mutation or history edit was performed.

## Correction and verification

Canonical history reads now halve their requested entry count only when the
existing transport or decoder refuses an oversized frame. Every retry preserves
the native, projection, direction and cursor. The wire bound stays 8 MiB. One
oversized entry still refuses. No retry in this path sends a message.

The 119 Paseo unit tests and 271 adapter contracts pass. The added hosted replay
case proves an oversized history can recover the exact existing delivery without
a send, and checks the actual page limits and unchanged cursor. The existing
oversized-frame test now also proves the retries stop at one entry.

| Mutant | Assertion | Result |
| --- | --- | --- |
| Disable smaller-page recovery | Existing delivery must be recovered | Killed |
| Divide page size by three instead of two | Actual requested limits must be 500 then 250 | Killed |
| Stop shrinking before one entry | Refusal read limits must be 10, 5, 2, 1 | Killed |

Each mutant was applied alone and restored byte-for-byte. This evidence does not
confirm delivery of the held live key. After deployment, resume that exact key
through Kontor and retain its canonical acknowledgement before routing more work.
