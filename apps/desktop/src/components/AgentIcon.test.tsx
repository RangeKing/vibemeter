// @vitest-environment jsdom

import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { AgentIcon, agentIconUrl } from "./AgentIcon";
import { AgentBadge } from "./ui";

const AGENTS = [
  "claude-code",
  "codex",
  "deepseek-harness",
  "kimi-code",
  "cursor",
  "openclaw",
  "hermes",
  "zcode",
];

describe("Agent brand icons", () => {
  afterEach(cleanup);

  it("assigns every supported Agent a distinct real brand asset", () => {
    const urls = AGENTS.map((agent) => agentIconUrl(agent));
    expect(urls.every(Boolean)).toBe(true);
    expect(new Set(urls).size).toBe(AGENTS.length);
    expect(agentIconUrl("deepseek-harness")).toContain("DeepSeek");
    expect(agentIconUrl("zcode")).toContain("ZCode");
    expect(agentIconUrl("deepseek-harness")).not.toBe(agentIconUrl("zcode"));
  });

  it("renders brand masks in shared icons and badges without letter fallbacks", () => {
    const { container } = render(
      <>
        {AGENTS.map((agent) => <AgentIcon key={agent} agent={agent} />)}
        <AgentBadge agent="zcode" compact />
      </>,
    );

    expect(container.querySelectorAll(".agent-icon")).toHaveLength(AGENTS.length + 1);
    expect(container.querySelector(".provider-mark-deepseek-harness")).toBeTruthy();
    expect(container.querySelector(".provider-mark-zcode")).toBeTruthy();
    expect(container.querySelector(".agent-glyph")?.textContent).toBe("");
  });
});
