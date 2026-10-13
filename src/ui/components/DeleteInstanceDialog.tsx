// Диалог удаления инстанса «Сланца» (D7): общий для Главной и страницы
// Инстансов. Выбор честный — «в корзину» (обратимо) или «навсегда» с данными.
import { X } from "lucide-react";
import { t } from "../../i18n";
import { useModalA11y } from "../hooks/useModalA11y";
import type { Instance } from "../../api/types";

export default function DeleteInstanceDialog({
  instance,
  onCancel,
  onConfirm,
}: {
  instance: Instance;
  onCancel: () => void;
  /** wipe=true — удалить навсегда вместе с данными; false — в корзину ОС. */
  onConfirm: (wipe: boolean) => void;
}) {
  // A33/A5.4: Escape, ловушка фокуса и возврат фокуса на кебаб-триггер.
  // D41: requestClose — закрытие с анимацией (для ✕ и подложки).
  const { containerRef, requestClose } = useModalA11y(onCancel);

  return (
    <div
      className="anim-fade-in fixed inset-0 z-50 grid place-items-center bg-black/50 p-4"
      onClick={requestClose}
    >
      <div
        ref={containerRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={t("instances.delete.title")}
        className="anim-dialog-in w-[440px] rounded-lg border border-border-app bg-card p-5 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-start justify-between gap-3">
          <h3 className="text-[15px] font-semibold">{t("instances.delete.title")}</h3>
          <button
            onClick={requestClose}
            aria-label={t("common.close")}
            className="icon-btn text-text-muted"
          >
            <X size={16} aria-hidden />
          </button>
        </div>
        <p className="mt-2 text-[13px] text-text-muted">
          {t("instances.delete.desc", { name: instance.name })}
        </p>
        <div className="mt-5 flex flex-col gap-2">
          <button
            onClick={() => onConfirm(false)}
            className="btn-ghost btn-sm h-10 justify-center"
          >
            {t("instances.delete.trash")}
          </button>
          <button
            onClick={() => onConfirm(true)}
            className="h-10 justify-center rounded-lg border border-error/50 px-3 text-sm font-medium text-error transition-colors hover:bg-error/10"
          >
            {t("instances.delete.forever")}
          </button>
          <button
            onClick={onCancel}
            className="btn-ghost btn-sm h-10 justify-center text-text-muted"
          >
            {t("common.cancel")}
          </button>
        </div>
      </div>
    </div>
  );
}
