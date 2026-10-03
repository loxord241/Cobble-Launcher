// Настройки → «Обновления лаунчера» (D46). Проверка — `update_check`
// (GitHub releases/latest, работает и без манифеста); состояние живёт в
// сторе updates.ts — его же наполняет тихий старт-чек. Установка —
// updater-плагин: подписанный апдейт (latest.json + minisign), прогресс
// скачивания, перезапуск через process-плагин. Если подписанного релиза
// ещё нет — честная надпись + ручной путь на страницу релизов.
// Самодостаточен: без пропсов, вставляется в SettingsPage одной строкой.
import { useCallback, useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Check, Download, ExternalLink, LoaderCircle, RefreshCw } from "lucide-react";
import { apiErrorText, errorText, t } from "../../i18n";
import { useUpdateStore } from "../../state/updates";

export default function UpdatesSection() {
  const [version, setVersion] = useState("");
  const info = useUpdateStore((s) => s.info);
  const busy = useUpdateStore((s) => s.busy);
  const error = useUpdateStore((s) => s.error);
  const check = useUpdateStore((s) => s.check);
  const checked = useUpdateStore((s) => s.checked);

  const [installing, setInstalling] = useState(false);
  // null = прогресс не определён (ContentLength не пришёл), иначе 0–100.
  const [progress, setProgress] = useState<number | null>(null);
  const [installError, setInstallError] = useState<string | null>(null);

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(""));
    // Тихий старт-чек мог уже отработать — если нет, проверим при заходе
    // в раздел (но не молча: это явное действие пользователя).
    if (!checked) void check();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const openReleases = useCallback(async (url: string) => {
    try {
      await openUrl(url);
    } catch (e) {
      setInstallError(apiErrorText(e));
    }
  }, []);

  // Установка: отдельная проверка updater-плагином даёт объект Update с
  // подписью; качаем с прогрессом и перезапускаемся. Релизы без latest.json
  // (до 0.2.1) дают ошибку — показываем честный ручной путь.
  const install = useCallback(async () => {
    setInstalling(true);
    setInstallError(null);
    setProgress(null);
    try {
      const { check: pluginCheck } = await import("@tauri-apps/plugin-updater");
      const update = await pluginCheck();
      if (!update) {
        setInstallError(t("updates.installerUnavailable"));
        return;
      }
      let total = 0;
      let received = 0;
      await update.downloadAndInstall((ev) => {
        if (ev.event === "Started") {
          total = ev.data.contentLength ?? 0;
        } else if (ev.event === "Progress") {
          received += ev.data.chunkLength ?? 0;
          if (total > 0) {
            setProgress(Math.min(100, Math.round((received * 100) / total)));
          }
        } else {
          setProgress(100);
        }
      });
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (e) {
      setInstallError(t("updates.installerUnavailable"));
    } finally {
      setInstalling(false);
    }
  }, []);

  const latest = info?.latest ?? null;
  const url = info?.url ?? null;
  const note = info?.note ?? null;
  // Ядро отдаёт код причины (`not_found`, `network`, …) — переводим словарями
  // errors.json; если однажды придёт человеческий текст, покажем его как есть.
  const noteText = note ? errorText({ code: note, message: note }) : null;
  const updateAvailable = Boolean(info?.updateAvailable && latest);
  const upToDate = Boolean(info?.checked && latest && !info?.updateAvailable);
  const couldNotCheck = Boolean(info?.checked && !latest && noteText);

  return (
    <section>
      <div className="rounded-lg border border-border-app bg-card p-4">
        <div className="flex items-center justify-between gap-4">
          <div>
            <div className="text-sm font-semibold">{t("app.name")}</div>
            <div className="font-mono text-xs text-text-muted">{info?.current || version || "?"}</div>
          </div>
          <button
            type="button"
            onClick={() => void check()}
            disabled={busy}
            aria-busy={busy}
            className="btn-ghost btn-sm"
          >
            {busy ? (
              <LoaderCircle size={14} aria-hidden className="animate-spin" />
            ) : (
              <RefreshCw size={14} aria-hidden />
            )}
            {t("mods.checkUpdates")}
          </button>
        </div>

        {/* Поломка IPC/opener — красная плашка (как в Shell) */}
        {error && (
          <p
            role="alert"
            className="mt-3 rounded-lg border border-error/40 bg-error/10 p-3 text-sm text-error"
          >
            {error}
          </p>
        )}

        {/* Есть обновление: «текущая → новая», установка — подписанный апдейт,
            ручной путь — ссылка на страницу релиза */}
        {updateAvailable && latest && (
          <div
            role="status"
            className="anim-fade-up mt-3 rounded-lg border border-success/40 bg-success/10 p-3 text-sm text-success"
          >
            <div className="flex flex-wrap items-center gap-2">
              <span className="font-mono">{info?.current || version} →</span>
              <span className="font-mono font-semibold">{latest}</span>
              {url && (
                <button
                  type="button"
                  onClick={() => void openReleases(url)}
                  className="inline-flex items-center gap-1 underline decoration-dotted underline-offset-2 hover:decoration-solid"
                >
                  {t("updates.openReleases")}
                  <ExternalLink size={13} aria-hidden />
                </button>
              )}
            </div>
            <div className="mt-2 flex flex-wrap items-center gap-3">
              <button
                type="button"
                onClick={() => void install()}
                disabled={installing}
                aria-busy={installing}
                className="btn-primary btn-sm"
              >
                {installing ? (
                  <LoaderCircle size={14} aria-hidden className="animate-spin" />
                ) : (
                  <Download size={14} aria-hidden />
                )}
                {installing ? t("updates.installing") : t("updates.installNow")}
              </button>
              {installing && progress !== null && (
                <div
                  className="h-1.5 w-40 overflow-hidden rounded-full bg-surface-2"
                  role="progressbar"
                  aria-valuenow={progress}
                  aria-valuemin={0}
                  aria-valuemax={100}
                >
                  <div
                    className="h-full rounded-full bg-success transition-[width] duration-300"
                    style={{ width: `${progress}%` }}
                  />
                </div>
              )}
              {installing && progress !== null && (
                <span className="font-mono text-xs">{progress}%</span>
              )}
            </div>
          </div>
        )}

        {/* Сбой установки (нет манифеста/подписи, сеть): честная причина +
            ручной путь на релизы */}
        {installError && (
          <div
            role="alert"
            className="mt-3 flex flex-wrap items-center gap-2 rounded-lg border border-warning/40 bg-warning/10 p-3 text-sm text-warning"
          >
            <span>{installError}</span>
            {url && (
              <button
                type="button"
                onClick={() => void openReleases(url)}
                className="inline-flex items-center gap-1 underline decoration-dotted underline-offset-2 hover:decoration-solid"
              >
                {t("updates.openReleases")}
                <ExternalLink size={13} aria-hidden />
              </button>
            )}
          </div>
        )}

        {/* Версия актуальна: тег последнего релиза + галка, без слов */}
        {upToDate && (
          <div
            role="status"
            className="anim-fade-up mt-3 flex items-center gap-2 rounded-lg border border-border-app bg-surface-2 p-3 text-sm text-text-muted"
          >
            <Check size={14} aria-hidden className="text-success" />
            <span>{t("updates.upToDate")}</span>
            <span className="font-mono text-text-muted">({latest})</span>
          </div>
        )}

        {/* Версию узнать не удалось: причина словами (сеть/репозиторий закрыт) */}
        {couldNotCheck && (
          <div
            role="status"
            className="anim-fade-up mt-3 rounded-lg border border-border-app bg-surface-2 p-3 text-sm text-text-muted"
          >
            {noteText}
          </div>
        )}
      </div>
    </section>
  );
}
