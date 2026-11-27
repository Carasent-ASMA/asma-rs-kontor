import type { PluginTheme } from "@getpaseo/plugin";
import { useRpc } from "@getpaseo/plugin/client";
import { useEffect, useRef, useState } from "react";
import { Text, View } from "react-native";
import {
  acceptPendingQuestions, questionKey, readPendingQuestions, type PendingQuestions, type SessionQuestionItem, type SessionQuestionOutcome,
  type SessionQuestionResult,
} from "../shared/session-questions";
import { OperationSheet, SheetFooter } from "./operation-sheet";
import { Action, type Colors } from "./page-controls";

export const SESSION_QUESTIONS_SHEET_TEST_ID = "sessions-pending-questions-sheet";
export const SESSION_QUESTIONS_CONFIRM_TEST_ID = "sessions-pending-questions-confirm";

export const QUESTION_OUTCOME_LABELS: Readonly<Record<SessionQuestionOutcome, string>> = {
  answered: "Answered",
  "already-answered": "Already answered",
  changed: "Changed since the review",
  unsupported: "Needs the operator",
  uncertain: "Not confirmed",
};

/** The directory's own pending field, summed over live rows; no second poll or inventory. */
export function pendingPermissionTotal(entries: readonly { readonly archivedAt: string | null; readonly pendingPermissionCount: number }[]): number {
  return entries.reduce((count, entry) => entry.archivedAt === null ? count + entry.pendingPermissionCount : count, 0);
}

/** The operator-facing action, visible only while the native directory reports pending work. */
export function PendingQuestionsButton({ count, colors, compact, disabled = false, onPress }: {
  readonly count: number;
  readonly colors: Colors;
  readonly compact: boolean;
  readonly disabled?: boolean;
  readonly onPress: () => unknown;
}) {
  if (count <= 0) {
    return null;
  }
  return <Action label={`Accept ${count}`} accessibilityLabel={`Accept ${count} pending permission ${count === 1 ? "request" : "requests"}`}
    help="Reviews every pending request before answering any" icon="Check" small={!compact} colors={colors} disabled={disabled} onPress={onPress} />;
}

type ResultMap = Readonly<Record<string, SessionQuestionResult>>;

export type SessionQuestionsState = {
  readonly open: boolean;
  readonly review: PendingQuestions | null;
  readonly results: ResultMap;
  readonly busy: boolean;
  readonly running: boolean;
  readonly checking: string | null;
  readonly error: string | null;
  begin: () => Promise<void>;
  confirm: () => Promise<readonly SessionQuestionResult[] | null>;
  check: (item: SessionQuestionItem) => Promise<void>;
  close: () => void;
};

const failureText = (failure: unknown) => (failure instanceof Error ? failure.message : String(failure));
const plural = (count: number, one: string, many: string) => `${count} ${count === 1 ? one : many}`;

/**
 * One review read and one confirmed batch per opening. A second press while a
 * read or batch is in flight does nothing; Check only re-reads and never sends.
 * The directory refresh keeps the button's count honest after the batch.
 */
export function useSessionQuestions(hostId: string, onChanged: () => void = () => {}): SessionQuestionsState {
  const read = useRpc(readPendingQuestions);
  const accept = useRpc(acceptPendingQuestions);
  const [open, setOpen] = useState(false);
  const [review, setReview] = useState<PendingQuestions | null>(null);
  const [results, setResults] = useState<ResultMap>({});
  const [busy, setBusy] = useState(false);
  const [running, setRunning] = useState(false);
  const [checking, setChecking] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const active = useRef(false);
  const dispatched = useRef(false);
  const sending = useRef(false);
  const generation = useRef(0);
  const changed = useRef(onChanged);
  changed.current = onChanged;

  useEffect(() => {
    active.current = false;
    dispatched.current = false;
    sending.current = false;
    generation.current += 1;
    setOpen(false);
    setReview(null);
    setResults({});
    setBusy(false);
    setRunning(false);
    setChecking(null);
    setError(null);
    return () => {
      active.current = false;
      generation.current += 1;
    };
  }, [hostId]);

  const begin = async () => {
    if (active.current) {
      return;
    }
    active.current = true;
    setOpen(true);
    setBusy(true);
    setReview(null);
    setResults({});
    setError(null);
    dispatched.current = false;
    const view = generation.current;
    try {
      const pending = await read({});
      if (generation.current !== view) {
        return;
      }
      setReview(pending);
    } catch (failure) {
      if (generation.current === view) {
        setError(`Pending permissions could not be read. ${failureText(failure)}`);
      }
    } finally {
      if (generation.current === view) {
        active.current = false;
        setBusy(false);
      }
    }
  };

  const confirm = async (): Promise<readonly SessionQuestionResult[] | null> => {
    if (!review || active.current) {
      return null;
    }
    // Unsupported requests are never sent. The hash is the review's pin; the response is recomputed server-side.
    const items = review.items.filter((item) => item.supported && item.response !== null)
      .map(({ agentId, requestId, hash }) => ({ agentId, requestId, hash }));
    if (items.length === 0 || dispatched.current) {
      return null;
    }
    dispatched.current = true;
    sending.current = true;
    active.current = true;
    setRunning(true);
    setError(null);
    const view = generation.current;
    try {
      const output = await accept({ items });
      if (generation.current !== view) {
        return null;
      }
      setResults(Object.fromEntries(output.results.map((item) => [questionKey(item), item])));
      changed.current();
      return output.results;
    } catch (failure) {
      if (generation.current === view) {
        setError(`The batch did not report results. Some requests may have been answered; review the sessions before trying again. ${failureText(failure)}`);
        // Any request may have been answered; read the directory again so the header count reflects it.
        changed.current();
      }
      return null;
    } finally {
      if (generation.current === view) {
        sending.current = false;
        active.current = false;
        setRunning(false);
      }
    }
  };

  const check = async (item: SessionQuestionItem) => {
    if (active.current) {
      return;
    }
    active.current = true;
    setChecking(questionKey(item));
    setError(null);
    const view = generation.current;
    try {
      // Check only reads the host; it never sends another answer.
      const pending = await read({});
      if (generation.current !== view) {
        return;
      }
      const still = pending.items.some((entry) => entry.agentId === item.agentId && entry.requestId === item.requestId);
      const result: SessionQuestionResult = still
        ? { agentId: item.agentId, requestId: item.requestId, outcome: "uncertain", message: "Still listed; it is not resent." }
        : { agentId: item.agentId, requestId: item.requestId, outcome: "answered", message: "No longer pending." };
      setResults((current) => ({ ...current, [questionKey(item)]: result }));
      if (!still) {
        changed.current();
      }
    } catch (failure) {
      if (generation.current === view) {
        setError(`The request could not be read again. ${failureText(failure)}`);
      }
    } finally {
      if (generation.current === view) {
        active.current = false;
        setChecking(null);
      }
    }
  };

  const close = () => {
    if (sending.current) {
      return;
    }
    // A read or check still in flight is ignored; a dispatched batch keeps the sheet open until it settles.
    generation.current += 1;
    active.current = false;
    dispatched.current = false;
    setOpen(false);
    setReview(null);
    setResults({});
    setBusy(false);
    setRunning(false);
    setChecking(null);
    setError(null);
  };

  return { open, review, results, busy, running, checking, error, begin, confirm, check, close };
}

function summaryText(review: PendingQuestions | null, results: ResultMap, done: boolean): string | null {
  if (!review) {
    return null;
  }
  const operator = review.items.filter((item) => !item.supported).length;
  if (!done) {
    return `${plural(review.items.filter((item) => item.supported).length, "request", "requests")} will be answered${operator ? ` · ${plural(operator, "request needs", "requests need")} the operator` : ""}.`;
  }
  const outcomes = Object.values(results);
  const count = (outcome: SessionQuestionOutcome) => outcomes.filter((result) => result.outcome === outcome).length;
  return [
    count("answered") ? `${count("answered")} answered` : null,
    count("already-answered") ? `${count("already-answered")} already answered` : null,
    count("changed") ? `${count("changed")} changed` : null,
    count("unsupported") ? `${count("unsupported")} unsupported` : null,
    count("uncertain") ? `${count("uncertain")} not confirmed` : null,
    operator ? `${plural(operator, "request needs", "requests need")} the operator` : null,
  ].filter((part): part is string => part !== null).join(" · ") || "Nothing was answered.";
}

/**
 * The review sheet: every pending request with its session, kind, title and the
 * exact sentence the server would send. Unsupported requests stay visible and
 * apart; only one confirmation runs the batch. Results keep each outcome, and
 * an uncertain row offers a read-only Check.
 */
export function SessionQuestionsSheet({ questions, colors }: {
  readonly questions: SessionQuestionsState;
  readonly colors: PluginTheme["colors"];
}) {
  const { review, results, busy, running, checking, error } = questions;
  const supported = review?.items.filter((item) => item.supported) ?? [];
  const operator = review?.items.filter((item) => !item.supported) ?? [];
  const done = !!review && !busy && !running && Object.keys(results).length > 0;
  const summary = summaryText(review, results, done);
  const title = running ? "Answering pending permissions…" : done ? "Pending permissions settled" : review ? `Review ${plural(review.items.length, "pending permission request", "pending permission requests")}` : "Pending permissions";
  const muted = { color: colors.foregroundMuted, fontSize: 12, lineHeight: 17 };

  const footer = running
    ? <SheetFooter colors={colors} primary={{ testID: SESSION_QUESTIONS_CONFIRM_TEST_ID, label: "Answering…", disabled: true, onPress: () => {} }} />
    : busy || (review === null && !error)
      ? <SheetFooter colors={colors} secondary={{ label: "Cancel", onPress: questions.close }}
        primary={{ testID: SESSION_QUESTIONS_CONFIRM_TEST_ID, label: "Reading…", disabled: true, onPress: () => {} }} />
      : done || error || (review !== null && review.items.length === 0)
        ? <SheetFooter colors={colors} secondary={{ label: "Close", onPress: questions.close }} />
        : <SheetFooter colors={colors} secondary={{ label: "Cancel", onPress: questions.close }}
          primary={{ testID: SESSION_QUESTIONS_CONFIRM_TEST_ID, label: `Accept ${supported.length}`, accessibilityLabel: `Accept ${plural(supported.length, "pending request", "pending requests")}`,
            disabled: busy || running || supported.length === 0, onPress: () => { void questions.confirm(); } }} />;

  return <OperationSheet testID={SESSION_QUESTIONS_SHEET_TEST_ID} title={title}
    subtitle="The server re-reads every request and sends the reviewed answer once. Unsupported requests stay for you."
    colors={colors} onClose={questions.close} footer={footer}>
    {busy && !review ? <Text style={muted}>Reading pending permission requests…</Text> : null}
    {review && review.items.length === 0 ? <Text style={muted}>No pending permission requests.</Text> : null}
    {review && review.items.length > 0 ? <View role="list" style={{ gap: 6 }}>
      {review.items.map((item) => {
        const result = results[questionKey(item)];
        return <View key={questionKey(item)} role="listitem" testID={`session-question-${item.requestId}`}
          style={{ padding: 8, gap: 3, borderRadius: 7, borderWidth: 1, borderColor: item.supported ? colors.border : colors.statusWarning, backgroundColor: colors.surface1 }}>
          <Text numberOfLines={2} style={{ color: colors.foreground, fontSize: 13, fontWeight: "600" }}>{item.agentTitle}</Text>
          <Text style={muted}>{`${item.kind} · ${item.title}`}</Text>
          <Text style={{ color: item.supported ? colors.foreground : colors.statusWarning, fontSize: 12, lineHeight: 17 }}>{item.supported ? item.label : "Needs the operator"}</Text>
          {!item.supported && item.reason ? <Text style={muted}>{item.reason}</Text> : null}
          {result ? <Text role="status" style={{ color: result.outcome === "answered" ? colors.statusSuccess : result.outcome === "already-answered" ? colors.foregroundMuted : colors.statusWarning, fontSize: 12, lineHeight: 17 }}>
            {`${QUESTION_OUTCOME_LABELS[result.outcome]}${result.message ? ` · ${result.message}` : ""}`}
          </Text> : null}
          {result?.outcome === "uncertain" ? <View style={{ flexDirection: "row" }}>
            <Action small colors={colors} label={checking === questionKey(item) ? "Checking…" : "Check"} accessibilityLabel={`Check ${item.title} without resending`}
              disabled={busy || running || checking !== null} onPress={() => { void questions.check(item); }} />
          </View> : null}
        </View>;
      })}
    </View> : null}
    {summary ? <Text testID="sessions-pending-questions-summary" role="status" accessibilityLiveRegion="polite" style={{ color: colors.foreground, fontSize: 12, lineHeight: 17 }}>{summary}</Text> : null}
    {error ? <Text accessibilityRole="alert" style={{ color: colors.statusWarning, fontSize: 12, lineHeight: 17 }}>{error}</Text> : null}
  </OperationSheet>;
}
