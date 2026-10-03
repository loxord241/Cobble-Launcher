// Таб «Скриншоты» страницы инстанса (F3, D37-B): превью png из
// minecraft/screenshots/, размер и удаление. Компонент монтируется только
// на своём табе — данные грузятся лениво, как «Миры». Ошибки показывает
// страница (единый баннер сверху), сюда передаётся колбэк.
// Миниатюру ядро отдаёт только для файлов до 300 КБ — большие карточки
// показывают серый плейсхолдер с иконкой.
import { useCallback, useEffect, useState } from "react";
import { Folder, Image, Trash2 } from "lucide-react";
import { api } from "../../../api/client";
import { formatSize } from "../../format";
import { apiErrorText, t } from "../../../i18n";
import type { ShotInfo } from "../../../api/types";

export default function ScreensTab({
  instanceId,
  setError,
}: {
  instanceId: string;
  setError: (msg: string | null) => void;
}) {
  const [shots, setShots] = useState<ShotInfo[] | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      setShots(await api.instanceScreens(instanceId));
    } catch (e) {
      setShots([]);
      setError(apiErrorText(e));
    }
  }, [instanceId, setError]);

  // Ленивая загрузка при активации таба (как «Миры»).
  useEffect(() => {
    void load();
  }, [load]);

  // Открыть папку скриншотов в проводнике (большие файлы иначе не посмотреть).
  const openFolder = () => {
    void api.instanceScreensOpen(instanceId).catch((e) => setError(apiErrorText(e)));
  };

  const remove = (s: ShotInfo) => {
    void (async () => {
      setBusy(true);
      setError(null);
      try {
        await api.screenshotDelete(instanceId, s.name);
        await load();
      } catch (e) {
        setError(apiErrorText(e));
      } finally {
        setBusy(false);
      }
    })();
  };

  return (
    <div className="rounded-lg border border-border-app bg-card p-3">
      {shots === null ? null : shots.length === 0 ? (
        <p className="px-2 py-6 text-center text-[13px] text-text-muted">{t("screens.empty")}</p>
      ) : (
        <>
          <div className="mb-2 flex items-center gap-2">
            <span className="chip-mono">{shots.length}</span>
            <button onClick={openFolder} className="btn-ghost btn-sm ml-auto">
              <Folder size={14} aria-hidden />
              {t("screens.open")}
            </button>
          </div>
          <div className="grid grid-cols-[repeat(auto-fill,minmax(160px,1fr))] gap-2">
            {shots.map((s) => (
              <div
                key={s.name}
                className="group overflow-hidden rounded-md border border-border-app transition-colors hover:border-accent/60"
              >
                <div
                  className={`flex aspect-video items-center justify-center bg-bg${s.thumb ? "" : " cursor-pointer"}`}
                  onClick={s.thumb ? undefined : openFolder}
                  title={s.thumb ? undefined : t("screens.open")}
                >
                  {s.thumb ? (
                    <img
                      src={s.thumb}
                      alt={s.name}
                      loading="lazy"
                      className="h-full w-full object-contain"
                    />
                  ) : (
                    <Image size={24} className="text-text-muted" aria-hidden />
                  )}
                </div>
                <div className="flex items-center justify-between gap-1 px-2 py-1.5">
                  <div className="min-w-0">
                    <div className="truncate text-[12px] font-medium leading-tight" title={s.name}>
                      {s.name}
                    </div>
                    <div className="font-mono text-[11px] text-text-muted">{formatSize(s.bytes)}</div>
                  </div>
                  <button
                    aria-label={t("screens.delete", { name: s.name })}
                    title={t("screens.delete", { name: s.name })}
                    disabled={busy}
                    onClick={() => remove(s)}
                    className="icon-btn shrink-0 text-error opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100 focus-visible:opacity-100"
                  >
                    <Trash2 size={14} aria-hidden />
                  </button>
                </div>
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
