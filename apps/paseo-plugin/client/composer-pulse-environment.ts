export interface PulseEnvironment {
  currentState: string | null;
  readReducedMotion(): Promise<boolean>;
  onReducedMotion(listener: (reduced: boolean) => void): () => void;
  onAppState(listener: (state: string) => void): () => void;
  onError(error: unknown): void;
}

export interface PulseEnvironmentState {
  /** The app is foregrounded: the shared clock may run and the colour flow animates. */
  foreground: boolean;
  /** The OS Reduce Motion accessibility setting: drops only the scale pulse, never the colour flow. */
  reducedMotion: boolean;
}

/**
 * Publishes the two motion facts separately (D3-r2): Reduce Motion drops only the scale pulse while the
 * colour flow keeps running, and a backgrounded app releases the shared clock entirely. Motion facts stay
 * unknown until accessibility answers, and follow live OS changes without polling.
 */
export function observePulseEnvironment(
  port: PulseEnvironment,
  publish: (state: PulseEnvironmentState) => void,
): () => void {
  let disposed = false;
  let reduced = true;
  let foreground = port.currentState === "active";
  let receivedMotionEvent = false;
  let published: PulseEnvironmentState | null = null;
  const update = () => {
    const next = { foreground, reducedMotion: reduced };
    if (disposed || (published && published.foreground === next.foreground && published.reducedMotion === next.reducedMotion)) return;
    published = next;
    publish(next);
  };
  const removeMotion = port.onReducedMotion((value) => {
    receivedMotionEvent = true;
    reduced = value;
    update();
  });
  const removeState = port.onAppState((value) => {
    foreground = value === "active";
    update();
  });
  void port
    .readReducedMotion()
    .then((value) => {
      if (!disposed && !receivedMotionEvent) {
        reduced = value;
        update();
      }
      return undefined;
    })
    .catch((error) => {
      if (!disposed) port.onError(error);
    });
  return () => {
    disposed = true;
    removeMotion();
    removeState();
  };
}
