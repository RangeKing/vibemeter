// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import { I18nextProvider } from "react-i18next";
import i18n from "../i18n";
import { SourceCapabilityLegend } from "./SourceCapabilityLegend";

describe("Data source capability legend", () => {
  beforeAll(async () => {
    await i18n.changeLanguage("zh-CN");
  });

  afterEach(cleanup);

  it("shows only populated source capability groups from the shared registry", () => {
    render(
      <I18nextProvider i18n={i18n}>
        <SourceCapabilityLegend locale="zh-CN" />
      </I18nextProvider>,
    );

    expect(screen.getByRole("complementary", { name: "来源能力" })).toBeTruthy();
    expect(screen.getByText("Claude Code、Codex、DeepSeek Harness、Kimi Code、Grok Build、ZCode")).toBeTruthy();
    expect(screen.getByText("Cursor、OpenClaw、Hermes")).toBeTruthy();
    expect(screen.getByText("精确实时")).toBeTruthy();
    expect(screen.queryByText("实验性近期活动")).toBeNull();
    expect(screen.getByText("仅历史证据")).toBeTruthy();
  });
});
