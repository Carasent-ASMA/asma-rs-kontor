# ASMA-7869 legacy confirmation provenance

This note is the non-secret, repository-readable evidence for the schema-98
remediation of the Committee finding against schema 97. It records only typed
receipt metadata and aggregate counts. It does not treat a gate citation as
producer evidence and does not reproduce command intent bodies.

## The two completion receipts are native confirmations

The deployed realm readback on 2026-09-16 returned these exact receipts:

| Receipt | Kind | Mode | Final state | Intent/result digest | Latest transition |
|---|---|---|---|---|---|
| `01a0aaeb-d6a7-7933-a532-e0abb545a4f9` | `advance_completion` | `local` | `confirmed` | `15f192ce76065b461417549a75538e13affce70d98780e364a62495d29d1a434` | sequence 2, `confirmed`, same evidence digest, `2026-09-16T15:52:57.77809Z` |
| `01a0aaee-20b6-76b2-9cfe-d906cedd4d36` | `advance_completion` | `local` | `confirmed` | `605b04fc41bc086bbaa9425f0f8113917d75e740ed9a7478500475bb4e88687d` | sequence 2, `confirmed`, same evidence digest, `2026-09-16T15:55:27.810698Z` |

Both receipts have sequence 1 `intent_persisted` followed by sequence 2
`confirmed`. They are not reconstructed task/gate receipts and are unaffected
by the schema-98 compatibility migration.

## Disputed cutoff check

The Committee correctly rejected schema 97's use of repository commit time as
a proxy for deployment time. A read-only query of the live realm over the
disputed interval `[2026-08-22T06:31:17Z, 2026-08-22T06:57:10Z)` returned zero
candidate local task-closure or gate-verdict receipts. That fact means removing
the cutoff does not reclassify an unseen receipt in that interval, but it does
not make the cutoff valid; schema 98 removes it.

## Schema-98 copied-realm preflight

The schema-98 migration was applied to a copy of the schema-97 live database.
The copy produced this typed provenance census:

| Source | Disposition | Receipt kind | Count |
|---|---|---|---:|
| `v97_reconstruction` | `certified` | `record_gate_verdict` | 92 |
| `v97_reconstruction` | `certified` | `transition_task` | 13 |
| `v98_reconstruction` | `certified` | `record_gate_verdict` | 1 |

The one post-cutoff row is receipt
`01a02850-7232-70f3-ba72-61bad2ace699`. It maps uniquely to
`gate:01a01adf-68e5-7b21-b762-db1a7d3d1ef0:code-review-gate:1`, recorded at
`2026-08-22T07:12:35.908677Z`, and receives certificate
`legacy-local-confirmation-v98:01a02850-7232-70f3-ba72-61bad2ace699`. It is
admitted because it has one exact receipt-to-mutation match, not because of its
date. The preflight also returned:

- zero rows whose latest transition disagreed with their typed disposition;
- `PRAGMA integrity_check = ok`;
- no foreign-key violations.

## Contract

Schema 98 separates runtime confirmation from compatibility reconstruction:

- runtime-confirmed receipts retain `result_ref == intent_hash`;
- reconstructed receipts receive a unique
  `legacy-local-confirmation-v98:<receipt-id>` certificate;
- the certificate has immutable provenance naming the exact mutation;
- a receipt is certified only when it matches exactly one mutation and that
  mutation matches exactly one receipt;
- ambiguous reconstructions receive a later `confirmation_unknown` transition
  when their receipt is still actionable, and cannot satisfy a closure
  certificate;
- the closure reader requires the receipt's latest transition to agree with
  its current state and result reference.

The durable producer-evidence query remains unchanged. This remediation does
not allow a gate verdict to manufacture producer evidence; it makes the
separate legacy closure certificate auditable and injective.
