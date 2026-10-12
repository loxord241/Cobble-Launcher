// Модалка «Переместить в группу…» (D70): название группы с автодополнением
// по уже существующим группам (datalist), «Готово» — применить, «Убрать из
// группы» — снять (показывается только когда группа есть). Паттерн —
// RenameInstanceModal (useModalA11y, anim-классы, btn-primary/btn-ghost).
import { useState } from "react";
import { X, Check, FolderMinus } from "lucide-react";
import { useInstances } from "../../state/instances";
import { t, apiErrorText } from "../../i18n";
import { useModalA11y } from "../hooks/useModalA11y";
import type { Instance } from "../../api/types";

// D70: лимит длины группы — зеркало MAX_GROUP_LEN в src-tauri
// (instances/mod.rs). Ядро валидирует то же значение на IPC-границе.
const MAX_GROUP_LEN = 64;

export default function InstanceGroupModal({
  instance,
  groups,
  onClose,
  onSaved,
}: {
  instance: Instance;
  /** Существующие группы (для datalist) — страница собирает из списка инстансов. */
  groups: string[];
  onClose: () => void;
  /** Вызывается после успешного setGroup (уведомление показывает страница). */
  onSaved: () => void;
}) {
  const [group, setGroup] = useState(instance.group ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { setGroup: setGroupStore } = useInstances();
  // A33: Escape, ловушка фокуса и возврат фокуса на триггер (единый хук).
  const { containerRef: dialogRef, requestClose } = useModalA11y(onClose, {
    initialFocus: "#group-input",
  });

  const apply = async (value: string | null) => {
    setBusy(true);
    setError(null);
    try {
      await setGroupStore(instance.id, value);
      onSaved();
      onClose();
    } catch (e) {
      setError(apiErrorText(e));
    } finally {
      setBusy(false);
    }
  };

  const submit = async () => {
    const trimmed = group.trim();
    // Граница длины дублируется на клиенте: ядро отклонит (>64), но честнее
    // показать подсказку сразу, не дожидаясь IPC-ошибки.
    if (trimmed.length > MAX_GROUP_LEN) {
      setError(t("instances.group.tooLong"));
      return;
    }
    // Пустое значение = убрать из группы (ядро нормализует так же).
    await apply(trimmed ? trimmed : null);
  };

  return (
    <div
      className="anim-fade-in fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onClick={(e) => {
        if (e.target === e.currentTarget) requestClose();
      }}
    >
      {/* ref и anim-классы на карточке, не на оверлее (D41/F10). */}
      <div
        ref={dialogRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={t("instances.group.moveTo")}
        className="anim-dialog-in w-[420px] rounded-lg border border-border-app bg-card p-5 shadow-xl"
      >
        <div className="flex items-center justify-between border-b border-border-app pb-3">
          <h2 className="text-lg font-semibold text-text">{t("instances.group.moveTo")}</h2>
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
            <label htmlFor="group-input" className="block text-sm font-medium text-text">
              {t("instances.group.placeholder")}
            </label>
            <input
              id="group-input"
              type="text"
              value={group}
              onChange={(e) => setGroup(e.target.value)}
              placeholder={t("instances.group.placeholder")}
              list="instance-group-options"
              maxLength={MAX_GROUP_LEN + 16} // запас: границу проверяем текстом tooLong, не жёсткой отрезкой
              autoFocus
              className="mt-1 h-10 w-full rounded-md border border-border-app bg-bg px-3 text-sm text-text outline-none focus:border-accent"
            />
            <datalist id="instance-group-options">
              {groups.map((g) => (
                <option key={g} value={g} />
              ))}
            </datalist>
          </div>

          {error && (
            <p className="rounded border border-error/40 bg-error/10 p-2 text-sm text-error" role="alert">
              {error}
            </p>
          )}

          <div className="flex items-center justify-between gap-3 pt-2">
            {instance.group && (
              <button
                type="button"
                disabled={busy}
                onClick={() => void apply(null)}
                className="btn-ghost flex h-10 items-center gap-2 px-4 text-sm"
              >
                <FolderMinus size={16} aria-hidden />
                {t("instances.group.remove")}
              </button>
            )}
            <div className="ml-auto flex gap-3">
              <button
                type="button"
                onClick={() => requestClose()}
                disabled={busy}
                className="btn-ghost h-10 px-4 text-sm"
              >
                {t("common.cancel")}
              </button>
              <button
                type="submit"
                disabled={busy}
                className="btn-primary flex h-10 items-center gap-2 px-5 text-sm"
              >
                <Check size={16} aria-hidden />
                {t("instances.group.apply")}
              </button>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
}
