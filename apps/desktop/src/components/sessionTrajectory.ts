import type { CanonicalEvent, ProcessPhase } from "../types";

export type TrajectoryLane = "input" | "agent" | "tools";
export type TrajectoryScale = "time" | "sequence";

export interface TrajectorySpan {
  event: CanonicalEvent;
  lane: TrajectoryLane;
  phaseId: string;
  phaseKey: string;
  position: number;
  width: number;
  durationMs: number | null;
  instant: boolean;
}

export interface TrajectoryBounds {
  startedAt?: string | null;
  endedAt?: string | null;
}

export interface SessionTrajectory {
  scale: TrajectoryScale;
  spans: TrajectorySpan[];
  durationMs: number | null;
}

const inputEvents = new Set(["prompt.observed"]);
const toolEvents = new Set(["tool.observed", "verification.observed"]);

export function trajectoryLaneForEventType(eventType: string): TrajectoryLane {
  if (inputEvents.has(eventType)) return "input";
  if (toolEvents.has(eventType)) return "tools";
  return "agent";
}

function validTimestamp(value: string | null | undefined): number | null {
  if (!value) return null;
  const timestamp = Date.parse(value);
  return Number.isFinite(timestamp) ? timestamp : null;
}

function boundedDomain(
  timestamps: number[],
  bounds: TrajectoryBounds,
): { start: number; end: number } {
  const firstEvent = Math.min(...timestamps);
  const lastEvent = Math.max(...timestamps);
  const requestedStart = validTimestamp(bounds.startedAt);
  const requestedEnd = validTimestamp(bounds.endedAt);
  return {
    start: requestedStart !== null && requestedStart <= firstEvent ? requestedStart : firstEvent,
    end: requestedEnd !== null && requestedEnd >= lastEvent ? requestedEnd : lastEvent,
  };
}

export function buildTrajectory(
  phases: ProcessPhase[],
  bounds: TrajectoryBounds = {},
): SessionTrajectory {
  const flattened = phases.flatMap((phase) => phase.events.map((event) => ({
    event,
    phaseId: phase.id,
    phaseKey: phase.phaseKey,
  })));
  if (flattened.length === 0) return { scale: "sequence", spans: [], durationMs: null };

  const timestamps = flattened.map(({ event }) => validTimestamp(event.occurredAt));
  const completeTimes = timestamps.every((timestamp): timestamp is number => timestamp !== null);
  const timeDomain = completeTimes ? boundedDomain(timestamps, bounds) : null;
  const scale: TrajectoryScale = timeDomain !== null && timeDomain.end > timeDomain.start
    ? "time"
    : "sequence";
  const domainStart = scale === "time" ? timeDomain!.start : 0;
  const domainEnd = scale === "time" ? timeDomain!.end : Math.max(1, flattened.length - 1);
  const domainDuration = Math.max(1, domainEnd - domainStart);
  const hasLifecycleStart = flattened.some(({ event }) => event.eventType === "lifecycle.start");
  let agentActive = !hasLifecycleStart;
  const spans: TrajectorySpan[] = [];

  const positionFor = (value: number) => ((value - domainStart) / domainDuration) * 100;
  const appendSpan = (
    index: number,
    lane: TrajectoryLane,
    start: number,
    end: number,
    instant = false,
  ) => {
    const item = flattened[index];
    if (!item) return;
    const boundedStart = Math.min(domainEnd, Math.max(domainStart, start));
    const boundedEnd = Math.min(domainEnd, Math.max(boundedStart, end));
    spans.push({
      ...item,
      lane,
      position: positionFor(boundedStart),
      width: ((boundedEnd - boundedStart) / domainDuration) * 100,
      durationMs: scale === "time" ? boundedEnd - boundedStart : null,
      instant: instant || boundedEnd === boundedStart,
    });
  };

  flattened.forEach(({ event }, index) => {
    const nextEventType = flattened[index + 1]?.event.eventType;
    const eventPosition = scale === "time"
      ? timestamps[index]!
      : flattened.length > 1 ? index : 0.5;
    const nextPosition = scale === "time"
      ? timestamps[index + 1] ?? domainEnd
      : flattened.length > 1 ? Math.min(domainEnd, index + 1) : 0.5;
    const beginsActivity = event.eventType === "lifecycle.start"
      || (!agentActive && event.eventType === "prompt.observed");
    const endsActivity = event.eventType === "lifecycle.complete"
      || (event.eventType === "lifecycle.error"
        && (nextEventType === "lifecycle.start" || nextEventType === "prompt.observed"));
    if (beginsActivity) agentActive = true;

    if (agentActive && !endsActivity) {
      appendSpan(index, "agent", eventPosition, nextPosition);
    }

    const evidenceLane = trajectoryLaneForEventType(event.eventType);
    if (evidenceLane === "input") {
      appendSpan(index, "input", eventPosition, eventPosition, true);
    } else if (evidenceLane === "tools") {
      const recordedEnd = scale === "time" && event.durationMs !== undefined
        ? eventPosition + Math.max(0, event.durationMs)
        : nextPosition;
      appendSpan(index, "tools", eventPosition, recordedEnd);
    }

    if (endsActivity) agentActive = false;
  });

  return {
    scale,
    spans,
    durationMs: scale === "time" ? domainEnd - domainStart : null,
  };
}
