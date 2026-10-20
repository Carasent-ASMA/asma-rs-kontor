import {
  DEFAULT_SESSION_FILTERS,
  type SessionFilters,
} from "./session-view";

export type SessionGrouping = "hierarchy" | "placement" | "role" | "flat";

export type SessionViewState = SessionFilters & {
  readonly scrollOffset: number;
  readonly shortcut?: string | null;
  /** Local choice; Workstreams, Role and Flat are also saved when settings are writable. */
  readonly grouping?: SessionGrouping;
  /** Selected workstreams by normalized label; absent means all. */
  readonly streams?: readonly string[] | null;
  readonly arranging?: boolean;
  /** Compact toolbar disclosure; local view state only, never saved to settings. */
  readonly toolbarOpen?: boolean;
};

const views = new Map<string, SessionViewState>();
const expansions = new Map<string, Readonly<Record<string, boolean>>>();
const NO_EXPANSION: Readonly<Record<string, boolean>> = Object.freeze({});

function keyOf(hostId: string): string {
  return hostId.trim();
}

export function readSessionView(hostId: string): SessionViewState {
  return (
    views.get(keyOf(hostId)) ?? {
      ...DEFAULT_SESSION_FILTERS,
      scrollOffset: 0,
    }
  );
}

export function writeSessionView(hostId: string, next: SessionViewState): void {
  views.set(keyOf(hostId), next);
}

export function patchSessionView(
  hostId: string,
  patch: Partial<SessionViewState>,
): SessionViewState {
  const next: SessionViewState = { ...readSessionView(hostId), ...patch };
  writeSessionView(hostId, next);
  return next;
}

export function writeScrollOffset(hostId: string, scrollOffset: number): void {
  patchSessionView(hostId, {
    scrollOffset: Number.isFinite(scrollOffset) && scrollOffset > 0 ? scrollOffset : 0,
  });
}

export function resetSessionView(hostId: string): SessionViewState {
  const next: SessionViewState = {
    ...DEFAULT_SESSION_FILTERS,
    scrollOffset: 0,
  };
  writeSessionView(hostId, next);
  return next;
}

/**
 * Disclosure the user chose in this client session. It applies at once and
 * keeps cached rows browseable while saved settings are unavailable or saving.
 */
export function readLocalExpansion(hostId: string): Readonly<Record<string, boolean>> {
  return expansions.get(keyOf(hostId)) ?? NO_EXPANSION;
}

export function patchLocalExpansion(
  hostId: string,
  patch: Readonly<Record<string, boolean>>,
): Readonly<Record<string, boolean>> {
  const next = { ...readLocalExpansion(hostId), ...patch };
  expansions.set(keyOf(hostId), next);
  return next;
}

/** Test seam: the store lives only for this client session, never on disk. */
export function clearSessionViews(): void {
  views.clear();
  expansions.clear();
}
