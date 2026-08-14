# KON-MVP-20 corrective close-out proof

## Committed MCP-only journey

`crates/kontor-daemon/tests/mcp_journey.rs:289-697` starts with an empty realm
and closes the epic through public MCP tools only. One `RouterTransport`, one
admin caller, and one credential perform discovery, project/epic application,
scheduler admission, role settlement, task close, and epic close. There is no
manual native-session administration, direct store call, or direct runtime
call. The observer companion test proves read-only authority.

Targeted archive command:

```text
cargo test -p kontor-daemon --test mcp_journey --locked
2 passed; 0 failed (exit 0)
```

The command log SHA-256 is
`bc4a1d6c95c78edf5b53cb79d271f22fd34cd3d6f319af816be797e6861fe554`.

## Real Paseo evidence, kept honest

External immutable input:
`/Users/igor/kon-mvp-20-scratch/evidence/kon18-closeout/live-20260814T193824Z/`.
Its aggregate file-manifest hash is
`ef2106e655384c7981ac232e726af165586094d48e0e77727ab009a36f592833`;
`manifest.json`, `verdict.json`, and `cleanup.json` hash to
`da7573010353b3bf02000ce961c7c4eb4469b808d774b63ef23cae9159e9946a`,
`cc3fc7b1ba7e2c02557ee08f807dc218b531835f1bbb97790cca974e3bfad258`,
and `6ea4cc3edf1561193bf94a438202718ceecb31b3e80687c865ea7183c8d43eba`
respectively.

The bundle proves a real Paseo CLI/daemon `0.3.1` at `127.0.0.1:6767`, a
reachable Grade-A plane, both bound seats settled by `kontor_turn_settle`, and
restart preservation of native session identity and one-effect message
identity. Cleanup stopped kontord, closed all three MCP children, archived the
workspace and both agents, and left no run-owned workspace. One empty
disposable project remains intentionally because Paseo 0.3.1 has no project
deletion surface.

The live bundle's own verdict remains, deliberately, **NON_COMPLIANT: 38 pass,
0 fail, 4 blocked**. Its harness did not invoke `kontor_role_slot_waive` for two
declared-but-unbound seats; it also left its gap/mutation cases blocked. This is
not relabelled. The committed deterministic journey and sealed waiver contract
supply the missing harness coverage, as independently recorded by:

- `docs/evidence/KON-MVP-18/QA-MCP-JOURNEY.md:68-128` — `COMPOSITE_PASS`;
- `docs/evidence/KON-MVP-18/AUDIT-MCP-JOURNEY.md:12-120` — `AUDITED_TRUE`.

Thus the composite corrective proof passes while retaining the negative live
verdict as a truthful source record. This evidence does not itself authorize
epic close; the mandatory final committee remains the next gate.
