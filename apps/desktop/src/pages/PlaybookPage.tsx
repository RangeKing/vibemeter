import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { BookMarked, Check, Clipboard, FileDiff, Pencil, Plus, Search, Trash2, X } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { EmptyState, ErrorState, LoadingState, PageHeader, Toggle } from "../components/ui";
import { api } from "../lib/api";
import type { PlaybookItem, SavePlaybookRequest } from "../types";

const blank: SavePlaybookRequest = { title: "", body: "", category: "workflow", applied: false };

export function PlaybookPage() {
  const { t } = useTranslation();
  const client = useQueryClient();
  const [search, setSearch] = useState("");
  const [draft, setDraft] = useState<SavePlaybookRequest>();
  const [patchOpen, setPatchOpen] = useState(false);
  const [notice, setNotice] = useState("");
  const query = useQuery({ queryKey: ["playbook", search], queryFn: () => api.playbook(search || undefined) });
  const save = useMutation({ mutationFn: (request: SavePlaybookRequest) => api.savePlaybook(request), onSuccess: async () => { setDraft(undefined); await client.invalidateQueries({ queryKey: ["playbook"] }); } });
  const remove = useMutation({ mutationFn: (id: string) => api.deletePlaybook(id), onSuccess: async () => { await client.invalidateQueries({ queryKey: ["playbook"] }); } });
  const edit = (item: PlaybookItem) => setDraft({ id: item.id, title: item.title, body: item.body, category: item.category, projectLabel: item.projectLabel, taskType: item.taskType, sourceReviewId: item.sourceReviewId, sourceFindingId: item.sourceFindingId, applied: item.applied });
  const patchText = query.data?.length ? [
    "*** Begin Patch",
    "*** Update File: AGENTS.md",
    "@@",
    "+",
    `+## ${t("playbook.patchSection")}`,
    ...query.data.flatMap((item) => [`+`, `+- **${item.title.replaceAll("\n", " ")}**: ${item.body.replaceAll("\n", " ")}`]),
    "*** End Patch",
  ].join("\n") : "";
  const copy = async (value: string) => { await writeText(value); setNotice(t("actions.copied")); };
  const exportPatch = async () => {
    const path = await saveDialog({ defaultPath: "aftervibe_AGENTS_suggestions.patch", filters: [{ name: "Patch", extensions: ["patch"] }] });
    if (!path) return;
    await api.exportTextFile(path, patchText);
    setNotice(t("share.exported", { format: "PATCH" }));
  };

  return (
    <div className="page playbook-page">
      <PageHeader title={t("playbook.title")} description={t("playbook.description")} actions={<div className="page-button-group"><button className="button secondary" onClick={() => setPatchOpen(true)} disabled={!query.data?.length}><FileDiff size={14} />{t("playbook.agentsPatch")}</button><button className="button primary" onClick={() => setDraft({ ...blank })}><Plus size={15} />{t("playbook.newItem")}</button></div>} />
      <div className="playbook-toolbar"><label className="search-field"><Search size={16} /><input value={search} onChange={(event) => setSearch(event.target.value)} placeholder={t("playbook.search")} />{search ? <button onClick={() => setSearch("")}><X size={14} /></button> : null}</label></div>
      {query.isLoading ? <LoadingState /> : query.isError || !query.data ? <ErrorState retry={() => void query.refetch()} /> : query.data.length ? <div className="playbook-grid">{query.data.map((item, index) => (
        <article className={`playbook-card ${item.applied ? "applied" : ""}`} key={item.id}>
          <header><span>{String(index + 1).padStart(2, "0")}</span><span className="playbook-category">{t(`playbook.categories.${item.category in { workflow: 1, verification: 1, prompting: 1, tooling: 1 } ? item.category : "workflow"}`)}</span>{item.applied ? <Check size={15} /> : null}</header>
          <BookMarked size={20} />
          <h2>{item.title}</h2><p>{item.body}</p>
          <div className="playbook-tags">{item.projectLabel ? <span>{item.projectLabel}</span> : null}{item.taskType ? <span>{item.taskType}</span> : null}{item.sourceExcluded ? <span className="warning">{t("playbook.sourceExcluded")}</span> : null}</div>
          <footer><button onClick={() => void copy(`${item.title}\n\n${item.body}`)}><Clipboard size={13} />{t("actions.copy")}</button><button onClick={() => edit(item)}><Pencil size={13} />{t("actions.edit")}</button><button className="danger" onClick={() => remove.mutate(item.id)}><Trash2 size={13} />{t("actions.delete")}</button></footer>
        </article>
      ))}</div> : <EmptyState title={t("playbook.emptyTitle")} body={t("playbook.emptyBody")} />}

      {draft ? <div className="editor-scrim" onMouseDown={(event) => { if (event.target === event.currentTarget) setDraft(undefined); }}><section className="playbook-editor">
        <header><div><span className="eyebrow">{draft.id ? t("actions.edit") : t("playbook.newItem")}</span><h2>{t("playbook.title")}</h2></div><button className="icon-button" onClick={() => setDraft(undefined)}><X size={17} /></button></header>
        <label><span>{t("playbook.itemTitle")}</span><input autoFocus value={draft.title} onChange={(event) => setDraft({ ...draft, title: event.target.value })} /></label>
        <label><span>{t("playbook.body")}</span><textarea rows={7} value={draft.body} onChange={(event) => setDraft({ ...draft, body: event.target.value })} /></label>
        <div className="form-two"><label><span>{t("playbook.category")}</span><select value={draft.category} onChange={(event) => setDraft({ ...draft, category: event.target.value })}><option value="workflow">{t("playbook.categories.workflow")}</option><option value="verification">{t("playbook.categories.verification")}</option><option value="prompting">{t("playbook.categories.prompting")}</option><option value="tooling">{t("playbook.categories.tooling")}</option></select></label><label><span>{t("playbook.project")}</span><input value={draft.projectLabel ?? ""} onChange={(event) => setDraft({ ...draft, projectLabel: event.target.value || undefined })} /></label></div>
        <label className="toggle-line"><span>{t("playbook.applied")}</span><Toggle checked={draft.applied} onCheckedChange={(applied) => setDraft({ ...draft, applied })} label={t("playbook.applied")} /></label>
        <footer><button className="button secondary" onClick={() => setDraft(undefined)}>{t("actions.cancel")}</button><button className="button primary" onClick={() => save.mutate(draft)} disabled={!draft.title.trim() || !draft.body.trim() || save.isPending}>{t("actions.save")}</button></footer>
      </section></div> : null}
      {patchOpen ? <div className="editor-scrim" onMouseDown={(event) => { if (event.target === event.currentTarget) setPatchOpen(false); }}><section className="patch-preview">
        <header><div><span className="eyebrow">{t("playbook.agentsPatch")}</span><h2>{t("playbook.patchTitle")}</h2><p>{t("playbook.patchBody")}</p></div><button className="icon-button" onClick={() => setPatchOpen(false)}><X size={17} /></button></header>
        {patchText ? <pre>{patchText}</pre> : <div className="quiet-empty">{t("playbook.patchEmpty")}</div>}
        <footer>{notice ? <span>{notice}</span> : <span />}{patchText ? <div><button className="button secondary" onClick={() => void copy(patchText)}><Clipboard size={14} />{t("actions.copy")}</button><button className="button primary" onClick={() => void exportPatch()}><FileDiff size={14} />{t("playbook.exportPatch")}</button></div> : null}</footer>
      </section></div> : null}
    </div>
  );
}
