// Настройки «Сланца»: панели с капс-заголовками; разделы Темы (карточки),
// Общие (RAM/параллелизм/снапшоты), Java (сканирование и установка Adoptium), Сеть и Данные.
import { useCallback, useEffect, useRef, useState } from "react";
import { Download, RefreshCw, Check, Coffee } from "lucide-react";
import { useSettings } from "../../state/settings";
import { api } from "../../api/client";
import { t } from "../../i18n";
import ThemeCards from "../components/ThemeCards";
import type { JavaInstall, RamGuide, Settings } from "../../api/types";

/** D20: пауза перед записью в ядро — поля не пишутся на каждый keystroke. */
const SAVE_DEBOUNCE_MS = 400;

function messageOf(e: unknown): string {
  return String((e as { message?: string })?.message ?? e);
}

export default function SettingsPage({ onOnboardingReset }: { onOnboardingReset: () => void }) {
  const { settings, update } = useSettings();
  const [dataDir, setDataDir] = useState("");
  const [guide, setGuide] = useState<RamGuide | null>(null);
  const [javaList, setJavaList] = useState<JavaInstall[]>([]);
  const [installingJava, setInstallingJava] = useState<number | null>(null);
  const [javaMsg, setJavaMsg] = useState<string | null>(null);
  const [proxyInput, setProxyInput] = useState("");
  const [azureInput, setAzureInput] = useState("");
  // D20: числовые поля держим локально, в ядро уходит отложенная пачка.
  const [ramInput, setRamInput] = useState(settings?.defaultRamMb ?? 2048);
  const [parInput, setParInput] = useState(settings?.downloadParallelism ?? 16);
  const [saveError, setSaveError] = useState<string | null>(null);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingPatch = useRef<Partial<Settings>>({});

  const commit = useCallback(
    (patch: Partial<Settings>) => {
      void update(patch)
        .then(() => setSaveError(null))
        .catch((e) => setSaveError(messageOf(e)));
    },
    [update],
  );

  /** Отложенная запись: копим патч и пишем его одной пачкой через ~400 мс. */
  const commitDebounced = useCallback(
    (patch: Partial<Settings>) => {
      pendingPatch.current = { ...pendingPatch.current, ...patch };
      if (saveTimer.current !== null) clearTimeout(saveTimer.current);
      saveTimer.current = setTimeout(() => {
        saveTimer.current = null;
        const pending = pendingPatch.current;
        pendingPatch.current = {};
        commit(pending);
      }, SAVE_DEBOUNCE_MS);
    },
    [commit],
  );

  // Уход со страницы не теряет последние правки.
  useEffect(
    () => () => {
      if (saveTimer.current === null) return;
      clearTimeout(saveTimer.current);
      saveTimer.current = null;
      const pending = pendingPatch.current;
      pendingPatch.current = {};
      if (Object.keys(pending).length > 0) void update(pending).catch(() => undefined);
    },
    [update],
  );

  useEffect(() => {
    api.dataDir().then(setDataDir).catch(() => setDataDir("?"));
    api.ramGuide().then(setGuide).catch(() => setGuide(null));
    loadJava();
  }, []);

  useEffect(() => {
    if (settings) {
      setProxyInput(settings.proxyUrl ?? "");
      setAzureInput(settings.azureClientId ?? "");
      setRamInput(settings.defaultRamMb);
      setParInput(settings.downloadParallelism);
    }
  }, [settings]);

  const loadJava = () => {
    api.javaList().then(setJavaList).catch(() => setJavaList([]));
  };

  const installAdoptium = async (major: number) => {
    setInstallingJava(major);
    setJavaMsg(null);
    try {
      const installed = await api.javaInstall(major);
      setJavaMsg(t("settings.javaInstalled", { major, path: installed.javaExe }));
      loadJava();
    } catch (e) {
      setJavaMsg(t("settings.javaInstallFailed", { error: String((e as { message?: string })?.message ?? e) }));
    } finally {
      setInstallingJava(null);
    }
  };

  const saveProxy = () => {
    const val = proxyInput.trim() || undefined;
    commit({ proxyUrl: val });
  };

  const saveAzure = () => {
    const val = azureInput.trim() || undefined;
    commit({ azureClientId: val });
  };

  if (!settings) return null;

  return (
    <div className="mx-auto flex max-w-[980px] flex-col gap-6 pb-10">
      <div className="mt-1 flex items-baseline gap-2">
        <h2 className="text-lg font-semibold tracking-tight">{t("nav.settings")}</h2>
      </div>

      {/* D20: ошибка сохранения любой секции — видна рядом с заголовком */}
      {saveError && (
        <p
          role="alert"
          className="rounded-lg border border-error/40 bg-error/10 p-3 text-sm text-error"
        >
          {saveError}
        </p>
      )}

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
              value={ramInput}
              onChange={(e) => {
                const value = Number(e.target.value) || 2048;
                setRamInput(value);
                commitDebounced({ defaultRamMb: value });
              }}
              className="h-10 w-44 rounded-lg border border-border-strong bg-bg px-3 text-text"
            />
          </Field>

          <Field label={t("settings.parallelism")}>
            <input
              type="number"
              min={1}
              max={64}
              value={parInput}
              onChange={(e) => {
                const value = Number(e.target.value) || 16;
                setParInput(value);
                commitDebounced({ downloadParallelism: value });
              }}
              className="h-10 w-44 rounded-lg border border-border-strong bg-bg px-3 text-text"
            />
          </Field>

          <Field label={t("settings.snapshots")}>
            <div className="flex items-center justify-between gap-4">
              <div>
                <div className="text-sm font-medium">{t("settings.uiScale")}</div>
                <div className="text-[13px] text-text-muted">{t("settings.uiScale.hint")}</div>
              </div>
              <select
                value={String(settings.uiScale ?? 100)}
                onChange={(e) => commit({ uiScale: Number(e.target.value) })}
                aria-label={t("settings.uiScale")}
                className="h-10 rounded-lg border border-border-strong bg-card px-3 text-sm"
              >
                {[90, 100, 110, 125, 150].map((v) => (
                  <option key={v} value={v}>
                    {v}%
                  </option>
                ))}
              </select>
            </div>
            <Toggle checked={settings.showSnapshots} onChange={(v) => commit({ showSnapshots: v })} />
          </Field>

          <Field label={t("settings.oldVersions")}>
            <Toggle checked={settings.showOldVersions} onChange={(v) => commit({ showOldVersions: v })} />
          </Field>
        </div>
      </section>

      {/* Java Runtimes */}
      <section>
        <div className="mb-3 flex items-center justify-between max-w-2xl">
          <h3 className="text-[13px] font-semibold uppercase tracking-wider text-text-muted">
            {t("settings.javaSection")}
          </h3>
          <button
            onClick={loadJava}
            className="flex items-center gap-1 text-xs text-accent hover:underline"
          >
            <RefreshCw size={12} aria-hidden />
            {t("settings.refresh")}
          </button>
        </div>

        <div className="max-w-2xl rounded-lg border border-border-app bg-card p-4">
          <div className="flex flex-col gap-2">
            {javaList.length === 0 ? (
              <p className="text-sm text-text-muted">{t("settings.noJavaFound")}</p>
            ) : (
              javaList.map((j) => (
                <div
                  key={j.javaExe}
                  className="flex items-center justify-between rounded-md border border-border-app bg-bg p-3"
                >
                  <div className="flex items-center gap-2">
                    <Coffee size={16} className="text-accent" />
                    <div>
                      <div className="text-sm font-semibold text-text">
                        Java {j.major} ({j.origin}, {j.archBits}-bit)
                      </div>
                      <code className="text-xs text-text-muted">{j.javaExe}</code>
                    </div>
                  </div>
                </div>
              ))
            )}
          </div>

          <div className="mt-4 border-t border-border-app pt-3">
            <div className="text-xs font-medium text-text-muted mb-2">
              {t("settings.installAdoptiumTitle")}
            </div>
            <div className="flex flex-wrap gap-2">
              <button
                onClick={() => void installAdoptium(17)}
                disabled={installingJava !== null}
                className="btn-ghost flex h-9 items-center gap-2 px-3 text-xs"
              >
                <Download size={14} />
                {installingJava === 17 ? t("settings.installing") : "Adoptium JRE 17 (LTS)"}
              </button>
              <button
                onClick={() => void installAdoptium(21)}
                disabled={installingJava !== null}
                className="btn-ghost flex h-9 items-center gap-2 px-3 text-xs"
              >
                <Download size={14} />
                {installingJava === 21 ? t("settings.installing") : "Adoptium JRE 21 (LTS)"}
              </button>
            </div>
            {javaMsg && (
              <p className="mt-2 text-xs text-accent">{javaMsg}</p>
            )}
          </div>
        </div>
      </section>

      {/* Сеть и Прокси */}
      <section>
        <h3 className="mb-3 text-[13px] font-semibold uppercase tracking-wider text-text-muted">
          {t("settings.networkSection")}
        </h3>
        <div className="max-w-2xl rounded-lg border border-border-app bg-card p-4">
          <Field label={t("settings.proxy")}>
            <div className="flex items-center gap-2">
              <input
                type="text"
                value={proxyInput}
                onChange={(e) => setProxyInput(e.target.value)}
                placeholder="http://127.0.0.1:7890"
                className="h-10 w-64 rounded-lg border border-border-strong bg-bg px-3 text-sm text-text"
              />
              <button onClick={saveProxy} className="btn-ghost btn-sm">
                <Check size={14} />
                {t("common.save")}
              </button>
            </div>
          </Field>

          <Field label={t("settings.azureClientId")}>
            <div className="flex items-center gap-2">
              <input
                type="text"
                value={azureInput}
                onChange={(e) => setAzureInput(e.target.value)}
                placeholder="00000000-0000-0000-0000-000000000000"
                className="h-10 w-64 rounded-lg border border-border-strong bg-bg px-3 text-sm text-text font-mono text-xs"
              />
              <button onClick={saveAzure} className="btn-ghost btn-sm">
                <Check size={14} />
                {t("common.save")}
              </button>
            </div>
          </Field>
          <p className="mt-1 text-xs text-text-muted">
            {t("settings.azureClientIdHint")}
          </p>
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
