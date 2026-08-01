import { useQuery } from "@tanstack/react-query";
import { save } from "@tauri-apps/plugin-dialog";
import { writeImage } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  CheckCircle2,
  ChevronDown,
  Clipboard,
  Download,
  EyeOff,
  History,
  Github,
  Image,
  Languages,
  LayoutTemplate,
  LockKeyhole,
  Maximize2,
  Minus,
  Plus,
  RotateCcw,
  ShieldAlert,
  ShieldCheck,
} from "lucide-react";
import { useDeferredValue, useEffect, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { RangePicker } from "../components/RangePicker";
import { ErrorState, LoadingState, PageHeader, Toggle } from "../components/ui";
import { api } from "../lib/api";
import { agentName } from "../lib/format";
import { useUiStore } from "../store";
import type { AspectRatio, Locale, ShareRenderRequest, ShareTemplate } from "../types";

const dataTemplates: ShareTemplate[] = ["usage-overview", "developer-wrapped", "agent-comparison", "session-recap"];
const identityTemplates: ShareTemplate[] = ["vcti-card", "catchphrases"];
const aspects: AspectRatio[] = ["1:1", "4:5", "3:4", "9:16", "16:9"];
const metricIds = ["sessions", "duration", "tokens", "activeDays", "files", "lines", "tools", "topAgent", "topModel", "activeDate", "cost"];
const templateMetricIds: Record<ShareTemplate, string[]> = {
  "usage-overview": ["sessions", "duration", "tokens", "activeDays", "topAgent", "topModel"],
  "developer-wrapped": ["sessions", "duration", "tokens", "files", "lines", "topAgent", "topModel", "activeDate"],
  "agent-comparison": ["sessions", "duration", "tokens", "files", "lines", "cost"],
  "session-recap": ["duration", "tokens", "files", "lines", "tools", "cost"],
  "vcti-card": ["startStructure", "delegation", "guardrail", "debugDepth", "shipping", "toolNomad"],
  catchphrases: [],
};

const previewFrames: Record<AspectRatio, { width: number; height: number }> = {
  "1:1": { width: 500, height: 500 },
  "2:3": { width: 373, height: 560 },
  "3:2": { width: 700, height: 467 },
  "3:4": { width: 420, height: 560 },
  "4:3": { width: 700, height: 525 },
  "4:5": { width: 448, height: 560 },
  "16:9": { width: 700, height: 394 },
  "9:16": { width: 315, height: 560 },
};

function ratioStyle(aspect: AspectRatio): CSSProperties {
  const [width, height] = aspect.split(":").map(Number);
  return width >= height
    ? { width: 27, aspectRatio: `${width} / ${height}` }
    : { height: 27, aspectRatio: `${width} / ${height}` };
}

function defaultRequest(locale: Locale, range: ShareRenderRequest["range"], templateId: ShareTemplate = "usage-overview"): ShareRenderRequest {
  return {
    templateId,
    locale,
    aspectRatio: "3:4",
    theme: "light",
    range,
    compareIds: [],
    title: "",
    summary: "",
    projectName: "",
    metrics: metricIds.map((id) => ({ id, visible: true })),
    showBrand: true,
    showModel: false,
    showCost: false,
    showProject: false,
    showBehaviorEvidence: false,
    privacyReviewed: false,
  };
}

function clampZoom(value: number): number {
  return Math.max(50, Math.min(160, value));
}

export function ShareStudioPage({ locale }: { locale: Locale }) {
  const { t } = useTranslation();
  const range = useUiStore((state) => state.range);
  const requestedTemplate = useUiStore((state) => state.shareTemplate);
  const [request, setRequest] = useState<ShareRenderRequest>(() => defaultRequest(locale, range, requestedTemplate));
  const sessionTemplate = request.templateId === "session-recap";
  const [zoom, setZoom] = useState(100);
  const [openPanels, setOpenPanels] = useState({ copy: false, display: false, metrics: false });
  const previewStageRef = useRef<HTMLDivElement>(null);
  const [working, setWorking] = useState<string>();
  const [notice, setNotice] = useState<string>();
  const sessions = useQuery({
    queryKey: ["share-sessions", range],
    queryFn: () => api.sessions({ range, page: 0, pageSize: 80 }),
    enabled: sessionTemplate,
  });
  const deferredRequest = useDeferredValue(request);
  const preview = useQuery({ queryKey: ["share-preview", deferredRequest], queryFn: () => api.previewShare(deferredRequest), retry: false });
  useEffect(() => {
    setNotice(undefined);
    setRequest((current) => current.range === range
      ? current
      : { ...current, range, privacyReviewed: false });
  }, [range]);
  useEffect(() => {
    if (!sessionTemplate || !request.sessionId || !sessions.data) return;
    if (sessions.data.items.some((session) => session.id === request.sessionId)) return;
    setRequest((current) => ({ ...current, sessionId: undefined, privacyReviewed: false }));
  }, [request.sessionId, sessionTemplate, sessions.data]);
  const patch = <K extends keyof ShareRenderRequest>(key: K, value: ShareRenderRequest[K]) => {
    setNotice(undefined);
    setRequest((current) => ({ ...current, [key]: value, privacyReviewed: key === "privacyReviewed" ? Boolean(value) : false }));
  };
  const chooseTemplate = (templateId: ShareTemplate) => {
    setRequest((current) => ({
      ...current,
      templateId,
      range,
      privacyReviewed: false,
    }));
    setOpenPanels((current) => ({ ...current, metrics: false }));
  };
  const chooseAspect = (aspectRatio: AspectRatio) => patch("aspectRatio", aspectRatio);
  const reset = () => {
    setRequest(defaultRequest(locale, range, request.templateId));
    setZoom(100);
    setNotice(undefined);
    setOpenPanels({ copy: false, display: false, metrics: false });
  };
  const hideSensitive = () => setRequest((current) => ({ ...current, showModel: false, showProject: false, showCost: false, showBehaviorEvidence: false, privacyReviewed: false }));
  const toggleMetric = (id: string, visible: boolean) => patch("metrics", request.metrics.map((metric) => metric.id === id ? { ...metric, visible } : metric));
  const togglePanel = (key: keyof typeof openPanels) => setOpenPanels((current) => ({ ...current, [key]: !current[key] }));
  const doExport = async (format: "png" | "svg") => {
    setWorking(format);
    setNotice(undefined);
    try {
      const date = new Date().toISOString().slice(0, 10);
      const ratio = request.aspectRatio.replace(":", "x");
      const path = await save({ defaultPath: `vibemeter_${ratio}_${request.templateId}_${date}.${format}`, filters: [{ name: format.toUpperCase(), extensions: [format] }] });
      if (!path) return;
      await api.exportShare(request, format, path);
      setNotice(t("share.exported", { format: format.toUpperCase() }));
    } catch { setNotice(t("share.exportFailed")); } finally { setWorking(undefined); }
  };
  const copy = async () => {
    setWorking("copy");
    setNotice(undefined);
    try {
      const bytes = await api.renderSharePng(request);
      await writeImage(new Uint8Array(bytes));
      setNotice(t("actions.copied"));
    } catch { setNotice(t("share.copyFailed")); } finally { setWorking(undefined); }
  };

  const baseFrame = previewFrames[request.aspectRatio];
  const frame = { width: Math.round(baseFrame.width * zoom / 100), height: Math.round(baseFrame.height * zoom / 100) };
  const zoomBy = (delta: number) => setZoom((current) => clampZoom(current + delta));
  const fitPreview = () => {
    const stage = previewStageRef.current;
    if (!stage) return;
    const horizontalPadding = 64;
    const verticalPadding = 64;
    const fit = Math.floor(Math.min(
      (stage.clientWidth - horizontalPadding) / baseFrame.width,
      (stage.clientHeight - verticalPadding) / baseFrame.height,
      1,
    ) * 100);
    setZoom(clampZoom(fit));
  };
  const metricOptions = templateMetricIds[request.templateId];

  return (
    <div className="page share-page">
      <PageHeader
        title={t("share.title")}
        description={t("share.description")}
        actions={<><RangePicker /><button className="button subtle" onClick={reset}><RotateCcw size={14} />{t("actions.reset")}</button></>}
      />
      <div className="share-layout">
        <section className="share-workspace">
          <div className="share-preview-shell">
            <div className="share-preview-toolbar">
              <div><strong>{t("share.preview")}</strong>{preview.data ? <span>{t("share.exactSize", { width: preview.data.width, height: preview.data.height })}</span> : null}</div>
              <div className="preview-zoom-controls" aria-label={t("share.zoom")}>
                <button onClick={() => zoomBy(-10)} disabled={zoom <= 50} aria-label={`${t("share.zoom")} -`}><Minus size={14} /></button>
                <output>{t("share.zoomValue", { value: zoom })}</output>
                <button onClick={() => zoomBy(10)} disabled={zoom >= 160} aria-label={`${t("share.zoom")} +`}><Plus size={14} /></button>
                <button onClick={fitPreview}><Maximize2 size={13} />{t("share.fit")}</button>
              </div>
            </div>
            <div ref={previewStageRef} className="preview-stage" onWheel={(event) => { if (!event.ctrlKey && !event.metaKey) return; event.preventDefault(); zoomBy(event.deltaY > 0 ? -10 : 10); }}>
              {preview.isLoading ? <LoadingState /> : preview.isError || !preview.data ? <ErrorState retry={() => void preview.refetch()} /> : <div className="preview-viewport"><div className="preview-frame" style={{ width: `${frame.width}px`, height: `${frame.height}px` }}><div className="svg-preview" dangerouslySetInnerHTML={{ __html: preview.data.svg }} /></div></div>}
            </div>
            <div className="share-bottom">
              <section className="guard-panel"><header>{preview.data?.findings.some((item) => item.level === "block") ? <ShieldAlert size={17} /> : <ShieldCheck size={17} />}<strong>{t("share.guardTitle")}</strong></header><div className="guard-findings">{preview.data?.findings.map((finding) => <span key={finding.id} className={finding.level}>{finding.level === "safe" ? <CheckCircle2 size={13} /> : <LockKeyhole size={13} />}{t(finding.messageKey)}</span>)}</div>{preview.data?.findings.some((finding) => finding.level === "review") ? <label className="review-check"><input type="checkbox" checked={request.privacyReviewed} onChange={(event) => patch("privacyReviewed", event.target.checked)} /><span>{t("share.reviewed")}</span></label> : null}</section>
              <div className="export-actions">{notice ? <span className="export-notice">{notice}</span> : null}<button className="button secondary" disabled={!preview.data?.canExport || Boolean(working)} onClick={() => void copy()}><Clipboard size={14} />{working === "copy" ? t("actions.exporting") : t("actions.copyImage")}</button><button className="button secondary" disabled={!preview.data?.canExport || Boolean(working)} onClick={() => void doExport("svg")}><Download size={14} />{t("actions.exportSvg")}</button><button className="button primary" disabled={!preview.data?.canExport || Boolean(working)} onClick={() => void doExport("png")}><Download size={14} />{t("actions.exportPng")}</button></div>
            </div>
          </div>
        </section>

        <aside className="share-controls">
          <section className="control-section">
            <h2><LayoutTemplate size={15} />{t("share.template")}</h2>
            <div className="template-list compact">
              <h3 className="template-group-label">{t("share.dataTemplates")}</h3>
              {dataTemplates.map((template, index) => (
                <button
                  key={template}
                  type="button"
                  className={request.templateId === template ? "active" : ""}
                  title={t(`share.templates.${template}.description`)}
                  onClick={() => chooseTemplate(template)}
                >
                  <i>D{index + 1}</i>
                  <span><strong>{t(`share.templates.${template}.title`)}</strong></span>
                  {request.templateId === template ? <CheckCircle2 size={14} /> : null}
                </button>
              ))}
              {sessionTemplate ? (
                <div className="session-share-picker">
                  <div>
                    <History size={15} />
                    <span><strong>{t("share.selectSession")}</strong><small>{t("share.selectSessionBody")}</small></span>
                  </div>
                  <select
                    aria-label={t("share.selectSession")}
                    value={request.sessionId ?? ""}
                    disabled={sessions.isLoading || !sessions.data?.items.length}
                    onChange={(event) => patch("sessionId", event.target.value || undefined)}
                  >
                    <option value="">{sessions.isLoading ? t("share.loadingSessions") : t("share.latestSession")}</option>
                    {sessions.data?.items.map((session) => (
                      <option key={session.id} value={session.id}>
                        {session.title || session.model || agentName(session.agent)} · {session.projectLabel}
                      </option>
                    ))}
                  </select>
                </div>
              ) : null}
              <h3 className="template-group-label">{t("share.identityTemplates")}</h3>
              {identityTemplates.map((template) => (
                <button
                  key={template}
                  type="button"
                  className={request.templateId === template ? "active identity" : "identity"}
                  title={t(`share.templates.${template}.description`)}
                  onClick={() => chooseTemplate(template)}
                >
                  <i>{template === "vcti-card" ? "V" : "C"}</i>
                  <span><strong>{t(`share.templates.${template}.title`)}</strong></span>
                  {request.templateId === template ? <CheckCircle2 size={14} /> : null}
                </button>
              ))}
            </div>
          </section>

          <section className="control-section">
            <h2><Image size={15} />{t("share.aspect")}</h2>
            <div className="ratio-preset-grid slim">
              {aspects.map((aspect) => (
                <button
                  key={aspect}
                  type="button"
                  className={`ratio-preset ${request.aspectRatio === aspect ? "active" : ""}`}
                  onClick={() => chooseAspect(aspect)}
                  aria-pressed={request.aspectRatio === aspect}
                >
                  <i><span style={ratioStyle(aspect)} /></i>
                  <strong>{aspect}</strong>
                  {request.aspectRatio === aspect ? <CheckCircle2 size={13} /> : null}
                </button>
              ))}
            </div>
            <p className="ratio-hint">{t("share.ratioHint")}</p>
          </section>

          <section className="control-section">
            <div className="share-control-grid">
              <div><h2><Languages size={15} />{t("share.language")}</h2><div className="segmented compact"><button type="button" className={request.locale === "zh-CN" ? "active" : ""} onClick={() => patch("locale", "zh-CN")}>中文</button><button type="button" className={request.locale === "en-US" ? "active" : ""} onClick={() => patch("locale", "en-US")}>EN</button></div></div>
              <div><h2>{t("share.theme")}</h2><div className="segmented compact"><button type="button" className={request.theme === "light" ? "active" : ""} onClick={() => patch("theme", "light")}>{t("share.light")}</button><button type="button" className={request.theme === "dark" ? "active" : ""} onClick={() => patch("theme", "dark")}>{t("share.dark")}</button></div></div>
            </div>
          </section>

          <section className="control-section">
            <button type="button" className="control-disclosure" aria-expanded={openPanels.copy} onClick={() => togglePanel("copy")}>
              <span>{t("share.copyFields")}</span>
              <ChevronDown size={15} />
            </button>
            {openPanels.copy ? (
              <div className="form-section disclosure-body">
                <label><span>{t("share.titleField")} <small>{t("share.optional")}</small></span><input value={request.title} onChange={(event) => patch("title", event.target.value)} maxLength={120} /></label>
                <label><span>{t("share.summaryField")} <small>{t("share.optional")}</small></span><textarea value={request.summary} onChange={(event) => patch("summary", event.target.value)} rows={3} maxLength={220} /></label>
                <label><span>{t("share.projectField")} <small>{t("share.optional")}</small></span><input value={request.projectName} onChange={(event) => patch("projectName", event.target.value)} maxLength={80} /></label>
              </div>
            ) : null}
          </section>

          <section className="control-section">
            <div className="control-heading">
              <button type="button" className="control-disclosure inline" aria-expanded={openPanels.display} onClick={() => togglePanel("display")}>
                <span>{t("share.display")}</span>
                <ChevronDown size={15} />
              </button>
              <button type="button" className="inline-action" onClick={hideSensitive}><EyeOff size={13} />{t("share.hideSensitive")}</button>
            </div>
            {openPanels.display ? (
              <div className="toggle-section disclosure-body">
                {(["showModel", "showCost", "showProject", "showBrand"] as const).map((key) => <label key={key}><span>{t(`share.${key}`)}</span><Toggle checked={request[key]} onCheckedChange={(value) => patch(key, value)} label={t(`share.${key}`)} /></label>)}
                {request.templateId === "vcti-card" ? <label><span><b>{t("share.showBehaviorEvidence")}</b><small>{t("share.showBehaviorEvidenceBody")}</small></span><Toggle checked={request.showBehaviorEvidence} onCheckedChange={(value) => patch("showBehaviorEvidence", value)} label={t("share.showBehaviorEvidence")} /></label> : null}
              </div>
            ) : null}
          </section>

          {metricOptions.length ? (
            <section className="control-section">
              <button type="button" className="control-disclosure" aria-expanded={openPanels.metrics} onClick={() => togglePanel("metrics")}>
                <span>{t("share.metricsTitle")}</span>
                <ChevronDown size={15} />
              </button>
              {openPanels.metrics ? (
                <div className="toggle-section disclosure-body">
                  {metricOptions.map((id) => <label key={id}><span>{t(`metrics.${id}`)}</span><Toggle checked={request.metrics.find((metric) => metric.id === id)?.visible !== false} onCheckedChange={(value) => toggleMetric(id, value)} label={t(`metrics.${id}`)} /></label>)}
                </div>
              ) : null}
            </section>
          ) : null}
          <button className="share-repository-link" type="button" onClick={() => void openUrl("https://github.com/RangeKing/vibemeter")}>
            <Github size={14} />
            <span>RangeKing/vibemeter</span>
          </button>
        </aside>
      </div>
    </div>
  );
}
