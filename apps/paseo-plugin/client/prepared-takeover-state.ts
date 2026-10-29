import type { ProceduralReceipt } from "../shared/procedural-takeover";

type Ref = { path: string; sha256: string };
type Answer = { canExecute: boolean; receipt: ProceduralReceipt; limits: string[] };
export type PreparedCalls = {
  import: (input: { sourceId: string; admission: Ref }) => Promise<Answer>;
  execute: (input: { operationId: string; admissionHash: string }) => Promise<ProceduralReceipt>;
  check: (input: { operationId: string }) => Promise<ProceduralReceipt>;
};
export const admissionProblem = (path: string, sha256: string) =>
  !path.trim()
    ? "Enter the admission's absolute host path."
    : !/^[a-f0-9]{64}$/.test(sha256)
      ? "Enter its exact 64-character lowercase SHA-256."
      : null;

/** Client recovery identity only. The existing server receipt remains the operation owner. */
export function preparedState(sourceId: string) {
  const listeners = new Set<() => void>();
  const notify = () => {
    for (const listener of listeners) listener();
  };
  return {
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    receipt: null as ProceduralReceipt | null,
    limits: [] as string[],
    problem: null as string | null,
    busy: false,
    dispatched: false,
    async import(calls: PreparedCalls, ref: Ref) {
      if (this.busy || this.receipt) return;
      this.problem = admissionProblem(ref.path, ref.sha256);
      if (this.problem) {
        notify();
        return;
      }
      this.busy = true;
      notify();
      try {
        const answer = await calls.import({ sourceId, admission: ref });
        if (answer.receipt.sourceId !== sourceId)
          throw new Error("Imported receipt names another session.");
        this.receipt = answer.receipt;
        this.limits = answer.limits;
        this.dispatched = !answer.canExecute || answer.receipt.phase !== "prepared";
      } catch (error) {
        this.problem = String(error);
      } finally {
        this.busy = false;
        notify();
      }
    },
    async act(calls: PreparedCalls, execute: boolean) {
      if (
        this.busy ||
        !this.receipt ||
        (execute && (this.dispatched || this.receipt.phase !== "prepared"))
      )
        return;
      const receipt = this.receipt;
      this.busy = true;
      this.problem = null;
      // Set before dispatch: a lost reply may only be checked, never sent again by this client.
      if (execute) this.dispatched = true;
      notify();
      try {
        const next = execute
          ? await calls.execute({
              operationId: receipt.operationId,
              admissionHash: receipt.admissionHash,
            })
          : await calls.check({ operationId: receipt.operationId });
        if (
          next.operationId !== receipt.operationId ||
          next.sourceId !== sourceId ||
          next.successorId !== receipt.successorId ||
          next.admissionHash !== receipt.admissionHash
        )
          throw new Error(
            "Receipt identity changed; keep the existing operation for owner reconciliation.",
          );
        this.receipt = next;
      } catch (error) {
        this.problem = `Unconfirmed: ${String(error)}. Check the same operation; nothing is resent.`;
      } finally {
        this.busy = false;
        notify();
      }
    },
  };
}
