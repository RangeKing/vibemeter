// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import { I18nextProvider } from "react-i18next";
import i18n from "../i18n";
import type { TaskSummary } from "../types";
import { WorkEventCard } from "./WorkEventCard";

const baseTask: TaskSummary = {
  id: "task",
  title: "Untitled session",
  projectLabel: "project",
  status: "unverified",
  confidence: 1,
  groupingState: "separate",
  groupingReasonKeys: [],
  sessionCount: 1,
  startedAt: "2026-08-03T00:00:00Z",
  agent: "cursor",
  filesChanged: 0,
  linesAdded: 0,
  linesDeleted: 0,
  totalTokens: 0,
  hasCommit: false,
  verificationState: "not-applicable",
  worthReviewing: false,
  reviewReasonKeys: [],
  sourceExcluded: false,
};

describe("WorkEventCard", () => {
  beforeAll(async () => {
    await i18n.changeLanguage("zh-CN");
  });

  afterEach(cleanup);

  it("does not call an unedited session unverified", () => {
    render(
      <I18nextProvider i18n={i18n}>
        <WorkEventCard task={baseTask} locale="zh-CN" />
      </I18nextProvider>,
    );

    expect(screen.getByText("无编辑证据")).toBeTruthy();
    expect(screen.queryByText("未验证")).toBeNull();
  });
});
