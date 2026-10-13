// F2 «Drag & drop»: перетаскивание файла на окно лаунчера устанавливает его.
// WebView2 не отдаёт File.path из DataTransfer, поэтому пути берём из
// нативного события Tauri — getCurrentWebviewWindow().onDragDropEvent:
// 'enter'/'drop' несут paths[], 'over' — только позицию курсора, 'leave' —
// перетаскивание ушло или отменено. Маршрутизация по расширению
// (регистронезависимо): .mrpack → mrpack_install сразу (прогресс виден в
// рельсе «Загрузки», инстанс появляется сам); .jar (мод) и .zip (ресурспак
// или шейдер) → модалка выбора инстанса + content_install_local; остальное —
// честное «файл не поддерживается» на 2 секунды.
import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  PackageOpen,
  LoaderCircle,
  Check,
  AlertTriangle,
  X,
} from "lucide-react";
import { t, apiErrorText } from "../../i18n";
import { api } from "../../api/client";
import { useInstances } from "../../state/instances";
import { useDropUI } from "../../state/drop";
import type { Instance } from "../../api/types";
import { useModalA11y } from "../hooks/useModalA11y";

/** Маршрутизация по расширению (регистронезависимо). */
const MRPACK_RE = /\.mrpack$/i;
const CONTENT_RE = /\.(jar|zip)$/i;

/** Сколько живут всплывающие статусы overlay, мс. */
const UNSUPPORTED_MS = 2000;
const RESULT_MS = 3000;

const IDLE: Phase = { kind: "idle" };

/** Машина состояний компонента: idle → drag (над окном) → drop →
 * progress (ставится .mrpack) / pick (модалка выбора инстанса) / flash
 * (краткий результат: успех, ошибка или «не поддерживается»). */
type Phase =
  | { kind: "idle" }
  | { kind: "drag" }
  | { kind: "progress"; text: string }
  | { kind: "flash"; tone: "ok" | "warn" | "error"; text: string }
  | { kind: "pick"; paths: string[] };

/** Имя файла из полного пути (разделители / и \ — путь приходит из ОС). */
function fileName(path: string): string {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return i >= 0 ? path.slice(i + 1) : path;
}

export default function DropInstall() {
  // Список инстансов для модалки выбора — узким селектором, как в Shell.
  const instances = useInstances((s) => s.list);
  const loadInstances = useInstances((s) => s.load);
  const [phase, setPhase] = useState<Phase>(IDLE);
  // Зеркало phase для обработчиков нативных событий (замыкание эффекта
  // подписки не пересоздаётся на каждый рендер — читаем актуальное через ref).
  const phaseRef = useRef(phase);
  phaseRef.current = phase;
  const timerRef = useRef<number | null>(null);

  const clearTimer = useCallback(() => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  useEffect(() => clearTimer, [clearTimer]);

  // Занятость UI (модалка/прогресс) — в общий стор: Shell прячет палитру.
  const setDropBusy = useDropUI((s) => s.set);
  useEffect(() => {
    setDropBusy(phase.kind === "pick" || phase.kind === "progress");
  }, [phase.kind, setDropBusy]);

  /** Краткий статус в overlay: через ms мс вернуться в idle. */
  const flash = useCallback(
    (tone: "ok" | "warn" | "error", text: string, ms: number) => {
      clearTimer();
      setPhase({ kind: "flash", tone, text });
      timerRef.current = window.setTimeout(() => {
        timerRef.current = null;
        setPhase(IDLE);
      }, ms);
    },
    [clearTimer],
  );

  /** Установка .mrpack: та же ядерная команда, что и из каталога модпаков.
   * Подтверждение не нужно — прогресс виден в рельсе «Загрузки», новый
   * инстанс создаётся сам (стор перечитываем после установки).
   * D71: возвращает успех явным значением — решение о показе pick для
   * смешанного drop принимается по ИСХОДУ промиса, а не по фазе (phaseRef
   * обновляется при рендере, и микротаска .then читала устаревшее значение). */
  const installMrpack = useCallback(
    async (paths: string[]): Promise<boolean> => {
      setPhase({ kind: "progress", text: t("drop.newPackInstall") });
      try {
        for (const path of paths) {
          await api.mrpackInstall(path);
          void loadInstances(); // новый инстанс — обновляем список и рельс
        }
        flash(
          "ok",
          t("drop.installed", {
            name:
              paths.length === 1 ? fileName(paths[0]) : String(paths.length),
          }),
          RESULT_MS,
        );
        return true;
      } catch (e) {
        flash("error", apiErrorText(e), RESULT_MS);
        return false;
      }
    },
    [flash, loadInstances],
  );

  /** Drop: маршрутизация по расширениям одного перетаскивания. Пока идёт
   * установка или открыт выбор инстанса — новые drop'ы игнорируем
   * (flash-статус можно перебить — он мгновенный). */
  const handleDrop = useCallback(
    (paths: string[]) => {
      // D69-ревизия (P0): в момент drop фаза почти всегда "drag" — пустой
      // гард killed всю фичу; блокируем только ЗАНЯТЫЕ состояния.
      const cur = phaseRef.current;
      if (cur.kind === "progress" || cur.kind === "pick") return;
      const mrpack = paths.filter((p) => MRPACK_RE.test(p));
      const content = paths.filter((p) => CONTENT_RE.test(p));
      if (mrpack.length > 0) {
        // Смешанный drop честен: .mrpack ставим, .jar/.zip в этом же
        // перетаскивании не теряем молча — предлагаем выбор инстанса после.
        const alsoContent = content.length > 0;
        // D71: pick открываем ТОЛЬКО по успешному исходу промиса (не по фазе:
        // phaseRef обновляется при рендере, и микротаска .then исполнялась
        // раньше — условие ложно, .jar/.zip терялись). Ошибка установки
        // остаётся во flash — pick не открываем.
        void installMrpack(mrpack).then((ok: boolean) => {
          if (alsoContent && ok) {
            setPhase({ kind: "pick", paths: content });
          }
        });
        return;
      }
      if (content.length > 0) {
        setPhase({ kind: "pick", paths: content });
        return;
      }
      flash("warn", t("drop.unsupported"), UNSUPPORTED_MS);
    },
    [flash, installMrpack],
  );

  // Подписка на нативный drag-drop Tauri. 'over' ничего не меняет (курсор
  // двигается внутри уже показанного overlay); overlay показываем только
  // когда среди файлов есть поддерживаемые.
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    let disposed = false;
    void getCurrentWebviewWindow()
      .onDragDropEvent((event) => {
        const p = event.payload;
        if (p.type === "enter") {
          if (p.paths.some((path) => MRPACK_RE.test(path) || CONTENT_RE.test(path))) {
            setPhase((prev) => (prev.kind === "idle" ? { kind: "drag" } : prev));
          }
        } else if (p.type === "leave") {
          setPhase((prev) => (prev.kind === "drag" ? IDLE : prev));
        } else if (p.type === "drop") {
          handleDrop(p.paths);
        }
      })
      .then((un) => {
        if (disposed) un();
        else unlisten = un;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [handleDrop]);

  if (phase.kind === "idle") return null;

  if (phase.kind === "pick") {
    return (
      <InstancePickModal
        paths={phase.paths}
        instances={instances}
        onClose={() => {
          clearTimer();
          setPhase(IDLE);
        }}
        onInstalled={() =>
          flash(
            "ok",
            t("drop.installed", {
              name:
                phaseRef.current.kind === "pick" && phaseRef.current.paths.length === 1
                  ? fileName(phaseRef.current.paths[0])
                  : String(
                      phaseRef.current.kind === "pick" ? phaseRef.current.paths.length : 1,
                    ),
            }),
            RESULT_MS,
          )
        }
      />
    );
  }

  // Полноэкранный overlay (drag-over / прогресс / результат): pointer-events-
  // none — не перехватывает мышь, drop доходит до нативного обработчика.
  return (
    <div className="anim-fade-in pointer-events-none fixed inset-0 z-[60] flex items-center justify-center bg-black/60">
      <div className="flex flex-col items-center gap-3 rounded-xl border border-border-app bg-card px-8 py-6 shadow-2xl">
        {phase.kind === "drag" && (
          <PackageOpen size={40} strokeWidth={1.5} aria-hidden className="text-accent" />
        )}
        {phase.kind === "progress" && (
          <LoaderCircle size={40} aria-hidden className="animate-spin text-accent" />
        )}
        {phase.kind === "flash" && phase.tone === "ok" && (
          <Check size={40} strokeWidth={1.5} aria-hidden className="text-success" />
        )}
        {phase.kind === "flash" && phase.tone === "warn" && (
          <AlertTriangle size={40} strokeWidth={1.5} aria-hidden className="text-warning" />
        )}
        {phase.kind === "flash" && phase.tone === "error" && (
          <AlertTriangle size={40} strokeWidth={1.5} aria-hidden className="text-error" />
        )}
        <p className="text-sm font-semibold text-text" role="status">
          {phase.kind === "drag" && t("drop.overlay")}
          {phase.kind === "progress" && phase.text}
          {phase.kind === "flash" && phase.text}
        </p>
      </div>
    </div>
  );
}

/** Модалка выбора инстанса для .jar/.zip (моды, ресурспаки, шейдеры):
 * ряд — существующий инстанс, клик ставит файл в него; пустой список —
 * честное «нет инстансов» с кнопкой закрыть. */
function InstancePickModal({
  paths,
  instances,
  onClose,
  onInstalled,
}: {
  paths: string[];
  instances: Instance[];
  onClose: () => void;
  onInstalled: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const busyRef = useRef(false);
  // A33: Escape, ловушка фокуса, возврат фокуса. Во время установки закрытие
  // блокируем (как попап обновления в Shell) — не сорвать установку.
  // canClose (D69-ревизия): блокировка ДО анимации — guard в onClose после
  // схлопывания оставлял модалку «кирпичом» при ошибке установки.
  const { containerRef: dialogRef, requestClose } = useModalA11y(onClose, {
    canClose: () => !busyRef.current,
  });

  const install = async (instance: Instance) => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      for (const path of paths) {
        await api.contentInstallLocal(instance.id, path);
      }
      onInstalled(); // родитель переключает фазу — модалка размонтируется
    } catch (e) {
      setError(apiErrorText(e)); // честный текст ошибки, модалка остаётся
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  // До 3 имён файлов, остальные — «+N» (число не требует перевода).
  const names = paths.map(fileName);

  return (
    <div
      className="anim-fade-in fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onClick={(e) => {
        if (e.target === e.currentTarget) requestClose();
      }}
    >
      {/* D41/F10: ref и anim-классы на карточке — .anim-closing схлопывает
          диалог, а не весь экран. */}
      <div
        ref={dialogRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={t("drop.pickTitle")}
        aria-busy={busy}
        className="anim-dialog-in w-[420px] max-w-full rounded-lg border border-border-app bg-card p-5 shadow-xl"
      >
        <div className="flex items-center justify-between border-b border-border-app pb-3">
          <h2 className="text-lg font-semibold text-text">{t("drop.pickTitle")}</h2>
          <button
            onClick={requestClose}
            aria-label={t("common.close")}
            className="icon-btn text-text-muted hover:text-text"
          >
            <X size={18} aria-hidden />
          </button>
        </div>

        <ul className="mt-3 flex flex-col gap-0.5">
          {names.slice(0, 3).map((name, i) => (
            <li
              key={`${name}-${i}`}
              title={name}
              className="truncate font-mono text-xs text-text-muted"
            >
              {name}
            </li>
          ))}
          {names.length > 3 && (
            <li className="font-mono text-xs text-text-muted">+{names.length - 3}</li>
          )}
        </ul>

        {instances.length > 0 ? (
          <div className="mt-3 max-h-64 overflow-y-auto overscroll-contain rounded-lg border border-border-app">
            {instances.map((inst) => (
              <button
                key={inst.id}
                onClick={() => void install(inst)}
                disabled={busy}
                className="flex h-11 w-full items-center gap-2 border-b border-border-app px-3 text-left text-sm last:border-b-0 hover:bg-surface-2 disabled:cursor-not-allowed disabled:opacity-50"
              >
                <span className="truncate font-semibold text-text">{inst.name}</span>
                <span className="ml-auto flex-none font-mono text-xs text-text-muted">
                  {inst.mcVersion}
                  {inst.loader ? ` · ${inst.loader}` : ""}
                </span>
              </button>
            ))}
          </div>
        ) : (
          // Пустой список — честное «нет инстансов», без выдуманных пунктов.
          <p className="mt-4 text-sm leading-relaxed text-text-muted">
            {t("instances.empty.title")}
          </p>
        )}

        {error && (
          <p role="alert" className="mt-3 rounded border border-error/40 bg-error/10 p-2 text-sm text-error">
            {error}
          </p>
        )}

        <div className="mt-4 flex justify-end">
          <button
            onClick={requestClose}
            disabled={busy}
            className="btn-ghost h-10 px-4 text-sm disabled:cursor-not-allowed disabled:opacity-50"
          >
            {t("drop.cancel")}
          </button>
        </div>
      </div>
    </div>
  );
}
