import { useState } from "react";
import { X, Check } from "lucide-react";
import { useInstances } from "../../state/instances";
import { t } from "../../i18n";
import { useModalA11y } from "../hooks/useModalA11y";
import type { Instance } from "../../api/types";

export default function RenameInstanceModal({
  instance,
  onClose,
}: {
  instance: Instance;
  onClose: () => void;
}) {
  const [name, setName] = useState(instance.name);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { rename } = useInstances();
  // A33: Escape, ловушка фокуса и возврат фокуса на триггер (единый хук).
  // D41: requestClose — закрытие с анимацией (для ✕ и подложки).
  const { containerRef: dialogRef, requestClose } = useModalA11y(onClose, {
    initialFocus: "#rename-input",
  });

  const submit = async () => {
    const trimmed = name.trim();
    if (!trimmed) {
      setError(t("instances.rename.emptyError"));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await rename(instance.id, trimmed);
      onClose();
    } catch (e) {
      setError(String((e as { message?: string })?.message ?? e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className="anim-fade-in fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onClick={(e) => {
        if (e.target === e.currentTarget) requestClose();
      }}
    >
      {/* D41/F10: ref и anim-классы на карточке, не на оверлее — иначе
          .anim-closing схлопывает весь экран, а не только диалог. */}
      <div
        ref={dialogRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={t("instances.rename.title")}
        className="anim-dialog-in w-[420px] rounded-lg border border-border-app bg-card p-5 shadow-xl"
      >
        <div className="flex items-center justify-between border-b border-border-app pb-3">
          <h2 className="text-lg font-semibold text-text">{t("instances.rename.title")}</h2>
          <button
            onClick={requestClose}
            aria-label={t("common.close")}
            className="icon-btn text-text-muted hover:text-text"
          >
            <X size={18} aria-hidden />
          </button>
        </div>

        <form
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
          className="mt-4 flex flex-col gap-4"
        >
          <div>
            <label htmlFor="rename-input" className="block text-sm font-medium text-text">
              {t("instances.rename.label")}
            </label>
            <input
              id="rename-input"
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              autoFocus
              className="mt-1 h-10 w-full rounded-md border border-border-app bg-bg px-3 text-sm text-text outline-none focus:border-accent"
            />
          </div>

          {error && (
            <p className="rounded border border-error/40 bg-error/10 p-2 text-sm text-error" role="alert">
              {error}
            </p>
          )}

          <div className="flex justify-end gap-3 pt-2">
            <button
              type="button"
              onClick={onClose}
              className="btn-ghost h-10 px-4 text-sm"
            >
              {t("common.cancel")}
            </button>
            <button
              type="submit"
              disabled={busy || !name.trim()}
              className="btn-primary flex h-10 items-center gap-2 px-5 text-sm"
            >
              <Check size={16} aria-hidden />
              {t("common.save")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
