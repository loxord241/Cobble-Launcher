// Внутри-приложенческий confirm вместо window.confirm. В Tauri v2 (wry/WebView2)
// window.confirm уходит в dialog-плагин, которого в приложении нет, и РЕЖЕТСЯ
// с «dialog.confirm not allowed. Command not found» — askConfirm ловил отказ и
// возвращал false, поэтому ВСЕ подтверждения молча не работали (репро владельца
// на удалении профиля, 2026-10-03; см. также D49: паттерн await остался).
// Сигнатура askConfirm прежняя — 8 мест вызова не менялись; добавился
// необязательный { danger, confirmLabel } для красной кнопки разрушительных действий.
// D64: askConfirm'ы ставятся в ОЧЕРЕДЬ (раньше новый confirm резолвил прежний
// false — при двух подряд Esc закрывал первый, а второй висел вечно).
import { create } from "zustand";
import { useEffect, useRef } from "react";
import { t } from "../i18n";

interface ConfirmRequest {
  message: string;
  detail: string | null;
  danger: boolean;
  confirmLabel: string | null;
  resolve: (v: boolean) => void;
}

interface ConfirmState {
  /** D64: очередь — подтверждения показываются по одному, следующие встают
   * после разрешения верхнего (раньше новый confirm резолвил прежний false,
   * и при двух askConfirm подряд Esc закрывал первый, а второй висел вечно). */
  queue: ConfirmRequest[];
}

const useConfirm = create<ConfirmState>(() => ({
  queue: [],
}));

export function askConfirm(
  message: string,
  opts?: { danger?: boolean; confirmLabel?: string; detail?: string },
): Promise<boolean> {
  return new Promise<boolean>((resolve) => {
    useConfirm.setState((s) => ({
      queue: [
        ...s.queue,
        {
          message,
          detail: opts?.detail ?? null,
          danger: opts?.danger ?? false,
          confirmLabel: opts?.confirmLabel ?? null,
          resolve,
        },
      ],
    }));
  });
}

export function ConfirmHost() {
  const queue = useConfirm((s) => s.queue);
  const current = queue[0] ?? null;
  const cancelRef = useRef<HTMLButtonElement>(null);
  const confirmRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!current) return;
    // Фокус на безопасном действии: у разрушительных — «Отмена» (Enter не
    // удалит случайно), у нейтральных — подтверждение.
    (current.danger ? cancelRef : confirmRef).current?.focus();
    // window+capture и stopPropagation: useModalA11y слушает keydown на
    // document в capture — Escape у confirm не должен закрыть и модалку под ним.
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        finish(false);
      } else if (e.key === "Tab") {
        // Ловушка фокуса: в диалоге ровно две кнопки.
        e.stopPropagation();
        e.preventDefault();
        const next = document.activeElement === cancelRef.current ? confirmRef : cancelRef;
        next.current?.focus();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
    // current — объект конкретного запроса: эффект (и замыкание finish)
    // переустанавливается при смене верхнего элемента очереди.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [current]);

  if (!current) return null;

  function finish(v: boolean) {
    current?.resolve(v);
    useConfirm.setState((s) => ({ queue: s.queue.slice(1) }));
  }

  return (
    <div
      className="anim-fade-in fixed inset-0 z-[60] flex items-center justify-center bg-black/60 p-4"
      onClick={(e) => {
        if (e.target === e.currentTarget) finish(false);
      }}
    >
      <div
        role="alertdialog"
        aria-modal="true"
        aria-label={current.message}
        className="anim-dialog-in w-[400px] max-w-full rounded-lg border border-border-app bg-card p-5 shadow-2xl"
      >
        <p className="text-sm leading-relaxed text-text">{current.message}</p>
        {current.detail && <p className="mt-1.5 text-xs leading-relaxed text-text-muted">{current.detail}</p>}
        <div className="mt-4 flex justify-end gap-2">
          <button
            ref={cancelRef}
            onClick={() => finish(false)}
            className="btn-ghost h-9 px-3 text-sm"
          >
            {t("common.cancel")}
          </button>
          <button
            ref={confirmRef}
            onClick={() => finish(true)}
            className={
              current.danger
                ? "h-9 rounded-md bg-error px-3 text-sm font-medium text-white hover:opacity-90"
                : "btn-primary h-9 px-3 text-sm"
            }
          >
            {current.confirmLabel ?? t("common.confirm")}
          </button>
        </div>
      </div>
    </div>
  );
}
