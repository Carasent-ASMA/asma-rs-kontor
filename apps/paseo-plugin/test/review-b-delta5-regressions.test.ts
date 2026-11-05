import { describe, expect, it } from "vitest";
import { opencodeModelVisible, opencodeSelection, type OpenCodeSelection } from "../server/opencode-config";

/**
 * Review-b delta-5, D5B-F1. Within one document, an alias-spelled declaration of a provider id must never
 * replace or weaken the canonical spelling of the same normalized id, whichever container either is in.
 * Documents and ids are synthetic.
 */
const path = "/synthetic/alias.fixture";

function selection(info: Record<string, unknown>): OpenCodeSelection {
  const read = opencodeSelection([{ path, info }]);
  if ("problem" in read) throw new Error(read.problem);
  return read.selection;
}

function expectCanonicalWhitelist(selection: OpenCodeSelection) {
  expect(opencodeModelVisible("azure-cognitive-services/one", selection)).toBe(true);
  expect(opencodeModelVisible("azure-cognitive-services/two", selection)).toBe(false);
  expect(opencodeModelVisible("azure-cognitive-services/three", selection)).toBe(false);
}

describe("D5B-F1 a canonical declaration survives an alias spelling", () => {
  it("keeps the canonical whitelist when a later alias sits in the other container", () => {
    expectCanonicalWhitelist(selection({
      provider: { azure: { whitelist: ["one"] } }, providers: { "azure-cognitive-services": {} },
    }));
  });

  it("keeps the canonical whitelist when a later alias sits in the canonical container", () => {
    expectCanonicalWhitelist(selection({
      providers: { azure: { whitelist: ["one"] }, "azure-cognitive-services": {} },
    }));
  });

  it("keeps the canonical whitelist when the alias is declared first in the canonical container", () => {
    expectCanonicalWhitelist(selection({
      providers: { "azure-cognitive-services": {}, azure: { whitelist: ["one"] } },
    }));
  });

  it("keeps the canonical whitelist when the alias is declared first in the other container", () => {
    expectCanonicalWhitelist(selection({
      provider: { "azure-cognitive-services": {} }, providers: { azure: { whitelist: ["one"] } },
    }));
  });

  it("keeps a canonical blacklist when a later alias has no filters", () => {
    const selected = selection({
      provider: { azure: { blacklist: ["two"] } }, providers: { "azure-cognitive-services": {} },
    });
    expect(opencodeModelVisible("azure-cognitive-services/one", selected)).toBe(true);
    expect(opencodeModelVisible("azure-cognitive-services/two", selected)).toBe(false);
    expect(opencodeModelVisible("azure-cognitive-services/three", selected)).toBe(true);
  });

  it("raises no problem when neither spelling declares a whitelist and the canonical filters stay", () => {
    const selected = selection({
      provider: { azure: { blacklist: ["two"] } },
      providers: { "azure-cognitive-services": { blacklist: ["three"] } },
    });
    expect(selected.filters.get("azure")).toEqual({ whitelist: null, blacklist: ["two"] });
    expect(opencodeModelVisible("azure-cognitive-services/three", selected)).toBe(true);
  });
});
