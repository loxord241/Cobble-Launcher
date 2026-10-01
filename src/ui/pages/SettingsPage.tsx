// Настройки «Сланца»: панели с капс-заголовками; разделы Темы (карточки),
// Общие (RAM/параллелизм/снапшоты), Java (сканирование и установка Adoptium), Сеть и Данные.
import { useCallback, useEffect, useRef, useState } from "react";
import { Download, RefreshCw, Check, Coffee, Trash2, Upload } from "lucide-react";
import { isHexColor, useSettings } from "../../state/settings";
import { api } from "../../api/client";
import { apiErrorText, t } from "../../i18n";
import { pickJsonPath, pickPngPath, pickSavePath } from "../components/pickFile";
import ThemeCards from "../components/ThemeCards";
import type { JavaInstall, RamGuide, Settings, StorageStats } from "../../api/types";
import UpdatesSection from "../components/UpdatesSection";

/** D20: пауза перед записью в ядро — поля не пишутся на каждый keystroke. */
const SAVE_DEBOUNCE_MS = 400;

/** Человеческий размер для статистики диска (F18): МБ с округлением, ГБ с десятыми. */
function fmtBytes(bytes: number): string {
  const mb = bytes / (1024 * 1024);
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} ГБ`;
  if (mb >= 1) return `${Math.round(mb)} МБ`;
  return `${Math.round(bytes / 1024)} КБ`;
}

/** Пресеты акцентного цвета (первый — сланцевый синий по умолчанию). */
const ACCENT_PRESETS = ["#7FB2F0", "#6FBF8F", "#E8A25C", "#E06C6C", "#A78BE0"];

/** Акцент текущей палитры (tokens.css) — что показать в пикере, пока кастом не задан. */
function themeAccent(): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim();
  return isHexColor(value) ? value : ACCENT_PRESETS[0];
}

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
  // F18: статистика диска + сообщение (результат уборки или ошибка).
  const [stats, setStats] = useState<StorageStats | null>(null);
  const [storageMsg, setStorageMsg] = useState<string | null>(null);
  const [storageBusy, setStorageBusy] = useState(false);
  // F20: сообщение экспорта/импорта настроек (результат или ошибка).
  const [settingsIoMsg, setSettingsIoMsg] = useState<string | null>(null);
  const [settingsIoBusy, setSettingsIoBusy] = useState(false);

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

  /** F18: подсчитать размеры областей каталога данных. */
  const scanStorage = async () => {
    setStorageBusy(true);
    setStorageMsg(null);
    try {
      setStats(await api.storageStats());
    } catch (e) {
      setStorageMsg(apiErrorText(e));
    } finally {
      setStorageBusy(false);
    }
  };

  /** F18: очистить кэш и корзину, затем авто-пересчёт статистики. */
  const cleanStorage = async () => {
    setStorageBusy(true);
    setStorageMsg(null);
    try {
      const res = await api.storageClean();
      setStorageMsg(t("settings.storage.cleaned", { freed: fmtBytes(res.freedBytes) }));
      setStats(await api.storageStats());
    } catch (e) {
      setStorageMsg(apiErrorText(e));
    } finally {
      setStorageBusy(false);
    }
  };

  /** F20: экспорт настроек в выбранный .json (секреты ядро не вывозит). */
  const exportSettings = async () => {
    setSettingsIoBusy(true);
    setSettingsIoMsg(null);
    try {
      const path = await pickSavePath("mc-launcher-settings.json");
      if (!path) return;
      await api.settingsExport(path);
      setSettingsIoMsg(t("settings.export.done", { path }));
    } catch (e) {
      setSettingsIoMsg(apiErrorText(e));
    } finally {
      setSettingsIoBusy(false);
    }
  };

  /** F20: импорт настроек из .json; стейт перечитываем из ядра целиком. */
  const importSettings = async () => {
    setSettingsIoBusy(true);
    setSettingsIoMsg(null);
    try {
      const path = await pickJsonPath();
      if (!path) return;
      await api.settingsImport(path);
      // Как после коммита: ядро — источник истины, load применит и сайд-эффекты
      // (тема/шрифт/акцент/язык/масштаб) на свежих настройках.
      await useSettings.getState().load();
      setSettingsIoMsg(t("settings.import.done"));
    } catch (e) {
      setSettingsIoMsg(apiErrorText(e));
    } finally {
      setSettingsIoBusy(false);
    }
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

      {/* Темы: карточки как в CurseForge, применяются мгновенно.
          Каскад секций (anim-fade-up): задержка = номер секции * 40 мс,
          кэп 200 мс (Math.min(idx, 5)) — хвост не тормозит приёмку. */}
      <section className="anim-fade-up" style={{ animationDelay: "0ms" }}>
        <h3 className="mb-3 text-[13px] font-semibold uppercase tracking-wider text-text-muted">
          {t("settings.themesSection")}
        </h3>
        <p className="mb-3 text-sm text-text-muted">{t("themes.hint")}</p>
        <ThemeCards />
      </section>

      {/* Кастомизация: шрифт и акцент поверх палитры выбранной темы */}
      <section className="anim-fade-up" style={{ animationDelay: "40ms" }}>
        <h3 className="mb-3 text-[13px] font-semibold uppercase tracking-wider text-text-muted">
          {t("settings.customizationSection")}
        </h3>
        <div className="rounded-lg border border-border-app bg-card p-4">
          <Field label={t("settings.font")}>
            <div className="flex flex-col items-end gap-1">
              <select
                value={settings.uiFont ?? ""}
                onChange={(e) => commit({ uiFont: e.target.value })}
                aria-label={t("settings.font")}
                className="h-10 rounded-lg border border-border-strong bg-card px-3 text-sm"
              >
                {/* "" — кастом не задан, шрифт из темы («Сланец» — имя системы) */}
                <option value="">{t("settings.accent.reset")}</option>
                <option value="system">{t("settings.font.system")}</option>
                <option value="serif">{t("settings.font.serif")}</option>
                <option value="mono">{t("settings.font.mono")}</option>
                <option value="round">{t("settings.font.round")}</option>
              </select>
              <span className="text-[13px] text-text-muted">{t("settings.font.hint")}</span>
            </div>
          </Field>

          <Field label={t("settings.accent")}>
            <div className="flex flex-col items-end gap-1">
              <div className="flex items-center gap-2">
                {ACCENT_PRESETS.map((hex) => {
                  const active = (settings.uiAccent ?? "").toLowerCase() === hex.toLowerCase();
                  return (
                    <button
                      key={hex}
                      type="button"
                      onClick={() => commit({ uiAccent: hex })}
                      aria-label={hex}
                      aria-pressed={active}
                      title={hex}
                      className={`size-6 rounded-full border ${active ? "border-text" : "border-border-strong"}`}
                      style={{ background: hex }}
                    />
                  );
                })}
                {/* Свой цвет: drag в пикере — через debounce, чтобы не писать ядро на каждый пиксель */}
                <input
                  type="color"
                  value={isHexColor(settings.uiAccent) ? settings.uiAccent : themeAccent()}
                  onChange={(e) => commitDebounced({ uiAccent: e.target.value })}
                  aria-label={t("settings.accent.custom")}
                  title={t("settings.accent.custom")}
                  className="h-7 w-9 cursor-pointer rounded-md border border-border-strong bg-card"
                />
                <button onClick={() => commit({ uiAccent: "" })} className="btn-ghost btn-sm">
                  {t("settings.accent.reset")}
                </button>
              </div>
              <span className="text-[13px] text-text-muted">{t("settings.accent.hint")}</span>
            </div>
          </Field>
        </div>
      </section>

      {/* Общие: реальные настройки из ядра */}
      <section className="anim-fade-up" style={{ animationDelay: "80ms" }}>
        <h3 className="mb-3 text-[13px] font-semibold uppercase tracking-wider text-text-muted">
          {t("settings.generalSection")}
        </h3>
        <div className="rounded-lg border border-border-app bg-card p-4">
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
                <div className="text-sm font-medium">{t("settings.language")}</div>
              </div>
              <select
                value={settings.language}
                onChange={(e) => commit({ language: e.target.value })}
                aria-label={t("settings.language")}
                className="h-10 rounded-lg border border-border-strong bg-card px-3 text-sm"
              >
                <option value="en">English</option>
                <option value="ru">Русский</option>
                <option value="uk">Українська</option>
              </select>
            </div>
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
            <Toggle
              checked={settings.showSnapshots}
              onChange={(v) => commit({ showSnapshots: v })}
              label={t("settings.snapshots")}
            />
          </Field>

          <Field label={t("settings.oldVersions")}>
            <Toggle
              checked={settings.showOldVersions}
              onChange={(v) => commit({ showOldVersions: v })}
              label={t("settings.oldVersions")}
            />
          </Field>
        </div>
      </section>

      {/* Java Runtimes */}
      <section className="anim-fade-up" style={{ animationDelay: "120ms" }}>
        <div className="mb-3 flex items-center justify-between">
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

        <div className="rounded-lg border border-border-app bg-card p-4">
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
      <section className="anim-fade-up" style={{ animationDelay: "160ms" }}>
        <h3 className="mb-3 text-[13px] font-semibold uppercase tracking-wider text-text-muted">
          {t("settings.networkSection")}
        </h3>
        <div className="rounded-lg border border-border-app bg-card p-4">
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

          <Field label={t("settings.speedLimit")}>
            <select
              value={settings.speedLimitKbps}
              onChange={(e) => commit({ speedLimitKbps: Number(e.target.value) })}
              aria-label={t("settings.speedLimit")}
              className="h-10 rounded-lg border border-border-strong bg-card px-3 text-sm"
            >
              {/* 0 — без ограничения; остальные пресеты — КБ/с, подпись в МБ/с */}
              <option value={0}>{t("settings.speedLimit.none")}</option>
              {[1024, 4096, 10240, 30720].map((kbps) => (
                <option key={kbps} value={kbps}>
                  {t("settings.speedLimit.mb", { mb: kbps / 1024 })}
                </option>
              ))}
            </select>
          </Field>
          {/* F26: Discord Rich Presence (нулевые инъекции — только pipe). */}
          <label className="mt-3 flex cursor-pointer items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={settings.discordRpc}
              onChange={(e) => commit({ discordRpc: e.target.checked })}
              className="accent-accent"
              aria-label={t("settings.discordRpc")}
            />
            <span>
              {t("settings.discordRpc")}
              <span className="ml-2 text-xs text-text-muted">{t("settings.discordRpc.hint")}</span>
            </span>
          </label>
        </div>
      </section>

      {/* Данные */}
      <section className="anim-fade-up" style={{ animationDelay: "200ms" }}>
        <h3 className="mb-3 text-[13px] font-semibold uppercase tracking-wider text-text-muted">
          {t("settings.dataDir")}
        </h3>
        <div className="rounded-lg border border-border-app bg-card p-4">
          <Field label={t("settings.dataDir")}>
            <div className="flex items-center gap-2">
              <code
                title={dataDir}
                className="h-10 min-w-0 flex-1 truncate rounded-md border border-border-app bg-bg px-2 font-mono text-xs leading-10 text-text-muted"
              >
                {dataDir}
              </code>
              <button onClick={() => void api.dirOpen()} className="btn-ghost btn-sm">
                {t("settings.open")}
              </button>
            </div>
          </Field>

          <Field label={t("settings.resetOnboarding.desc")}>
            <button onClick={onOnboardingReset} className="btn-ghost btn-sm">
              {t("settings.resetOnboarding")}
            </button>
          </Field>
        </div>
      </section>

      {/* D38: выбор логотипа лаунчера (иконка окна + бренд-чип). */}
      <section className="anim-fade-up" style={{ animationDelay: "200ms" }}>
        <h3 className="mb-3 text-[13px] font-semibold uppercase tracking-wider text-text-muted">
          {t("settings.logo")}
        </h3>
        <div className="rounded-lg border border-border-app bg-card p-4">
          <div className="grid grid-cols-[repeat(auto-fill,minmax(96px,1fr))] gap-2">
            {(
              [
                ["grass", "settings.logo.grass", true],
                ["copper", "settings.logo.copper", false],
                ["chest", "settings.logo.chest", false],
                ["honey", "settings.logo.honey", false],
                ["flat", "settings.logo.flat", false],
                ["custom", "settings.logo.custom", false],
              ] as const
            ).map(([id, label, isDefault]) => {
              const selected = (settings?.logo ?? "") === id || (isDefault && !settings?.logo);
              return (
                <button
                  key={id}
                  aria-pressed={selected}
                  aria-label={`${t(label)}${isDefault ? ` (${t("settings.logo.default")})` : ""}`}
                  onClick={() => {
                    if (id === "custom") {
                      void (async () => {
                        const path = await pickPngPath();
                        if (!path) return;
                        try {
                          await api.logoSetCustom(path);
                          await update({ logo: "custom" });
                          await api.applyLogo("custom");
                        } catch (err) {
                          setSaveError(apiErrorText(err));
                        }
                      })();
                      return;
                    }
                    void update({ logo: id })
                      .then(() => api.applyLogo(id))
                      .then(() => setSaveError(null))
                      .catch((e) => setSaveError(messageOf(e)));
                  }}
                  className={`flex flex-col items-center gap-1.5 rounded-lg border p-2 transition-colors ${
                    selected
                      ? "border-accent bg-accent-soft/30"
                      : "border-border-app hover:border-border-strong"
                  }`}
                >
                  {id === "custom" ? (
                    <span
                      aria-hidden
                      className={`grid size-14 place-items-center rounded-lg border border-dashed border-border-strong text-[11px] font-semibold text-text-muted ${
                        settings?.logo === "custom" ? "border-accent text-accent" : ""
                      }`}
                    >
                      PNG
                    </span>
                  ) : (
                    <img src={`/logos/${id}.png`} alt="" className="size-14 rounded-lg object-cover" />
                  )}
                  <span className="flex w-full items-center gap-1">
                    <span className="text-[11px] leading-tight text-text-muted">{t(label)}</span>
                    <span className="ml-auto grid size-5 place-items-center">
                      {selected && (
                        <span key="selected" className="anim-pop-in grid size-5 place-items-center rounded-full bg-accent text-on-accent">
                          <Check size={12} aria-hidden strokeWidth={2.5} />
                        </span>
                      )}
                    </span>
                  </span>
                </button>
              );
            })}
          </div>
          <p className="mt-2 text-xs text-text-muted">{t("settings.logo.hint")}</p>
        </div>
      </section>

      <UpdatesSection />

      {/* Дисковое пространство (F18) и экспорт/импорт настроек (F20) */}
      <section className="anim-fade-up" style={{ animationDelay: "200ms" }}>
        <h3 className="mb-3 text-[13px] font-semibold uppercase tracking-wider text-text-muted">
          {t("settings.storage")}
        </h3>
        <div className="rounded-lg border border-border-app bg-card p-4">
          <div className="flex flex-wrap gap-2">
            <button
              onClick={() => void scanStorage()}
              disabled={storageBusy}
              aria-label={t("settings.storage.scan")}
              className="btn-ghost btn-sm flex items-center gap-2"
            >
              <RefreshCw size={14} aria-hidden />
              {t("settings.storage.scan")}
            </button>
            <button
              onClick={() => void cleanStorage()}
              disabled={storageBusy}
              aria-label={t("settings.storage.clean")}
              className="btn-ghost btn-sm flex items-center gap-2"
            >
              <Trash2 size={14} aria-hidden />
              {t("settings.storage.clean")}
            </button>
          </div>
          {stats && (
            <p className="mt-3 text-xs text-text-muted">
              {t("settings.storage.stats", {
                instances: fmtBytes(stats.instancesBytes),
                cache: fmtBytes(stats.cacheBytes),
                trash: fmtBytes(stats.trashBytes),
                logs: fmtBytes(stats.logsBytes),
              })}
            </p>
          )}
          {storageMsg && <p className="mt-2 text-xs text-accent">{storageMsg}</p>}

          <div className="mt-4 border-t border-border-app pt-3">
            <div className="flex flex-wrap gap-2">
              <button
                onClick={() => void exportSettings()}
                disabled={settingsIoBusy}
                aria-label={t("settings.export")}
                className="btn-ghost btn-sm flex items-center gap-2"
              >
                <Upload size={14} aria-hidden />
                {t("settings.export")}
              </button>
              <button
                onClick={() => void importSettings()}
                disabled={settingsIoBusy}
                aria-label={t("settings.import")}
                className="btn-ghost btn-sm flex items-center gap-2"
              >
                <Download size={14} aria-hidden />
                {t("settings.import")}
              </button>
            </div>
            {settingsIoMsg && <p className="mt-2 text-xs text-accent">{settingsIoMsg}</p>}
          </div>
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

function Toggle({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  /** A35: доступное имя переключателя (role=switch без него анонимный). */
  label: string;
}) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      aria-label={label}
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
