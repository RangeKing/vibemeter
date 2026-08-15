import type { CSSProperties } from "react";
import claudeCodeIconUrl from "@lobehub/icons-static-svg/icons/claudecode.svg";
import codexIconUrl from "@lobehub/icons-static-svg/icons/codex.svg";
import cursorIconUrl from "@lobehub/icons-static-svg/icons/cursor.svg";
import deepSeekIconUrl from "@lobehub/icons-static-svg/icons/deepseek.svg";
import hermesIconUrl from "@lobehub/icons-static-svg/icons/hermesagent.svg";
import kimiIconUrl from "@lobehub/icons-static-svg/icons/kimi.svg";
import openClawIconUrl from "@lobehub/icons-static-svg/icons/openclaw.svg";
import zcodeIconUrl from "../assets/providers/zcode.svg";

const AGENT_ICON_URLS: Record<string, string> = {
  "claude-code": claudeCodeIconUrl,
  codex: codexIconUrl,
  cursor: cursorIconUrl,
  "deepseek-harness": deepSeekIconUrl,
  hermes: hermesIconUrl,
  "kimi-code": kimiIconUrl,
  openclaw: openClawIconUrl,
  zcode: zcodeIconUrl,
};

export function agentIconUrl(agent: string): string | undefined {
  return AGENT_ICON_URLS[agent];
}

export function agentIconKind(agent: string): string {
  if (agent === "claude-code") return "claude";
  if (agent === "deepseek-harness") return "deepseek";
  if (agent === "kimi-code") return "kimi";
  return AGENT_ICON_URLS[agent] ? agent : "vibemeter";
}

export function AgentIcon({
  agent,
  size = 16,
  className = "",
}: {
  agent: string;
  size?: number;
  className?: string;
}) {
  const iconUrl = agentIconUrl(agent) ?? codexIconUrl;
  return (
    <span
      aria-hidden="true"
      className={`agent-icon ${agentIconKind(agent)} provider-mark-${agent} ${className}`.trim()}
      style={{
        width: size,
        height: size,
        "--agent-icon": `url("${iconUrl}")`,
      } as CSSProperties}
    />
  );
}
