import { describe, expect, it, vi } from "vitest";

vi.mock("react-native", () => ({ Platform: { OS: "web" }, View: "View", Text: "Text", Pressable: "Pressable", ScrollView: "ScrollView" }));
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: () => () => Promise.resolve({}) }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon", TextInput: "TextInput" }));
vi.mock("../client/page-controls", () => ({ Action: "Action", NO_PRESS_DELAY: {}, pressFeedback: () => ({}), spaceActivates: () => ({}), COARSE_POINTER: false }));
vi.mock("../client/operation-sheet", () => ({ SheetFooter: "SheetFooter" }));
vi.mock("../client/vendor-mark", () => ({ VendorMark: "VendorMark", vendorName: (value: string) => value, vendorShortName: (value: string) => value }));
vi.mock("../client/search-picker", () => ({ SearchPicker: "SearchPicker" }));

import { effortShort, routeLabel, rowModel, type SessionUsageEntry } from "../client/session-usage";
import { featureLabels, featureValue } from "../client/model-picker";
import { issueTypeIcon } from "../client/jira-issue-picker";
import {
  createFeatureSchema, createRequestSchema, featureValuesProblem, featureValuesSchema, keptFeatureValues, type CreateFeature, type FeatureValues,
} from "../shared/session-create";

/**
 * Review-a delta-11. D11-F1: presentation tables read by open route/effort/issue-type ids must answer owned
 * entries only. D11-F2: an operator value keyed by a setting id such as `__proto__` must be read, kept and
 * admitted as an own data property rather than silently lost on the prototype setter. Synthetic data only.
 */
const INHERITED = ["constructor", "__proto__", "toString", "hasOwnProperty", "valueOf"] as const;

function select(id: string): CreateFeature {
  return createFeatureSchema.parse({
    id, type: "select", label: "House setting", value: "normal",
    options: [{ id: "normal", label: "Normal", isDefault: true }, { id: "slow", label: "Slow", isDefault: false }],
  });
}

function toggle(id: string): CreateFeature {
  return createFeatureSchema.parse({ id, type: "toggle", label: "Plan mode", value: true });
}

describe("D11-F1 presentation tables answer owned entries only", () => {
  it.each(INHERITED)("effortShort(%s) uses the documented string fallback", (id) => {
    expect(effortShort(id)).toBe(id.slice(0, 3).toLocaleUpperCase());
  });

  it.each(INHERITED)("routeLabel(%s) retains the route id", (id) => {
    expect(routeLabel(id)).toBe(`via ${id}`);
  });

  it.each(INHERITED)("issueTypeIcon(%s) keeps the generic icon", (id) => {
    expect(issueTypeIcon(id)).toBe("Tag");
  });

  it("rowModel retains the route and effort strings", () => {
    const entry = {
      provider: "constructor", model: "house-model",
      telemetry: { effectiveThinkingOptionId: "__proto__" },
    } as unknown as SessionUsageEntry;
    const row = rowModel(entry);
    expect(row.route).toBe("via constructor");
    expect(row.effort).toBe("__P");
  });
});

describe("D11-F2 setting ids keep their defaults and explicit values", () => {
  it("uses the listed select default when constructor has no owned value", () => {
    expect(featureValue(select("constructor"), {})).toBe("normal");
  });

  it("shows the default select label for an unset constructor setting", () => {
    expect(featureLabels([select("constructor")], {})).toEqual({ selects: ["Normal"], toggles: [] });
  });

  it("uses the harness true toggle default for __proto__", () => {
    expect(featureValue(toggle("__proto__"), {})).toBe(true);
    expect(featureLabels([toggle("__proto__")], {})).toEqual({ selects: [], toggles: ["Plan mode on"] });
  });

  it("keeps an offered explicit __proto__ selection through keeping", () => {
    const features = [select("__proto__")], values = Object.fromEntries([["__proto__", "slow"]]) as FeatureValues;
    expect(featureValuesProblem(features, values)).toBeNull();
    const result = keptFeatureValues(features, values);
    expect(Object.hasOwn(result.kept, "__proto__")).toBe(true);
    expect(result.kept["__proto__"]).toBe("slow");
    expect(result.dropped).toEqual([]);
  });

  it("admits an offered own __proto__ value through the create request", () => {
    const parsed = createRequestSchema.parse({
      requestId: "00000000-0000-4000-8000-000000000000", role: "LSA", start: "ad-hoc",
      projectId: "synthetic-project", provider: "opencode", model: "house-model", effort: null, modeId: null,
      featureValues: Object.fromEntries([["__proto__", "slow"]]),
    });
    expect(Object.hasOwn(parsed.featureValues!, "__proto__")).toBe(true);
    expect(parsed.featureValues!["__proto__"]).toBe("slow");
    expect(Object.hasOwn(featureValuesSchema.parse(Object.fromEntries([["__proto__", true]])), "__proto__")).toBe(true);
  });

  it("never reports a value as kept and missing at the same time", () => {
    const features = [select("constructor"), select("__proto__")];
    const values = Object.fromEntries([["constructor", "normal"], ["__proto__", "slow"]]) as FeatureValues;
    const result = keptFeatureValues(features, values);
    for (const key of Object.keys(values)) {
      expect(Object.hasOwn(result.kept, key), key).toBe(true);
      expect(result.kept[key], key).toBe(values[key]);
    }
    expect(result.dropped).toEqual([]);
  });
});
