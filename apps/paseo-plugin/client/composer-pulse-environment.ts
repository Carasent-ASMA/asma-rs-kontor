export interface PulseEnvironment {
  currentState: string | null;
  readReducedMotion(): Promise<boolean>;
  onReducedMotion(listener: (reduced: boolean) => void): () => void;
  onAppState(listener: (state: string) => void): () => void;
  onError(error: unknown): void;
}

/** Motion stays off until accessibility is known, and follows live OS changes without polling. */
export function observePulseEnvironment(
  port: PulseEnvironment,
  publish: (enabled: boolean) => void,
): () => void {
  let disposed = false;
  let reduced = true;
  let foreground = port.currentState === "active";
  let receivedMotionEvent = false;
  let enabled = false;
  const update = () => {
    const next = foreground && !reduced;
    if (!disposed && next !== enabled) {
      enabled = next;
      publish(next);
    }
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
