import { describe, expect, it } from "vitest";
import { enUS, zhCN } from "../i18n/resources";

describe("onboarding ritual copy", () => {
  it("covers the three-step ritual in both locales", () => {
    for (const resources of [enUS, zhCN]) {
      expect(resources.onboarding.startScan.length).toBeGreaterThan(0);
      expect(resources.onboarding.scanTitle.length).toBeGreaterThan(0);
      expect(resources.onboarding.revealTitle.length).toBeGreaterThan(0);
      expect(resources.onboarding.revealNeedMore).toContain("{{count}}");
      expect(resources.onboarding.vctiTitle.length).toBeGreaterThan(0);
      expect("reviewTitle" in resources.onboarding).toBe(false);
    }
  });
});
