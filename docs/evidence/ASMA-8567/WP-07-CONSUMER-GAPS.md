# WP-05 to WP-07 consumer gaps

The ASMA end-to-end consumer run is WP-07. WP-05 does not claim that the parent
ASMA config passes `pack.py check`, and this pull request does not edit the
private ASMA checkout.

| Gap | Disposition |
| --- | --- |
| Plan graphs | `pack.py check` defaults to `--plan-graphs all`. ASMA validates changed plans only, so WP-07 runs `--plan-graphs changed --base <base>` on the project checkout, or `--plan-graphs marker` for marker-bearing plans only. Changed mode does not read sibling resource-root git ranges; that remains a named follow-up. |
| Non-pack skills | Placeholder refusal is limited to the pack's own rendered skill names. ASMA-only skills keep their own template syntax. |
| Links | ASMA-only skills and legacy `_docs` pages use the workspace-relative `asma-modules/` convention and cite module repositories that are not cloned in the same checkout. WP-05 refuses links that leave the rendered skill tree; it does not claim ASMA's full private link graph. WP-07 renders the pack skills, removes the private duplicates and runs the owning link checker. |
| Routed-once | Nested routers and case-insensitive `When to Load` tables are supported. ASMA's `_docs` has no single root router and keeps legacy root documents, so WP-07 either adds the owning index or records an owned exclusion. `--no-routed-once` only disables this check and is not a pass. |
| Public-example lint | The ASMA documentation tree is private. `scripts/public_tree.py` owns the public boundary. `--no-public-lint` only suppresses this check for the private tree and is not a pass; WP-07 runs the check on the selected public subtree. |
| Consumer config | The parent ASMA config folder currently refuses `pack.py check` with `missing project`. No retained receipt is claimed for a full consumer run; WP-07 prepares that config folder and records the real run. |
| Mirror drift | The inherited `asma-pr-review` `.agents` mirror differs from its canonical copy. The owning change fixes it; WP-05 does not. `_tools/scripts/check-codex-agent-config.py` remains in place for the mirror and adapter contract. |

`check-codex-agent-config.py` remains in place. The equivalent owning checks
for scoped legacy/private exclusions are `_tools/scripts/check-docs-links.py`
(relative links and anchors, including the workspace-relative and
uncloned-module rules), `_tools/scripts/check-skill-mirrors.py` (skill mirror),
`_tools/scripts/check-instruction-duplicates.py` (duplicate rule homes), and
`_tools/ai-orchestration/plan_graph.py` (plan graphs). Routed-once has no
existing equivalent; WP-07 must add the root index or record an owned
exclusion. A disabled pack switch (`--no-links`, `--no-routed-once`,
`--no-public-lint`) is not a pass: it only suppresses that pack check.

`pack.py` accepts `KONTOR_PLAN_GRAPH_VALIDATOR` and the compatible
`ASMA_PLAN_GRAPH_VALIDATOR`; a set-but-missing path is refused rather than
silently skipping validation.

## Named non-blocking follow-ups

- Changed-plan scope: `--plan-graphs changed` reads the project root's git
  range. Plans in sibling resource-root checkouts need a per-root base before
  WP-07 can use changed mode for them.
- PyYAML parity: the PyYAML-first branch is dispatch-tested with a stub because
  the public test environment has no PyYAML. Real PyYAML duplicate-key and
  YAML-1.1 scalar behavior needs an optional-dependency test.
- WP-02 country codes: `holiday_countries: [NO]` parses as `[False]` under
  YAML 1.1, including the fallback after the YAML 1.1 alignment. No current
  pack render consumes it. WP-02 must quote/type-check the ISO country codes
  (for example `["NO"]`) and validate the list.

## Evidence boundary

The WP-05 implementation merged at `e2b56390` is not TASK-005, TEST-001 or
TEST-003 end-to-end qualification. Only WP-07's pinned combined pack proof,
against the real ASMA config and its owning checks, can close those. This
ledger records the dependency; it is not a combined unconditional PASS.

The WP-03 pack-skill tests remain in the `scripts/tests` lane; WP-05 adds the
render/check tests beside them rather than moving the owning suite.
