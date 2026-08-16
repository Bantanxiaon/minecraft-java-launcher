import { describe, expect, it } from "vitest";
import { highlightsFor } from "./versionHighlights";

describe("highlightsFor", () => {
  it("returns items for a released version", () => {
    expect(highlightsFor("0.8.1").length).toBeGreaterThan(0);
    expect(highlightsFor("0.8.0").length).toBeGreaterThan(0);
  });

  it("returns empty list for unknown versions", () => {
    expect(highlightsFor("9.9.9")).toEqual([]);
  });
});
