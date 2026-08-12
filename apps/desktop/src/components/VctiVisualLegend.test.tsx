// @vitest-environment jsdom

import { fireEvent, render, screen } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { I18nextProvider } from "react-i18next";
import i18n from "../i18n";
import { VctiVisualLegend } from "./VctiVisualLegend";

describe("VctiVisualLegend", () => {
  beforeAll(async () => {
    await i18n.changeLanguage("zh-CN");
  });

  it("explains all six layers and uses plain missing-data language", () => {
    const onOpenEvidence = vi.fn();
    render(
      <I18nextProvider i18n={i18n}>
        <VctiVisualLegend
          inputs={[
            { id: "identity", available: true, status: "recorded" },
            { id: "dimensions", available: true, status: "recorded" },
            { id: "work-periods", available: false, status: "not-recorded" },
            { id: "active-days", available: false, status: "not-recorded" },
            { id: "session-density", available: true, status: "recorded" },
            { id: "subagent-starts", available: true, status: "recorded" },
            { id: "parallel-batches", available: true, status: "recorded" },
            { id: "tool-categories", available: true, status: "recorded" },
            { id: "explicit-skills", available: true, status: "recorded" },
            { id: "errors", available: true, status: "recorded" },
            { id: "retries", available: false, status: "not-recorded" },
            { id: "rollbacks", available: true, status: "recorded" },
          ]}
          onOpenEvidence={onOpenEvidence}
        />
      </I18nextProvider>,
    );

    expect(screen.getAllByRole("listitem")).toHaveLength(6);
    expect(screen.getByText("部分行为数据未记录，本次视觉仅根据现有证据生成")).toBeTruthy();
    expect(screen.getAllByText("该部分数据未记录").length).toBeGreaterThanOrEqual(2);
    fireEvent.click(screen.getByRole("button", { name: "查看证据与覆盖" }));
    expect(onOpenEvidence).toHaveBeenCalledOnce();
  });
});
