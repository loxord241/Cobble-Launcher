// Настройки → «Обновления лаунчера»: ядро (`update_check`) спрашивает GitHub
// releases/latest и сравнивает версии. Авто-обновления нет (нет ключей подписи),
// поэтому UI только честно показывает результат и ведёт на страницу релизов.
// Самодостаточен: без пропсов, вставляется в SettingsPage одной строкой.
import { useCallback, useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Check, ExternalLink, LoaderCircle, RefreshCw } from "lucide-react";
import { api } from "../../api/client";
import type { UpdateInfo } from "../../api/types";
import { apiErrorText, errorText, t } from "../../i18n";

export default function UpdatesSection() {
  const [version, setVersion] = useState("");
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [busy, setBusy] = useState(false);
  // Сбой IPC/opener — не то же самое, что «ядро не смогло проверить» (info.note):
  // первое означает поломку, второе — честный «не удалось» от ядра.
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(""));
  }, []);

  const check = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      setInfo(await api.updateCheck());
    } catch (e) {
      setInfo(null);
      setError(apiErrorText(e));
    } finally {
      setBusy(false);
    }
  }, []);

  const openReleases = useCallback(async (url: string) => {
    try {
      await openUrl(url);
    } catch (e) {
      setError(apiErrorText(e));
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

        {/* Есть обновление: «текущая → новая», тег — ссылка на страницу релиза */}
        {updateAvailable && latest && (
          <div
            role="status"
            className="anim-fade-up mt-3 flex flex-wrap items-center gap-2 rounded-lg border border-success/40 bg-success/10 p-3 text-sm text-success"
          >
            <span className="font-mono">{info?.current || version} →</span>
            {url ? (
              <button
                type="button"
                onClick={() => void openReleases(url)}
                className="inline-flex items-center gap-1 font-mono font-semibold underline decoration-dotted underline-offset-2 hover:decoration-solid"
              >
                {t("updates.openReleases")}
                <ExternalLink size={13} aria-hidden />
              </button>
            ) : (
              <span className="font-mono font-semibold">{latest}</span>
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
