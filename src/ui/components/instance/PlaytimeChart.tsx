// D67: график наигранных часов инстанса за 14 дней на вкладке «Обзор».
// Чистый SVG без библиотек графиков (новые зависимости запрещены): столбики —
// холодный акцент из tokens.css (утилита fill-accent), день без игры —
// крошечный след на оси, под осью — редкие подписи «дд.мм» (каждый 2-й день,
// самая свежая дата подписана). Числа — tabular-nums, как в остальных местах
// панели; своей анимации нет — вкладка «Обзор» появляется целиком через
// anim-fade-up. Данные — api.playtimeStats (нулевые дни ядро включает сам);
// перечитываем после завершения игровой сессии тем же механизмом, которым
// страница узнаёт о конце игры, — phase из стора useInstances.
import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../../../api/client";
import type { PlaytimeDay } from "../../../api/types";
import { t } from "../../../i18n";
import { useInstances } from "../../../state/instances";

const DAYS = 14;
/** Геометрия (px): базлайн-ось, базовая линия подписей и полная высота SVG. */
const BASELINE = 92;
const LABEL_Y = 110;
const SVG_H = 116;

/** «05.03» из «2026-03-05»: формат дд.мм без локальных сюрпризов. */
function dayLabel(iso: string): string {
  return `${iso.slice(8, 10)}.${iso.slice(5, 7)}`;
}

/** Часы с одним знаком (3.7) — и для итога окна, и в тултипах столбиков. */
function hours1(secs: number): string {
  return (secs / 3600).toFixed(1);
}

export default function PlaytimeChart({ instanceId }: { instanceId: string }) {
  const [days, setDays] = useState<PlaytimeDay[] | null>(null);

  const reload = useCallback(() => {
    api
      .playtimeStats(instanceId, DAYS)
      .then(setDays)
      // Статистика не прочиталась — тихая пустая подпись вместо сломанного вида.
      .catch(() => setDays([]));
  }, [instanceId]);

  // Секция живёт на вкладке «Обзор» — грузим при монтировании.
  useEffect(() => {
    reload();
  }, [reload]);

  // Игра завершилась (phase → exited): ядро дописал статистику — перечитываем.
  const phase = useInstances((s) => s.status[instanceId]?.phase);
  const prevPhase = useRef(phase);
  useEffect(() => {
    if (phase === "exited" && prevPhase.current !== "exited") reload();
    prevPhase.current = phase;
  }, [phase, reload]);

  if (days === null) return null;

  const totalSecs = days.reduce((acc, d) => acc + d.secs, 0);
  const empty = days.length === 0 || totalSecs === 0;

  return (
    <section className="rounded-lg border border-border-app bg-card p-4">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <h3 className="text-sm font-semibold">{t("instances.stats.playtimeTitle")}</h3>
        {!empty && (
          <span className="text-[12px] text-text-muted tabular-nums">
            {t("instances.stats.playtimeWindow", { n: days.length })}
            {" · "}
            <span className="font-semibold text-text">
              {t("instances.stats.playtimeHours", { n: hours1(totalSecs) })}
            </span>
          </span>
        )}
      </div>

      {empty ? (
        <p className="mt-2 text-[13px] text-text-muted">{t("instances.stats.playtimeEmpty")}</p>
      ) : (
        <Chart days={days} totalSecs={totalSecs} />
      )}
    </section>
  );
}

function Chart({ days, totalSecs }: { days: PlaytimeDay[]; totalSecs: number }) {
  const max = Math.max(...days.map((d) => d.secs), 1);
  const slot = 100 / days.length; // доля одного дня в % ширины карточки
  const barW = slot * 0.62;
  return (
    <svg
      role="img"
      aria-label={`${t("instances.stats.playtimeWindow", { n: days.length })}: ${t(
        "instances.stats.playtimeHours",
        { n: hours1(totalSecs) },
      )}`}
      className="mt-2 w-full"
      height={SVG_H}
    >
      {/* Ось во всю ширину карточки. */}
      <line
        x1="0"
        y1={BASELINE + 0.5}
        x2="100%"
        y2={BASELINE + 0.5}
        className="stroke-border-app"
        strokeWidth={1}
      />
      {days.map((d, i) => {
        const x = i * slot + (slot - barW) / 2;
        const label = `${dayLabel(d.date)} · ${t("instances.stats.playtimeHours", {
          n: hours1(d.secs),
        })}`;
        const h =
          d.secs > 0 ? Math.max(3, Math.round((d.secs / max) * (BASELINE - 8))) : 0;
        return (
          <g key={d.date}>
            {/* Нативный тултип SVG (<title> — родной механизм SVG вместо
                HTML-атрибута title, который React для SVG-элементов не даёт). */}
            <title>{label}</title>
            {/* Невидимая зона наведения во всю высоту — тултип ловится всей колонкой. */}
            <rect
              x={`${(i * slot).toFixed(3)}%`}
              y={0}
              width={`${slot.toFixed(3)}%`}
              height={BASELINE}
              fill="transparent"
            />
            {d.secs > 0 ? (
              <rect
                x={`${x.toFixed(3)}%`}
                y={BASELINE - h}
                width={`${barW.toFixed(3)}%`}
                height={h}
                rx={2}
                className="fill-accent"
              />
            ) : (
              // День без игры — крошечный след на оси (не путать с игровым днём).
              <rect
                x={`${x.toFixed(3)}%`}
                y={BASELINE - 2}
                width={`${barW.toFixed(3)}%`}
                height={2}
                rx={1}
                className="fill-accent opacity-25"
              />
            )}
            {/* Редкие подписи: каждый 2-й день, последняя — самый свежий. */}
            {i % 2 === 1 && (
              <text
                x={`${((i + 0.5) * slot).toFixed(3)}%`}
                y={LABEL_Y}
                textAnchor="middle"
                className="fill-text-muted text-[10px] tabular-nums"
              >
                {dayLabel(d.date)}
              </text>
            )}
          </g>
        );
      })}
    </svg>
  );
}
