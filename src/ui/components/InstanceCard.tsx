// Карточка инстанса в стиле CurseForge (D35, по скрину владельца): арт
// сверху (своя иконка, панорама Minecraft или пиксель-пейзаж), бейдж версии
// поверх арта, ниже — имя и подпись; Play всплывает на арте при наведении.
// aria-label кнопок прежние («Играть — …», «Остановить — …») — приёмки живут.
import { memo, useEffect, useState } from "react";
import { Play, Square } from "lucide-react";
import { currentLanguage, t } from "../../i18n";
import type { Instance } from "../../api/types";
import type { LaunchStatus } from "../../state/instances";
import { defaultArtwork } from "../defaultArt";
import { instancePanorama } from "../panorama";
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
  // Новая иконка могла быть валидной — флаг протухания сбрасываем.
  useEffect(() => setIconBroken(false), [inst.icon]);
  // D64: битый арт из галереи пака не должен оставлять дыру в карточке —
  // цепочка продолжается (иконка → панорама/процедурный арт).
  const [artBroken, setArtBroken] = useState(false);
  useEffect(() => setArtBroken(false), [inst.art]);

  // D63: панорама из собственных файлов игры — пока грузится/её нет,
  // показывается пиксель-пейзаж. D64: грузим не только при отсутствии
  // арта/иконки, но и когда они битые — иначе панораме нет шанса.
  // Второй аргумент — версия MC (ядро подбирает панораму своей эпохи).
  const [pano, setPano] = useState<string | null>(null);
  const fallbackArt = (!inst.art || artBroken) && (!inst.icon || iconBroken);
  useEffect(() => {
    let alive = true;
    setPano(null);
    if (fallbackArt) {
      instancePanorama(inst.id, inst.mcVersion).then((url) => {
        if (alive) setPano(url);
      });
    }
    return () => {
      alive = false;
    };
  }, [inst.id, inst.mcVersion, fallbackArt]);

  return (
    <div className="group flex h-full flex-col overflow-hidden rounded-lg border border-border-app bg-card transition-all duration-200 hover:border-accent/70 hover:shadow-lg hover:shadow-black/25">
      {/* Арт: большой арт модпака из галереи, своя иконка (размытая подложка +
          чёткая копия в натуральном размере) или процедурная «карта» от имени */}
      <div className="relative aspect-[4/3] w-full shrink-0 overflow-hidden bg-surface-2">
        {inst.art && !artBroken ? (
          // Арт из галереи пака: полноценное изображение на всю область;
          // битое — падаем дальше по цепочке (иконка → панорама/процедурный).
          <img
            src={inst.art}
            alt=""
            draggable={false}
            onError={() => setArtBroken(true)}
            className="size-full object-cover"
          />
        ) : inst.icon && !iconBroken ? (
          <>
            {/* Иконка Modrinth — 96×96: тянуть её на всю область нельзя (каша).
                «Ambient»-стиль: размытая подложка — div с background, НЕ <img>
                (у трансформированной картинки transformed-rect вылезает за
                карточку, и клики по координатам попадают в зазор сетки —
                грабли приёмок), сверху чёткая копия 96px по центру. */}
            <div
              aria-hidden
              className="absolute inset-0 scale-125 bg-cover bg-center blur-lg brightness-[.35]"
              style={{ backgroundImage: `url(${inst.icon})` }}
            />
            <img
              src={inst.icon}
              onError={() => setIconBroken(true)}
              alt=""
              draggable={false}
              className="absolute inset-0 m-auto size-24 object-contain"
            />
          </>
        ) : (
          <img
            src={pano ?? defaultArtwork(inst.id || inst.name)}
            alt=""
            draggable={false}
            className="size-full object-cover"
          />
        )}
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
