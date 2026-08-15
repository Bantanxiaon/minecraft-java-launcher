import { describe, expect, it } from "vitest";
import { gameVersionMatches, loaderLabel } from "./ui";

describe("gameVersionMatches", () => {
  it("parses maven-style ranges", () => {
    expect(gameVersionMatches("[1.0,)", "1.20.1")).toBe(true);
    expect(gameVersionMatches("(,2.0]", "1.20.1")).toBe(true);
    expect(gameVersionMatches("[1.0,2.0)", "2.0.0")).toBe(false);
    expect(gameVersionMatches("[1.0,2.0)", "1.16.5")).toBe(true);
  });

  it("parses exact versions and operators", () => {
    expect(gameVersionMatches("1.20.1", "1.20.1")).toBe(true);
    expect(gameVersionMatches(">=1.0", "1.20.1")).toBe(true);
    expect(gameVersionMatches("<=2.0", "1.20.1")).toBe(true);
    expect(gameVersionMatches("<=1.19", "1.20.1")).toBe(false);
  });

  it("treats template placeholders as any version", () => {
    expect(gameVersionMatches("${minecraft_version_range}", "1.20.1")).toBe(true);
    expect(gameVersionMatches("*", "1.20.1")).toBe(true);
  });
});

describe("loaderLabel", () => {
  it("returns readable loader names", () => {
    expect(loaderLabel("forge")).toBe("Forge");
    expect(loaderLabel("fabric")).toBe("Fabric");
    expect(loaderLabel("unknown")).toBe("unknown");
  });
});
