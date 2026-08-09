import { describe, expect, it } from "vitest";
import { enUS, zhCN } from "./resources";

function keys(value: object, prefix = ""): string[] {
  return Object.entries(value).flatMap(([key, child]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    return typeof child === "object" && child !== null ? keys(child, path) : [path];
  });
}

describe("localization resources", () => {
  it("keeps English and Chinese key coverage identical", () => {
    expect(keys(zhCN).sort()).toEqual(keys(enUS).sort());
  });

  it("contains no blank production translations", () => {
    const values = (value: object): string[] => Object.values(value).flatMap((child) =>
      typeof child === "object" && child !== null ? values(child) : [String(child)],
    );
    expect(values(enUS).every((value) => value.trim().length > 0)).toBe(true);
    expect(values(zhCN).every((value) => value.trim().length > 0)).toBe(true);
  });

  it("uses the approved VibeMeter product tagline", () => {
    expect(zhCN.app.tagline).toBe("追踪你的 Agent，发现你的 AI 编程人格。");
    expect(enUS.app.tagline).toBe("Track your agents. Discover your coding type.");
  });

  it("keeps Notch phase and action labels compact in both locales", () => {
    expect(zhCN.notch.phase.compacting).toBe("压缩上下文");
    expect(enUS.notch.phase.compacting).toBe("Compact");
    expect(zhCN.notch.phase["running-tool"]).toBe("调用工具");
    expect(enUS.notch.phase["running-tool"]).toBe("Tool use");
    expect(enUS.notch.action.waiting).toBe("Approval");
  });

  it("keeps all 24 VCTI personas distinct and shareable", () => {
    const personas = Object.values(zhCN.vcti.types);
    expect(personas).toHaveLength(24);
    expect(new Set(personas.map((persona) => persona.name))).toHaveLength(24);
    expect(new Set(personas.map((persona) => persona.tagline))).toHaveLength(24);
    expect(zhCN.vcti.types.BOSS).toEqual({
      name: "Agent 包工头",
      tagline: "别人把 Agent 当助手，你已经给它们排班、派活、验收。",
    });
  });

  it("explains earned VCTI badges as evidence-bound traits", () => {
    expect(zhCN.vcti.badgeAtlasBody).toContain("副标签不是第二人格");
    expect(zhCN.vcti.badgeAtlasBody).not.toContain("这里只展示");
    expect(Object.keys(zhCN.vcti.badges)).toHaveLength(9);
    expect(zhCN.vcti.badges.MARATHON.description).toContain("工具、修改或验证事件");
    expect(zhCN.vcti.badges.TURBO.description).toContain("第一个结果");
  });

  it("does not expose the retired review workspace", () => {
    expect("reviews" in zhCN.navigation).toBe(false);
    expect("reviews" in enUS.navigation).toBe(false);
    expect("playbook" in zhCN.navigation).toBe(false);
    expect("playbook" in enUS.navigation).toBe(false);
    expect("deepReview" in zhCN.settings).toBe(false);
    expect("deepReview" in enUS.settings).toBe(false);
  });

  it("does not ship a generic session-story vocabulary", () => {
    expect("story" in zhCN.sessions).toBe(false);
    expect("story" in enUS.sessions).toBe(false);
  });

  it("keeps internal source-parser jargon out of the interface", () => {
    expect(JSON.stringify(zhCN.sources)).not.toContain("解析警告");
    expect(JSON.stringify(zhCN.sources)).not.toContain("full 覆盖");
    expect(JSON.stringify(enUS.sources)).not.toContain("parser warnings");
  });

  it("distinguishes exact live, experimental activity, and historical-only sources", () => {
    expect(zhCN.live.description).toContain("精确实时生命周期");
    expect(zhCN.live.description).toContain("实验性近期活动");
    expect(zhCN.sources.liveCapabilities.historyOnly).toBe("仅提供历史证据");
    expect(enUS.live.description).toContain("exact live lifecycle status");
    expect(enUS.live.description).toContain("experimental recent activity");
    expect(enUS.sources.liveCapabilities.historyOnly).toBe("Historical evidence only");
    expect(zhCN.sources.parserVersion).toContain("{{version}}");
    expect(enUS.sources.parserVersion).toContain("{{version}}");
  });
});
