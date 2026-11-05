import { describe, expect, it, vi } from "vitest";
import { z } from "zod";
import { defineRpc } from "@getpaseo/plugin";
import { AgentCreateRequestSchema } from "@getpaseo/protocol/messages";
import { callPluginRpc } from "../node_modules/@getpaseo/plugin/dist/rpc.js";
import {
  createFeatureSchema, createRequestSchema, featureValuesProblem, featureValuesSchema, keptFeatureValues, type FeatureValues,
} from "../shared/session-create";
import { createFeatures } from "../server/session-create";
import { sourceFeatureValues } from "../server/session-takeover";

/**
 * Review-a delta-12 under the narrowed ruling: every setting id the resolved host protocol can carry is preserved
 * as an own data property; `__proto__`, which the SDK record drops, is refused with a named problem at the listed
 * features, create admission and takeover inheritance. Synthetic values only; no host connection or model call.
 */
const CARRIED = ["house", "constructor", "toString", "hasOwnProperty", "valueOf"] as const;

function select(id: string): ReturnType<typeof createFeatureSchema.parse> {
  return createFeatureSchema.parse({
    id, type: "select", label: "House setting", value: "normal",
    options: [{ id: "normal", label: "Normal", isDefault: true }, { id: "slow", label: "Slow", isDefault: false }],
  });
}

const listClient = (features: unknown[]) => ({
  providers: { listFeatures: async () => ({ features }) },
}) as unknown as Parameters<typeof createFeatures>[0];

const choice = { provider: "claude", model: "house-model", modeId: null, effort: null };

describe("D12-F2 recording shape rejection", () => {
  const shapes = [
    { label: "plain record", value: { house: "slow" }, accepted: true },
    { label: "null-prototype record", value: Object.assign(Object.create(null) as Record<string, unknown>, { house: "slow" }), accepted: true },
    { label: "null", value: null, accepted: false },
    { label: "undefined", value: undefined, accepted: false },
    { label: "string", value: "slow", accepted: false },
    { label: "number", value: 7, accepted: false },
    { label: "boolean", value: true, accepted: false },
    { label: "array", value: ["slow"], accepted: false },
    { label: "Date", value: new Date(0), accepted: false },
    { label: "Map", value: new Map([["house", "slow"]]), accepted: false },
    { label: "Set", value: new Set(["slow"]), accepted: false },
    { label: "boxed string", value: new String("slow"), accepted: false },
  ];
  it.each(shapes)("rules $label as the former record schema did", ({ value, accepted }) => {
    expect(featureValuesSchema.safeParse(value).success).toBe(accepted);
  });

  it("refuses an empty setting id", () => {
    expect(featureValuesSchema.safeParse(Object.fromEntries([["", "slow"]])).success).toBe(false);
  });

  it("names an invalid setting id", () => {
    const key = "x".repeat(201);
    const read = featureValuesSchema.safeParse(Object.fromEntries([[key, "slow"]]));
    expect(read.success).toBe(false);
    if (!read.success) expect(read.error.issues.map((issue) => issue.message).join(" ")).toContain(key);
  });

  it("names every rejected value key", () => {
    const read = featureValuesSchema.safeParse({ first: 7, second: null });
    expect(read.success).toBe(false);
    if (!read.success) {
      const text = read.error.issues.map((issue) => issue.message).join(" ");
      expect(text).toContain("first");
      expect(text).toContain("second");
      expect(read.error.issues).toHaveLength(2);
    }
  });

  it("does not forward a non-record Map through the SDK RPC helper", async () => {
    const invoke = vi.fn(async () => null);
    const rpc = defineRpc({ name: "fixture.values", input: featureValuesSchema, output: z.null() });
    const result = await callPluginRpc(rpc, invoke, new Map([["house", "slow"]]) as unknown as Record<string, unknown>)
      .then(() => ({ accepted: true }), () => ({ accepted: false }));
    expect(result.accepted).toBe(false);
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("D12-F1 takeover inheritance", () => {
  for (const [type, value] of [["select", "slow"], ["toggle", false]] as const) {
    it(`names a ${type} __proto__ setting instead of omitting it`, () => {
      const feature = type === "select"
        ? { type, id: "__proto__", label: "House setting", value, options: [{ id: "slow", label: "Slow" }] }
        : { type, id: "__proto__", label: "House setting", value };
      const read = sourceFeatureValues({ features: [feature] } as unknown as Parameters<typeof sourceFeatureValues>[0]);
      expect(read.values).toBeNull();
      expect(read.problem).toContain("__proto__");
    });
  }

  it("keeps ordinary prototype names and omits a null select", () => {
    const features = [
      { type: "select", id: "constructor", label: "House setting", value: "slow", options: [{ id: "slow", label: "Slow" }] },
      { type: "select", id: "unset", label: "Unset setting", value: null, options: [{ id: "slow", label: "Slow" }] },
    ];
    const read = sourceFeatureValues({ features } as unknown as Parameters<typeof sourceFeatureValues>[0]);
    expect(read.problem).toBeNull();
    expect(Object.hasOwn(read.values!, "constructor")).toBe(true);
    expect(read.values!["constructor"]).toBe("slow");
    expect(Object.hasOwn(read.values!, "unset")).toBe(false);
  });
});

describe("narrow option (a) boundaries", () => {
  it("refuses a listed __proto__ setting before the picker can offer it", async () => {
    const read = await createFeatures(listClient([{ id: "__proto__", label: "House setting", type: "toggle", value: true }]), "/synthetic/cwd", choice);
    expect(read.features).toEqual([]);
    expect(read.problem).toContain("__proto__");
  });

  it("lists ordinary prototype-named settings unchanged", async () => {
    const read = await createFeatures(listClient([{ id: "constructor", label: "House setting", type: "toggle", value: false }]), "/synthetic/cwd", choice);
    expect(read.problem).toBeNull();
    expect(read.features).toHaveLength(1);
    expect(read.features[0]!.id).toBe("constructor");
  });

  it("names a refused __proto__ value and never drops it silently", () => {
    const values = Object.fromEntries([["__proto__", "slow"]]) as FeatureValues;
    expect(featureValuesProblem([select("__proto__")], values)).toContain("__proto__");
    const result = keptFeatureValues([select("__proto__")], values);
    expect(Object.hasOwn(result.kept, "__proto__")).toBe(false);
    expect(result.dropped).toHaveLength(1);
    const admitted = createRequestSchema.safeParse({
      requestId: "00000000-0000-4000-8000-000000000001", role: "LSA", start: "ad-hoc",
      projectId: "fixture-project", provider: "claude", model: "house-model", effort: null, modeId: null, featureValues: values,
    });
    expect(admitted.success).toBe(false);
    if (!admitted.success) expect(admitted.error.issues.map((issue) => issue.message).join(" ")).toContain("__proto__");
  });

  it("keeps every admitted setting through the resolved SDK create contract", () => {
    const admitted = featureValuesSchema.parse(Object.fromEntries(CARRIED.map((id) => [id, "slow"])));
    const request = AgentCreateRequestSchema.parse({
      type: "agent.create.request", requestId: "fixture-request",
      config: { provider: "claude", cwd: "/synthetic/workspace", model: "house-model", featureValues: admitted },
    });
    for (const id of CARRIED) {
      expect(Object.hasOwn(request.config.featureValues!, id), id).toBe(true);
      expect(request.config.featureValues![id], id).toBe("slow");
    }
  });
});
