import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { BrainCircuit, Check, ChevronRight, Eye, FileText, Languages, Plus, RotateCw, Save, ShieldCheck, Trash2, X } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { TextareaHTMLAttributes } from "react";
import { useTranslation } from "react-i18next";
import { AgentBadge, EmptyState, ErrorState, LoadingState, PageHeader } from "../components/ui";
import { api } from "../lib/api";
import { formatDateTime } from "../lib/format";
import type { DeepReviewPreview, Locale, ReviewContent } from "../types";

function localDate(offsetDays = 0): string {
  const date = new Date();
  date.setDate(date.getDate() + offsetDays);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

function monday(): string {
  const date = new Date();
  const day = date.getDay() || 7;
  date.setDate(date.getDate() - day + 1);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

const primarySections: Array<keyof ReviewContent> = ["outcome", "whatWorked", "friction", "nextRun"];
const evidenceSections: Array<keyof ReviewContent> = ["whatHappened", "lessons"];

function AutoResizeTextarea({ value, ...props }: TextareaHTMLAttributes<HTMLTextAreaElement> & { value: string }) {
  const ref = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const resize = () => {
      element.style.height = "auto";
      element.style.height = `${element.scrollHeight + 2}px`;
    };
    let width = element.clientWidth;
    resize();
    const observer = new ResizeObserver(() => {
      const nextWidth = element.clientWidth;
      if (Math.abs(nextWidth - width) < 1) return;
      width = nextWidth;
      resize();
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [value]);
  return <textarea {...props} ref={ref} value={value} />;
}

export function ReviewsPage({ locale }: { locale: Locale }) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const reviews = useQuery({ queryKey: ["reviews"], queryFn: () => api.reviews() });
  const tasks = useQuery({ queryKey: ["review-tasks"], queryFn: () => api.tasks("30d") });
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const [selectedId, setSelectedId] = useState<string>();
  const [reviewType, setReviewType] = useState<"task" | "daily" | "weekly">("task");
  const [reviewLocale, setReviewLocale] = useState<Locale>(locale);
  const [taskId, setTaskId] = useState("");
  const [dateTarget, setDateTarget] = useState(localDate());
  const [weekTarget, setWeekTarget] = useState(monday());
  const [title, setTitle] = useState("");
  const [content, setContent] = useState<ReviewContent>({ outcome: "", whatHappened: "", whatWorked: "", friction: "", lessons: "", nextRun: "" });
  const [notice, setNotice] = useState("");
  const [deepPreview, setDeepPreview] = useState<DeepReviewPreview>();
  const [composerOpen, setComposerOpen] = useState(false);
  const [showEvidence, setShowEvidence] = useState(false);

  const selected = useMemo(() => reviews.data?.items.find((item) => item.id === selectedId) ?? reviews.data?.items[0], [reviews.data?.items, selectedId]);
  useEffect(() => {
    if (!selected) return;
    setSelectedId(selected.id);
    setTitle(selected.title);
    setContent(selected.content);
  }, [selected?.id]);
  useEffect(() => { if (!taskId && tasks.data?.[0]) setTaskId(tasks.data[0].id); }, [taskId, tasks.data]);

  const generate = useMutation({
    mutationFn: ({ type, targetId, language }: { type: "task" | "session" | "daily" | "weekly"; targetId: string; language: Locale }) => api.generateReview(type, targetId, language),
    onSuccess: async (item) => {
      await client.invalidateQueries({ queryKey: ["reviews"] });
      setSelectedId(item.id);
      setComposerOpen(false);
      setNotice(item.version > 1 ? t("reviews.generatedDraft") : t("reviews.saved"));
    },
  });
  const generateComposer = () => generate.mutate({ type: reviewType, targetId: reviewType === "task" ? taskId : reviewType === "daily" ? dateTarget : weekTarget, language: reviewLocale });
  const previewDeep = useMutation({
    mutationFn: () => api.previewDeepReview(taskId, reviewLocale, settings.data!.deepReviewMode, settings.data!.deepReviewProvider, settings.data!.deepReviewModel),
    onSuccess: setDeepPreview,
  });
  const generateDeep = useMutation({
    mutationFn: (preview: DeepReviewPreview) => api.generateDeepReview({ taskId: preview.taskId, locale: reviewLocale, mode: preview.mode, provider: preview.provider, model: preview.model, payloadHash: preview.payloadHash }),
    onSuccess: async (item) => {
      setDeepPreview(undefined);
      await client.invalidateQueries({ queryKey: ["reviews"] });
      setSelectedId(item.id);
      setNotice(item.version > 1 ? t("reviews.generatedDraft") : t("reviews.saved"));
    },
  });
  const save = useMutation({
    mutationFn: () => selected ? api.updateReview(selected.id, title, content) : Promise.resolve(),
    onSuccess: async () => { await client.invalidateQueries({ queryKey: ["reviews"] }); setNotice(t("reviews.saved")); },
  });
  const accept = useMutation({
    mutationFn: (id: string) => api.acceptReview(id),
    onSuccess: async () => { await client.invalidateQueries({ queryKey: ["reviews"] }); setNotice(t("reviews.saved")); },
  });
  const remove = useMutation({
    mutationFn: (id: string) => api.deleteReview(id),
    onSuccess: async (_void, id) => {
      const remaining = reviews.data?.items.filter((item) => item.id !== id) ?? [];
      await client.invalidateQueries({ queryKey: ["reviews"] });
      setSelectedId(remaining[0]?.id);
      setNotice(t("reviews.deleted"));
    },
  });

  if (reviews.isLoading || tasks.isLoading || settings.isLoading) return <LoadingState />;
  if (reviews.isError || tasks.isError || settings.isError || !settings.data) return <ErrorState retry={() => void Promise.all([reviews.refetch(), tasks.refetch(), settings.refetch()])} />;

  const visibleEvidence = evidenceSections.filter((key) => content[key].trim().length > 0);

  return (
    <div className="page reviews-page">
      <PageHeader
        title={t("reviews.title")}
        description={t("reviews.description")}
        actions={<button className="button secondary" onClick={() => setComposerOpen((open) => !open)}>{composerOpen ? <X size={14} /> : <Plus size={14} />}{composerOpen ? t("reviews.hideComposer") : t("reviews.showComposer")}</button>}
      />
      {composerOpen ? <section className="review-composer">
        <header><Plus size={16} /><strong>{t("reviews.newReview")}</strong></header>
        <div className="review-compose-fields">
          <label><span>{t("reviews.reviewType")}</span><select value={reviewType} onChange={(event) => setReviewType(event.target.value as typeof reviewType)}><option value="task">{t("reviews.task")}</option><option value="daily">{t("reviews.daily")}</option><option value="weekly">{t("reviews.weekly")}</option></select></label>
          <label className="target-field"><span>{t("reviews.target")}</span>{reviewType === "task" ? <select value={taskId} onChange={(event) => setTaskId(event.target.value)}><option value="">{t("reviews.firstTask")}</option>{tasks.data?.map((task) => <option key={task.id} value={task.id}>{task.title || task.projectLabel} · {task.sessionCount} {t("metrics.sessions")}</option>)}</select> : <input type="date" value={reviewType === "daily" ? dateTarget : weekTarget} onChange={(event) => reviewType === "daily" ? setDateTarget(event.target.value) : setWeekTarget(event.target.value)} />}</label>
          <label><span>{t("reviews.language")}</span><div className="segmented compact"><button className={reviewLocale === "zh-CN" ? "active" : ""} onClick={() => setReviewLocale("zh-CN")}>中文</button><button className={reviewLocale === "en-US" ? "active" : ""} onClick={() => setReviewLocale("en-US")}>English</button></div></label>
          <span className="review-generate-actions"><button className="button secondary" onClick={() => previewDeep.mutate()} disabled={previewDeep.isPending || reviewType !== "task" || !taskId}>{previewDeep.isPending ? <RotateCw className="spin" size={15} /> : <BrainCircuit size={15} />}{t("deepReview.action")}</button><button className="button primary" onClick={generateComposer} disabled={generate.isPending || (reviewType === "task" && !taskId)}>{generate.isPending ? <RotateCw className="spin" size={15} /> : <FileText size={15} />}{t("actions.generate")}</button></span>
        </div>
      </section> : null}

      <div className="reviews-workspace">
        <aside className="version-rail">
          <header><strong>{t("reviews.versions")}</strong><span>{reviews.data?.items.length ?? 0}</span></header>
          {reviews.data?.items.map((review) => (
            <div key={review.id} className={`version-row ${selected?.id === review.id ? "active" : ""}`}>
              <button className="version-select" onClick={() => setSelectedId(review.id)}>
                <span className={`version-language ${review.locale === "zh-CN" ? "zh" : "en"}`}>{review.locale === "zh-CN" ? "中" : "EN"}</span>
                <span><strong>{review.title}</strong><small>{t(`reviews.${review.status}`)} · {t("reviews.version", { version: review.version })}{review.sourceExcluded ? ` · ${t("task.sourceExcluded")}` : ""}</small></span>
                <ChevronRight size={14} />
              </button>
              <button
                className="version-delete"
                aria-label={t("actions.delete")}
                onClick={() => {
                  if (window.confirm(t("reviews.confirmDelete"))) remove.mutate(review.id);
                }}
              >
                <Trash2 size={13} />
              </button>
            </div>
          ))}
          {!reviews.data?.items.length ? <EmptyState title={t("reviews.noReviewsTitle")} body={t("reviews.noReviewsBody")} /> : null}
        </aside>

        {selected ? <article className="review-editor">
          <header className="review-editor-header">
            <div><span className="eyebrow"><Languages size={13} />{selected.locale === "zh-CN" ? "中文" : "English"} · {t("reviews.version", { version: selected.version })}</span><input className="review-title-input" value={title} onChange={(event) => setTitle(event.target.value)} /></div>
            <div className="review-actions">
              {notice ? <span className="save-notice"><Check size={13} />{notice}</span> : null}
              <button className="button secondary" onClick={() => generate.mutate({ type: selected.reviewType, targetId: selected.targetId, language: selected.locale })} disabled={generate.isPending}><RotateCw size={14} />{t("actions.regenerate")}</button>
              {selected.status !== "current" ? <button className="button secondary" onClick={() => { if (window.confirm(t("reviews.confirmAccept"))) accept.mutate(selected.id); }}><ShieldCheck size={14} />{t("actions.accept")}</button> : null}
              <button className="button primary" onClick={() => save.mutate()} disabled={save.isPending}><Save size={14} />{t("actions.save")}</button>
              <button className="button danger-button" onClick={() => { if (window.confirm(t("reviews.confirmDelete"))) remove.mutate(selected.id); }} disabled={remove.isPending}><Trash2 size={14} />{t("actions.delete")}</button>
            </div>
          </header>

          <div className="review-primary-flow review-reuse-flow">
            {primarySections.map((key, index) => (
              <label key={key} className={`review-${key}`}>
                <span><i>{String(index + 1).padStart(2, "0")}</i>{t(`reviews.${key}`)}</span>
                <AutoResizeTextarea rows={key === "outcome" ? 3 : 4} value={content[key]} onChange={(event) => setContent((current) => ({ ...current, [key]: event.target.value }))} />
              </label>
            ))}
          </div>

          <details className="review-evidence-details" open={showEvidence} onToggle={(event) => setShowEvidence(event.currentTarget.open)}>
            <summary><span>{t("reviews.moreContext")}</span><span>{selected.findings.length + visibleEvidence.length}</span></summary>
            <div className="review-evidence-body">
              {visibleEvidence.length ? <div className="review-content-grid">
                {visibleEvidence.map((key) => (
                  <label key={key}>
                    <span>{t(`reviews.${key}`)}</span>
                    <AutoResizeTextarea rows={4} value={content[key]} onChange={(event) => setContent((current) => ({ ...current, [key]: event.target.value }))} />
                  </label>
                ))}
              </div> : null}
              <section className="finding-section">
                <header><h3>{t("reviews.findings")}</h3><span>{selected.findings.length}</span></header>
                {selected.findings.map((finding) => <article className={`finding ${finding.tier}`} key={finding.id}><span className="finding-tier">{t(`insights.${finding.tier in { fact: 1, inference: 1, suggestion: 1 } ? finding.tier : "inference"}`)}</span><div><h4>{finding.title}</h4><p>{finding.detail}</p><div className="evidence-chips">{finding.evidence.map((evidence) => <span key={`${evidence.kind}-${evidence.id}`}>{evidence.kind} · {evidence.label}</span>)}</div></div></article>)}
                {!selected.findings.length && !visibleEvidence.length ? <div className="quiet-empty">{t("reviews.noEvidence")}</div> : null}
              </section>
            </div>
          </details>
          <footer className="review-footer"><AgentBadge agent="vibemeter" compact /><span>{formatDateTime(selected.updatedAt, locale)} · {selected.userEdited ? t("actions.edit") : t("actions.generate")}</span></footer>
        </article> : null}
      </div>
      {deepPreview ? <div className="modal-backdrop" role="presentation"><section className="deep-review-modal" role="dialog" aria-modal="true" aria-labelledby="deep-review-title"><header><div><span className="eyebrow"><Eye size={13} />{t("deepReview.previewEyebrow")}</span><h2 id="deep-review-title">{t("deepReview.previewTitle")}</h2></div><button className="icon-button" onClick={() => setDeepPreview(undefined)} aria-label={t("actions.close")}><X size={17} /></button></header><div className="deep-review-route"><span>{deepPreview.mode === "cli" ? t("deepReview.cli") : t("deepReview.api")}</span><strong>{deepPreview.provider}{deepPreview.model ? ` · ${deepPreview.model}` : ""}</strong><small>{t("deepReview.characterCount", { count: deepPreview.characterCount })}</small></div><ul>{deepPreview.privacyNotes.map((key) => <li key={key}>{t(key)}</li>)}</ul><pre>{deepPreview.payload}</pre><footer><button className="button secondary" onClick={() => setDeepPreview(undefined)}>{t("actions.cancel")}</button><button className="button primary" onClick={() => generateDeep.mutate(deepPreview)} disabled={generateDeep.isPending}>{generateDeep.isPending ? <RotateCw className="spin" size={15} /> : <BrainCircuit size={15} />}{t("deepReview.confirm")}</button></footer>{generateDeep.isError ? <p className="inline-error">{String(generateDeep.error)}</p> : null}</section></div> : null}
    </div>
  );
}
