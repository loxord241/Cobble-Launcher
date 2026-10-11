// Модалка смены версии Minecraft у инстанса (как в Prism): выбор версии из
// манифеста, предупреждение про моды, подтверждение. Ядро (instance_version_
// change) само докачивает файлы новой версии; миры/моды/настройки остаются.
import { useEffect, useMemo, useState } from "react";
import { X, Check, Loader2 } from "lucide-react";
import { api } from "../../api/client";
import { apiErrorText, t } from "../../i18n";
import { useSettings } from "../../state/settings";
import { useModalA11y } from "../hooks/useModalA11y";
import type { Instance, VersionEntry } from "../../api/types";

export default function ChangeVersionModal({
  instance,
  onClose,
  onDone,
}: {
  instance: Instance;
  onClose: () => void;
  /** Успех: ядро сменило версию. Вызвавшая страница закрывает модалку,
   *  показывает notice (changeVersion.done) и перечитывает список инстансов. */
  onDone: (newVersion: string) => void;
}) {
  const { settings } = useSettings();
  // Манифест версий грузим лениво, на открытие модалки (один лёгкий запрос).
  const [versions, setVersions] = useState<VersionEntry[] | null>(null);
  // Выбранная версия: по умолчанию текущая — подтверждение заблокировано,
  // пока пользователь не выбрал другую.
  const [sel, setSel] = useState(instance.mcVersion);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // A33: Escape, ловушка фокуса и возврат фокуса на триггер (единый хук).
  // D41: requestClose — закрытие с анимацией (для ✕ и подложки).
  const { containerRef: dialogRef, requestClose } = useModalA11y(onClose, {
    initialFocus: "#change-version-select",
  });

  useEffect(() => {
    let cancelled = false;
    api
      .manifestVersions(settings?.showSnapshots ?? false, settings?.showOldVersions ?? false)
      .then((vs) => {
        if (!cancelled) setVersions(vs);
      })
      .catch((e) => {
        if (!cancelled) setError(apiErrorText(e));
      });
    return () => {
      cancelled = true;
    };
  }, [settings?.showSnapshots, settings?.showOldVersions]);

  // Текущая версия всегда в списке: манифест мог её отфильтровать
  // (showOldVersions выключен, а инстанс стоит на старой версии) — тогда
  // select не показал бы выбор и подсветку текущей.
  const options = useMemo(() => {
    const current: VersionEntry = {
      id: instance.mcVersion,
      type: "release",
      releaseTime: "",
    };
    if (!versions) return [current]; // манифест ещё едет — показываем текущую
    return versions.some((v) => v.id === instance.mcVersion) ? versions : [current, ...versions];
  }, [versions, instance.mcVersion]);

  const same = sel === instance.mcVersion;

  const submit = async () => {
    if (same) return;
    setBusy(true);
    setError(null);
    try {
      await api.instanceVersionChange(instance.id, sel);
      onDone(sel);
    } catch (e) {
      setError(apiErrorText(e));
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
        aria-label={t("changeVersion.title")}
        className="anim-dialog-in w-[420px] rounded-lg border border-border-app bg-card p-5 shadow-xl"
      >
        <div className="flex items-center justify-between border-b border-border-app pb-3">
          <h2 className="text-lg font-semibold text-text">{t("changeVersion.title")}</h2>
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
          <p className="text-sm text-text-muted">
            {t("changeVersion.current", { version: instance.mcVersion })}
          </p>

          <div>
            <label
              htmlFor="change-version-select"
              className="flex items-center gap-1.5 text-sm font-medium text-text"
            >
              {t("changeVersion.newVersion")}
              {versions === null && (
                // Манифест грузится с сети — маленький спиннер у подписи.
                <Loader2 size={14} aria-hidden className="animate-spin text-text-muted" />
              )}
            </label>
            <select
              id="change-version-select"
              value={sel}
              onChange={(e) => setSel(e.target.value)}
              disabled={busy}
              className="mt-1 h-10 w-full rounded-md border border-border-app bg-bg px-3 text-sm text-text outline-none focus:border-accent"
            >
              {options.map((v) => (
                <option key={v.id} value={v.id}>
                  {/* Снапшоты/старые версии — с читаемым суффиксом типа. */}
                  {v.type === "release" ? v.id : `${v.id} (${v.type})`}
                </option>
              ))}
            </select>
          </div>

          <p className="rounded-md border border-warning/40 bg-warning/10 p-2 text-[13px] text-warning">
            {t("changeVersion.warning")}
          </p>

          {error && (
            <p className="rounded border border-error/40 bg-error/10 p-2 text-sm text-error" role="alert">
              {error}
            </p>
          )}

          <div className="flex items-center justify-end gap-3 pt-2">
            {/* Статус слева от кнопок: выбор не менялся / идёт докачка файлов. */}
            <span className="mr-auto text-[13px] text-text-muted" aria-live="polite">
              {busy ? t("changeVersion.working") : same ? t("changeVersion.sameVersion") : ""}
            </span>
            <button type="button" onClick={onClose} className="btn-ghost h-10 px-4 text-sm">
              {t("common.cancel")}
            </button>
            <button
              type="submit"
              disabled={busy || same}
              className="btn-primary flex h-10 items-center gap-2 px-5 text-sm"
            >
              <Check size={16} aria-hidden />
              {t("changeVersion.confirm")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
