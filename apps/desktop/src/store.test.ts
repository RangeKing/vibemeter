import { describe, expect, it } from "vitest";
import { useUiStore } from "./store";

describe("UI store", () => {
  it("opens the data page by default", () => {
    expect(useUiStore.getState().page).toBe("data");
  });
});
