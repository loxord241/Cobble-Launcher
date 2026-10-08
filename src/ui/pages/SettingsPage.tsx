// Настройки «Сланца»: панели с капс-заголовками; разделы Темы (карточки),
// Общие (RAM/параллелизм/снапшоты), Java (сканирование и установка Adoptium), Сеть и Данные.
import { useCallback, useEffect, useRef, useState } from "react";
import { Download, RefreshCw, Check, Coffee, Trash2, Upload, Search } from "lucide-react";
import { isHexColor, useSettings } from "../../state/settings";
import { useInstances } from "../../state/instances";
import { api } from "../../api/client";
import { apiErrorText, t } from "../../i18n";
import { formatSize } from "../format";
import { pickJsonPath, pickPngPath, pickSavePath } from "../components/pickFile";
import ThemeCards from "../components/ThemeCards";
import type { ForeignInstance, ForeignSource, JavaInstall, RamGuide, Settings, StorageStats } from "../../api/types";
import UpdatesSection from "../components/UpdatesSection";

/** D20: пауза перед записью в ядро — поля не пишутся на каждый keystroke. */
const SAVE_DEBOUNCE_MS = 400;

/** Кламп числового поля в границы HTML min/max: пустое/нечисловое — фолбэк. */
function clampBounded(raw: string, min: number, max: number, fallback: number): number {
  const value = Number(raw) || fallback;
  return Math.min(Math.max(value, min), max);
}


/** Пресеты акцентного цвета (первый — сланцевый синий по умолчанию). */
const ACCENT_PRESETS = ["#7FB2F0", "#6FBF8F", "#E8A25C", "#E06C6C", "#A78BE0"];

/** D67: мелкая метка «версия · загрузчик» чужого инстанса. Правило «нет
 * данных — нет элемента»: null-поля честно не показываем. */
function foreignMeta(inst: ForeignInstance): string {
  const parts: string[] = [];
  if (inst.mcVersion) parts.push(inst.mcVersion);
  if (inst.loader) parts.push(inst.loaderVersion ? `${inst.loader} ${inst.loaderVersion}` : inst.loader);
  return parts.join(" · ");
}

/** Акцент текущей палитры (tokens.css) — что показать в пикере, пока кастом не задан. */
function themeAccent(): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim();
  return isHexColor(value) ? value : ACCENT_PRESETS[0];
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
  // D20: числовые поля держим локально (в поле — сырой ввод, кламп на blur
  // и при записи в ядро), в ядро уходит отложенная пачка.
  const [ramInput, setRamInput] = useState(String(settings?.defaultRamMb ?? 2048));
  const [parInput, setParInput] = useState(String(settings?.downloadParallelism ?? 16));
  const [saveError, setSaveError] = useState<string | null>(null);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingPatch = useRef<Partial<Settings>>({});
  // Значения, отправленные в ядро с этой страницы: эффект синхронизации
  // сравнивает с ними и не затирает незакоммиченный ввод (отложенный коммит
  // возвращает настройки — эффект не должен переписывать поле посреди ввода).
  const lastSent = useRef<{ ram?: number; par?: number }>({});
  // Текстовые поля, правленные вручную: эффект синхронизации их не перетирает
  // при фоновом обновлении настроек (импорт, правка из второго окна).
  // Метка снимается при сохранении — поле снова живёт из стора.
  const dirtyInputs = useRef<Set<"proxy" | "azure">>(new Set());
  // F18: статистика диска + сообщение (результат уборки или ошибка).
  const [stats, setStats] = useState<StorageStats | null>(null);
  const [storageMsg, setStorageMsg] = useState<string | null>(null);
  const [storageBusy, setStorageBusy] = useState(false);
  // F20: сообщение экспорта/импорта настроек (результат или ошибка).
  const [settingsIoMsg, setSettingsIoMsg] = useState<string | null>(null);
  const [settingsIoBusy, setSettingsIoBusy] = useState(false);
  // D67: импорт инстансов из чужих лаунчеров (Prism/GDLauncher/ATLauncher/ванила).
  // sources=null — ещё не сканировали; выбор — по dir (уникален), статус — по dir.
  const [foreignSources, setForeignSources] = useState<ForeignSource[] | null>(null);
  const [foreignBusy, setForeignBusy] = useState(false);
  const [foreignError, setForeignError] = useState<string | null>(null);
  const [foreignSelected, setForeignSelected] = useState<Set<string>>(new Set());
  const [foreignImportBusy, setForeignImportBusy] = useState(false);
  const [foreignStatus, setForeignStatus] = useState<
    Record<string, { state: "progress" | "done" | "error"; text?: string }>
  >({});
  // По завершении импорта перечитываем список инстансов — так же, как это
  // делает импорт архивов на странице инстансов.
  const loadInstances = useInstances((s) => s.load);

  const commit = useCallback(
    (patch: Partial<Settings>) => {
      void update(patch)
        .then(() => setSaveError(null))
        .catch((e) => setSaveError(apiErrorText(e)));
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
      // Поля с ручным вводом не затираем: догоняем стор, только пока
      // пользователь их не трогал (иначе несохранённый ввод терялся).
      if (!dirtyInputs.current.has("proxy")) setProxyInput(settings.proxyUrl ?? "");
      if (!dirtyInputs.current.has("azure")) setAzureInput(settings.azureClientId ?? "");
      // Числовые поля: синхронизируем из стора только внешние изменения
      // (импорт настроек, правка из другого окна) — своё отправленное
      // значение эффекта не интересует, иначе ввод затирается.
      if (settings.defaultRamMb !== lastSent.current.ram)
        setRamInput(String(settings.defaultRamMb));
      if (settings.downloadParallelism !== lastSent.current.par)
        setParInput(String(settings.downloadParallelism));
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
      // Через apiErrorText: коды ядра локализуются (en/uk/pl), сырой message — фолбэк.
      setJavaMsg(t("settings.javaInstallFailed", { error: apiErrorText(e) }));
    } finally {
      setInstallingJava(null);
    }
  };

  const saveProxy = () => {
    const val = proxyInput.trim() || undefined;
    dirtyInputs.current.delete("proxy"); // сохранено — поле снова живёт из стора
    commit({ proxyUrl: val });
  };

  const saveAzure = () => {
    const val = azureInput.trim() || undefined;
    dirtyInputs.current.delete("azure"); // сохранено — поле снова живёт из стора
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
      setStorageMsg(t("settings.storage.cleaned", { freed: formatSize(res.freedBytes) }));
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
      // Импорт применяем последним: отменяем ожидающий debounce-пуш, иначе
      // накопленный патч после load() откатил бы импортированные числа.
      if (saveTimer.current !== null) {
        clearTimeout(saveTimer.current);
        saveTimer.current = null;
      }
      pendingPatch.current = {};
      // Импортированные прокси/Azure тоже показываем: поля снова из стора.
      dirtyInputs.current.clear();
      // importFrom = settingsImport + load + принудительная смена языка:
      // селект языка больше не врёт после импорта (D64).
      await useSettings.getState().importFrom(path);
      setSettingsIoMsg(t("settings.import.done"));
    } catch (e) {
      setSettingsIoMsg(apiErrorText(e));
    } finally {
      setSettingsIoBusy(false);
    }
  };

  /** D67: скан установленных чужих лаунчеров. Выбор и статусы сбрасываем —
   * список мог обновиться. */
  const scanForeign = async () => {
    setForeignBusy(true);
    setForeignError(null);
    try {
      const sources = await api.foreignScan();
      setForeignSources(sources);
      setForeignSelected(new Set());
      setForeignStatus({});
    } catch (e) {
      setForeignError(apiErrorText(e));
    } finally {
      setForeignBusy(false);
    }
  };

  const toggleForeign = (dir: string) => {
    setForeignSelected((prev) => {
      const next = new Set(prev);
      if (next.has(dir)) next.delete(dir);
      else next.add(dir);
      return next;
    });
  };

  /** D67: последовательный импорт выбранных — копирование тяжёлое, по одному,
   * никаких параллельных запусков (кнопки заблокированы на время работы).
   * Ошибка прерывает остаток очереди; список перечитываем всегда — частичный
   * успех должен быть виден (как у импорта архивов, D62). */
  const importForeign = async () => {
    if (foreignImportBusy || foreignSelected.size === 0) return;
    const queue = (foreignSources ?? [])
      .flatMap((src) => src.instances)
      .filter((inst) => foreignSelected.has(inst.dir));
    setForeignImportBusy(true);
    setForeignError(null);
    try {
      for (const inst of queue) {
        setForeignStatus((prev) => ({ ...prev, [inst.dir]: { state: "progress" } }));
        try {
          await api.foreignImport(inst.dir, inst.name);
          setForeignStatus((prev) => ({ ...prev, [inst.dir]: { state: "done" } }));
        } catch (e) {
          setForeignStatus((prev) => ({
            ...prev,
            [inst.dir]: { state: "error", text: apiErrorText(e) },
          }));
          setForeignError(apiErrorText(e));
          break;
        }
      }
    } finally {
      setForeignImportBusy(false);
      await loadInstances();
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
                // В поле — сырой ввод: кламп на каждый keystroke ломал набор
                // («1024096» превращался в «10240»). В ядро уходит уже
                // клампнутое значение — кламп живёт здесь и на blur, не в поле.
                const raw = e.target.value;
                setRamInput(raw);
                const value = clampBounded(raw, 1024, 32768, 2048);
                lastSent.current.ram = value;
                commitDebounced({ defaultRamMb: value });
              }}
              onBlur={() => {
                // Кламп в границы HTML min/max — при выходе из поля.
                const value = clampBounded(ramInput, 1024, 32768, 2048);
                lastSent.current.ram = value;
                setRamInput(String(value));
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
                // Сырой ввод, как у поля RAM: кламп на keystroke ломал набор.
                const raw = e.target.value;
                setParInput(raw);
                const value = clampBounded(raw, 1, 64, 16);
                lastSent.current.par = value;
                commitDebounced({ downloadParallelism: value });
              }}
              onBlur={() => {
                // Кламп в границы HTML min/max — при выходе из поля.
                const value = clampBounded(parInput, 1, 64, 16);
                lastSent.current.par = value;
                setParInput(String(value));
                commitDebounced({ downloadParallelism: value });
              }}
              className="h-10 w-44 rounded-lg border border-border-strong bg-bg px-3 text-text"
            />
          </Field>

          {/* F7: язык, масштаб и снапшоты — три самостоятельных Field,
              а не один Field «Снапшоты» с чужими контролами внутри. */}
          <Field label={t("settings.language")}>
            <select
              value={settings.language}
              onChange={(e) => commit({ language: e.target.value })}
              aria-label={t("settings.language")}
              className="h-10 rounded-lg border border-border-strong bg-card px-3 text-sm"
            >
              <option value="en">English</option>
              <option value="ru">Русский</option>
              <option value="uk">Українська</option>
              <option value="pl">Polski</option>
            </select>
          </Field>

          <Field label={t("settings.uiScale")}>
            <div className="flex flex-col items-end gap-1">
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
              <span className="text-[13px] text-text-muted">{t("settings.uiScale.hint")}</span>
            </div>
          </Field>

          <Field label={t("settings.snapshots")}>
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
                onChange={(e) => {
                  dirtyInputs.current.add("proxy");
                  setProxyInput(e.target.value);
                }}
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
                onChange={(e) => {
                  dirtyInputs.current.add("azure");
                  setAzureInput(e.target.value);
                }}
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
              <button
                onClick={() =>
                  void api
                    .dirOpen()
                    .then(() => setSaveError(null))
                    .catch((e) => setSaveError(apiErrorText(e)))
                }
                className="btn-ghost btn-sm"
              >
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
                      .catch((e) => setSaveError(apiErrorText(e)));
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
                instances: formatSize(stats.instancesBytes),
                cache: formatSize(stats.cacheBytes),
                trash: formatSize(stats.trashBytes),
                logs: formatSize(stats.logsBytes),
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

          {/* D67: импорт инстансов из чужих лаунчеров — скан, выбор, импорт. */}
          <div className="mt-4 border-t border-border-app pt-3">
            <div className="mb-2 text-xs font-medium text-text-muted">
              {t("foreign.section.title")}
            </div>
            <div className="flex flex-wrap gap-2">
              <button
                onClick={() => void scanForeign()}
                disabled={foreignBusy || foreignImportBusy}
                aria-label={t("foreign.scan.button")}
                className="btn-ghost btn-sm flex items-center gap-2"
              >
                <Search size={14} aria-hidden />
                {t("foreign.scan.button")}
              </button>
            </div>
            {foreignError && (
              <p
                role="alert"
                className="mt-2 rounded-lg border border-error/40 bg-error/10 p-2 text-xs text-error"
              >
                {foreignError}
              </p>
            )}

            {foreignSources !== null &&
              (foreignSources.length === 0 ? (
                <p className="mt-2 text-xs text-text-muted">{t("foreign.scan.none")}</p>
              ) : (
                <div className="mt-3 flex flex-col gap-3">
                  <div className="text-[13px] font-semibold text-text">
                    {t("foreign.scan.title")}
                  </div>
                  {foreignSources.map((src) => (
                    <div key={src.root} className="rounded-md border border-border-app bg-bg p-3">
                      <div className="flex items-baseline justify-between gap-2">
                        <span className="shrink-0 text-sm font-semibold text-text">
                          {t(`foreign.source.${src.kind}`)}
                        </span>
                        <code
                          title={src.root}
                          className="min-w-0 truncate font-mono text-[11px] text-text-muted"
                        >
                          {src.root}
                        </code>
                      </div>
                      <div className="mt-2 flex flex-col gap-1.5">
                        {src.instances.map((inst) => {
                          const status = foreignStatus[inst.dir];
                          const meta = foreignMeta(inst);
                          return (
                            <div key={inst.dir} className="flex flex-col gap-0.5">
                              <label className="flex cursor-pointer items-center gap-2 text-sm">
                                <input
                                  type="checkbox"
                                  className="accent-accent"
                                  checked={foreignSelected.has(inst.dir)}
                                  disabled={foreignImportBusy}
                                  onChange={() => toggleForeign(inst.dir)}
                                  aria-label={inst.name}
                                />
                                <span className="min-w-0 truncate">{inst.name}</span>
                                {meta && <span className="chip-mono shrink-0">{meta}</span>}
                                <span className="ml-auto shrink-0 text-xs text-text-muted">
                                  {t("foreign.size.approx", { size: formatSize(inst.sizeBytes) })}
                                </span>
                              </label>
                              {status && (
                                <p
                                  role={status.state === "error" ? "alert" : "status"}
                                  className={`pl-6 text-xs ${
                                    status.state === "error" ? "text-error" : "text-text-muted"
                                  }`}
                                >
                                  {status.state === "progress"
                                    ? t("foreign.import.progress", { name: inst.name })
                                    : status.state === "done"
                                      ? t("foreign.import.done", { name: inst.name })
                                      : status.text}
                                </p>
                              )}
                            </div>
                          );
                        })}
                      </div>
                    </div>
                  ))}
                  <p className="text-xs text-text-muted">{t("foreign.scan.hint")}</p>
                  {/* Превентивная подпись: импорт с загрузчиком без сети упадёт. */}
                  {settings.workOffline &&
                    foreignSources.some((src) =>
                      src.instances.some((inst) => foreignSelected.has(inst.dir) && !!inst.loader),
                    ) && (
                      <p className="text-xs text-text-muted">
                        {t("foreign.import.offline_hint")}
                      </p>
                    )}
                  <button
                    onClick={() => void importForeign()}
                    disabled={foreignImportBusy || foreignBusy || foreignSelected.size === 0}
                    className="btn-primary btn-sm self-start"
                  >
                    {t("foreign.import.button")}
                  </button>
                </div>
              ))}
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
