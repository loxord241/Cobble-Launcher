// Таб «Конфиги» страницы инстанса (F29): файлы minecraft/config/ со
// встроенным текстовым редактором (внутри таба, не модалка). Файлы больше
// 1 МБ не открываются — показывается подсказка «открой в проводнике».
// Компонент монтируется только на своём табе — данные грузятся лениво,
// как панель «Миры». Ошибки/уведомления показывает страница (единый баннер
// сверху), сюда передаются колбэки.
import { useCallback, useEffect, useState } from "react";
import { FileText, Save } from "lucide-react";
import { api } from "../../../api/client";
import { apiErrorText, t } from "../../../i18n";
import { askConfirm } from "../../confirm";
import type { ConfigFile } from "../../../api/types";

/** Порог редактора: тот же cap, что на ядре (instances::configs). */
const MAX_EDIT_BYTES = 1024 * 1024;

/** Размер файла в строке списка: КБ с округлением. */
function fmtSize(bytes: number): string {
  return `${Math.round(bytes / 1024)} КБ`;
}

export default function ConfigsTab({
  instanceId,
  setError,
  setNotice,
  onDirtyChange,
}: {
  instanceId: string;
  setError: (msg: string | null) => void;
  setNotice: (msg: string) => void;
  /** Сообщить странице о несохранённых правках в редакторе (F6). */
  onDirtyChange?: (dirty: boolean) => void;
}) {
  const [files, setFiles] = useState<ConfigFile[] | null>(null);
  const [selected, setSelected] = useState<ConfigFile | null>(null);
  const [text, setText] = useState<string | null>(null);
  // Текст файла на момент открытия — эталон для признака несохранённых правок.
  const [initialText, setInitialText] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // Несохранённые правки: текст в редакторе разошёлся с открытым файлом.
  const isDirty = text !== null && text !== initialText;

  // Сообщаем странице об изменении признака правок (контракт с InstanceDetailPage).
  useEffect(() => {
    onDirtyChange?.(isDirty);
  }, [isDirty, onDirtyChange]);

  const load = useCallback(async () => {
    try {
      setFiles(await api.instanceConfigs(instanceId));
    } catch (e) {
      setFiles([]);
      setError(apiErrorText(e));
    }
  }, [instanceId, setError]);

  // Ленивая загрузка при активации таба (как «Миры»).
  useEffect(() => {
    void load();
  }, [load]);

  const run = useCallback(
    async (op: () => Promise<void>) => {
      setBusy(true);
      setError(null);
      try {
        await op();
      } catch (e) {
        setError(apiErrorText(e));
      } finally {
        setBusy(false);
      }
    },
    [setError],
  );

  /** Клик по файлу: открыть редактор (или подсказку для больших файлов). */
  const open = async (f: ConfigFile) => {
    // Правки предыдущего файла пропадут — спрашиваем подтверждение (F6;
    // D49: confirm в Tauri асинхронен — await).
    if (isDirty && !(await askConfirm(t("instance.configs.unsaved")))) return;
    setSelected(f);
    setText(null);
    setInitialText(null);
    if (f.bytes > MAX_EDIT_BYTES) return; // редактор не открываем, покажем tooBig
    void run(async () => {
      const c = await api.configRead(instanceId, f.rel);
      setText(c.text);
      setInitialText(c.text);
    });
  };

  const save = () => {
    const target = selected;
    if (!target) return;
    void run(async () => {
      await api.configWrite(instanceId, target.rel, text ?? "");
      setInitialText(text ?? ""); // сохранено — правок больше нет, dirty сбрасывается
      setNotice(t("instance.configs.saved", { file: target.rel }));
      // Размер мог измениться — перечитываем список и обновляем карточку.
      const fresh = await api.instanceConfigs(instanceId);
      setFiles(fresh);
      setSelected(fresh.find((f) => f.rel === target.rel) ?? target);
    });
  };

  return (
    <div className="rounded-lg border border-border-app bg-card p-3">
      {files === null ? null : files.length === 0 ? (
        <p className="px-2 py-3 text-[13px] text-text-muted">{t("instance.configs.empty")}</p>
      ) : (
        <ul className="flex flex-col gap-1">
          {files.map((f) => (
            <li key={f.rel}>
              <button
                onClick={() => open(f)}
                disabled={busy}
                className={`grid min-h-11 w-full grid-cols-[1fr_auto] items-center gap-3 rounded-md border px-3 py-2 text-left transition-colors hover:border-accent/60 hover:bg-card-hover disabled:opacity-60 ${
                  selected?.rel === f.rel
                    ? "border-accent/60 bg-card-hover"
                    : "border-border-app"
                }`}
              >
                <span className="flex min-w-0 items-center gap-2">
                  <FileText size={14} aria-hidden className="shrink-0 text-text-muted" />
                  <span className="truncate font-mono text-[13px]" title={f.rel}>
                    {f.rel}
                  </span>
                </span>
                <span className="font-mono text-[12px] text-text-muted">{fmtSize(f.bytes)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}

      {selected && (
        <div className="mt-3 border-t border-border-app pt-3">
          {selected.bytes > MAX_EDIT_BYTES ? (
            <p className="px-2 py-3 text-[13px] text-text-muted">
              {t("instance.configs.tooBig")}
            </p>
          ) : (
            <>
              <div className="truncate font-mono text-[12px] text-text-muted" title={selected.rel}>
                {selected.rel}
              </div>
              <textarea
                value={text ?? ""}
                onChange={(e) => setText(e.target.value)}
                onKeyDown={(e) => {
                  // Ctrl+S / Cmd+S — сохранить, не уходя с клавиатуры (F6).
                  if ((e.metaKey || e.ctrlKey) && e.key === "s") {
                    e.preventDefault();
                    if (!busy && text !== null) save();
                  }
                }}
                disabled={busy || text === null}
                spellCheck={false}
                className="mt-2 h-80 w-full resize-y rounded-md border border-border-app bg-bg px-3 py-2 font-mono text-xs text-text outline-none focus:border-accent disabled:opacity-60"
              />
              <div className="mt-2 flex justify-end">
                <button onClick={save} disabled={busy || text === null} className="btn-primary btn-sm">
                  <Save size={14} aria-hidden />
                  {t("instance.configs.save")}
                </button>
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}
