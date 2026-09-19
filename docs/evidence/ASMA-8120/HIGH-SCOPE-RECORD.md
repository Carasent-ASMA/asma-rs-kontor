# ASMA-8120 high-scope record

Date: 2026-09-19
Artifact: `high-scope-record`
Task: Jira `ASMA-8120` / Kontor `01a07722-c3ed-7a63-94e6-cefd22e438ab`
Epic: Jira `ASMA-8049` / Kontor `01a0539a-51c9-7301-9bd7-26c09167b23e`
Project: `01a0064a-e056-7603-9968-ef64fdaacb75`, revision 7
Phase: `high-scope`, task revision 2
TeamRun: `01a0bb61-b8e0-7bf0-a14f-b44713ca10d7`

## Outcome

Complete ASMA-8120 only through the existing supported Kontor surfaces:

1. confirm the compatible deployed runtime and close the attachment-convergence
   question;
2. validate and publish the four naming-only successors prepared by ASMA-8117;
3. select Operational v6 as the future project default;
4. canary ASMA-8049, then migrate each other eligible current epic through
   `team_definition_upgrade` preview/apply with exact identity readback;
5. prove restart persistence and preserve the retained definitions and binary
   deployment as the rollback route.

No new API, schema, migration runner, naming path, topology, or replacement
container is needed. Missing or ambiguous identity inputs are fenced under
[the open-question ledger](OPEN-QUESTIONS.md); they are never guessed.

This record is controlled by Jira ASMA-8120, TASK-005 in
`plan/feature-kontor-jira-key-identity-1.md`, and the governing Jira-key
identity architecture amendment. It does not authorize ASMA-8121 closeout
mutations.

## Frozen deployment evidence

PR #243 merged as
`95bcf8683c3db1635bb7b4ff378e5504274f483d`. The canonical worktree HEAD is
its reviewed source commit `ccfb4555025f97cfeab820ceec920488eb504cd5`;
both have tree `35f41e501a9649d883e97660e7c3957073d90011`.

Deployment bundle:
`~/.local/state/kontor/asma/deployments/ASMA-8120-20260919T205522Z-95bcf86`.
The staged and live binaries were byte-identical at inspection:

| Binary | SHA-256 |
|---|---|
| `kontor` | `405c95a7056741f1ce752419d1b233c86027eb067fd54e4a0eab2ffb255cd6b3` |
| `kontor-daemon` | `b2cd455af57c3c4dc9bcfc1a832fc50b012a5345ea23c4b70796cbfded4b3bd9` |
| `kontor-mcp` | `464d81a6260892339630ebf1d0b2c556e941252e714ec7b67bce2fd2a4fa212b` |

The retained predecessor differs only in `kontor-daemon` and has hash
`cdf608a1dcae74fe9e12e90fa756194d0e9831a791a803ec97e218aa97054bcf`;
its protected database receipt has hash
`96b3a9fef5b128337c5c30c65ff3ef5d1c0f42fae0f6483cbcc867d7c3a24554`.
Rollback must use the retained binary/definitions and supported Kontor
receipts, never write an old database image over live state.

The live database remained schema 108 with `PRAGMA integrity_check = ok` and
no foreign-key violations. The post-deploy launchd process started at
`2026-09-19T20:55:48Z`; its barrier opened after binding re-attestation at
`20:56:19Z`. Deployment convergence remains subject to OQ-8120-01.

The root-cause change rehydrates the exact inspected native-child binding and
its parent project into the runtime adapter's ephemeral placement ledger. It
does not weaken binding equality or introduce title/cwd discovery. The focused
check passed:

```text
cargo test -p kontor-runtime-paseo --test contract \
  exact_container_inspection_rehydrates_launch_placement_after_restart -- --exact
# 1 passed; 0 failed; 236 filtered out
```

PR verification also records all 237 Paseo contract tests, strict crate
Clippy, formatting and diff checks green.

## Immutable successor set

ASMA-8117 prepared four complete candidates. A fresh live validation on
2026-09-19 returned no violations and reproduced these hashes:

| Source | Target | Target canonical hash |
|---|---|---|
| Operational v1 | Operational v4 | `d3775bd5fbebadeec06cc235793be23248de2fed23c4ef01e3a523fecc282284` |
| Operational v2 | Operational v5 | `10a0682b16786a3f94400d1d9562dfd9a8dd34379fef9c319a18a92a95989fc1` |
| Operational v3 | Operational v6 | `24e2c5810060ef521e6af087e3430754bab86eaf138868fe125aa4fdd2904794` |
| Recovery v2 | Recovery v3 | `c4c2f19a52258872dd0fb68ec8c648ad9d6ae2674ad0e014cce42a270644ae73` |

Operational lineage: `01936f5a-2000-7000-8000-000000000001`.
Recovery lineage: `01a07400-1000-7000-8000-000000008098`.
The candidates live under
`docs/evidence/ASMA-8117/team-definition-successors/` and differ from their
recorded sources only by version and the approved item-code to Jira-key token
substitutions. All four target revisions were unpublished at the scope read.

Publish all four before selecting or upgrading anything. For each candidate,
reuse its validation hash, publish with fresh project revision and one stable
idempotency key, then re-read the exact `{id, version, canonical_hash}`. A
partial receipt is pending work and is retried only with that same key.

After every target is confirmed published, preview and select Operational v6
as the future default. Its selection must not rename an existing epic.

## Baseline current-fleet census

The live schema-108 topology contains 143 nodes. The runtime advertises the
required discovery, prepare, launch, inspect/adopt, history, live-event,
retitle and lifecycle capabilities. Thirteen current epics have both a
confirmed Jira binding and an exact source pin and are eligible for supported
upgrade preview:

| Target | Current epics | Active bound / active unbound / archived-retired |
|---|---|---|
| Operational v4 | ASMA-8109 | 5 / 0 / 0 |
| Operational v5 | ASMA-7869, ASMA-8108, ASMA-8111 | 27 / 0 / 18 |
| Operational v6 | ASMA-8049, ASMA-8101, ASMA-8113, ASMA-8155, ASMA-8186, ASMA-8188, ASMA-8190, ASMA-8208 | 34 / 7 / 5 |
| Recovery v3 | ASMA-8098 | 6 / 0 / 1 |

Archived and retired nodes are census evidence only and must remain untouched.
Unbound active nodes are not invented as runtime targets; the supported
preview decides whether their containing epic can proceed and any typed refusal
fences that exact epic.

Four other active epics lack confirmed migration inputs and are listed in
OQ-8120-02. They are excluded from mutation unless that question is resolved
with a supported binding/pin path.

A fresh current-definition native-name preview for canary ASMA-8049 at project
revision 7 returned eleven live targets: ESW, ECP, five TSWs and four seats.
Each observed name exactly matched its current KBI-based desired name and no
target would change under the current pin. The preview included the current
ASMA-8120 scope and implementation seats and excluded archived nodes.

## Ordered rollout and stop conditions

Every mutating step starts from fresh project/epic/definition reads and a clean
database integrity/foreign-key check. Preserve one protected pre-rollout
backup receipt. Never restore it over a newer live database.

1. Close OQ-8120-01. If convergence is not proven, stop before publication.
2. Validate, publish and re-read the four immutable successors one at a time.
3. Preview/select/re-read the project default as Operational v6.
4. Preview ASMA-8049 from Operational v3 to v6. Require exact source pin,
   expected project revision, the expected Jira-key desired names, and no
   unexplained target omission/addition. Capture the preview hash.
5. Apply the unchanged canary request with that hash and a stable idempotency
   key. Re-read the epic pin, every affected native name and the identity tuple
   for every target: Kontor id, native id, parent, kind, cwd, seat binding,
   lifecycle and evidence linkage. A mismatch or partial effect fences the
   canary and blocks the fleet.
6. With the canary green, repeat preview/apply/readback sequentially for the
   remaining twelve eligible epics in the table. Use each epic's mapped
   successor; never bulk-edit or manually retitle.
7. Restart the deployed daemon once through the supported service route, wait
   for its startup barrier, and repeat exact pin/name/identity reads for all
   migrated epics. Run database integrity and foreign-key checks again.

For every epic, a revision conflict triggers a fresh preview; it does not
reuse the old hash. A transport timeout with an issued receipt is retried only
with the same idempotency key. A typed partial effect remains pending and
fenced until the same request is reconciled. Any identity change, archived
target, missing confirmation, ambiguous legacy consultation topic, unexpected
native target, or readback mismatch stops that epic and all later rollout.

Rollback means stop and use retained immutable source definitions plus the
supported preview/apply route where it validates. If that route refuses, leave
the exact state fenced for operator recovery. Never replace topology, allocate
new native identities, infer names from titles, manually edit the database, or
overwrite it from backup.

## Required implementation evidence

The `high-change` artifact must contain:

- the merged source/tree, bundle path, all live hashes, schema and startup
  receipts;
- publication and default-selection receipts with exact revisions, hashes and
  readbacks;
- one row per current epic, including the four fenced entries, with binding,
  source/target pin, preview/apply receipt, target counts and disposition;
- before/after/restart identity tuples for every migrated target;
- database integrity, foreign-key and archived/retired non-effect evidence;
- the exact focused regression command above and the full PR verification
  receipts;
- any still-open ledger entry and its explicit fence.

No new mutation campaign is added here: ASMA-8121 already owns MUT-003 and the
full-fleet closeout proof. Reusing the existing supported upgrade surface and
the one restart-rehydration regression is the smallest complete ASMA-8120
change.

## Handoff

Hand this artifact to the already-attached implementation AgentRun
`01a0bb74-ee78-7aa3-b6bc-bd9c43733496` (native
`6426ec59-57d0-4001-922c-86fc0f2de258`). It must continue in this canonical
worktree, preserve all current identities, close OQ-8120-01 before the first
publication, and keep OQ-8120-02 fenced until an authoritative disposition is
recorded. The post-turn control caller may settle the scope turn against task
revision 2 after this artifact revision is approved; this turn does not claim
its own terminal response as settled evidence.
