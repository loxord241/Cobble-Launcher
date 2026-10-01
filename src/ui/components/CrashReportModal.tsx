import { useEffect, useRef, useState } from "react";
import {
  AlertTriangle,
  X,
  Copy,
  Check,
  FolderOpen,
  ChevronDown,
  ChevronUp,
  ExternalLink,
  LoaderCircle,
  Upload,
} from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api } from "../../api/client";
import { apiErrorText, t } from "../../i18n";
import { useModalA11y } from "../hooks/useModalA11y";
import { useInstances, type CrashReport } from "../../state/instances";
import { useAccounts } from "../../state/accounts";
import { useSettings } from "../../state/settings";

export default function CrashReportModal({
  report,
  onClose,
}: {
  report: CrashReport;
  onClose: () => void;
}) {
  const [showLog, setShowLog] = useState(false);
  const [copied, setCopied] = useState(false);
  // Шаринг на mclo.gs (F22): команда сама читает latest.log инстанса.
  const [shareBusy, setShareBusy] = useState(false);
  const [shareUrl, setShareUrl] = useState<string | null>(null);
  const [shareError, setShareError] = useState<string | null>(null);
  // A33: Escape, ловушка фокуса и возврат фокуса на триггер (единый хук).
  // D41: requestClose — закрытие с анимацией (для ✕ и подложки).
  const { containerRef: dialogRef, requestClose } = useModalA11y(onClose);
  const copyTimer = useRef<number | null>(null);

  // F30: безопасный режим предлагается после >=2 падений запуска подряд.
  // Профиль запуска берём здесь же, как в Shell (модалка глобальная):
  // активный из настроек, иначе первый в списке.
  const accounts = useAccounts((s) => s.list);
  const settings = useSettings((s) => s.settings);
  const activeAccount =
    accounts.find((a) => a.id === settings?.accountsActiveId) ?? accounts[0] ?? null;
  const offerSafeMode = report.crashCount >= 2 && activeAccount !== null;
  const [starting, setStarting] = useState(false);

  /** F30: перезапуск из модалки (yes — безопасный, no — обычный). Модал
   * закроется сам: launch/launchSafe сбрасывают crashReport этого инстанса. */
  const startIn = (safe: boolean) => {
    if (!activeAccount || starting) return;
    setStarting(true);
    const { launch, launchSafe } = useInstances.getState();
    const start = safe
      ? launchSafe(report.instanceId, activeAccount.name)
      : launch(report.instanceId, activeAccount.name);
    void start.finally(() => setStarting(false));
  };

  // A56: таймер индикатора «Скопировано!» не переживает размонтирование.
  useEffect(
    () => () => {
      if (copyTimer.current !== null) window.clearTimeout(copyTimer.current);
    },
    [],
  );

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
      copyTimer.current = window.setTimeout(() => setCopied(false), 2000);
    } catch {
      // fallback
    }
  };

  const openFolder = () => {
    void api.instanceOpenDir(report.instanceId);
  };

  const share = async () => {
    setShareBusy(true);
    setShareError(null);
    setShareUrl(null);
    try {
      const { url } = await api.logShareMclogs(report.instanceId);
      setShareUrl(url);
    } catch (e) {
      setShareError(apiErrorText(e));
    } finally {
      setShareBusy(false);
    }
  };

  const openLink = async (url: string) => {
    try {
      await openUrl(url);
    } catch (e) {
      setShareError(apiErrorText(e));
    }
  };

  return (
    <div
      ref={dialogRef}
      className="anim-fade-in fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
      role="alertdialog"
      aria-modal="true"
      aria-label={t("crash.title")}
      tabIndex={-1}
      onClick={(e) => {
        if (e.target === e.currentTarget) requestClose();
      }}
    >
      <div className="anim-dialog-in flex max-h-[90vh] w-[640px] max-w-full flex-col rounded-lg border border-error/50 bg-card p-6 shadow-2xl">
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
            onClick={requestClose}
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
        {(shareUrl || shareError) && (
          <div className="mt-3 flex flex-wrap items-center gap-2 text-xs">
            {shareUrl && (
              <span className="flex items-center gap-2 text-text-muted">
                {t("logs.share.done", { url: shareUrl })}
                <button
                  onClick={() => void openLink(shareUrl)}
                  aria-label={shareUrl}
                  className="icon-btn text-accent"
                >
                  <ExternalLink size={14} aria-hidden />
                </button>
              </span>
            )}
            {shareError && <span className="text-error">{shareError}</span>}
          </div>
        )}
        {/* F30: серия падений запуска — предложение безопасного режима
            (ядро сбросит JVM-флаги и выключит шейдеры, затем запустит игру). */}
        {offerSafeMode && (
          <div className="mt-3 rounded-lg border border-accent/30 bg-accent-soft/40 p-4">
            <p className="text-sm font-semibold text-text">
              {t("instances.safeMode.title", { n: report.crashCount })}
            </p>
            <div className="mt-3 flex flex-wrap gap-2">
              <button
                onClick={() => startIn(true)}
                disabled={starting}
                aria-busy={starting}
                className="btn-primary flex h-10 items-center gap-2 px-4 text-sm disabled:cursor-not-allowed disabled:opacity-60"
              >
                {starting && <LoaderCircle size={16} aria-hidden className="animate-spin" />}
                {t("instances.safeMode.yes")}
              </button>
              <button
                onClick={() => startIn(false)}
                disabled={starting}
                className="btn-ghost flex h-10 items-center gap-2 px-4 text-sm disabled:cursor-not-allowed disabled:opacity-60"
              >
                {t("instances.safeMode.no")}
              </button>
            </div>
          </div>
        )}
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
              {copied ? (
                <Check key="copied" size={16} aria-hidden className="anim-pop-in" />
              ) : (
                <Copy size={16} aria-hidden />
              )}
              {copied ? t("common.copied") : t("crash.copyReport")}
            </button>
            <button
              onClick={() => void share()}
              disabled={shareBusy}
              aria-busy={shareBusy}
              className="btn-ghost flex h-10 items-center gap-2 px-3 text-sm"
            >
              {shareBusy ? (
                <LoaderCircle size={16} aria-hidden className="animate-spin" />
              ) : (
                <Upload size={16} aria-hidden />
              )}
              {t("logs.share")}
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
