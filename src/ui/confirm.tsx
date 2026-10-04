// Внутри-приложенческий confirm вместо window.confirm. В Tauri v2 (wry/WebView2)
// window.confirm уходит в dialog-плагин, которого в приложении нет, и РЕЖЕТСЯ
// с «dialog.confirm not allowed. Command not found» — askConfirm ловил отказ и
// возвращал false, поэтому ВСЕ подтверждения молча не работали (репро владельца
// на удалении профиля, 2026-10-03; см. также D49: паттерн await остался).
// Сигнатура askConfirm прежняя — 8 мест вызова не менялись; добавился
// необязательный { danger, confirmLabel } для красной кнопки разрушительных действий.
import { create } from "zustand";
import { useEffect, useRef } from "react";
import { t } from "../i18n";

interface ConfirmState {
  open: boolean;
  message: string;
  detail: string | null;
  danger: boolean;
  confirmLabel: string | null;
  resolve: ((v: boolean) => void) | null;
}

const useConfirm = create<ConfirmState>(() => ({
  open: false,
  message: "",
  detail: null,
  danger: false,
  confirmLabel: null,
  resolve: null,
}));

export function askConfirm(
  message: string,
  opts?: { danger?: boolean; confirmLabel?: string; detail?: string },
): Promise<boolean> {
  // Новый confirm отменяет незакрытый прежний (Promise предыдущего = false).
  useConfirm.getState().resolve?.(false);
  return new Promise<boolean>((resolve) => {
    useConfirm.setState({
      open: true,
      message,
      detail: opts?.detail ?? null,
      danger: opts?.danger ?? false,
      confirmLabel: opts?.confirmLabel ?? null,
      resolve,
    });
  });
}

export function ConfirmHost() {
  const { open, message, detail, danger, confirmLabel, resolve } = useConfirm();
  const cancelRef = useRef<HTMLButtonElement>(null);
  const confirmRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    // Фокус на безопасном действии: у разрушительных — «Отмена» (Enter не
    // удалит случайно), у нейтральных — подтверждение.
    (danger ? cancelRef : confirmRef).current?.focus();
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, danger]);

  if (!open) return null;

  function finish(v: boolean) {
    resolve?.(v);
    useConfirm.setState({ open: false, resolve: null });
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
        aria-label={message}
        className="anim-dialog-in w-[400px] max-w-full rounded-lg border border-border-app bg-card p-5 shadow-2xl"
      >
        <p className="text-sm leading-relaxed text-text">{message}</p>
        {detail && <p className="mt-1.5 text-xs leading-relaxed text-text-muted">{detail}</p>}
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
              danger
                ? "h-9 rounded-md bg-error px-3 text-sm font-medium text-white hover:opacity-90"
                : "btn-primary h-9 px-3 text-sm"
            }
          >
            {confirmLabel ?? t("common.confirm")}
          </button>
        </div>
      </div>
    </div>
  );
}
