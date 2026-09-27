// Настройки инстанса: RAM (память), выбор Java, флаги JVM, аргументы игры.
import { useEffect, useState } from "react";
import { X, Save, Sparkles } from "lucide-react";
import { api } from "../../api/client";
import { t } from "../../i18n";
import type { Instance, JavaInstall, RamGuide } from "../../api/types";

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
  const [gameArgs, setGameArgs] = useState(instance.gameArgsExtra.join(" "));
  const [notes, setNotes] = useState(instance.notes ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.ramGuide().then(setRamGuide).catch(() => setRamGuide(null));
    api.javaList().then(setJavaList).catch(() => setJavaList([]));
  }, []);

  const totalRam = ramGuide?.totalMb ?? 16384;
  const maxRam = Math.min(totalRam, 32768);
  // Честная статистика из instance.json (пишется ядром при запусках).
  const hours = Math.floor((instance.playSeconds ?? 0) / 3600);
  const minutes = Math.round(((instance.playSeconds ?? 0) % 3600) / 60);
  const lastPlayed = instance.lastPlayed
    ? new Date(instance.lastPlayed * 1000).toLocaleString()
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
        ramMb,
        javaPath: javaPath.trim() || undefined,
        jvmFlags: jvmFlags
          .split(/\s+/)
          .map((s) => s.trim())
          .filter(Boolean),
        gameArgsExtra: gameArgs
          .split(/\s+/)
          .map((s) => s.trim())
          .filter(Boolean),
        notes: notes.trim(),
      };
      const res = await api.instanceSettingsSet(updated);
      onSaved(res);
      onClose();
    } catch (e) {
      setError(String((e as { message?: string })?.message ?? e));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      role="dialog"
      aria-modal="true"
      aria-label={t("instances.settings.title", { name: instance.name })}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="max-h-[90vh] w-[520px] overflow-y-auto rounded-lg border border-border-app bg-card p-6 shadow-xl">
        <div className="flex items-center justify-between border-b border-border-app pb-3">
          <h2 className="text-lg font-semibold text-text">
            {t("instances.settings.title", { name: instance.name })}
          </h2>
          <button
            onClick={onClose}
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
                {ramMb} MB ({(ramMb / 1024).toFixed(1)} GB)
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
            <select
              id="inst-java"
              value={javaPath}
              onChange={(e) => setJavaPath(e.target.value)}
              className="mt-1 h-10 w-full rounded-md border border-border-app bg-bg px-3 text-sm text-text outline-none focus:border-accent"
            >
              <option value="">{t("instances.settings.javaAuto")}</option>
              {javaList.map((j) => (
                <option key={j.javaExe} value={j.javaExe}>
                  Java {j.major} ({j.origin}, {j.archBits}-bit) — {j.javaExe}
                </option>
              ))}
            </select>
            <p className="mt-1 text-xs text-text-muted">
              {t("instances.settings.javaHint")}
            </p>
          </div>

          {/* JVM Flags */}
          <div>
            <label htmlFor="inst-jvm-flags" className="block text-sm font-medium text-text">
              {t("instances.settings.jvmFlags")}
            </label>
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
