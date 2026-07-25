import { MessageCircleMore, Sparkles } from "lucide-react";
import { useTranslation } from "react-i18next";
import { agentName, formatCompact } from "../lib/format";
import type { Locale, PhraseCloud, PhraseCloudResponse } from "../types";
import { EmptyState } from "./ui";

const AGENT_COLORS: Record<string, string> = {
  codex: "#ff7a59",
  "claude-code": "#d8a06f",
  claude: "#d8a06f",
  "kimi-code": "#62a9ff",
  kimi: "#62a9ff",
  cursor: "#8b82f6",
  openclaw: "#3fbf9b",
  hermes: "#ee6a9d",
};

function agentColor(agent: string): string {
  return AGENT_COLORS[agent] ?? "#9b8cb8";
}

function phraseDetails(
  item: PhraseCloud["items"][number],
  locale: Locale,
  sessionsLabel: string,
): string {
  const summary = `${item.phrase} · ${formatCompact(item.occurrences, locale)}× · ${formatCompact(item.sessionCount, locale)} ${sessionsLabel}`;
  if (!item.agents.length) return summary;
  return `${summary}\n${item.agents
    .map((entry) => `${agentName(entry.agent)} ${formatCompact(entry.occurrences, locale)}×`)
    .join(" · ")}`;
}

function CloudCard({
  title,
  body,
  cloud,
  locale,
  agentColors,
}: {
  title: string;
  body: string;
  cloud: PhraseCloud;
  locale: Locale;
  agentColors: boolean;
}) {
  const { t } = useTranslation();
  return (
    <article className={`catchphrase-card ${agentColors ? "agent-cloud" : "user-cloud"}`}>
      <header>
        <div className="catchphrase-icon">{agentColors ? <Sparkles size={17} /> : <MessageCircleMore size={17} />}</div>
        <div>
          <h3>{title}</h3>
          <p>{body}</p>
        </div>
        <span className="catchphrase-sample">{t("catchphrases.sessions", { count: cloud.sampleSessions })}</span>
      </header>
      {cloud.status === "ready" ? (
        <div className="catchphrase-cloud" aria-label={title}>
          {cloud.items.map((item) => {
            const color = item.dominantAgent ? agentColor(item.dominantAgent) : undefined;
            return (
              <span
                className="catchphrase-token"
                key={item.phrase}
                title={phraseDetails(item, locale, t("metrics.sessions").toLowerCase())}
                style={{
                  "--phrase-weight": item.weight,
                  "--phrase-color": color,
                  fontSize: `${12 + item.weight * 18}px`,
                } as React.CSSProperties}
              >
                {item.phrase}
              </span>
            );
          })}
        </div>
      ) : (
        <EmptyState title={t("catchphrases.insufficientTitle")} body={t("catchphrases.insufficientBody")} />
      )}
    </article>
  );
}

export function CatchphraseClouds({
  data,
  locale,
}: {
  data: PhraseCloudResponse;
  locale: Locale;
}) {
  const { t } = useTranslation();
  return (
    <section className="catchphrase-section">
      <header className="panel-heading catchphrase-heading">
        <div>
          <span className="section-index">VOICE</span>
          <h2>{t("catchphrases.title")}</h2>
          <p>{t("catchphrases.body")}</p>
        </div>
        <span className="panel-kicker">{t("catchphrases.localOnly")}</span>
      </header>
      <div className="catchphrase-grid">
        <CloudCard
          title={t("catchphrases.mine")}
          body={t("catchphrases.mineBody")}
          cloud={data.user}
          locale={locale}
          agentColors={false}
        />
        <CloudCard
          title={t("catchphrases.agent")}
          body={t("catchphrases.agentBody")}
          cloud={data.agents}
          locale={locale}
          agentColors
        />
      </div>
      {data.legend.length ? (
        <div className="catchphrase-legend" aria-label={t("catchphrases.legend")}>
          <strong>{t("catchphrases.legend")}</strong>
          {data.legend.map((item) => (
            <span key={item.agent}>
              <i style={{ background: agentColor(item.agent) }} />
              {agentName(item.agent)}
            </span>
          ))}
        </div>
      ) : null}
      <p className="catchphrase-method">{t("catchphrases.method")}</p>
    </section>
  );
}
