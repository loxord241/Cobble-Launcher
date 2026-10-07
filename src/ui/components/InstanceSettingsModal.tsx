// Настройки инстанса: RAM (память), выбор Java, флаги JVM, аргументы игры,
// быстрый запуск (F1), профиль для запуска (F11), пресеты JVM (F5).
import { useEffect, useState } from "react";
import { X, Save, Sparkles } from "lucide-react";
import { api } from "../../api/client";
import { t, apiErrorText, currentLanguage } from "../../i18n";
import { useModalA11y } from "../hooks/useModalA11y";
import { formatRamMb } from "../format";
import { useAccounts, accountKindLabelKey } from "../../state/accounts";
import type { Instance, JavaInstall, JavaTestInfo, RamGuide } from "../../api/types";

/** F5: пресет ЗАМЕНЯЕТ содержимое поля флагов (пользователь может доредактировать).
 * Флаги сверены с валидатором ядра (FORBIDDEN_JVM_SUBSTRINGS) — запрещённых нет. */
const JVM_PRESETS: Record<string, string[]> = {
  aikar: [
    "-XX:+UseG1GC",
    "-XX:+ParallelRefProcEnabled",
    "-XX:+MaxGCPauseMillis=200",
    "-XX:+UnlockExperimentalVMOptions",
    "-XX:+DisableExplicitGC",
    "-XX:G1NewSizePercent=30",
    "-XX:G1MaxNewSizePercent=40",
    "-XX:G1HeapRegionSize=8M",
    "-XX:G1ReservePercent=20",
    "-XX:G1HeapWastePercent=5",
    "-XX:G1MixedGCCountTarget=4",
    "-XX:InitiatingHeapOccupancyPercent=15",
    "-XX:G1MixedGCLiveThresholdPercent=90",
    "-XX:G1RSetUpdatingPauseTimePercent=5",
    "-XX:SurvivorRatio=32",
    "-XX:+PerfDisableSharedMem",
    "-XX:MaxTenuringThreshold=1",
  ],
  zgc: ["-XX:+UseZGC", "-XX:+ZGenerational"],
  lowmem: ["-XX:+UseSerialGC", "-XX:MinHeapFreeRatio=10", "-XX:MaxHeapFreeRatio=30"],
};

/** F1: quick play доступен с MC 1.20 (major.minor); снапшоты/беты не парсятся. */
function mcSupportsQuickPlay(mcVersion: string): boolean {
  const [major, minor] = mcVersion.split(".").map((p) => parseInt(p, 10));
  return major === 1 && (minor ?? 0) >= 20;
}

/** Токенизация строки аргументов с учётом двойных кавычек:
 *  -Dp="C:\My Game" → один токен -Dp=C:\My Game (кавычки снимаются).
 *  Куски без пробела между ними склеиваются: сам regex даёт `-Dp=` и
 *  `"C:\My Game"` отдельными совпадениями — без склейки флаг рвался бы
 *  пополам, как и при наивном сплите по пробелам. */
function splitArgs(s: string): string[] {
  const out: string[] = [];
  const re = /[^\s"']+|"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'/g;
  let cur = "";
  let prevEnd = -1;
  for (let m = re.exec(s); m !== null; m = re.exec(s)) {
    // Разрыв (пробел) закрывает текущий токен; смежные куски склеиваются.
    if (prevEnd >= 0 && m.index !== prevEnd && cur) {
      out.push(cur);
      cur = "";
    }
    cur += m[0].replace(/^"(.*)"$/, "$1").replace(/^'(.*)'$/, "$1");
    prevEnd = m.index + m[0].length;
  }
  if (cur) out.push(cur);
  return out;
}

export default function InstanceSettingsModal({
  instance,
  onClose,
  onSaved,
}: {
  instance: Instance;
  onClose: () => void;
  onSaved: (updated: Instance) => void;
}) {
  const [ramMb, setRamMb] = useState(instance.ramMb || 4096);
  const [ramGuide, setRamGuide] = useState<RamGuide | null>(null);
  const [javaList, setJavaList] = useState<JavaInstall[]>([]);
  const [javaPath, setJavaPath] = useState(instance.javaPath ?? "");
  const [jvmFlags, setJvmFlags] = useState(instance.jvmFlags.join(" "));
  // F5: выбранный пресет (none — ничего не делает).
  const [jvmPreset, setJvmPreset] = useState("none");
  const [gameArgs, setGameArgs] = useState(instance.gameArgsExtra.join(" "));
  // F1: быстрый запуск.
  const supportsQuickPlay = mcSupportsQuickPlay(instance.mcVersion);
  const [quickPlayMode, setQuickPlayMode] = useState<"none" | "world" | "server">(() =>
    instance.quickPlayWorld ? "world" : instance.quickPlayServer ? "server" : "none",
  );
  const [quickPlayWorld, setQuickPlayWorld] = useState(instance.quickPlayWorld ?? "");
  const [quickPlayServer, setQuickPlayServer] = useState(instance.quickPlayServer ?? "");
  // F11: профиль для запуска ("" = глобальный). Висячая ссылка (профиль удалён)
  // не храним: показываем глобальный, при сохранении пишем undefined.
  const { list: accounts } = useAccounts();
  const accountExists = (id?: string) => !!id && accounts.some((a) => a.id === id);
  const [accountId, setAccountId] = useState(() =>
    accountExists(instance.accountId) ? (instance.accountId as string) : "",
  );
  // F14: рекомендуемая Java — подсказка; ошибка/нет данных → ничего не показываем.
  const [javaRecommendedMajor, setJavaRecommendedMajor] = useState<number | null>(null);
  // F15: пробный запуск выбранной java (кнопка «Проверить»).
  const [javaTestBusy, setJavaTestBusy] = useState(false);
  const [javaTestResult, setJavaTestResult] = useState<JavaTestInfo | null>(null);
  const [javaTestError, setJavaTestError] = useState<string | null>(null);
  const [notes, setNotes] = useState(instance.notes ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // A33: Escape, ловушка фокуса и возврат фокуса на триггер (единый хук).
  // D41: requestClose — закрытие с анимацией (для ✕ и подложки).
  const { containerRef: dialogRef, requestClose } = useModalA11y(onClose);

  // D64: сохранённая RAM может лежать вне нового диапазона ползунка (максимум
  // зависит от машины: ramGuide.totalMb) — клампим и при открытии (показываем
  // фактическое), и при сохранении, иначе ползунок молча хранит старое.
  const totalRam = ramGuide?.totalMb ?? 16384;
  const maxRam = Math.min(totalRam, 32768);
  const clampRam = (v: number) => Math.min(Math.max(v, 1024), maxRam);

  useEffect(() => {
    api
      .ramGuide()
      .then((g) => {
        setRamGuide(g);
        setRamMb((cur) => Math.min(Math.max(cur, 1024), Math.min(g.totalMb, 32768)));
      })
      .catch(() => setRamGuide(null));
    api.javaList().then(setJavaList).catch(() => setJavaList([]));
    // F14: подсказка грузится один раз при открытии; это не данные — при ошибке
    // чип просто не появляется.
    api
      .javaRecommended(instance.mcVersion)
      .then(setJavaRecommendedMajor)
      .catch(() => setJavaRecommendedMajor(null));
  }, [instance.mcVersion]);

  /** F5: пресет заменяет текст поля флагов; «без пресета» ничего не меняет. */
  const applyJvmPreset = (preset: string) => {
    setJvmPreset(preset);
    const flags = JVM_PRESETS[preset];
    if (flags) setJvmFlags(flags.join(" "));
  };

  /** F15: пробный запуск выбранной java; результат показываем под селектом. */
  const runJavaTest = async () => {
    const exe = javaPath.trim();
    if (!exe || javaTestBusy) return;
    setJavaTestBusy(true);
    setJavaTestResult(null);
    setJavaTestError(null);
    try {
      setJavaTestResult(await api.javaTestPath(exe));
    } catch (e) {
      setJavaTestError(apiErrorText(e));
    } finally {
      setJavaTestBusy(false);
    }
  };

  /** Смена Java сбрасывает результат проверки — он относится к прежней java. */
  const changeJavaPath = (value: string) => {
    setJavaPath(value);
    setJavaTestResult(null);
    setJavaTestError(null);
  };

  // Честная статистика из instance.json (пишется ядром при запусках).
  const hours = Math.floor((instance.playSeconds ?? 0) / 3600);
  const minutes = Math.round(((instance.playSeconds ?? 0) % 3600) / 60);
  const lastPlayed = instance.lastPlayed
    ? new Date(instance.lastPlayed * 1000).toLocaleString(currentLanguage())
    : null;

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      // D19: снапшот instance мог устареть (ядро дописало playSeconds/
      // launchCount за время, пока модалка открыта) — перечитываем актуальный
      // инстанс и накладываем только редактируемые поля.
      const fresh = await api.instanceSettingsGet(instance.id);
      const updated: Instance = {
        ...fresh,
        // D64: RAM вне ползунка (машина сменилась, max меньше) — кламп при записи.
        ramMb: clampRam(ramMb),
        javaPath: javaPath.trim() || undefined,
        // splitArgs, а не сплит по пробелам: -Dcustom.path="C:\Games\My Pack"
        // — один токен, иначе JVM получает обрезанный флаг и не стартует.
        jvmFlags: splitArgs(jvmFlags),
        gameArgsExtra: splitArgs(gameArgs),
        notes: notes.trim(),
        // F1: пустая строка/не тот режим → undefined (не храним мусор).
        quickPlayWorld:
          supportsQuickPlay && quickPlayMode === "world" && quickPlayWorld.trim()
            ? quickPlayWorld.trim()
            : undefined,
        quickPlayServer:
          supportsQuickPlay && quickPlayMode === "server" && quickPlayServer.trim()
            ? quickPlayServer.trim()
            : undefined,
        // F11: несуществующий профиль не сохраняем — пишем undefined.
        accountId: accountExists(accountId) ? accountId : undefined,
      };
      const res = await api.instanceSettingsSet(updated);
      onSaved(res);
      onClose();
    } catch (e) {
      setError(apiErrorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className="anim-fade-in fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onClick={(e) => {
        if (e.target === e.currentTarget) requestClose();
      }}
    >
      {/* D41/F10: ref и anim-классы на карточке, не на оверлее — иначе
          .anim-closing схлопывает весь экран, а не только диалог. */}
      <div
        ref={dialogRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={t("instances.settings.title", { name: instance.name })}
        className="anim-dialog-in max-h-[90vh] w-[520px] overflow-y-auto rounded-lg border border-border-app bg-card p-6 shadow-xl"
      >
        <div className="flex items-center justify-between border-b border-border-app pb-3">
          <h2 className="text-lg font-semibold text-text">
            {t("instances.settings.title", { name: instance.name })}
          </h2>
          <button
            onClick={requestClose}
            aria-label={t("common.close")}
            className="icon-btn text-text-muted hover:text-text"
          >
            <X size={18} aria-hidden />
          </button>
        </div>

        <div className="mt-3 flex flex-wrap gap-2 text-[13px]">
          <span className="chip-mono">{t("instances.stats.launches", { n: instance.launchCount ?? 0 })}</span>
          <span className="chip-mono">
            {t("instances.stats.playTime", { h: hours, m: minutes })}
          </span>
          {lastPlayed && (
            <span className="chip-mono">{t("instances.stats.lastPlayed", { at: lastPlayed })}</span>
          )}
        </div>

        <div className="mt-4 flex flex-col gap-4">
          {/* RAM выделение */}
          <div>
            <div className="flex items-center justify-between">
              <label htmlFor="inst-ram" className="text-sm font-medium text-text">
                {t("instances.settings.ram")}
              </label>
              <span className="font-mono text-sm font-semibold text-accent">
                {formatRamMb(ramMb)}
              </span>
            </div>
            <input
              id="inst-ram"
              type="range"
              min={1024}
              max={maxRam}
              step={512}
              value={ramMb}
              onChange={(e) => setRamMb(Number(e.target.value))}
              className="mt-2 h-2 w-full cursor-pointer appearance-none rounded-lg bg-surface-2 accent-accent"
            />
            <div className="mt-1 flex justify-between text-xs text-text-muted">
              <span>1024 MB</span>
              {ramGuide && (
                <button
                  type="button"
                  onClick={() => setRamMb(ramGuide.recommendedMb)}
                  className="flex items-center gap-1 text-accent hover:underline"
                >
                  <Sparkles size={12} aria-hidden />
                  {t("instances.settings.ramRecommended", { mb: ramGuide.recommendedMb })}
                </button>
              )}
              <span>{maxRam} MB</span>
            </div>
          </div>

          {/* Java Runtime */}
          <div>
            <label htmlFor="inst-java" className="block text-sm font-medium text-text">
              {t("instances.settings.java")}
            </label>
            <div className="mt-1 flex items-center gap-2">
              <select
                id="inst-java"
                value={javaPath}
                onChange={(e) => changeJavaPath(e.target.value)}
                className="h-10 min-w-0 flex-1 rounded-md border border-border-app bg-bg px-3 text-sm text-text outline-none focus:border-accent"
              >
                <option value="">{t("instances.settings.javaAuto")}</option>
                {javaList.map((j) => (
                  <option key={j.javaExe} value={j.javaExe}>
                    Java {j.major} ({j.origin}, {j.archBits}-bit) — {j.javaExe}
                  </option>
                ))}
              </select>
              {/* F15: пробный запуск выбранной java; без выбора кнопка неактивна. */}
              <button
                type="button"
                onClick={() => void runJavaTest()}
                disabled={!javaPath.trim() || javaTestBusy}
                className="btn-ghost h-10 shrink-0 px-3 text-sm"
              >
                {t("instances.java.test")}
              </button>
            </div>
            {javaTestResult && (
              <p className="mt-1 text-xs text-success" role="status">
                {t("instances.java.test.ok", {
                  major: javaTestResult.major,
                  bits: javaTestResult.bits,
                })}
              </p>
            )}
            {javaTestError && (
              <p className="mt-1 text-xs text-error" role="alert">
                {t("instances.java.test.fail", { error: javaTestError })}
              </p>
            )}
            <p className="mt-1 text-xs text-text-muted">
              {t("instances.settings.javaHint")}
            </p>
            {/* F14: рекомендуемая мажорная версия Java для этой версии MC. */}
            {javaRecommendedMajor !== null && (
              <p className="mt-1 text-xs text-accent">
                {t("instances.java.recommended", { major: javaRecommendedMajor })}
              </p>
            )}
          </div>

          {/* JVM Flags */}
          <div>
            <label htmlFor="inst-jvm-flags" className="block text-sm font-medium text-text">
              {t("instances.settings.jvmFlags")}
            </label>
            {/* F5: пресет заменяет содержимое поля, дальше можно доредактировать. */}
            <label htmlFor="inst-jvm-preset" className="mt-2 block text-xs text-text-muted">
              {t("instances.jvm.presets")}
            </label>
            <select
              id="inst-jvm-preset"
              value={jvmPreset}
              onChange={(e) => applyJvmPreset(e.target.value)}
              className="mt-1 h-10 w-full rounded-md border border-border-app bg-bg px-3 text-sm text-text outline-none focus:border-accent"
            >
              <option value="none">{t("instances.jvm.preset.none")}</option>
              <option value="aikar">{t("instances.jvm.preset.aikar")}</option>
              <option value="zgc">{t("instances.jvm.preset.zgc")}</option>
              <option value="lowmem">{t("instances.jvm.preset.lowmem")}</option>
            </select>
            <textarea
              id="inst-jvm-flags"
              value={jvmFlags}
              onChange={(e) => setJvmFlags(e.target.value)}
              rows={2}
              placeholder="-XX:+UseG1GC -XX:+UnlockExperimentalVMOptions"
              className="mt-1 w-full rounded-md border border-border-app bg-bg px-3 py-2 font-mono text-xs text-text outline-none focus:border-accent"
            />
          </div>

          {/* Game arguments extra */}
          <div>
            <label htmlFor="inst-game-args" className="block text-sm font-medium text-text">
              {t("instances.settings.gameArgs")}
            </label>
            <input
              id="inst-game-args"
              type="text"
              value={gameArgs}
              onChange={(e) => setGameArgs(e.target.value)}
              placeholder="--demo"
              className="mt-1 h-10 w-full rounded-md border border-border-app bg-bg px-3 text-sm text-text outline-none focus:border-accent"
            />
          </div>

          {/* Быстрый запуск (F1): доступен с MC 1.20. */}
          <div>
            <label htmlFor="inst-quickplay" className="block text-sm font-medium text-text">
              {t("instances.quickplay.label")}
            </label>
            {supportsQuickPlay ? (
              <div className="mt-1 flex flex-col gap-2">
                <select
                  id="inst-quickplay"
                  value={quickPlayMode}
                  onChange={(e) =>
                    setQuickPlayMode(e.target.value as "none" | "world" | "server")
                  }
                  className="h-10 w-full rounded-md border border-border-app bg-bg px-3 text-sm text-text outline-none focus:border-accent"
                >
                  <option value="none">{t("instances.quickplay.none")}</option>
                  <option value="world">{t("instances.quickplay.world")}</option>
                  <option value="server">{t("instances.quickplay.server")}</option>
                </select>
                {quickPlayMode === "world" && (
                  <input
                    id="inst-quickplay-world"
                    type="text"
                    value={quickPlayWorld}
                    onChange={(e) => setQuickPlayWorld(e.target.value)}
                    maxLength={64}
                    placeholder={t("instances.quickplay.worldName")}
                    className="h-10 w-full rounded-md border border-border-app bg-bg px-3 text-sm text-text outline-none focus:border-accent"
                  />
                )}
                {quickPlayMode === "server" && (
                  <input
                    id="inst-quickplay-server"
                    type="text"
                    value={quickPlayServer}
                    onChange={(e) => setQuickPlayServer(e.target.value)}
                    maxLength={255}
                    placeholder={t("instances.quickplay.address")}
                    className="h-10 w-full rounded-md border border-border-app bg-bg px-3 text-sm text-text outline-none focus:border-accent"
                  />
                )}
              </div>
            ) : (
              <p className="mt-1 text-xs text-text-muted">
                {t("instances.quickplay.oldVersion")}
              </p>
            )}
          </div>

          {/* Профиль для запуска (F11). */}
          <div>
            <label htmlFor="inst-account" className="block text-sm font-medium text-text">
              {t("instances.account.launch")}
            </label>
            <select
              id="inst-account"
              value={accountId}
              onChange={(e) => setAccountId(e.target.value)}
              className="mt-1 h-10 w-full rounded-md border border-border-app bg-bg px-3 text-sm text-text outline-none focus:border-accent"
            >
              <option value="">{t("instances.account.global")}</option>
              {accounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name} ({t(accountKindLabelKey[a.kind])})
                </option>
              ))}
            </select>
          </div>

          {/* Notes */}
          <div>
            <label htmlFor="inst-notes" className="block text-sm font-medium text-text">
              {t("instances.settings.notes")}
            </label>
            <textarea
              id="inst-notes"
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              rows={2}
              placeholder={t("instances.settings.notesPlaceholder")}
              className="mt-1 w-full rounded-md border border-border-app bg-bg px-3 py-2 text-sm text-text outline-none focus:border-accent"
            />
          </div>
        </div>

        {error && (
          <p className="mt-3 rounded border border-error/40 bg-error/10 p-2 text-sm text-error" role="alert">
            {error}
          </p>
        )}

        <div className="mt-6 flex justify-end gap-3 border-t border-border-app pt-4">
          <button
            type="button"
            onClick={onClose}
            className="btn-ghost h-10 px-4 text-sm"
          >
            {t("common.cancel")}
          </button>
          <button
            type="button"
            onClick={() => void save()}
            disabled={busy}
            className="btn-primary flex h-10 items-center gap-2 px-5 text-sm"
          >
            <Save size={16} aria-hidden />
            {t("common.save")}
          </button>
        </div>
      </div>
    </div>
  );
}
