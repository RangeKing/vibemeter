import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { LiveSession } from "../types";
import { AgentActivityGlyph } from "./NotchSurface";

const session = {
  status: "running",
  phase: "editing",
} satisfies Pick<LiveSession, "phase" | "status">;

describe("AgentActivityGlyph", () => {
  it("uses the current work phase instead of an unrelated generic signal", () => {
    const markup = renderToStaticMarkup(<AgentActivityGlyph session={session} />);
    expect(markup).toContain("status-running");
    expect(markup).toContain("phase-editing");
    expect(markup).toContain("lucide-file-pen-line");
  });

  it("switches to an explicit completion mark", () => {
    const markup = renderToStaticMarkup(
      <AgentActivityGlyph session={{ status: "completed", phase: "completed" }} />,
    );
    expect(markup).toContain("status-completed");
    expect(markup).toContain("lucide-check");
  });
});
