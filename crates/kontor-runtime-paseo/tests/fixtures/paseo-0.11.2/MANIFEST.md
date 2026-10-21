# Paseo 0.11.2 contract fixtures (KR-08 / TEST-004)

These fixtures are the Paseo 0.11.2 contract baseline for the adapter
alignment. They are sanitized copies of the shapes in the tagged upstream
source:

- `packages/protocol/src/messages.ts` at tag `v0.11.2`
  (`CreateAgentRequestMessageSchema`, `AgentCreateRequestSchema`,
  `CreationSubscribeRequestSchema`, `AgentCreateResponseSchema`,
  `CreationSubscribeResponseSchema`, `SendAgentMessageResponseMessageSchema`,
  `ServerInfoStatusPayloadSchema`).
- `packages/server/src/server/creation/index.ts` at tag `v0.11.2`
  (the durable creation phases and `outcomeUnknown`).
- `packages/server/src/server/message-receipts/index.ts` at tag `v0.11.2`
  (the `agent_request_outcome_unknown` message receipt).

Shapes that are byte-identical between 0.8.0 and 0.11.2 reuse the 0.8.0
recordings under `../paseo-0.8.0/` (agent snapshot, agent list, timeline,
permission resolution). Only the 0.11 shapes that changed or were added are
copied here.

The placeholder `__IDEMPOTENCY_KEY__` is replaced by the test with the exact
launch key the adapter generated, because the fixture cannot know a UUID the
test mints. Nothing else in these files is substituted.
