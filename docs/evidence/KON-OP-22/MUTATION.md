# KON-OP-22 mutation proof

Date: 2026-09-03

## Killed mutation

The Jira entity-neutral apply boundary was deliberately changed to accept an
`applied` response without refetch confirmation.

The exact regression
`entity_neutral_delegation_binds_apply_to_observation_route_and_readback`
failed because the malformed response was accepted. The confirmation guard was
restored and the same test passed. No mutation remains in the tree.

## Test-strengthening result

A preliminary mutation that emitted an append signal after every epic-conflict
attempt survived the original request-count-only assertion because the
existing open conflict short-circuits subsequent durable insertion. Production
was restored immediately. The regression now additionally observes the append
signal directly and proves a deduplicated conflict publishes no new wake. This
exercise changed the test only; it did not weaken or remove the production
`inserted` guard.
