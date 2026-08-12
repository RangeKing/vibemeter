import { describe, expect, it } from "vitest";
import type { SourceStatus } from "../types";
import {
  capabilityTranslationKey,
  dataFilterAgents,
  defaultDataAgents,
  parseSourceCapabilities,
  sourceCapabilityNameGroups,
  sourceNamesForLiveCapability,
  sourceLiveTranslationKey,
} from "./sourceStatus";

describe("source status language", () => {
  it("maps internal capability enums to user-facing translation keys", () => {
    expect(capabilityTranslationKey("full", true)).toBe("sources.capabilities.full");
    expect(capabilityTranslationKey("partial", true)).toBe("sources.capabilities.partial");
    expect(capabilityTranslationKey("basic", true)).toBe("sources.capabilities.basic");
    expect(capabilityTranslationKey("full", false)).toBe("sources.capabilities.unavailable");
  });

  it("groups sources by the shared live capability contract", () => {
    expect(sourceNamesForLiveCapability("exact")).toEqual(["Claude Code", "Codex"]);
    expect(sourceNamesForLiveCapability("experimental")).toEqual(["Kimi Code", "ZCode"]);
    expect(sourceNamesForLiveCapability("none")).toEqual(["Cursor", "OpenClaw", "Hermes"]);
    expect(sourceLiveTranslationKey("exact", true)).toBe("sources.liveCapabilities.exactReady");
    expect(sourceLiveTranslationKey("experimental", true)).toBe("sources.liveCapabilities.experimentalReady");
    expect(sourceLiveTranslationKey("none", true)).toBe("sources.liveCapabilities.historyOnly");
    expect(sourceLiveTranslationKey("exact", undefined)).toBe("sources.liveCapabilities.unknown");
    expect(sourceLiveTranslationKey("none", undefined)).toBe("sources.liveCapabilities.historyOnly");
  });

  it("builds the product-copy source groups from the same registry", () => {
    expect(sourceCapabilityNameGroups()).toEqual({
      exact: "Claude Code、Codex",
      experimental: "Kimi Code、ZCode",
      historyOnly: "Cursor、OpenClaw、Hermes",
    });
  });

  it("rejects an unknown capability value instead of silently downgrading it", () => {
    expect(() => parseSourceCapabilities({
      version: 1,
      sources: [{
        agent: "codex",
        displayName: "Codex",
        historyCapability: "full",
        liveCapability: "typo",
        jumpSupported: true,
      }],
    })).toThrow("Unknown live capability");
  });

  it("only shows detected, selected Agents with positive usage by default", () => {
    const source = (overrides: Partial<SourceStatus>): SourceStatus => ({
      agent: "codex",
      available: true,
      selected: true,
      capabilityLevel: "full",
      liveCapability: "exact",
      parserVersion: "test-parser",
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

  it("keeps experimental live sources available as Data filters before history is observed", () => {
    const source = (overrides: Partial<SourceStatus>): SourceStatus => ({
      agent: "codex",
      available: true,
      selected: true,
      capabilityLevel: "full",
      liveCapability: "exact",
      parserVersion: "test-parser",
      sessionCount: 1,
      status: "ready",
      warningCount: 0,
      pathLabel: "",
      ...overrides,
    });

    expect(dataFilterAgents([
      source({ agent: "codex" }),
      source({
        agent: "zcode",
        available: false,
        liveCapability: "experimental",
        sessionCount: 0,
        status: "not-found",
      }),
      source({
        agent: "kimi-code",
        available: false,
        liveCapability: "experimental",
        sessionCount: 0,
        status: "not-found",
      }),
      source({ agent: "openclaw", available: false, liveCapability: "none", sessionCount: 0 }),
    ])).toEqual(["codex", "zcode", "kimi-code"]);
  });
});
