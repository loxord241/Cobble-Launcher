// Диалог «Скопировать в другой инстанс» страницы инстанса (F10), вынесен из
// InstanceDetailPage ради ловушки фокуса (F13-аудит): useModalA11y даёт Escape,
// Tab-цикл внутри диалога и возврат фокуса на кнопку-триггер. Логики копирования
// здесь нет: диалог только выбирает целевой инстанс и зовёт onPick(toId) —
// API-вызов и уведомления остаются на странице.
import { t } from "../../../i18n";
import { fileBase } from "../../pages/InstanceDetailPage";
import { useModalA11y } from "../../hooks/useModalA11y";
import type { Instance } from "../../../api/types";

export default function CopyContentDialog({
  open,
  file,
  targets,
  busy,
  onPick,
  onClose,
}: {
  open: boolean;
  /** Логический путь копируемого файла; null — диалог не открыт. */
  file: string | null;
  /** Цели для копирования — все инстансы, кроме текущего. */
  targets: Instance[];
  busy: boolean;
  /** Выбрана цель; саму копию выполняет страница. */
  onPick: (toId: string) => void;
  onClose: () => void;
}) {
  // A33/D41: Escape и клик-мимо закрывают с анимацией, фокус возвращается на триггер.
  const { containerRef, requestClose } = useModalA11y(onClose);

  if (!open || file === null) return null;

  return (
    <div
      className="anim-fade-in fixed inset-0 z-50 grid place-items-center bg-black/60 p-4"
      onClick={requestClose}
    >
      <div
        ref={containerRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={t("mods.copy.choose", { file: fileBase(file) })}
        className="anim-dialog-in flex max-h-[70vh] w-[420px] max-w-full flex-col rounded-lg border border-border-app bg-card p-4 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="text-sm font-semibold">
          {t("mods.copy.choose", { file: fileBase(file) })}
        </div>
        <div className="mt-3 flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto">
          {targets.map((i) => (
            <button
              key={i.id}
              disabled={busy}
              onClick={() => onPick(i.id)}
              className="flex h-10 items-center gap-2 rounded-md border border-border-app px-3 text-left text-sm hover:border-accent/60 hover:bg-card-hover disabled:opacity-50"
            >
              <span className="truncate font-semibold">{i.name}</span>
              <span className="chip-mono ml-auto">{i.mcVersion}</span>
            </button>
          ))}
          {targets.length === 0 && (
            <p className="px-1 py-3 text-[13px] text-text-muted">{t("instances.empty.title")}</p>
          )}
        </div>
        <div className="mt-3 flex justify-end border-t border-border-app pt-2">
          <button onClick={requestClose} className="btn-ghost btn-sm">
            {t("common.close")}
          </button>
        </div>
      </div>
    </div>
  );
}
