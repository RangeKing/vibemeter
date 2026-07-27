import { describe, expect, it } from "vitest";
import { useUiStore } from "./store";

describe("UI store", () => {
  it("opens the VCTI page by default", () => {
    expect(useUiStore.getState().page).toBe("vcti");
  });

  it("defaults the global range to 90 days", () => {
    expect(useUiStore.getState().range).toBe("90d");
  });
});
