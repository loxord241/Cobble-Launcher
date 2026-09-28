// Таб «Миры» страницы инстанса (F24): миры из saves/, бэкап мира в zip и
// удаление мира/Незера/Энда. Компонент монтируется только на своём табе —
// данные грузятся лениво, как панель «Моды». Ошибки/уведомления показывает
// страница (единый баннер сверху), сюда передаются колбэки.
import { useCallback, useEffect, useState } from "react";
import { Archive, Globe, Trash2 } from "lucide-react";
import { api } from "../../../api/client";
import { apiErrorText, currentLanguage, t } from "../../../i18n";
import type { WorldInfo, WorldScope } from "../../../api/types";

/** Размер мира в строке списка: МБ с округлением (как fmtBytes в настройках). */
function fmtSize(bytes: number): string {
  return `${Math.round(bytes / (1024 * 1024))} МБ`;
}

/** Дата изменения мира — в локали интерфейса (A38), стиль даты страницы. */
function fmtDate(unixSecs: number): string {
  return new Date(unixSecs * 1000).toLocaleString(currentLanguage(), {
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export default function WorldsTab({
  instanceId,
  setError,
  setNotice,
}: {
  instanceId: string;
  setError: (msg: string | null) => void;
  setNotice: (msg: string) => void;
}) {
  const [worlds, setWorlds] = useState<WorldInfo[] | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      setWorlds(await api.instanceWorlds(instanceId));
    } catch (e) {
      setWorlds([]);
      setError(apiErrorText(e));
    }
  }, [instanceId, setError]);

  // Ленивая загрузка при активации таба (как «Моды»).
  useEffect(() => {
    void load();
  }, [load]);

  const run = useCallback(
    async (op: () => Promise<void>) => {
      setBusy(true);
      setError(null);
      try {
        await op();
      } catch (e) {
        setError(apiErrorText(e));
      } finally {
        setBusy(false);
      }
    },
    [setError],
  );

  const backup = (w: WorldInfo) =>
    void run(async () => {
      const r = await api.worldBackup(instanceId, w.name);
      setNotice(t("worlds.backup.done", { name: w.name, path: r.path }));
      await load();
    });

  const removeData = (w: WorldInfo, scope: WorldScope) => {
    // Подтверждение — только на безвозвратное удаление мира целиком.
    if (scope === "all" && !window.confirm(t("worlds.deleteConfirm", { name: w.name }))) return;
    void run(async () => {
      await api.worldDeleteData(instanceId, w.name, scope);
      await load();
    });
  };

  return (
    <div className="rounded-lg border border-border-app bg-card p-3">
      {worlds === null ? null : worlds.length === 0 ? (
        <div className="flex min-h-[320px] flex-col items-center justify-center gap-1 text-center">
          <Globe size={32} aria-hidden className="text-text-muted" />
          <p className="mt-2 font-semibold">{t("instances.worlds.emptyTitle")}</p>
          <p className="text-[13px] text-text-muted">{t("instances.worlds.emptyHint")}</p>
          <button
            onClick={() =>
              void api.instanceOpenDir(instanceId).catch((e) => setError(apiErrorText(e)))
            }
            className="btn-ghost btn-sm mt-3"
          >
            {t("instances.worlds.openFolder")}
          </button>
        </div>
      ) : (
        <ul className="flex flex-col gap-1">
          {worlds.map((w) => (
            <li
              key={w.name}
              className="grid min-h-14 grid-cols-[1fr_auto] items-center gap-3 rounded-md border border-border-app px-3 py-2 transition-colors hover:border-accent/60 hover:bg-card-hover"
            >
              <div className="min-w-0">
                <div className="truncate text-[13px] font-semibold leading-tight" title={w.name}>
                  {w.name}
                </div>
                <div className="truncate font-mono text-[12px] text-text-muted">
                  {fmtDate(w.lastModified)} · {fmtSize(w.sizeBytes)}
                </div>
              </div>
              <div className="flex flex-wrap items-center justify-end gap-2">
                {w.hasNether && <span className="badge-soft">{t("worlds.nether")}</span>}
                {w.hasEnd && <span className="badge-soft">{t("worlds.end")}</span>}
                <button
                  onClick={() => backup(w)}
                  disabled={busy}
                  className="btn-ghost btn-sm"
                >
                  <Archive size={14} aria-hidden />
                  {t("worlds.backup")}
                </button>
                {w.hasNether && (
                  <button
                    aria-label={t("worlds.deleteNether", { name: w.name })}
                    title={t("worlds.deleteNether", { name: w.name })}
                    disabled={busy}
                    onClick={() => removeData(w, "nether")}
                    className="btn-ghost btn-sm text-error"
                  >
                    <Trash2 size={14} aria-hidden />
                    {t("worlds.nether")}
                  </button>
                )}
                {w.hasEnd && (
                  <button
                    aria-label={t("worlds.deleteEnd", { name: w.name })}
                    title={t("worlds.deleteEnd", { name: w.name })}
                    disabled={busy}
                    onClick={() => removeData(w, "end")}
                    className="btn-ghost btn-sm text-error"
                  >
                    <Trash2 size={14} aria-hidden />
                    {t("worlds.end")}
                  </button>
                )}
                <button
                  aria-label={t("worlds.delete", { name: w.name })}
                  title={t("worlds.delete", { name: w.name })}
                  disabled={busy}
                  onClick={() => removeData(w, "all")}
                  className="icon-btn text-error"
                >
                  <Trash2 size={15} aria-hidden />
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
