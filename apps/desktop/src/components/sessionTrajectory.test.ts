import { describe, expect, it } from "vitest";
import type { CanonicalEvent, ProcessPhase } from "../types";
import { buildTrajectory, trajectoryLaneForEventType } from "./sessionTrajectory";

function event(sequence: number, eventType: string, occurredAt?: string): CanonicalEvent {
  return { sequence, eventType, category: "execute", name: eventType, occurredAt, provenance: "test" };
}

function phase(events: CanonicalEvent[]): ProcessPhase {
  return { id: "phase-1", phaseKey: "execute", eventCount: events.length, provenance: "test", events };
}

describe("session trajectory", () => {
  it("keeps input and tools distinct while treating other evidence as Agent activity", () => {
    expect(trajectoryLaneForEventType("prompt.observed")).toBe("input");
    expect(trajectoryLaneForEventType("agent.activity")).toBe("agent");
    expect(trajectoryLaneForEventType("verification.observed")).toBe("tools");
    expect(trajectoryLaneForEventType("goal.changed")).toBe("agent");
    expect(trajectoryLaneForEventType("future.event")).toBe("agent");
  });

  it("projects Agent work and tool calls to their observed wall-clock spans", () => {
    const trajectory = buildTrajectory([phase([
      event(1, "prompt.observed", "2026-08-14T10:00:00Z"),
      event(2, "agent.activity", "2026-08-14T10:00:10Z"),
      event(3, "tool.observed", "2026-08-14T10:00:40Z"),
    ])]);

    expect(trajectory.scale).toBe("time");
    const agentSpans = trajectory.spans.filter((span) => span.lane === "agent");
    const toolSpan = trajectory.spans.find((span) => span.lane === "tools");
    expect(agentSpans.map((span) => [Math.round(span.position), Math.round(span.width)])).toEqual([
      [0, 25],
      [25, 75],
      [100, 0],
    ]);
    expect(toolSpan && [Math.round(toolSpan.position), Math.round(toolSpan.width)]).toEqual([100, 0]);
  });

  it("falls back to evenly spaced sequence positions when any time is missing", () => {
    const trajectory = buildTrajectory([phase([
      event(1, "prompt.observed", "2026-08-14T10:00:00Z"),
      event(2, "agent.activity"),
      event(3, "tool.observed", "2026-08-14T10:00:40Z"),
    ])]);

    expect(trajectory.scale).toBe("sequence");
    const agentSpans = trajectory.spans.filter((span) => span.lane === "agent");
    expect(agentSpans.map((span) => span.position)).toEqual([0, 50, 100]);
  });

  it("does not paint idle gaps between completed and resumed activity cycles", () => {
    const trajectory = buildTrajectory([phase([
      event(1, "lifecycle.start", "2026-08-14T10:00:00Z"),
      event(2, "prompt.observed", "2026-08-14T10:00:05Z"),
      event(3, "lifecycle.complete", "2026-08-14T10:00:20Z"),
      event(4, "lifecycle.start", "2026-08-14T10:01:40Z"),
      event(5, "prompt.observed", "2026-08-14T10:01:45Z"),
      event(6, "lifecycle.complete", "2026-08-14T10:01:50Z"),
    ])]);

    const agentSpans = trajectory.spans.filter((span) => span.lane === "agent");
    expect(agentSpans.some((span) => span.position < 20 && span.position + span.width > 80)).toBe(false);
    expect(agentSpans.map((span) => span.durationMs)).toEqual([5_000, 15_000, 5_000, 5_000]);
  });

  it("uses the session bounds for the shared time axis", () => {
    const trajectory = buildTrajectory([phase([
      event(1, "lifecycle.start", "2026-08-14T10:00:05Z"),
      event(2, "lifecycle.complete", "2026-08-14T10:00:20Z"),
    ])], {
      startedAt: "2026-08-14T10:00:00Z",
      endedAt: "2026-08-14T10:01:00Z",
    });

    expect(trajectory.durationMs).toBe(60_000);
    expect(Math.round(trajectory.spans[0]?.position ?? -1)).toBe(8);
    expect(Math.round(trajectory.spans[0]?.width ?? -1)).toBe(25);
  });

  it("ends an errored activity cycle when the next observed event is a resume prompt", () => {
    const trajectory = buildTrajectory([phase([
      event(1, "lifecycle.start", "2026-08-14T10:00:00Z"),
      event(2, "lifecycle.error", "2026-08-14T10:00:20Z"),
      event(3, "prompt.observed", "2026-08-14T10:02:00Z"),
      event(4, "lifecycle.complete", "2026-08-14T10:02:10Z"),
    ])]);

    const agentSpans = trajectory.spans.filter((span) => span.lane === "agent");
    expect(agentSpans.map((span) => span.durationMs)).toEqual([20_000, 10_000]);
  });
});
