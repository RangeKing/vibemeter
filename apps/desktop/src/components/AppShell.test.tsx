// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "../i18n";
import { useUiStore } from "../store";
import { AppShell } from "./AppShell";

beforeAll(async () => {
  Object.defineProperty(HTMLElement.prototype, "scrollTo", {
    configurable: true,
    value: vi.fn(),
  });
  await i18n.changeLanguage("zh-CN");
});

beforeEach(() => {
  useUiStore.setState({ page: "data", selectedSessionId: undefined });
});

afterEach(cleanup);

describe("AppShell primary navigation", () => {
  it("shows Sessions beside Data and navigates to its own page", () => {
    render(
      <I18nextProvider i18n={i18n}>
        <AppShell><div>content</div></AppShell>
      </I18nextProvider>,
    );

    const sessions = screen.getByRole("button", { name: "会话" });
    fireEvent.click(sessions);

    expect(useUiStore.getState().page).toBe("sessions");
    expect(sessions.getAttribute("aria-current")).toBe("page");
  });
});
