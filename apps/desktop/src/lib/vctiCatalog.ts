export const VCTI_GUILDS = ["start", "agent", "quality", "debug", "delivery", "tools"] as const;
export type VctiGuild = (typeof VCTI_GUILDS)[number];

export const VCTI_TYPES = [
  ["VIBE", "start"], ["SPEC", "start"], ["HACK", "start"], ["MIX", "start"],
  ["YOLO", "agent"], ["LOOP", "agent"], ["BOSS", "agent"], ["SWARM", "agent"],
  ["DIFF", "quality"], ["TEST", "quality"], ["DOCS", "quality"], ["UNDO", "quality"],
  ["DEBUG", "debug"], ["PATCH", "debug"], ["STACK", "debug"], ["AUTO", "debug"],
  ["SHIP", "delivery"], ["RUSH", "delivery"], ["MVP", "delivery"], ["DETAIL", "delivery"],
  ["FORK", "tools"], ["TOKEN", "tools"], ["CACHE", "tools"], ["BUDDY", "tools"],
] as const;

export const VCTI_TYPE_GUILD = Object.fromEntries(VCTI_TYPES) as Record<string, VctiGuild>;

export const VCTI_TYPE_POSITION: Record<string, string> = {
  VIBE: "0% 0%",
  SPEC: "20% 0%",
  HACK: "0% 33.333%",
  MIX: "20% 33.333%",
  YOLO: "40% 0%",
  LOOP: "60% 0%",
  BOSS: "40% 33.333%",
  SWARM: "60% 33.333%",
  DIFF: "80% 0%",
  TEST: "100% 0%",
  DOCS: "80% 33.333%",
  UNDO: "100% 33.333%",
  DEBUG: "0% 66.667%",
  PATCH: "20% 66.667%",
  STACK: "0% 100%",
  AUTO: "20% 100%",
  SHIP: "40% 66.667%",
  RUSH: "60% 66.667%",
  MVP: "40% 100%",
  DETAIL: "60% 100%",
  FORK: "80% 66.667%",
  TOKEN: "100% 66.667%",
  CACHE: "80% 100%",
  BUDDY: "100% 100%",
};

export const VCTI_GUILD_PREVIEW_TYPE: Record<VctiGuild, string> = {
  start: "VIBE",
  agent: "BOSS",
  quality: "TEST",
  debug: "DEBUG",
  delivery: "SHIP",
  tools: "BUDDY",
};

export const VCTI_GUILD_ACCENT: Record<VctiGuild, string> = {
  start: "#f17b36",
  agent: "#df5b3f",
  quality: "#245ca6",
  debug: "#27354a",
  delivery: "#ee7532",
  tools: "#3473a7",
};
