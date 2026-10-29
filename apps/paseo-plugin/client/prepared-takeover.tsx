import { useEffect, useState } from "react";
import { Text, View } from "react-native";
import { TextInput } from "@getpaseo/plugin/client/react-native";
import { useRpc } from "@getpaseo/plugin/client";
import {
  readPreparedTakeoverCapability,
  importPreparedTakeover,
  executeProceduralTakeover,
  checkProceduralTakeover,
} from "../shared/procedural-takeover";
import { preparedState, admissionProblem } from "./prepared-takeover-state";
import { OperationSheet, SheetFooter } from "./operation-sheet";
import type { DirectoryEntry } from "./session-directory";
import type { Colors } from "./page-controls";

const sessions = new Map<string, ReturnType<typeof preparedState>>();
export function usePreparedCapability(hostId: string) {
  const read = useRpc(readPreparedTakeoverCapability);
  const [state, setState] = useState<{ hostId: string; problem: string | null } | null>(null);
  useEffect(() => {
    let live = true;
    Promise.resolve()
      .then(() => read({}))
      .then(
        () => {
          if (live) setState({ hostId, problem: null });
        },
        (error: unknown) => {
          if (live)
            setState({ hostId, problem: `Prepared takeover is unavailable: ${String(error)}` });
        },
      );
    return () => {
      live = false;
    };
  }, [hostId]);
  return state?.hostId === hostId ? state.problem : "Reading prepared takeover support…";
}
export function PreparedTakeoverSheet({
  hostId,
  source,
  colors,
  onClose,
}: {
  hostId: string;
  source: DirectoryEntry;
  colors: Colors;
  onClose: () => void;
}) {
  const imported = useRpc(importPreparedTakeover),
    execute = useRpc(executeProceduralTakeover),
    check = useRpc(checkProceduralTakeover);
  const key = JSON.stringify([hostId, source.id]);
  let state = sessions.get(key);
  if (!state) {
    state = preparedState(source.id);
    sessions.set(key, state);
  }
  const model = state;
  const [path, setPath] = useState(""),
    [sha, setSha] = useState("");
  const [, redraw] = useState(0);
  useEffect(() => {
    const stop = model.subscribe(() => redraw((n) => n + 1));
    // Read again after subscribing in case an operation settled between render and this effect.
    redraw((n) => n + 1);
    return stop;
  }, [model]);
  const calls = { import: imported, execute, check };
  const run = (work: () => Promise<void>) => {
    void work();
  };
  const receipt = model.receipt,
    inputProblem = admissionProblem(path, sha);
  const confirm = receipt?.phase === "prepared" && !model.dispatched;
  const label = !receipt
    ? "Read prepared admission"
    : confirm
      ? "Confirm procedural takeover"
      : "Check same operation";
  const text = { color: colors.foreground, fontSize: 12 };
  return (
    <OperationSheet
      testID="prepared-takeover"
      title="Prepared procedural takeover"
      subtitle={`${source.title} · ${source.id}`}
      colors={colors}
      onClose={onClose}
      footer={
        <SheetFooter
          colors={colors}
          secondary={{ label: "Close", onPress: onClose }}
          primary={{
            label,
            disabled: model.busy || (!receipt && !!inputProblem),
            hint: model.busy ? "Reading this operation…" : inputProblem,
            onPress: () =>
              run(() =>
                receipt ? model.act(calls, !!confirm) : model.import(calls, { path, sha256: sha }),
              ),
          }}
          note="Closing keeps this operation. After an app restart, import the same admission to read its durable receipt."
        />
      }
    >
      <Text style={text}>
        Only externally owner-prepared, eligible non-LSA/non-canonical sessions. No automatic
        takeover, bulk operation or model picker. Governed, canonical, pending and conflicting
        ownership are refused by the server.
      </Text>
      {!receipt ? (
        <View style={{ gap: 8 }}>
          <TextInput
            accessibilityLabel="Admission host path"
            value={path}
            onChangeText={setPath}
            placeholder="Absolute admission path on this host"
            style={text}
            editable={!model.busy}
          />
          <TextInput
            accessibilityLabel="Admission SHA-256"
            value={sha}
            onChangeText={setSha}
            placeholder="Exact lowercase SHA-256"
            style={text}
            editable={!model.busy}
            autoCapitalize="none"
          />
          <Text style={text}>
            {inputProblem ??
              "Read verifies the selected session and prepared evidence; it may hydrate public history but sends no task."}
          </Text>
        </View>
      ) : (
        <View style={{ gap: 6 }}>
          <Text
            selectable
            style={text}
          >{`Operation ${receipt.operationId} · revision ${receipt.revision}\nSource ${receipt.sourceId}\nReserved successor ${receipt.successorId}\nMode ${receipt.admission.mode.workflow} / ${receipt.admission.mode.orchestration}\nConfig ${receipt.admission.config.provider} / ${receipt.admission.config.model} / ${receipt.admission.config.effort} / ${receipt.admission.config.mode}\nCheckout ${receipt.admission.cwd}\nHEAD ${receipt.admission.checkout.head}\nPhase ${receipt.phase}\n${receipt.message}`}</Text>
          <Text style={text}>
            Confirm authorizes predecessor retirement, successor creation, bootstrap and the
            prepared task continuation. Active confirms correlated continuation evidence, not task
            completion.
          </Text>
          {model.limits.map((limit) => (
            <Text key={limit} style={text}>
              {limit}
            </Text>
          ))}
        </View>
      )}
      {model.problem ? (
        <Text accessibilityRole="alert" style={{ color: colors.statusWarning }}>
          {model.problem}
        </Text>
      ) : null}
    </OperationSheet>
  );
}
