// Просмотр логов инстанса / игры в реальном времени.
import { useEffect, useRef, useState } from "react";
import { X, Copy, Check, FolderOpen, ArrowDown } from "lucide-react";
import { useInstances } from "../../state/instances";
import { api } from "../../api/client";
import { t } from "../../i18n";
import type { Instance } from "../../api/types";

export default function LogViewerModal({
  instance,
  onClose,
}: {
  instance: Instance;
  onClose: () => void;
}) {
  const { logs } = useInstances();
  const [copied, setCopied] = useState(false);
  const [autoScroll, setAutoScroll] = useState(true);
  const [fallbackLogs, setFallbackLogs] = useState<string[]>([]);
  const containerRef = useRef<HTMLDivElement>(null);

  const instanceLogs = logs[instance.id];
  const displayLogs =
    instanceLogs && instanceLogs.length > 0 ? instanceLogs : fallbackLogs;

  useEffect(() => {
    // Если в памяти еще нет логов сессии, подтягиваем последние строки из файла логов лаунчера
    if (!instanceLogs || instanceLogs.length === 0) {
      api.logsTail(150).then((txt) => {
        if (txt.trim()) setFallbackLogs(txt.split("\n"));
      }).catch(() => undefined);
    }
  }, [instance.id, instanceLogs]);

  useEffect(() => {
    if (autoScroll && containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight;
    }
  }, [displayLogs, autoScroll]);

  const copyLogs = async () => {
    try {
      await navigator.clipboard.writeText(displayLogs.join("\n"));
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // fallback
    }
  };

  const openFolder = () => {
    void api.instanceOpenDir(instance.id);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      role="dialog"
      aria-modal="true"
      aria-label={t("instances.logs.title", { name: instance.name })}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="flex h-[80vh] w-[860px] max-w-full flex-col rounded-lg border border-border-app bg-card p-5 shadow-2xl">
        <div className="flex items-center justify-between border-b border-border-app pb-3">
          <div className="flex items-center gap-3">
            <h2 className="text-lg font-semibold text-text">
              {t("instances.logs.title", { name: instance.name })}
            </h2>
            <span className="chip-mono text-xs">
              {displayLogs.length} {t("instances.logs.lines")}
            </span>
          </div>
          <button
            onClick={onClose}
            aria-label={t("common.close")}
            className="icon-btn text-text-muted hover:text-text"
          >
            <X size={18} aria-hidden />
          </button>
        </div>

        {/* Логи */}
        <div
          ref={containerRef}
          className="mt-3 flex-1 overflow-y-auto rounded-md border border-border-app bg-bg p-3 font-mono text-xs leading-relaxed text-text selection:bg-accent selection:text-on-accent"
        >
          {displayLogs.length === 0 ? (
            <p className="text-text-muted">{t("instances.logs.empty")}</p>
          ) : (
            displayLogs.map((line, idx) => (
              <div key={idx} className="whitespace-pre-wrap break-all hover:bg-surface-2/40">
                {line}
              </div>
            ))
          )}
        </div>

        {/* Панель управления */}
        <div className="mt-4 flex flex-wrap items-center justify-between gap-3 border-t border-border-app pt-3">
          <label className="flex cursor-pointer items-center gap-2 text-sm text-text-muted hover:text-text">
            <input
              type="checkbox"
              checked={autoScroll}
              onChange={(e) => setAutoScroll(e.target.checked)}
              className="accent-accent"
            />
            <ArrowDown size={14} aria-hidden />
            {t("instances.logs.autoScroll")}
          </label>

          <div className="flex items-center gap-2">
            <button
              onClick={openFolder}
              className="btn-ghost flex h-10 items-center gap-2 px-3 text-sm"
            >
              <FolderOpen size={16} aria-hidden />
              {t("instances.folder")}
            </button>
            <button
              onClick={() => void copyLogs()}
              disabled={displayLogs.length === 0}
              className="btn-primary flex h-10 items-center gap-2 px-4 text-sm"
            >
              {copied ? <Check size={16} aria-hidden /> : <Copy size={16} aria-hidden />}
              {copied ? t("common.copied") : t("common.copy")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
