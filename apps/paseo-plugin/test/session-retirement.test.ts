import { describe, expect, it } from "vitest";
import {
  deriveSessionProjection, MISSING_OWNER_ADAPTER, READY_IS_DESCRIPTIVE,
  type ArchiveIntentObservation, type DutyObservation, type HoldObservation, type NativeSessionObservation,
  type OwnerReleaseObservation, type ProjectionConflict, type ProjectionFreshness, type ProjectionSource,
  type ProjectionSourceKind, type ScopeBinding, type ScopeClaim, type ScopeClaimKind, type SessionProjectionInput,
} from "../shared/session-lifecycle";

const src = (id: string, kind: ProjectionSourceKind = "owner", fresh: ProjectionFreshness = "current"): ProjectionSource =>
  ({ id, kind, detail: `fixture ${id}`, observedAt: "2026-10-24T08:00:00.000Z", revision: "rev-fixture", fresh });
const session = (patch: Partial<NativeSessionObservation> = {}): NativeSessionObservation => ({
  id: "fix-session", status: "idle", archived: false, archivedAt: null, activeTurn: false, pendingInteraction: false,
  title: "SWE • ASMA-1 • fixture", workspaceId: "wks_fixture", projectKey: "prj_fixture",
  provider: "opencode/deepseek/deepseek-flash", model: null,
  updatedAt: "2026-10-24T08:00:00.000Z", observedAt: "2026-10-24T08:00:01.000Z", ...patch,
});
const intentNone: ArchiveIntentObservation = { state: "none", reviewId: null, sessionId: "fix-session", observedAt: null, detail: "No unresolved archive receipt is recorded." };
const input = (patch: Partial<SessionProjectionInput> = {}): SessionProjectionInput => ({
  hostId: "host-fixture", session: session(), coverage: { archiveInventory: "complete", observedAt: "2026-10-24T08:00:02.000Z", detail: "fixture complete archive inventory" },
  intent: intentNone, release: { state: "unavailable", reason: MISSING_OWNER_ADAPTER }, now: "2026-10-24T08:05:00.000Z", ...patch,
});
const claim = (kind: ScopeClaimKind, value: string, binding: ScopeBinding = "claimed", source: ProjectionSource = src(`claim:${kind}:${value}`, "label")): ScopeClaim => ({ kind, value, binding, source });
const verifiedBinding = (kind: ScopeClaimKind = "epic", value = "ASMA-1"): ScopeClaim => claim(kind, value, "verified", src(`binding:${kind}:${value}`, "owner"));
const duty = (name: string, state: DutyObservation["state"] = "open", source = src(`duty:${name}`)): DutyObservation => ({ duty: name, state, source });
const hold = (patch: Partial<HoldObservation> = {}): HoldObservation => ({ reason: "awaiting an operator decision", owner: "operator-fixture", liftsWhen: "the decision is recorded", reviewBy: "2026-10-24T09:00:00.000Z", source: src("hold:fixture"), ...patch });
type Available = Extract<OwnerReleaseObservation, { state: "available" }>;
const owner = (patch: Partial<Available> = {}): Available => ({ state: "available", owner: "fixture owner", sessionId: "fix-session", accepted: true, dutiesSettled: true, descendantsSettled: true, wakesSettled: true, executor: "verified", source: src("owner:fixture"), ...patch });
const responsibility = (patch: Partial<NonNullable<SessionProjectionInput["responsibilities"]>[number]> = {}): NonNullable<SessionProjectionInput["responsibilities"]>[number] =>
  ({ owner: "fixture owner", sessionId: "fix-session", role: "SWE", responsibility: "fixture duty", source: src("responsibility:fixture"), ...patch });
const ready = (patch: Partial<SessionProjectionInput> = {}): Partial<SessionProjectionInput> =>
  ({ workClaims: [verifiedBinding()], responsibilities: [responsibility()], release: owner(), ...patch });
const conflict = (kind: ProjectionConflict["kind"] = "stale-or-mismatched-evidence"): ProjectionConflict => ({ kind, detail: "fixture conflict", sources: [src("conflict:fixture")] });
const ticket = (patch: Partial<NonNullable<SessionProjectionInput["ticket"]>> = {}): NonNullable<SessionProjectionInput["ticket"]> =>
  ({ kind: "delivery", seats: [duty("delivery", "settled")], pullRequest: "merged", pullRequestSource: src("ticket:pull-request", "ticket"), escalation: "none", escalationSource: src("ticket:escalation", "ticket"), ...patch });
const peer = () => ({ role: "tpm", shutdownObservedAt: "2026-10-24T08:03:00.000Z", source: src("peer:tpm", "native") });

describe("installation boundary", () => {
  it("never manufactures release from an idle installed session", () => {
    const projection = deriveSessionProjection(input());
    expect(projection.native).toEqual({ status: "idle", archiveMembership: "unarchived" });
    expect(projection.retirement.disposition).toBe("unknown");
    expect(projection.retirement.reason).toContain("Release evidence is incomplete");
    expect(projection.retirement.missing).toContain(MISSING_OWNER_ADAPTER);
    expect(projection.responsibility).toMatchObject({ status: "unknown", role: null, claimedRole: null, missing: [MISSING_OWNER_ADAPTER] });
    expect(projection.freshness).toEqual({ nativeObservedAt: "2026-10-24T08:00:01.000Z", inventoryObservedAt: "2026-10-24T08:00:02.000Z", receiptObservedAt: null });
  });

  it("keeps title, label, plan and placement associations as claims with provenance", () => {
    const planSource: ProjectionSource = src("source:plan-fixture", "plan");
    const projection = deriveSessionProjection(input({
      workClaims: [claim("epic", "ASMA-1", "claimed", src("label:epic", "label")), claim("ticket", "ASMA-2", "claimed"), claim("project", "prj_fixture", "placement", src("placement:project", "project")), claim("epic", "ASMA-9", "claimed", planSource)],
      roleClaims: [claim("role", "SWE", "claimed", src("title:fixture", "title"))],
    }));
    expect(projection.scope.placement).toMatchObject({ kind: "project", value: "prj_fixture", binding: "placement" });
    expect(projection.scope.claims.map((entry) => [entry.kind, entry.binding, entry.source.kind])).toEqual([
      ["epic", "claimed", "label"], ["ticket", "claimed", "label"], ["project", "placement", "project"], ["epic", "claimed", "plan"],
    ]);
    expect(projection.responsibility).toMatchObject({ status: "unknown", claimedRole: "SWE" });
    expect(projection.retirement.disposition).toBe("unknown");
  });
});

describe("evidenced obligations retain", () => {
  it("retains a worker awaiting QA while ownership stays unknown", () => {
    const projection = deriveSessionProjection(input({ duties: [duty("worker awaits QA")] }));
    expect(projection.retirement.disposition).toBe("retain");
    expect(projection.retirement.reason).toContain("Open duty: worker awaits QA");
    expect(projection.responsibility.status).toBe("unknown");
    expect(projection.retirement.missing).toContain(MISSING_OWNER_ADAPTER);
  });

  it("separates a finished reviewer from an author lead whose pull request is open", () => {
    const reviewer = deriveSessionProjection(input({ session: session({ title: "QA • ASMA-1 • review" }), duties: [duty("review complete", "settled")] }));
    const author = deriveSessionProjection(input({ duties: [], ticket: ticket({ pullRequest: "open" }) }));
    expect(reviewer.retirement.disposition).toBe("unknown");
    expect(author.retirement.disposition).toBe("retain");
    expect(author.retirement.reason).toContain("pull request is open");
  });

  it("retains a pending rereview and not a settled consultation", () => {
    expect(deriveSessionProjection(input({ duties: [duty("consultation awaits rereview")] })).retirement.disposition).toBe("retain");
    expect(deriveSessionProjection(input({ duties: [duty("consultation settled", "settled")] })).retirement.disposition).toBe("unknown");
  });

  it("retains an exact durable desk but not a same-named association", () => {
    const exact = deriveSessionProjection(input({ durable: { match: "exact", source: src("desk:exact", "desk") } }));
    const named = deriveSessionProjection(input(ready({ durable: { match: "name-only", source: src("desk:name", "desk") } })));
    expect(exact.retirement.disposition).toBe("retain");
    expect(exact.retirement.reason).toContain("durable");
    expect(named.retirement.disposition).toBe("unknown");
    expect(named.retirement.attention).toBe(true);
    expect(named.retirement.missing.join(" ")).toContain("name-only");
  });

  it("retains a closed runtime with open work and keeps native Closed separate", () => {
    const projection = deriveSessionProjection(input({ session: session({ status: "closed" }), duties: [duty("open follow-up")] }));
    expect(projection.retirement.disposition).toBe("retain");
    expect(projection.native).toEqual({ status: "closed", archiveMembership: "unarchived" });
  });

  it("retains an authorized next milestone and treats a completed one as no release evidence", () => {
    expect(deriveSessionProjection(input({ milestone: { state: "open", source: src("milestone:open") } })).retirement.disposition).toBe("retain");
    expect(deriveSessionProjection(input({ milestone: { state: "complete", source: src("milestone:complete") } })).retirement.disposition).toBe("unknown");
  });

  it("retains every evidenced archived conflict and never revives the session", () => {
    const running = deriveSessionProjection(input({ session: session({ status: "running", archived: true, archivedAt: "2026-10-24T08:02:00.000Z", activeTurn: true }) }));
    const pending = deriveSessionProjection(input({ session: session({ archived: true, archivedAt: "2026-10-24T08:02:00.000Z", pendingInteraction: true }) }));
    const turnWithoutStart = deriveSessionProjection(input({ session: session({ archived: true, archivedAt: "2026-10-24T08:02:00.000Z", activeTurn: true }) }));
    for (const projection of [running, pending, turnWithoutStart]) {
      expect(projection.retirement.disposition).toBe("retain");
      expect(projection.native.archiveMembership).toBe("archived");
      expect(projection.conflicts.some((entry) => entry.kind.startsWith("archived-with-"))).toBe(true);
      expect(projection.retirement.attention).toBe(true);
    }
    expect(running.conflicts.map((entry) => entry.kind)).toContain("archived-with-active-execution");
    expect(running.native.status).toBe("running");
    expect(pending.conflicts.map((entry) => entry.kind)).toContain("archived-with-pending-interaction");
  });

  it("retains an LSA with remaining supervision after its TPM is observed shut down", () => {
    const projection = deriveSessionProjection(input({ session: session({ title: "LSA • ASMA-1 • architecture" }), duties: [duty("LSA supervises delivery")], peer: peer() }));
    expect(projection.retirement.disposition).toBe("retain");
    expect(projection.retirement.reason).toContain("LSA supervises delivery");
    expect(projection.sources.map((entry) => entry.id)).toContain("peer:tpm");
  });

  it("retains a ticket lead with an open escalation and a TSW seat awaiting review", () => {
    const escalation = deriveSessionProjection(input({ ticket: ticket({ escalation: "open" }) }));
    const seat = deriveSessionProjection(input({ ticket: ticket({ seats: [duty("review rework")] }) }));
    expect(escalation.retirement.disposition).toBe("retain");
    expect(escalation.retirement.reason).toContain("escalation is open");
    expect(seat.retirement.disposition).toBe("retain");
    expect(seat.retirement.reason).toContain("review rework");
  });
});

describe("a hold protects but never grants release", () => {
  it("retains a complete hold even when release facts are also supplied", () => {
    const projection = deriveSessionProjection(input(ready({ hold: hold() })));
    expect(projection.retirement.disposition).toBe("retain");
    expect(projection.retirement.reason).toContain("awaiting an operator decision");
  });

  it.each<[string, HoldObservation]>([
    ["missing owner", hold({ owner: null })],
    ["missing lifts_when", hold({ liftsWhen: " " })],
    ["missing review_by", hold({ reviewBy: null })],
    ["malformed review_by", hold({ reviewBy: "not-a-date" })],
    ["expired review_by", hold({ reviewBy: "2026-10-24T08:04:00.000Z" })],
  ])("never releases on a %s hold, even with complete owner facts", (_label, value) => {
    const projection = deriveSessionProjection(input(ready({ hold: value })));
    expect(projection.retirement.disposition).toBe("unknown");
    expect(projection.retirement.attention).toBe(true);
    expect(projection.retirement.reason).not.toContain(READY_IS_DESCRIPTIVE);
  });
});

describe("conflicts and uncertainty remain Unknown", () => {
  it("rejects disagreeing verified work claims and disagreeing role claims", () => {
    const work = deriveSessionProjection(input({ duties: [duty("open work")], workClaims: [claim("epic", "ASMA-1", "verified", src("owner:a")), claim("epic", "ASMA-2", "verified", src("owner:b"))] }));
    const roles = deriveSessionProjection(input({ roleClaims: [claim("role", "TPM", "claimed", src("label:role", "label")), claim("role", "LSA", "claimed", src("title:role", "title"))] }));
    expect(work.retirement.disposition).toBe("unknown");
    expect(work.conflicts.map((entry) => entry.kind)).toContain("conflicting-work-claims");
    expect(roles.retirement.disposition).toBe("unknown");
    expect(roles.conflicts.map((entry) => entry.kind)).toContain("conflicting-role-claims");
  });

  it("rejects stale verified bindings and evidence read for another identity", () => {
    const stale = deriveSessionProjection(input({ workClaims: [claim("epic", "ASMA-1", "verified", src("owner:stale", "owner", "stale"))] }));
    const mismatched = deriveSessionProjection(input({ evidenceConflicts: [conflict()] }));
    expect(stale.retirement.disposition).toBe("unknown");
    expect(stale.conflicts.map((entry) => entry.kind)).toContain("stale-or-mismatched-evidence");
    expect(mismatched.retirement.disposition).toBe("unknown");
  });

  it("keeps an unsettled archive intent exact and separate from native membership", () => {
    const projection = deriveSessionProjection(input({ session: session({ archived: true, archivedAt: "2026-10-24T08:02:00.000Z" }), intent: { state: "unsettled", reviewId: "review-fixture", sessionId: "fix-session", observedAt: "2026-10-24T08:03:00.000Z", detail: "uncertain: lost reply" } }));
    expect(projection.retirement.disposition).toBe("unknown");
    expect(projection.intent).toMatchObject({ state: "unsettled", reviewId: "review-fixture", sessionId: "fix-session" });
    expect(projection.conflicts.map((entry) => entry.kind)).toContain("archived-with-unsettled-intent");
    expect(projection.native.archiveMembership).toBe("archived");
    expect(projection.native.status).toBe("idle");
  });

  it("marks partial archive coverage Unknown instead of reporting zero conflicts", () => {
    const projection = deriveSessionProjection(input({ coverage: { archiveInventory: "partial", observedAt: null, detail: "walk did not finish" } }));
    expect(projection.coverage).toBe("partial");
    expect(projection.retirement.disposition).toBe("unknown");
    expect(projection.retirement.missing.join(" ")).toContain("partial");
  });

  it("keeps a missing external executor or circular finalizer Unknown", () => {
    const missing = deriveSessionProjection(input(ready({ release: owner({ executor: "missing" }) })));
    const circular = deriveSessionProjection(input(ready({ release: owner({ executor: "circular" }) })));
    for (const projection of [missing, circular]) {
      expect(projection.retirement.disposition).toBe("unknown");
      expect(projection.retirement.reason).not.toContain(READY_IS_DESCRIPTIVE);
    }
    expect(missing.retirement.missing.join(" ")).toContain("external executor is missing");
    expect(circular.retirement.missing.join(" ")).toContain("circular");
  });

  it("reports a name-only paused owner as an unprotected claim", () => {
    const unverified = deriveSessionProjection(input({ ownerPaused: { paused: true, verified: false, source: src("desk:paused", "desk") } }));
    const verified = deriveSessionProjection(input({ ownerPaused: { paused: true, verified: true, source: src("desk:paused-exact", "desk") } }));
    expect(unverified.retirement.disposition).toBe("unknown");
    expect(unverified.retirement.attention).toBe(true);
    expect(verified.retirement.disposition).toBe("retain");
    expect(verified.retirement.reason).toContain("intentionally paused");
  });
});

describe("tickets desk", () => {
  it("retains the container until every seat is released and the pull request is merged or closed", () => {
    expect(deriveSessionProjection(input({ ticket: ticket({ seats: [duty("audit")] }) })).retirement.disposition).toBe("retain");
    expect(deriveSessionProjection(input({ ticket: ticket({ pullRequest: "open" }) })).retirement.disposition).toBe("retain");
    expect(deriveSessionProjection(input({ ticket: ticket({ seats: [] }) })).retirement.disposition).toBe("unknown");
  });

  it("treats an explicit spike without a pull request as complete and an unknown one as attention", () => {
    const explicit = deriveSessionProjection(input(ready({ ticket: ticket({ kind: "spike", seats: [], pullRequest: "none-explicit" }) })));
    const unknown = deriveSessionProjection(input(ready({ ticket: ticket({ kind: "spike", seats: [], pullRequest: "unknown" }) })));
    expect(explicit.retirement.disposition).toBe("ready");
    expect(unknown.retirement.disposition).toBe("unknown");
    expect(unknown.retirement.attention).toBe(true);
  });

  it("lets declared complete facts release an ordinary ticket container", () => {
    expect(deriveSessionProjection(input(ready({ ticket: ticket() }))).retirement.disposition).toBe("ready");
  });
});

describe("current, consistent owner evidence", () => {
  it("never marks a stale release source as current", () => {
    const projection = deriveSessionProjection(input(ready({ release: owner({ source: src("owner:stale", "owner", "stale") }) })));
    expect(projection.retirement.disposition).toBe("unknown");
    expect(projection.retirement.attention).toBe(true);
    expect(projection.retirement.missing.join(" ")).toContain("Release evidence is stale");
  });

  it("never releases on an unobserved release source", () => {
    const projection = deriveSessionProjection(input(ready({ release: owner({ source: { ...src("owner:unobserved", "owner", "unknown"), observedAt: null, revision: null } }) })));
    expect(projection.retirement.disposition).toBe("unknown");
    expect(projection.retirement.missing.join(" ")).toContain("lacks an observation time and revision");
  });

  it("rejects same-role owners that disagree", () => {
    const projection = deriveSessionProjection(input(ready({ responsibilities: [responsibility(), responsibility({ owner: "owner-b", responsibility: "other duty", source: src("responsibility:other") })] })));
    expect(projection.retirement.disposition).toBe("unknown");
    expect(projection.responsibility.status).toBe("unknown");
    expect(projection.conflicts.map((entry) => entry.kind)).toContain("conflicting-role-claims");
  });

  it("never verifies a stale responsibility", () => {
    const projection = deriveSessionProjection(input(ready({ responsibilities: [responsibility({ source: src("responsibility:stale", "responsibility", "stale") })] })));
    expect(projection.retirement.disposition).toBe("unknown");
    expect(projection.responsibility.status).toBe("unknown");
    expect(projection.responsibility.missing.join(" ")).toContain("stale");
  });

  it("does not release without a current verified binding", () => {
    const projection = deriveSessionProjection(input(ready({ workClaims: [] })));
    expect(projection.retirement.disposition).toBe("unknown");
    expect(projection.retirement.missing.join(" ")).toContain("No current verified work binding");
  });

  it("does not treat stale settled duty evidence as settlement", () => {
    const projection = deriveSessionProjection(input(ready({ duties: [duty("delivery", "settled", src("duty:stale", "owner", "stale"))] })));
    expect(projection.retirement.disposition).toBe("unknown");
    expect(projection.retirement.missing.join(" ")).toContain("Duty settlement evidence is stale");
  });

  it("requires the release and responsibility to name the same exact session and owner", () => {
    const otherSession = deriveSessionProjection(input(ready({ release: owner({ sessionId: "other-session" }) })));
    const otherOwner = deriveSessionProjection(input(ready({ release: owner({ owner: "other owner" }) })));
    expect(otherSession.retirement.disposition).toBe("unknown");
    expect(otherSession.retirement.missing.join(" ")).toContain("does not name this exact native session");
    expect(otherOwner.retirement.disposition).toBe("unknown");
    expect(otherOwner.retirement.missing.join(" ")).toContain("disagree on the owner");
  });
});

describe("running execution evidence", () => {
  const archivedAt = "2026-10-24T08:02:00.000Z";
  it("retains an unarchived running snapshot without an activeTurn object", () => {
    const projection = deriveSessionProjection(input(ready({ session: session({ status: "running", activeTurn: false }) })));
    expect(projection.retirement.disposition).toBe("retain");
    expect(projection.retirement.reason).toContain("Native running status is present.");
  });

  it("retains and marks an archived running snapshot without an activeTurn object", () => {
    const projection = deriveSessionProjection(input(ready({ session: session({ status: "running", archived: true, archivedAt, activeTurn: false }) })));
    expect(projection.retirement.disposition).toBe("retain");
    expect(projection.native).toEqual({ status: "running", archiveMembership: "archived" });
    expect(projection.conflicts.map((entry) => entry.kind)).toContain("archived-with-active-execution");
    expect(projection.retirement.attention).toBe(true);
  });

  it("keeps a turn without a start time as execution evidence", () => {
    const projection = deriveSessionProjection(input(ready({ session: session({ status: "idle", archived: true, archivedAt, activeTurn: true }) })));
    expect(projection.retirement.disposition).toBe("retain");
    expect(projection.conflicts.map((entry) => entry.kind)).toContain("archived-with-active-execution");
  });
});

describe("archived ticket obligations keep their provenance", () => {
  const archivedAt = "2026-10-24T08:02:00.000Z";
  const archivedTicket = (patch: Partial<NonNullable<SessionProjectionInput["ticket"]>> = {}) => input(ready({
    session: session({ archived: true, archivedAt }), ticket: ticket(patch),
  }));

  it("marks an archived open pull request as an archive conflict", () => {
    const projection = deriveSessionProjection(archivedTicket({ pullRequest: "open" }));
    expect(projection.retirement.disposition).toBe("retain");
    const conflict = projection.conflicts.find((entry) => entry.kind === "archived-with-open-obligation");
    expect(conflict?.detail).toContain("pull request");
    expect(projection.native.archiveMembership).toBe("archived");
  });

  it("marks an archived operator escalation as an archive conflict", () => {
    const projection = deriveSessionProjection(archivedTicket({ escalation: "open" }));
    expect(projection.retirement.disposition).toBe("retain");
    expect(projection.conflicts.find((entry) => entry.kind === "archived-with-open-obligation")?.detail).toContain("escalation");
  });

  it("classifies obligations independently of the source kind", () => {
    const projection = deriveSessionProjection(archivedTicket({ escalation: "open", pullRequest: "open",
      escalationSource: src("ticket:escalation", "native"), pullRequestSource: src("ticket:pull-request", "native") }));
    const conflict = projection.conflicts.find((entry) => entry.kind === "archived-with-open-obligation");
    expect(conflict?.detail).toContain("escalation");
    expect(conflict?.detail).toContain("pull request");
  });
});

describe("declared complete owner facts", () => {
  it("are the only path to Ready and remain descriptive", () => {
    const projection = deriveSessionProjection(input(ready()));
    expect(projection.retirement.disposition).toBe("ready");
    expect(projection.retirement.reason).toContain(READY_IS_DESCRIPTIVE);
    expect(projection.native).toEqual({ status: "idle", archiveMembership: "unarchived" });
    expect(projection.retirement.missing).toEqual([]);
  });
});

describe("MUT-001 release invariants", () => {
  it("MUT-001(a): idle alone never grants release", () => {
    expect(deriveSessionProjection(input({ session: session({ status: "idle", activeTurn: false }) })).retirement.disposition).not.toBe("ready");
  });
  it("MUT-001(b): an empty queue never grants release", () => {
    expect(deriveSessionProjection(input({ duties: [] })).retirement.disposition).toBe("unknown");
  });
  it("MUT-001(c): an expired or malformed hold never grants release", () => {
    for (const value of [hold({ reviewBy: "2026-10-24T08:04:00.000Z" }), hold({ reviewBy: "bad" }), hold({ owner: null })]) {
      expect(deriveSessionProjection(input(ready({ hold: value }))).retirement.disposition).toBe("unknown");
    }
  });
  it("MUT-001(d): a historical Jira Done never grants release", () => {
    const projection = deriveSessionProjection(input({ jira: { status: "Done", source: src("jira:history", "jira") } }));
    expect(projection.retirement.disposition).toBe("unknown");
    expect(projection.sources.map((entry) => entry.id)).toContain("jira:history");
  });
  it("MUT-001(e): a TPM shutdown never releases the LSA", () => {
    const settled = deriveSessionProjection(input({ session: session({ title: "LSA • ASMA-1 • architecture" }), duties: [duty("supervision settled", "settled")], peer: peer() }));
    const supervising = deriveSessionProjection(input({ session: session({ title: "LSA • ASMA-1 • architecture" }), duties: [duty("LSA supervises delivery")], peer: peer() }));
    expect(settled.retirement.disposition).toBe("unknown");
    expect(settled.retirement.disposition).not.toBe("ready");
    expect(supervising.retirement.disposition).toBe("retain");
  });
  it("keeps open milestones and remaining supervision visible beside the same invariants", () => {
    const projection = deriveSessionProjection(input({ milestone: { state: "open", source: src("milestone:open") }, duties: [duty("supervision")] }));
    expect(projection.retirement.disposition).toBe("retain");
    expect(projection.retirement.reason).toContain("next milestone");
    expect(projection.retirement.reason).toContain("supervision");
  });
});
