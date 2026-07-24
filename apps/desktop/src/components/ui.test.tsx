// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import { I18nextProvider } from "react-i18next";
import i18n from "../i18n";
import type { SessionSummary } from "../types";
import { SessionEvidence } from "./ui";

const baseSession: SessionSummary = {
  id: "session",
  agent: "codex",
  title: "Observed session",
  projectLabel: "private",
  startedAt: "2026-07-20T00:00:00Z",
  activeSeconds: 120,
  usage: {
    inputTokens: 100,
    outputTokens: 20,
    cacheReadTokens: 0,
    cacheWriteTokens: 0,
    cacheWrite1hTokens: 0,
    reasoningTokens: 0,
  },
  costCoverage: 1,
  toolCalls: 7,
  filesTouched: 0,
  linesAdded: 0,
  linesDeleted: 0,
  errors: 0,
  retries: 0,
  hasCommit: false,
  verificationState: "not-applicable",
  longestUninterruptedSeconds: 120,
  subagentCount: 0,
  provenance: "observed",
};

function renderEvidence(overrides: Partial<SessionSummary> = {}) {
  render(
    <I18nextProvider i18n={i18n}>
      <SessionEvidence session={{ ...baseSession, ...overrides }} locale="zh-CN" />
    </I18nextProvider>,
  );
}

describe("SessionEvidence", () => {
  beforeAll(async () => {
    await i18n.changeLanguage("zh-CN");
  });

  afterEach(cleanup);

  it("shows session-specific tool activity instead of a missing-evidence badge", () => {
    renderEvidence();
    expect(screen.getByText("7")).toBeTruthy();
    expect(screen.getByText("工具调用")).toBeTruthy();
    expect(screen.queryByText("无编辑或验证证据")).toBeNull();
  });

  it("shows observed patch lines for an edited session", () => {
    renderEvidence({ filesTouched: 2, linesAdded: 12, linesDeleted: 3, verificationState: "unverified" });
    expect(screen.getByText("+12 / −3")).toBeTruthy();
    expect(screen.getByText("补丁行数")).toBeTruthy();
    expect(screen.queryByText("已观测到编辑，未见验证")).toBeNull();
  });

  it("only emphasizes verification when it was observed", () => {
    renderEvidence({ verificationState: "verified" });
    expect(screen.getByText("已验证")).toBeTruthy();
  });
});
