# WP-05 to WP-07 consumer gaps

The ASMA consumer leg is WP-07. WP-05 does not claim that ASMA's
current checks pass, and this pull request does not edit the private ASMA
checkout.

With explicit `KEY=PATH` resource roots and the scoped options below, the only
remaining `pack.py check` finding on an `<asma-modules>` checkout is the
inherited `asma-pr-review` mirror drift. That drift is not introduced here and
is left to its owner.

| Gap | Disposition |
| --- | --- |
| Plan graphs | `pack.py check` defaults to `--plan-graphs all`. ASMA validates changed plans only, so WP-07 runs `--plan-graphs changed --base <base>` on the PR range, or `--plan-graphs marker` for marker-bearing plans only. |
| Non-pack skills | Placeholder refusal is limited to the pack's own rendered skill names. ASMA-only skills keep their own template syntax. |
| Links | ASMA-only skills and legacy `_docs` pages use the workspace-relative `asma-modules/` convention and cite module repositories that are not cloned in the same checkout. WP-05 refuses links that leave the rendered skill tree; it does not claim ASMA's full private link graph. WP-07 renders the pack skills, removes the private duplicates and runs the owning link checker. |
| Routed-once | Nested routers and case-insensitive `When to Load` tables are supported. ASMA's `_docs` has no single root router and keeps legacy root documents, so WP-07 either adds the owning index or records their exclusion. Until then the consumer leg uses `--no-routed-once`. |
| Public-example lint | The ASMA documentation tree is private. WP-07 defines the public boundary and runs the lint there; the consumer leg uses `--no-public-lint`. |
| Mirror drift | The inherited `asma-pr-review` `.agents` mirror differs from its canonical copy. The owning change fixes it; WP-05 does not. |

The consumer leg used for this handoff is:

```sh
python3 scripts/pack.py --config-dir <asma-config> check <asma-modules> \
  --resource-root modules=<asma-modules> --resource-root kontor=<asma-rs-kontor> \
  --plan-graphs marker --no-links --no-routed-once --no-public-lint
```

Set `ASMA_PLAN_GRAPH_VALIDATOR` to the owning ASMA validator when plan graphs
are present; `pack.py` refuses a set-but-missing path rather than silently
skipping validation.

It reports one error: the inherited `asma-pr-review` mirror drift. WP-07 owns
the full ASMA check and the gaps above.

The WP-03 pack-skill tests remain in the `scripts/tests` lane; WP-05 adds the
render/check tests beside them rather than moving the owning suite.
