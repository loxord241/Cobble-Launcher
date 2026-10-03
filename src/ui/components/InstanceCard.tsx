// Карточка инстанса в стиле CurseForge (D35, по скрину владельца): арт
// сверху (своя иконка или процедурная «карта»), бейдж версии поверх арта,
// ниже — имя и подпись; Play всплывает на арте при наведении, кебаб — в ноге.
// aria-label кнопок прежние («Играть — …», «Остановить — …») — приёмки живут.
import { memo, useState } from "react";
import { Play, Square } from "lucide-react";
import { currentLanguage, t } from "../../i18n";
import type { Instance } from "../../api/types";
import type { LaunchStatus } from "../../state/instances";
import { defaultArtwork } from "../defaultArt";
import InstanceKebabMenu, { type KebabCallbacks } from "./InstanceKebabMenu";

function InstanceCard({
  inst,
  status,
  callbacks,
}: {
  inst: Instance;
  status?: LaunchStatus;
  callbacks: KebabCallbacks & {
    onLaunch: () => void;
    onStop: () => void;
  };
}) {
  const phase = status?.phase;
  const busy = phase !== undefined && phase !== "exited";

  // Статус запуска инлайн в подписи, aria-live для скринридеров.
  const statusText = !phase
    ? null
    : phase === "downloading"
      ? t("play.downloading", { done: status?.doneFiles ?? 0, total: status?.totalFiles ?? 0 })
      : phase === "running"
        ? t("play.running")
          : phase === "preparing" || phase === "launching"
            ? t("play.preparing")
            : null;

  // Полоса прогресса скачивания: доля файлов; вне фазы downloading — полосы нет.
  const downloadProgress =
    phase === "downloading" && (status?.totalFiles ?? 0) > 0
      ? Math.min((status?.doneFiles ?? 0) / (status?.totalFiles ?? 0), 1)
      : null;

  const sub = inst.loader
    ? `${inst.loader} ${inst.loaderVersion ?? ""}`
    : t("instances.loader.none");

  // Битая иконка (файл-протух, крошечная 1×1 и т.п.) → процедурный арт,
  // чтобы карточка не превращалась в пустой прямоугольник (D54).
  const [iconBroken, setIconBroken] = useState(false);
  const art = iconBroken || !inst.icon ? defaultArtwork(inst.name) : inst.icon;

  return (
    <div className="group flex h-full flex-col overflow-hidden rounded-lg border border-border-app bg-card transition-all duration-200 hover:border-accent/70 hover:shadow-lg hover:shadow-black/25">
      {/* Арт: своя иконка инстанса или процедурная «карта» от имени */}
      <div className="relative aspect-[4/3] w-full shrink-0 overflow-hidden bg-surface-2">
        <img
          src={art}
          onError={() => setIconBroken(true)}
          alt=""
          draggable={false}
          className="size-full object-cover"
        />
        {/* Бейдж версии поверх арта (как в CurseForge) */}
        <span className="chip-mono absolute right-1.5 top-1.5 bg-bg/85">{inst.mcVersion}</span>

        {/* Play поверх арта появляется на hover/focus; при запущенной игре —
            красный Стоп всегда виден (ловим клик мимо — открытие страницы). */}
        <div className="pointer-events-none absolute inset-0 bg-gradient-to-t from-black/35 to-transparent opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100" />
        {busy ? (
          <button
            onClick={callbacks.onStop}
            aria-label={`${t("instances.stop")} — ${inst.name}`}
            className="absolute inset-0 m-auto grid size-12 place-items-center rounded-full bg-error text-on-danger scale-90 shadow-lg transition-[opacity,transform,scale] duration-200 hover:opacity-90 group-hover:scale-100"
          >
            <Square size={18} aria-hidden fill="currentColor" />
          </button>
        ) : (
          <button
            onClick={callbacks.onLaunch}
            aria-label={`${t("play")} — ${inst.name}`}
            className="absolute inset-0 m-auto grid size-12 place-items-center rounded-full bg-accent text-on-accent scale-90 opacity-0 shadow-lg transition-[opacity,transform,scale] duration-200 hover:bg-accent-hover group-hover:scale-100 group-hover:opacity-100 group-focus-within:opacity-100 focus-visible:opacity-100"
          >
            <Play size={20} aria-hidden fill="currentColor" strokeWidth={0} />
          </button>
        )}
      </div>

      {/* Нога: имя + подпись слева, кебаб справа */}
      <div className="flex min-h-[56px] flex-1 items-start gap-1 p-2.5">
        <div className="min-w-0 flex-1">
          <div className="truncate text-[15px] font-semibold leading-tight" title={inst.name}>
            {inst.name}
          </div>
          <div className="truncate text-[12px] leading-snug text-text-muted" aria-live="polite">
            {phase === "running" && (
              // Декоративная точка «игра идёт»: текст aria-live достаточен.
              <span
                aria-hidden
                className="mr-1.5 inline-block size-1.5 rounded-full bg-accent align-middle anim-dot-pulse"
              />
            )}
            {sub}
            {statusText ? ` · ${statusText}` : ""}
          </div>
          {/* Прогресс скачивания: заполнение через scaleX (композитно, токен). */}
          {downloadProgress !== null && (
            <div className="mt-1 h-1 w-full overflow-hidden rounded-full bg-surface-2">
              <div
                className="h-full rounded-full bg-accent anim-progress-fill"
                style={{ transform: `scaleX(${downloadProgress})` }}
              />
            </div>
          )}
          {/* Критерий сортировки «По последнему запуску» должен быть виден. */}
          <div className="truncate text-[12px] leading-snug text-text-muted">
            {inst.lastPlayed
              ? t("instances.card.lastPlayed", {
                  when: new Date(inst.lastPlayed * 1000).toLocaleDateString(currentLanguage()),
                })
              : t("instances.card.never")}
          </div>
        </div>
        <InstanceKebabMenu inst={inst} busy={busy} callbacks={callbacks} />
      </div>
    </div>
  );
}

// D8: карточка перерисовывается только при смене своих пропсов (inst/status/
// колбэки), а не на каждое событие стора.
export default memo(InstanceCard);
