// Диалог краш-репорта: отображение результатов анализа лога и рекомендаций.
import { useState } from "react";
import { AlertTriangle, X, Copy, Check, FolderOpen, ChevronDown, ChevronUp } from "lucide-react";
import { api } from "../../api/client";
import { t } from "../../i18n";
import type { CrashReport } from "../../state/instances";

export default function CrashReportModal({
  report,
  onClose,
}: {
  report: CrashReport;
  onClose: () => void;
}) {
  const [showLog, setShowLog] = useState(false);
  const [copied, setCopied] = useState(false);

  const copyLog = async () => {
    try {
      const summary = [
        `Minecraft Crash Report (Exit Code: ${report.exitCode})`,
        ...report.diagnoses.map((d) => `[${d.title}] ${d.advice}`),
        "",
        "--- Log Tail ---",
        report.logTail,
      ].join("\n");
      await navigator.clipboard.writeText(summary);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // fallback
    }
  };

  const openFolder = () => {
    void api.instanceOpenDir(report.instanceId);
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
      role="alertdialog"
      aria-modal="true"
      aria-label={t("crash.title")}
      onKeyDown={(e) => {
        if (e.key === "Escape") onClose();
      }}
    >
      <div className="flex max-h-[90vh] w-[640px] max-w-full flex-col rounded-lg border border-error/50 bg-card p-6 shadow-2xl">
        <div className="flex items-center justify-between border-b border-border-app pb-3">
          <div className="flex items-center gap-3">
            <span className="grid size-10 place-items-center rounded-lg bg-error/15 text-error">
              <AlertTriangle size={22} aria-hidden />
            </span>
            <div>
              <h2 className="text-lg font-bold text-text">{t("crash.title")}</h2>
              <p className="text-xs text-text-muted">
                {t("crash.exitCode", { code: report.exitCode })}
              </p>
            </div>
          </div>
          <button
            onClick={onClose}
            aria-label={t("common.close")}
            className="icon-btn text-text-muted hover:text-text"
          >
            <X size={18} aria-hidden />
          </button>
        </div>

        <div className="mt-4 flex-1 overflow-y-auto pr-1">
          {/* Диагнозы */}
          {report.diagnoses.length > 0 ? (
            <div className="flex flex-col gap-3">
              {report.diagnoses.map((d, i) => (
                <div
                  key={i}
                  className="rounded-lg border border-accent/30 bg-accent-soft/40 p-4 text-text"
                >
                  <div className="text-sm font-bold text-accent">{d.title}</div>
                  <div className="mt-1 text-sm leading-relaxed">{d.advice}</div>
                </div>
              ))}
            </div>
          ) : (
            <div className="rounded-lg border border-border-app bg-surface-2/40 p-4 text-sm text-text-muted">
              {t("crash.unknown")}
            </div>
          )}

          {/* Сворачиваемый хвост лога */}
          <div className="mt-4">
            <button
              onClick={() => setShowLog((v) => !v)}
              className="flex w-full items-center justify-between rounded-md border border-border-app bg-bg px-3 py-2 text-xs font-semibold text-text-muted hover:text-text"
            >
              <span>{t("crash.viewLogTail")}</span>
              {showLog ? <ChevronUp size={16} /> : <ChevronDown size={16} />}
            </button>
            {showLog && (
              <pre className="mt-2 max-h-60 overflow-y-auto rounded-md border border-border-app bg-bg p-3 font-mono text-xs leading-relaxed text-text selection:bg-accent selection:text-on-accent whitespace-pre-wrap break-all">
                {report.logTail || t("instances.logs.empty")}
              </pre>
            )}
          </div>
        </div>

        {/* Действия */}
        <div className="mt-5 flex flex-wrap items-center justify-between gap-3 border-t border-border-app pt-4">
          <div className="flex items-center gap-2">
            <button
              onClick={openFolder}
              className="btn-ghost flex h-10 items-center gap-2 px-3 text-sm"
            >
              <FolderOpen size={16} aria-hidden />
              {t("instances.folder")}
            </button>
            <button
              onClick={() => void copyLog()}
              className="btn-ghost flex h-10 items-center gap-2 px-3 text-sm"
            >
              {copied ? <Check size={16} aria-hidden /> : <Copy size={16} aria-hidden />}
              {copied ? t("common.copied") : t("crash.copyReport")}
            </button>
          </div>

          <button
            onClick={onClose}
            className="btn-primary h-10 px-5 text-sm"
          >
            {t("common.ok")}
          </button>
        </div>
      </div>
    </div>
  );
}
