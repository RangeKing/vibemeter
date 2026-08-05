import { describe, expect, it } from "vitest";
import type { SourceStatus } from "../types";
import { capabilityTranslationKey, defaultDataAgents } from "./sourceStatus";

describe("source status language", () => {
  it("maps internal capability enums to user-facing translation keys", () => {
    expect(capabilityTranslationKey("full", true)).toBe("sources.capabilities.full");
    expect(capabilityTranslationKey("partial", true)).toBe("sources.capabilities.partial");
    expect(capabilityTranslationKey("basic", true)).toBe("sources.capabilities.basic");
    expect(capabilityTranslationKey("full", false)).toBe("sources.capabilities.unavailable");
  });

  it("only shows detected, selected Agents with positive usage by default", () => {
    const source = (overrides: Partial<SourceStatus>): SourceStatus => ({
      agent: "codex",
      available: true,
      selected: true,
      capabilityLevel: "full",
      sessionCount: 1,
      status: "ready",
      warningCount: 0,
      pathLabel: "",
      ...overrides,
    });

    expect(
      defaultDataAgents(
        [
          source({ agent: "codex" }),
          source({ agent: "claude-code" }),
          source({ agent: "cursor", sessionCount: 2 }),
          source({ agent: "kimi-code", available: false }),
          source({ agent: "openclaw", selected: false }),
          source({ agent: "hermes", sessionCount: 0 }),
        ],
        [
          { label: "codex", value: 128 },
          { label: "claude-code", value: 0 },
          { label: "cursor", value: 0 },
          { label: "kimi-code", value: 64 },
          { label: "openclaw", value: 64 },
          { label: "hermes", value: 64 },
        ],
      ),
    ).toEqual(["codex"]);
  });
});
