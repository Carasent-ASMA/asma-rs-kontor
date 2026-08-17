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

A runtime contract, so a runtime that can rename says so and one that cannot
refuses precisely.

- `RuntimeCapability::RetitleContainer`, declared as changing the runtime.
- `RuntimeAdapter::retitle_container` and `preview_retitle_container`, both
  defaulting to `unsupported_capability`. The preview refuses for every reason the
  apply refuses, because a preview that succeeded against a runtime that cannot
  rename would promise an apply that cannot happen.
- `RetitleContainerRequest` addresses the container by its **durable binding id,
  native id and generation** — never by title (the value being corrected) and
  never by `cwd` (which containers share). No parent, no projection, no
  placement.
- It carries **no finished title**. `structural_name` is what the control plane
  can render alone — the node kind's declared template — and `task_id` names the
  scope the plane renders the rest from. The plane holds the Jira issue and the
  short code; the control plane holds the template and the node. Neither half can
  write this title alone, which is what makes a caller-authored title impossible
  rather than merely rejected.
- `RetitleContainerOutcome` carries the derived title, the title read back from
  the runtime, and whether anything changed — so a replay reports
  `changed: false` and a silently ignored rename is distinguishable from a
  successful one.
- The scripted fake implements both: identity, root and projection preserved; a
  wrong native id or generation refused as `stale_binding`; a task the plane has
  no scope for refused rather than half-titled.
- The AO adapter refuses both with its own reason; that crate's oracle — every
  capability is either declared or explained — is what forced it.

## Paseo renames through the daemon's MCP facade

`rename_workspace(workspaceId, title)` on `POST /mcp/agents`, which is the same
daemon and the same port as the session socket. Verified live before any code was
written: `tools/list` serves it, and `list_workspaces` reports `workspaceId`,
`projectId`, `cwd` and `title` — the exact evidence a readback needs.

| Piece | Where |
| --- | --- |
| The seam | `crates/kontor-runtime-paseo/src/mcp.rs` — one tool call, one answer, and a strict endpoint derivation (`ws://host/ws` → `http://host/mcp/agents`) that refuses a shape it does not recognize rather than guessing at a URL it would send renames to |
| The route | `PaseoAdapter::with_mcp`, optional. No route means the capability is **undeclared**, so a caller can tell before asking, and every adapter built without it behaves exactly as before |
| The plan | `retitle_plan` — capability check, generation bound, ledger agreement, lookup by native id inside the bound project, correlation proved *before* the rename rewrites the string the label lives in, then the title derived |
| The call | id and title and nothing else. Paseo is handed nothing it could move |
| The readback | `fetch_workspaces` on the same native id: same project, same directory, and the exact title, or the answer is a refusal |

Archive-and-recreate and writing the daemon's own state are still refused, for the
original reasons: a rename that loses the native id is not a rename, and an
undocumented surface has no contract and no readback.

**One renderer.** `container_title` is the only place a container's name is
decided, used by the bind path that creates one and the retitle path that repairs
one, correlation label included. A retitle computing its own answer is how a repair
renames a container that was already right — and
`the_retitle_and_the_bind_path_agree_on_what_a_container_is_called` is the proof
that it cannot.

## The `/v1` operations

Two, Admin, on the node that owns the container:

```text
POST /v1/projects/{project_id}/topology/nodes/{topology_node_id}/container:retitle-preview
POST /v1/projects/{project_id}/topology/nodes/{topology_node_id}/container:retitle-apply
```

The body is `{ "expected_revision": <project revision> }`. That is the whole
request: `deny_unknown_fields` means a caller who tries to supply a title is
rejected before any handler sees it, and the project's revision is the one a caller
can actually read before presenting it. The apply takes an `Idempotency-Key`; the
preview takes none, because it is a read.

`retitle_request` in the daemon derives everything else — the node, its pinned
specification, the kind's template, the durable container binding and the runtime
family that holds it. A replay still reads the container back, through the preview
that changes nothing: answering `changed: false` from the receipt ledger alone
would be reporting a title nobody looked at.

Registered in `kontor-mcp` as `kontor_container_retitle_preview` and
`kontor_container_retitle_apply`, both Admin, neither with a title argument.

## Schema v30

One migration, and it adds one thing: the `retitle_container` command kind. No
title column. What a container is called is the runtime's fact, and a stored copy
would be a second answer to a question the runtime answers authoritatively — one
that goes stale the first time anything renames a workspace outside this operation.
`0028` and `0029` are untouched.

## Tests

| Suite | Proves |
| --- | --- |
| `crates/kontor-runtime/tests/retitle_container.rs` (8) | identity/root/projection preserved, readback not echo, replay is not a second change, derivation from the plane's scope, an unscoped task refused with the title left alone, a preview that answers the same thing and writes nothing, a preview refusing what an apply would refuse |
| `crates/kontor-runtime-paseo/tests/contract.rs` (120) | the rename carries exactly two arguments, the readback proves id/project/directory/title, an already-correct container renames nothing, a native id the project does not hold is refused before any call, a generation ahead of the plane's is refused, a rename the daemon did not perform is never reported as done, no facade route means `unsupported_capability` and an undeclared capability |
| `crates/kontor-runtime-paseo/src/mcp.rs` (4) | endpoint derivation and refusal, one server-sent answer read as one result, a JSON-RPC or tool error never read as success |
| `crates/kontor-daemon/tests/loopback_api.rs` | `a_misnamed_container_is_repaired_from_the_pinned_topology_and_never_from_a_caller`: preview → apply → replay → settled preview, plus Observer and Operator refused, a stale project revision `409`, an unknown node `404`, and a caller-supplied title rejected as an unknown field |

**Mutation checks.** A preview that secretly writes the title: caught by the fake
suite and the loopback regression. A readback that echoes the requested title
instead of verifying it: caught by
`a_rename_the_daemon_did_not_perform_is_never_reported_as_done`.

## `wks_5afdf8a83682cc8e` — still not renamed

Its live title reads `TSW · ASMA-7872 · OP-03 [kontor-node-01a00c26-…]`, which is
what this contract renders. Nothing in this delivery touched it: the correction now
has a supported route, and running it against a live workspace is a deployment step
for the LSA rather than a code change. The disposition above stands until then.
