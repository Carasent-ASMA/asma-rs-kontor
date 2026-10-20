/**
 * Every side effect the plugin asks of its host lands here instead of a real
 * host: settings saves, session/workspace opening, links, clipboard, toasts.
 * The studio shows the log so a reviewer can see what would have happened.
 */
export type HostEvent = {
  readonly seq: number;
  readonly at: string;
  readonly kind:
    | "settings.save"
    | "keyboard.save"
    | "navigation.openAgent"
    | "navigation.openWorkspace"
    | "openScreen"
    | "linking.openURL"
    | "openExternalUrl"
    | "copyText"
    | "toast"
    | "simulated.update"
    | "paseo.unsupported";
  readonly detail: string;
};

const events: HostEvent[] = [];
const listeners = new Set<() => void>();
let seq = 0;

export function logHostEvent(kind: HostEvent["kind"], detail: string): void {
  seq += 1;
  events.push({ seq, at: new Date().toISOString(), kind, detail });
  if (events.length > 200) events.shift();
  for (const listener of listeners) listener();
  // A structured line for browser checks: they read the console instead of the DOM.
  console.info(`[kontor-prototype] ${kind} ${detail}`);
}

export function hostEvents(): readonly HostEvent[] {
  return events;
}

export function subscribeHostEvents(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function countHostEvents(kind: HostEvent["kind"]): number {
  return events.filter((event) => event.kind === kind).length;
}
