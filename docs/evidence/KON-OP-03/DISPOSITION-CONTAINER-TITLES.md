# KON-OP-03 — container titles: fix, capability and pending disposition

## The contract

| Thing | Title |
| --- | --- |
| Task session workspace | `TSW · <jira issue> · <short ticket code>` |
| Seat | `<role_code> · <short ticket code>` |

`SA/SWE/AUD/QA/UAT · OP-03` were already correct and are untouched.

## Root cause and fix — `aaf7f4b`

Topology admission calls `Services::ensure_container`, which builds
`display_name` through `Services::container_name` as
`<kind name_template> · <topology_node_id>`. That is the only name the daemon
*can* build: the Jira issue and the short ticket code are the plane's
configuration, not the control plane's. `PaseoAdapter::bind_native_child` then
used it verbatim, while the older `prepare_workspace` path — same adapter, same
plane — already rendered `TSW · … · …` through `workspace_display_name_for`.

Fixed where the authority is. `bind_native_child` now resolves the title through
`child_display_name`:

- a child that names a delivery task is titled from that task's scope, and the
  caller's `display_name` is ignored;
- a child that names none keeps it — project and epic roots are structural, not
  ticket-scoped, and there is no task scope to render them from;
- a task the plane holds no scope for is **refused before any native mutation**,
  because falling back would put an identity in a human-facing title
  permanently.

No hard-coded epic, task or native id, and no caller-authored native name: the
Jira issue and short code come from `PaseoExecutionScope`, the same typed scope
the workspace path reads.

Correlation is unchanged. The node id stays in the bracketed
`[kontor-node-…]` label every readback resolves by; the tests assert the display
half and the correlation half separately.

**Tests** (`crates/kontor-runtime-paseo/tests/contract.rs`): a task-scoped child
is titled from its ticket and not its node id; a child naming no task keeps the
structural name; an unscoped task is refused with no mutation reaching the
daemon. Mutation-verified — restoring `request.display_name` for task-scoped
children turns the first red.

## `wks_5afdf8a83682cc8e` — rename_pending

**Not renamed. Identity, parent, cwd and bindings unchanged.**

| Field | Value |
| --- | --- |
| Native id | `wks_5afdf8a83682cc8e` |
| Title now | rendered by the pre-`aaf7f4b` rule — `<kind name_template> · <topology_node_id>` |
| Correct title | `TSW · ASMA-7872 · OP-03` |
| Disposition | `rename_pending` — display-only deviation |

### The supported surface, corrected

| Surface | Verbs | Rename? |
| --- | --- | --- |
| CLI this adapter shells out to | `workspace create`, `workspace archive`, `agent run`, `agent update-labels`, `agent reload`, `agent stop`, `agent archive` | no |
| RPC this adapter exchanges | `project.list`, `project.add`, `fetch_workspaces`, `fetch_agents`, `fetch_agent`, `fetch_agent_timeline`, `send_agent_message` | no |
| The same daemon's MCP facade | `rename_workspace(workspace_id, title)` | **yes** |

The first version of this note said no rename verb existed anywhere. That was
wrong, and the correction matters more than the tidier claim: the daemon can
rename a workspace — `rename_workspace` is served on its MCP facade, addressed
by workspace id, setting the user-visible title and nothing else. What is
missing is a **route from this adapter to that operation**.

So the refusal stays `unsupported_capability`, because the adapter genuinely
cannot perform it, and it now says so for the right reason and names the
correction: teach the adapter that request, then read the title back through
`fetch_workspaces` on the same native id. That is the follow-up this note is
handing over.

The two shortcuts that would "work" today are still refused deliberately:

- **archive and recreate** destroys the native id every Kontor binding, seat and
  readback resolves by — a rename that loses the identity is not a rename;
- **writing the daemon's own state** is an undocumented internal surface with no
  contract, no readback and no versioning.

### Closeout

The workspace is left exactly as it was. Renaming it out-of-band through the MCP
facade would be the caller-authored native rename this work exists to remove —
the title has to come from Kontor's typed scope through Kontor's own operation,
or the next container is named wrong again for the same reason this one was. The
correction is therefore authorized as its own step, and when it runs it is one
call against the durable native id with the title above.

Until then: the workspace keeps working, its bindings are correct, and only the
visible title is stale. Every workspace created *after* `aaf7f4b` carries the
right title.

## The rename capability — `RetitleContainer`

Added as a runtime contract so a runtime that *can* rename says so, and one that
cannot refuses precisely rather than vaguely.

- `RuntimeCapability::RetitleContainer`, declared as changing the runtime.
- `RuntimeAdapter::retitle_container`, defaulting to `unsupported_capability`.
- `RetitleContainerRequest` addresses the container by `bound_native_id` **and
  generation only** — never by title (the value being corrected) and never by
  `cwd` (which containers share). No parent, no projection, no placement.
- `RetitleContainerOutcome` carries the title read back from the runtime and
  whether anything changed, so a replay reports `changed: false` and a silently
  ignored rename is distinguishable from a successful one.
- The scripted fake implements it: identity, root and projection preserved, a
  wrong native id or a wrong generation refused as `stale_binding`.
- The Paseo adapter refuses with `unsupported_capability` naming the capability,
  documenting why and what the correction would cost. It does not declare the
  capability, so a caller can tell before asking.
- The AO adapter's `UNSUPPORTED` table gains the same entry with its own reason;
  that crate's oracle — every capability is either declared or explained —
  is what forced it.

**Tests**: `crates/kontor-runtime/tests/retitle_container.rs` (4) and two in the
Paseo contract suite proving the refusal reaches nothing and creates no
replacement container.

## Not delivered: the `/v1` retitle operations

The Admin-facing preview/apply pair is **not** in this delivery, and the reason
is a design dependency I would rather surface than half-wire.

A preview has to answer "what is this container called *now*", and Kontor does
not persist that: `NativeContainerBinding` holds identity, kind, `cwd` and
readback instants, but no title. Making the preview honest therefore needs

1. `ContainerOutcome` to report the title the adapter actually used — the
   adapter may derive a better one than the request carried, which is precisely
   what the fix above does;
2. a schema generation adding `observed_title` to the container binding, plus
   the repository plumbing;
3. a `RetitleContainer` command kind (another closed-list migration);
4. the daemon composition, MCP/CLI registration and the parity-oracle updates.

I drafted (1) and reverted it: a field no operation consumes is a speculative
addition, and shipping the route half-wired is worse than not shipping it.

What exists now is the whole runtime contract, which is the part that had to be
settled first — including the answer to the question the brief actually asked.
Paseo can rename a workspace; this adapter has no route to the operation, so the
capability is undeclared and the refusal is truthful today and cheap to make
false tomorrow.
