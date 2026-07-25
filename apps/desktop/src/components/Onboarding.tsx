import { BookOpenCheck, GitBranch, HardDrive, RadioTower, ScanSearch, ShieldCheck } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import appIconUrl from "../../src-tauri/icons/vibemeter-icon-source.png";
import { Toggle } from "./ui";

interface Consent {
  credentialsAllowed: boolean;
  gitReadAllowed: boolean;
  vctiPromptStructure: boolean;
}

export function Onboarding({ onFinish }: { onFinish: (value: Consent) => Promise<void> }) {
  const { t } = useTranslation();
  const [credentialsAllowed, setCredentialsAllowed] = useState(false);
  const [gitReadAllowed, setGitReadAllowed] = useState(false);
  const [vctiPromptStructure, setVctiPromptStructure] = useState(true);
  const [saving, setSaving] = useState(false);
  const finish = async () => {
    setSaving(true);
    try { await onFinish({ credentialsAllowed, gitReadAllowed, vctiPromptStructure }); } finally { setSaving(false); }
  };
  const items = [
    [HardDrive, t("onboarding.localTitle"), t("onboarding.localBody")],
    [RadioTower, t("onboarding.liveTitle"), t("onboarding.liveBody")],
    [BookOpenCheck, t("onboarding.reviewTitle"), t("onboarding.reviewBody")],
    [ShieldCheck, t("onboarding.shareTitle"), t("onboarding.shareBody")],
  ] as const;

  return (
    <div className="onboarding-window">
      <div className="titlebar-drag" data-tauri-drag-region />
      <section className="onboarding-card">
        <div className="onboarding-brand"><img src={appIconUrl} alt="" /><span>{t("app.name")}</span></div>
        <span className="eyebrow">{t("onboarding.eyebrow")}</span>
        <h1>{t("onboarding.title")}</h1>
        <p className="lead">{t("onboarding.body")}</p>
        <div className="onboarding-list">
          {items.map(([Icon, title, body]) => (
            <div className="onboarding-item" key={title}>
              <span><Icon size={18} /></span>
              <div><strong>{title}</strong><p>{body}</p></div>
            </div>
          ))}
        </div>
        <div className="consent-stack">
          <label className="consent-row">
            <span><strong>{t("onboarding.vctiStructure")}</strong><small><ScanSearch size={12} />{t("onboarding.vctiStructureBody")}</small></span>
            <Toggle checked={vctiPromptStructure} onCheckedChange={setVctiPromptStructure} label={t("onboarding.vctiStructure")} />
          </label>
          <label className="consent-row">
            <span><strong>{t("onboarding.allowGit")}</strong><small><GitBranch size={12} />{t("onboarding.gitOff")}</small></span>
            <Toggle checked={gitReadAllowed} onCheckedChange={setGitReadAllowed} label={t("onboarding.allowGit")} />
          </label>
          <label className="consent-row">
            <span><strong>{t("onboarding.allowCredentials")}</strong></span>
            <Toggle checked={credentialsAllowed} onCheckedChange={setCredentialsAllowed} label={t("onboarding.allowCredentials")} />
          </label>
        </div>
        <button className="button primary full" onClick={() => void finish()} disabled={saving}>
          {saving ? t("actions.refreshing") : t("onboarding.finish")}
        </button>
      </section>
    </div>
  );
}
