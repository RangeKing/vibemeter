import { beforeEach, describe, expect, it } from "vitest";
import { useUiStore } from "./store";

describe("UI store", () => {
  beforeEach(() => {
    useUiStore.setState({ page: "data", range: "90d", selectedSessionId: undefined });
  });

  it("opens the Data page by default", () => {
    expect(useUiStore.getState().page).toBe("data");
  });

  it("defaults the global range to 90 days", () => {
    expect(useUiStore.getState().range).toBe("90d");
  });

  it("opens sessions as a first-class page and keeps an optional deep link", () => {
    useUiStore.getState().openSessions("session-42");

    expect(useUiStore.getState().page).toBe("sessions");
    expect(useUiStore.getState().selectedSessionId).toBe("session-42");
  });

  it("clears the selected session when leaving the sessions page", () => {
    useUiStore.getState().openSessions("session-42");
    useUiStore.getState().setPage("data");

    expect(useUiStore.getState().page).toBe("data");
    expect(useUiStore.getState().selectedSessionId).toBeUndefined();
  });
});
