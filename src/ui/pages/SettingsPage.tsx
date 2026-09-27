// Настройки «Сланца»: панели с капс-заголовками; разделы Темы (карточки),
// Общие (RAM/параллелизм/снапшоты), Данные (каталог, сброс онбординга).
import { useEffect, useState } from "react";
import { useSettings } from "../../state/settings";
import { api } from "../../api/client";
import { t } from "../../i18n";
import ThemeCards from "../components/ThemeCards";

export default function SettingsPage({ onOnboardingReset }: { onOnboardingReset: () => void }) {
  const { settings, update } = useSettings();
  const [dataDir, setDataDir] = useState("");
  const [guide, setGuide] = useState<import("../../api/types").RamGuide | null>(null);

  useEffect(() => {
    api.dataDir().then(setDataDir).catch(() => setDataDir("?"));
    api.ramGuide().then(setGuide).catch(() => setGuide(null));
  }, []);

  if (!settings) return null;

  return (
    <div className="mx-auto flex max-w-[980px] flex-col gap-6">
      <div className="mt-1 flex items-baseline gap-2">
        <h2 className="text-lg font-semibold tracking-tight">{t("nav.settings")}</h2>
      </div>

      {/* Темы: карточки как в CurseForge, применяются мгновенно */}
      <section>
        <h3 className="mb-3 text-[13px] font-semibold uppercase tracking-wider text-text-muted">
          {t("settings.themesSection")}
        </h3>
        <p className="mb-3 max-w-2xl text-sm text-text-muted">{t("themes.hint")}</p>
        <ThemeCards />
      </section>

      {/* Общие: реальные настройки из ядра */}
      <section>
        <h3 className="mb-3 text-[13px] font-semibold uppercase tracking-wider text-text-muted">
          {t("settings.generalSection")}
        </h3>
        <div className="max-w-2xl rounded-lg border border-border-app bg-card p-4">
          {guide && (
            <p className="mb-3 rounded-md border border-border-app bg-surface-2 px-3 py-2 text-xs text-text-muted">
              {t("settings.ramHint", {
                recommended: guide.recommendedMb,
                total: Math.round(guide.totalMb / 1024),
              })}
              {guide.warning ? ` ${guide.warning}` : ""}
            </p>
          )}

          <Field label={t("settings.ram")}>
            <input
              type="number"
              min={1024}
              max={32768}
              step={512}
              value={settings.defaultRamMb}
              onChange={(e) => void update({ defaultRamMb: Number(e.target.value) || 2048 })}
              className="h-10 w-44 rounded-lg border border-border-strong bg-bg px-3"
            />
          </Field>

          <Field label={t("settings.parallelism")}>
            <input
              type="number"
              min={1}
              max={64}
              value={settings.downloadParallelism}
              onChange={(e) => void update({ downloadParallelism: Number(e.target.value) || 16 })}
              className="h-10 w-44 rounded-lg border border-border-strong bg-bg px-3"
            />
          </Field>

          <Field label={t("settings.snapshots")}>
            <Toggle checked={settings.showSnapshots} onChange={(v) => void update({ showSnapshots: v })} />
          </Field>

          <Field label={t("settings.oldVersions")}>
            <Toggle checked={settings.showOldVersions} onChange={(v) => void update({ showOldVersions: v })} />
          </Field>
        </div>
      </section>

      {/* Данные */}
      <section>
        <h3 className="mb-3 text-[13px] font-semibold uppercase tracking-wider text-text-muted">
          {t("settings.dataDir")}
        </h3>
        <div className="max-w-2xl rounded-lg border border-border-app bg-card p-4">
          <Field label={t("settings.dataDir")}>
            <div className="flex items-center gap-2">
              <code className="max-w-72 truncate rounded-md border border-border-app bg-bg px-2 py-1.5 font-mono text-xs text-text-muted">
                {dataDir}
              </code>
              <button onClick={() => void api.dirOpen()} className="btn-ghost btn-sm">
                {t("settings.open")}
              </button>
            </div>
          </Field>

          <Field label={t("settings.resetOnboarding")}>
            <button onClick={onOnboardingReset} className="btn-ghost btn-sm">
              {t("settings.resetOnboarding")}
            </button>
          </Field>
        </div>
      </section>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex min-h-12 items-center justify-between gap-4 py-2">
      <span className="text-sm">{label}</span>
      {children}
    </div>
  );
}

function Toggle({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className={`h-6 w-11 rounded-full transition-colors ${checked ? "bg-accent" : "bg-border-strong"}`}
    >
      <span
        className={`block h-5 w-5 rounded-full transition-transform ${
          checked ? "translate-x-5 bg-on-accent" : "translate-x-0.5 bg-text-muted"
        }`}
      />
    </button>
  );
}
