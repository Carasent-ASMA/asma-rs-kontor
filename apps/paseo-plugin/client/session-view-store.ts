import {
  DEFAULT_SESSION_FILTERS,
  type SessionFilters,
} from "./session-view";

export type SessionViewState = SessionFilters & {
  readonly scrollOffset: number;
  readonly favoritesOnly?: boolean;
  readonly shortcut?: string | null;
};

const views = new Map<string, SessionViewState>();

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

/** Test seam: the store lives only for this client session, never on disk. */
export function clearSessionViews(): void {
  views.clear();
}
