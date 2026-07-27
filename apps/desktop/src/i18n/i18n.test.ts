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
});
