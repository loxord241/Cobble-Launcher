// Плитка инстанса «GD-библиотека» (D65, по макету redesign-mockups/gdlauncher.html,
// экран 2 «Мои инстансы», .tile.i): арт на всю плитку (своя иконка, панорама
// Minecraft или пиксель-пейзаж), бейдж версии слева-сверху, кебаб в правом
// углу, ПОСТОЯННАЯ нижняя плашка-вуаль с именем, последним запуском и кнопкой
// Play/Стоп — имя и кнопка видны ВСЕГДА, hover-зависимость запрещена владельцем
// (hover даёт только лёгкое затемнение/подъём). Клик по плитке открывает
// страницу инстанса; случайный запуск игры кликом в центр исключён.
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
    // Плитка .tile.i макета: фиксированная высота, спокойный hover (кайма +
    // мягкая тень, без «неона»); overflow-hidden не срезает меню кебаба —
    // оно рендерится порталом в body.
    <div className="group relative h-[300px] w-full overflow-hidden rounded-lg border border-border-app bg-card transition-all duration-200 hover:-translate-y-0.5 hover:border-accent/60 hover:shadow-lg hover:shadow-black/25">
      {/* Арт на всю плитку: большой арт модпака из галереи, своя иконка
          (размытая подложка + чёткая копия в натуральном размере) или
          процедурная «карта» / панорама от имени */}
      <div className="absolute inset-0 bg-surface-2">
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
      </div>

      {/* Затемнение под Play на hover/focus (как .playbtn макета). */}
      <div className="pointer-events-none absolute inset-0 bg-[rgb(7_10_15_/_0.26)] opacity-0 transition-opacity duration-150 group-hover:opacity-100 group-focus-within:opacity-100" />

      {/* Бейдж версии поверх арта, слева-сверху (как .vtag макета). */}
      <span className="chip-mono absolute left-2.5 top-2.5 bg-bg/85">{inst.mcVersion}</span>

      {/* Постоянная нижняя плашка-вуаль (НЕ hover): имя видно всегда —
          требование владельца; здесь же статус запуска, прогресс и Play/Стоп
          (D65-фикс: кнопка запуска в плашке, а не hover-кругом по центру —
          клик в центр плитки открывает страницу, случайный запуск исключён,
          и кнопка доступна без ховера, как и имя). */}
      <div className="absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/85 via-black/45 to-transparent px-3 pb-2.5 pt-10">
        <div className="flex items-end gap-3">
          <div className="min-w-0 flex-1">
            <div
              className="truncate text-[14px] font-semibold leading-tight text-white [text-shadow:0_2px_8px_rgb(0_0_0_/_0.6)]"
              title={inst.name}
            >
              {inst.name}
            </div>
            {/* Статус запуска инлайн в плашке, aria-live для скринридеров. */}
            <div aria-live="polite" className="truncate text-[12px] leading-snug text-white/80">
              {phase === "running" && (
                // Декоративная точка «игра идёт»: текст aria-live достаточен.
                <span
                  aria-hidden
                  className="mr-1.5 inline-block size-1.5 rounded-full bg-accent align-middle anim-dot-pulse"
                />
              )}
              {statusText}
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
            <div className="mt-0.5 truncate text-[11.5px] leading-snug text-white/70">
              {inst.lastPlayed
                ? t("instances.card.lastPlayed", {
                    when: new Date(inst.lastPlayed * 1000).toLocaleDateString(currentLanguage()),
                  })
                : t("instances.card.never")}
            </div>
          </div>
          {busy ? (
            <button
              onClick={callbacks.onStop}
              aria-label={`${t("instances.stop")} — ${inst.name}`}
              className="grid size-10 shrink-0 place-items-center rounded-xl bg-error text-on-danger shadow-lg shadow-black/30 transition-transform duration-200 hover:scale-105"
            >
              <Square size={18} aria-hidden fill="currentColor" />
            </button>
          ) : (
            <button
              onClick={callbacks.onLaunch}
              aria-label={`${t("play")} — ${inst.name}`}
              className="grid size-10 shrink-0 place-items-center rounded-full bg-accent text-on-accent shadow-lg shadow-black/30 transition-transform duration-200 hover:scale-105 hover:bg-accent-hover"
            >
              <Play size={17} aria-hidden fill="currentColor" strokeWidth={0} />
            </button>
          )}
        </div>
      </div>

      {/* Кебаб в правом верхнем углу (как .kebab макета): виден на hover и при
          таб-фокусе — без ховера доступен с клавиатуры; компонент не тронут,
          изменилось только расположение (меню — портал в body). */}
      <div className="absolute right-1.5 top-1.5 opacity-0 transition-opacity duration-150 group-hover:opacity-100 group-focus-within:opacity-100 [&_.icon-btn]:size-8 [&_.icon-btn]:bg-black/55 [&_.icon-btn]:text-white/85 [&_.icon-btn]:shadow-sm [&_.icon-btn:hover]:bg-black/75 [&_.icon-btn:hover]:text-white">
        <InstanceKebabMenu inst={inst} busy={busy} callbacks={callbacks} />
      </div>
    </div>
  );
}

// D8: карточка перерисовывается только при смене своих пропсов (inst/status/
// колбэки), а не на каждое событие стора.
export default memo(InstanceCard);
