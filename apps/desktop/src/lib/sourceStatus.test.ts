import { describe, expect, it } from "vitest";
import { capabilityTranslationKey } from "./sourceStatus";

describe("source status language", () => {
  it("maps internal capability enums to user-facing translation keys", () => {
    expect(capabilityTranslationKey("full", true)).toBe("sources.capabilities.full");
    expect(capabilityTranslationKey("partial", true)).toBe("sources.capabilities.partial");
    expect(capabilityTranslationKey("basic", true)).toBe("sources.capabilities.basic");
    expect(capabilityTranslationKey("full", false)).toBe("sources.capabilities.unavailable");
  });
});
