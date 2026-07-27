// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import { I18nextProvider } from "react-i18next";
import i18n from "../i18n";
import type { PhraseCloudResponse } from "../types";
import { CatchphraseClouds } from "./CatchphraseClouds";

const response: PhraseCloudResponse = {
  range: "30d",
  generatedAt: "2026-07-25T00:00:00Z",
  user: {
    status: "ready",
    sampleSessions: 4,
    items: [{
      phrase: "先验证一下",
      occurrences: 6,
      sessionCount: 4,
      weight: 1,
      agents: [{ agent: "codex", occurrences: 6, sessionCount: 4 }],
      models: [{ model: "gpt-5.4", occurrences: 6, sessionCount: 4 }],
    }],
  },
  agents: {
    status: "ready",
    sampleSessions: 3,
    items: [
      {
        phrase: "我会先",
        occurrences: 8,
        sessionCount: 3,
        weight: 1,
        dominantAgent: "codex",
        dominantModel: "gpt-5.4",
        agents: [{ agent: "codex", occurrences: 8, sessionCount: 3 }],
        models: [{ model: "gpt-5.4", occurrences: 8, sessionCount: 3 }],
      },
      {
        phrase: "你接受……吗",
        occurrences: 4,
        sessionCount: 2,
        weight: 0.55,
        dominantAgent: "codex",
        dominantModel: "gpt-5.4",
        agents: [{ agent: "codex", occurrences: 4, sessionCount: 2 }],
        models: [{ model: "gpt-5.4", occurrences: 4, sessionCount: 2 }],
      },
    ],
  },
  legend: [{ agent: "codex", occurrences: 12 }],
};

describe("CatchphraseClouds", () => {
  beforeAll(async () => {
    await i18n.changeLanguage("zh-CN");
  });

  afterEach(cleanup);

  it("promotes a complete phrase and shows model-first evidence", () => {
    render(
      <I18nextProvider i18n={i18n}>
        <CatchphraseClouds data={response} locale="zh-CN" />
      </I18nextProvider>,
    );

    expect(screen.getByText("我会先")).toBeTruthy();
    expect(screen.getByText("8 次 · 横跨 3 个会话")).toBeTruthy();
    expect(screen.getByText("gpt-5.4")).toBeTruthy();
    expect(screen.getByText("你接受……吗")).toBeTruthy();
  });
});
