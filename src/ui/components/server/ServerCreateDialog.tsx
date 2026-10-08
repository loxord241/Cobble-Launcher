// Диалог «Сервер из инстанса» (D68): имя, мир-копия, порт, RAM, online-mode.
// Создание качает файлы сервера (минуты) — busy на весь вызов: кнопки и
// закрытие (✕/подложка/Esc) заблокированы, отмена невозможна. Успех → сводка
// по модам + «Готово»; ошибка → apiErrorText в модалке, можно закрыть.
import { useEffect, useState } from "react";
import { LoaderCircle, X } from "lucide-react";
import { api } from "../../../api/client";
import { apiErrorText, t } from "../../../i18n";
import { formatRamMb } from "../../format";
import { useModalA11y } from "../../hooks/useModalA11y";
import type { ServerCreateResult, WorldInfo } from "../../../api/types";

const RAM_MIN = 512;
const RAM_MAX = 32768;
const RAM_STEP = 512;
const PORT_DEFAULT = 25565;
const RAM_DEFAULT = 2048;
/** В сводке пропущенных модов показываем максимум 5 имён, остальное — «+N». */
const SKIPPED_SHOWN = 5;

export default function ServerCreateDialog({
  instanceId,
  instanceName,
  onClose,
  onCreated,
}: {
  instanceId: string;
  /** Имя инстанса-источника: дефолт имени сервера («<Инстанс> Server»). */
  instanceName: string;
  onClose: () => void;
  /** Успех: список серверов за модалкой перечитывает вкладка. */
  onCreated: () => void;
}) {
  const [name, setName] = useState(`${instanceName} Server`.trim());
  // "" — пункт «Без мира» (ядро сгенерирует свой мир).
  const [world, setWorld] = useState("");
  const [worlds, setWorlds] = useState<WorldInfo[]>([]);
  const [portText, setPortText] = useState(String(PORT_DEFAULT));
  const [ramMb, setRamMb] = useState(RAM_DEFAULT);
  const [onlineMode, setOnlineMode] = useState(true);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<ServerCreateResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  // A33/D41: Escape, ловушка фокуса, возврат фокуса, анимация закрытия.
  const { containerRef: dialogRef, requestClose } = useModalA11y(onClose, {
    initialFocus: "#sv-name",
  });

  // busy блокирует закрытие: ✕ и подложка не зовут requestClose, а Esc
  // гасится window-capture слушателем (window раньше document, где сидит
  // хук useModalA11y — stopPropagation до него не пускает событие).
  useEffect(() => {
    if (!busy) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        e.preventDefault();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [busy]);

  // Миры инстанса (те же, что на вкладке «Миры»); пусто — только «Без мира».
  useEffect(() => {
    api
      .instanceWorlds(instanceId)
      .then(setWorlds)
      .catch(() => setWorlds([]));
  }, [instanceId]);

  const requestCloseIfIdle = () => {
    if (!busy) requestClose();
  };

  const submit = async () => {
    if (busy || result) return;
    const trimmed = name.trim();
    if (!trimmed) return;
    setBusy(true);
    setError(null);
    try {
      const port = Math.min(
        Math.max(Math.trunc(Number(portText) || PORT_DEFAULT), 1),
        65535,
      );
      const res = await api.serverCreate(
        instanceId,
        trimmed,
        world || null,
        port,
        ramMb,
        onlineMode,
      );
      setResult(res);
      // Список серверов перечитывается сразу — модалка ещё показывает сводку.
      onCreated();
    } catch (e) {
      setError(apiErrorText(e));
    } finally {
      setBusy(false);
    }
  };

  const skippedNames = result
    ? result.skippedProjects.slice(0, SKIPPED_SHOWN).join(", ")
    : "";
  const skippedRest = result ? result.skippedProjects.length - SKIPPED_SHOWN : 0;

  return (
    <div
      className="anim-fade-in fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onClick={(e) => {
        if (e.target === e.currentTarget) requestCloseIfIdle();
      }}
    >
      {/* D41: ref и anim-классы на карточке, не на оверлее. */}
      <div
        ref={dialogRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-busy={busy}
        aria-label={t("servers.create")}
        className="anim-dialog-in flex max-h-[85vh] w-[480px] max-w-full flex-col overflow-y-auto rounded-lg border border-border-app bg-card p-5"
      >
        <div className="flex items-center justify-between">
          <h2 className="text-lg font-semibold">{t("servers.create")}</h2>
          <button
            onClick={requestCloseIfIdle}
            disabled={busy}
            aria-label={t("common.close")}
            className="text-text-muted hover:text-text disabled:cursor-not-allowed disabled:opacity-50"
          >
            <X size={18} aria-hidden />
          </button>
        </div>

        {result ? (
          <>
            {/* Успех: сводка по модам (что скопировано/пропущено) + «Готово». */}
            <p className="mt-3 text-sm font-semibold text-success">{t("servers.created")}</p>
            <ul className="mt-2 flex flex-col gap-1 text-[13px] text-text-muted">
              <li>{t("servers.mods.copied", { n: result.modsServer })}</li>
              <li>{t("servers.mods.skippedClient", { n: result.modsClientSkipped })}</li>
              <li>{t("servers.mods.unchecked", { n: result.modsUnchecked })}</li>
              {result.skippedProjects.length > 0 && (
                <li>
                  {t("servers.mods.skippedList", {
                    names:
                      skippedNames + (skippedRest > 0 ? ` +${skippedRest}` : ""),
                  })}
                </li>
              )}
            </ul>
            <div className="mt-5 flex justify-end">
              <button onClick={onClose} className="btn-primary h-10 px-5 text-sm">
                {t("common.done")}
              </button>
            </div>
          </>
        ) : (
          <>
            <p className="mt-2 text-[13px] text-text-muted">{t("servers.create.hint")}</p>

            <label className="mt-4 block text-sm text-text-muted" htmlFor="sv-name">
              {t("servers.name")}
            </label>
            <input
              id="sv-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              disabled={busy}
              className="mt-1 w-full rounded-md border border-border-app bg-bg px-3 py-2 text-text outline-none focus:border-accent disabled:opacity-60"
            />

            <label className="mt-3 block text-sm text-text-muted" htmlFor="sv-world">
              {t("servers.world")}
            </label>
            <select
              id="sv-world"
              value={world}
              onChange={(e) => setWorld(e.target.value)}
              disabled={busy}
              className="mt-1 w-full rounded-md border border-border-app bg-bg px-3 py-2 text-text outline-none focus:border-accent disabled:opacity-60"
            >
              <option value="">{t("servers.world.none")}</option>
              {worlds.map((w) => (
                <option key={w.name} value={w.name}>
                  {w.name}
                </option>
              ))}
            </select>

            <label className="mt-3 block text-sm text-text-muted" htmlFor="sv-port">
              {t("servers.port")}
            </label>
            <input
              id="sv-port"
              type="number"
              inputMode="numeric"
              min={1}
              max={65535}
              value={portText}
              onChange={(e) => setPortText(e.target.value)}
              disabled={busy}
              className="mt-1 w-full rounded-md border border-border-app bg-bg px-3 py-2 font-mono tabular-nums text-text outline-none focus:border-accent disabled:opacity-60"
            />

            {/* RAM — ползунок как в настройках инстанса, но диапазон сервера
                шире внизу (512 МБ хватает ванильному серверу). */}
            <div className="mt-3">
              <div className="flex items-center justify-between">
                <label htmlFor="sv-ram" className="text-sm text-text-muted">
                  {t("servers.ram")}
                </label>
                <span className="font-mono text-sm font-semibold text-accent tabular-nums">
                  {formatRamMb(ramMb)}
                </span>
              </div>
              <input
                id="sv-ram"
                type="range"
                min={RAM_MIN}
                max={RAM_MAX}
                step={RAM_STEP}
                value={ramMb}
                onChange={(e) => setRamMb(Number(e.target.value))}
                disabled={busy}
                className="mt-2 h-2 w-full cursor-pointer appearance-none rounded-lg bg-surface-2 accent-accent disabled:opacity-60"
              />
              <div className="mt-1 flex justify-between text-xs text-text-muted tabular-nums">
                <span>{RAM_MIN} MB</span>
                <span>{RAM_MAX} MB</span>
              </div>
            </div>

            <label className="mt-4 flex cursor-pointer items-center gap-2 text-sm text-text hover:text-text">
              <input
                type="checkbox"
                checked={onlineMode}
                onChange={(e) => setOnlineMode(e.target.checked)}
                disabled={busy}
                className="accent-accent"
              />
              {t("servers.onlineMode")}
            </label>

            {error && (
              <div className="mt-3 text-sm text-error" role="alert">
                <p className="whitespace-pre-line text-[12px]">{error}</p>
              </div>
            )}

            <div className="mt-5 flex justify-end gap-3">
              <button
                onClick={requestCloseIfIdle}
                disabled={busy}
                className="btn-ghost h-10 px-4 text-sm"
              >
                {t("common.cancel")}
              </button>
              <button
                onClick={() => void submit()}
                disabled={busy || !name.trim()}
                aria-busy={busy}
                className="btn-primary flex h-10 items-center gap-2 px-5 text-sm"
              >
                {busy && <LoaderCircle size={16} aria-hidden className="animate-spin" />}
                {busy ? t("servers.creating") : t("servers.create")}
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
