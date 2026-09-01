# ASMA-8062 / KBI-8062 — open questions

Date: 2026-09-01
Raised by: high-scope seat `01a05e46-f96a-7a00-b081-ed4773408e53`
Status: closed — all four questions disposed by the user's explicit naming agreement

This ledger attaches to
[`2026-09-01-21-45-plan-configuration-driven-native-naming-live-migration.md`](2026-09-01-21-45-plan-configuration-driven-native-naming-live-migration.md)
and the approved contract in [`../../docs/NATIVE_NAMING.md`](../../docs/NATIVE_NAMING.md).
The user's ASMA-8062 naming agreement closes every question below. Its decisions
are supported by the approved contract's single-authority, fail-closed,
identity-preserving and historical-evidence rules.

## OQ-KBI-8062-001 — Team Definition aggregate boundary

**Subject:** the durable aggregate that owns hierarchy and native naming.

**Attaches to:** plan §3.1 and `docs/NATIVE_NAMING.md` “Recommended definition
shape”.

**Why ambiguous:** the approved contract requires one immutable Team Definition
revision, but the repository has no such aggregate. `ProjectSessionTopologySpec`
currently owns hierarchy and naming, while `TeamTemplateSpec` owns role slots and
handoffs. An epic may use several delivery, Advisor and Committee templates, so
extending any one current team template could give the same epic conflicting
container authorities. No approved record chooses the storage boundary.

**Options seen:**

1. Add the plan's proposed epic-pinned `TeamDefinitionSpec`, referencing a
   topology revision for legality and containing the one hierarchy/naming policy.
2. Extend `TeamTemplateSpec`; this needs an additional rule selecting which of
   several templates owns epic containers.
3. Keep `ProjectSessionTopologySpec` as the naming authority; this contradicts
   the approved contract.

**Decision — CLOSED:** option 1. `TeamDefinitionSpec` is the epic-wide immutable
authority. The approved contract states that one immutable, versioned Team
Definition owns hierarchy and native naming, while
`ProjectSessionTopologySpec` validates structural legality and runtime
projection capabilities rather than acting as a second naming authority. Every
run inherits the epic's exact pin.

## OQ-KBI-8062-002 — topics for legacy ASW/CSW objects

**Subject:** the source of the required `<TOPIC>` when migrating an existing
consultation.

**Attaches to:** plan §§3.2–3.4 and the ASW/CSW rows in
`docs/NATIVE_NAMING.md`.

**Why ambiguous:** new invocations can require and persist a topic, but
`StoredConsultationRun` currently stores only the full question. Existing
ASW/CSW objects therefore have no authoritative topic value, and the naming
contract forbids deriving one from a question, profile, title, UUID or AI label.

**Options seen:**

1. Require an explicit per-consultation topic map in migration preview/apply and
   persist the supplied value with migration provenance.
2. Leave legacy consultations on their historical names while migrating other
   objects, which does not satisfy a complete whole-epic migration.
3. Derive a topic from existing prose or labels, which the approved fail-closed
   contract forbids.

**Decision — CLOSED:** option 1. Legacy consultations require an explicit topic
mapping; preview refuses every unmapped consultation before runtime mutation.
The approved contract requires the topic token, fails on a missing or ambiguous
topic, forbids fallbacks from titles, UUIDs and legacy codes, and preserves old
literal names as historical evidence rather than treating them as current
templates.

## OQ-KBI-8062-003 — pin switch across partial runtime effects

**Subject:** when the epic/run Team Definition pin becomes current during an
identity-preserving multi-object retitle.

**Attaches to:** plan §§3.4 and 8.

**Why ambiguous:** the plan says apply the pin and retitles from one complete
preview, but also allows typed partial runtime progress. SQLite cannot transact
atomically with several external runtime renames, so a crash can occur between
any two effects.

**Options seen:**

1. Persist a resumable migration intent, retain the old pin while applying exact
   retitles, then switch all governed pins only after every target reads back the
   desired title and unchanged identity.
2. Switch pins before retitles, leaving current configuration inconsistent with
   native state after a partial failure.
3. Attempt compensating renames, adding an unsafe rollback path across an
   external runtime.

**Decision — CLOSED:** option 1. Persist the resumable intent, retain the old pin,
retitle exact bound natives, verify every unchanged native id and desired title,
then switch the governed pins. The approved contract makes display names
non-identifying, requires existing runs to keep their pin until explicit
preview/apply succeeds, and forbids recreation as migration. Recovery remains
same-key replay.

## OQ-KBI-8062-004 — Jira connector alias cleanup ownership

**Subject:** whether canonicalizing historical `jira` versus `connector.jira`
links belongs to this naming task.

**Attaches to:** plan §2 and REQ-009.

**Why ambiguous:** neither ASMA-8062's title nor the approved native-naming
contract assigns connector-alias cleanup. The live KBI graph carries both link
labels but already exposes one durable confirmed Jira execution scope and runs
this task. No evidence yet shows the historical alias blocks Team Definition
rendering or retitle.

**Options seen:**

1. Keep connector canonicalization in ASMA-8062 and prove it is required by a
   failing naming or migration contract.
2. Preserve both historical link records and defer alias canonicalization to a
   separate connector task.
3. Delete or rewrite duplicate history, which violates the evidence contract.

**Decision — CLOSED:** option 2 unless a focused failing migration test proves
alias canonicalization is a prerequisite. Historical connector evidence is
preserved; canonical alias behavior may be delivered as a separate contained
fix. The approved contract requires old receipts and literal evidence to retain
their original bytes and forbids rewriting history into current configuration.
Option 3 remains forbidden.
