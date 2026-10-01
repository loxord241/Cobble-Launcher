// Просмотр логов инстанса / игры в реальном времени + архивные ротаты (F23)
// и шаринг на mclo.gs (F22). Просматриваемый файл — живой поток сессии либо
// текст архивного лога; фильтр уровней применяется к открытому сейчас.
import { useEffect, useMemo, useRef, useState } from "react";
import {
  X,
  Copy,
  Check,
  FolderOpen,
  ArrowDown,
  ArrowLeft,
  ChevronDown,
  ChevronUp,
  ExternalLink,
  LoaderCircle,
  Upload,
} from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useInstances } from "../../state/instances";
import { api } from "../../api/client";
import { apiErrorText, currentLanguage, plural, t } from "../../i18n";
import { useModalA11y } from "../hooks/useModalA11y";
import type { Instance, LogFileInfo } from "../../api/types";

type LevelFilter = "all" | "errors" | "warns";

const FILTERS: LevelFilter[] = ["all", "errors", "warns"];

// Человеческий размер для списка истории (как fmtBytes в SettingsPage).
function fmtBytes(bytes: number): string {
  const mb = bytes / (1024 * 1024);
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} ГБ`;
  if (mb >= 1) return `${Math.round(mb)} МБ`;
  return `${Math.round(bytes / 1024)} КБ`;
}

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
  const containerRef = useRef<HTMLDivElement>(null);
  const copyTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // A33: Escape + ловушка фокуса + возврат фокуса на кебаб-триггер.
  // D41: requestClose — закрытие с анимацией (для ✕ и подложки).
  const { containerRef: dialogRef, requestClose } = useModalA11y(onClose);

  // A31: живой поток — только логи этой сессии инстанса. Системный
  // launcher.log лаунчера под игровой лог не подмешиваем — пусто значит пусто.
  const displayLogs = logs[instance.id] ?? [];

  // Просматриваемый файл: null — живой лог сессии; иначе — текст архивного.
  const [archived, setArchived] = useState<{ name: string; text: string } | null>(null);

  // Фильтр уровней по строкам открытого лога; сбрасывается при смене файла.
  const [filter, setFilter] = useState<LevelFilter>("all");
  useEffect(() => {
    setFilter("all");
  }, [archived?.name]);

  const baseLines = useMemo(
    () => (archived ? archived.text.split("\n") : displayLogs),
    [archived, displayLogs],
  );
  const lines = useMemo(() => {
    if (filter === "all") return baseLines;
    const needle = filter === "errors" ? "ERROR" : "WARN";
    return baseLines.filter((line) => line.includes(needle));
  }, [baseLines, filter]);

  // История архивных логов (F23): грузим лениво, при первом раскрытии секции.
  const [historyOpen, setHistoryOpen] = useState(false);
  const [history, setHistory] = useState<LogFileInfo[] | null>(null);
  const [readingName, setReadingName] = useState<string | null>(null);

  // Шаринг на mclo.gs (F22): команда сама читает latest.log инстанса.
  const [shareBusy, setShareBusy] = useState(false);
  const [shareUrl, setShareUrl] = useState<string | null>(null);
  // Статусная строка модалки: ошибка IPC/сети/чтения архива/открытия ссылки.
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (autoScroll && containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight;
    }
  }, [lines, autoScroll]);

  // A56: таймер индикатора «Скопировано!» не должен жить после закрытия окна.
  useEffect(
    () => () => {
      if (copyTimer.current !== null) clearTimeout(copyTimer.current);
    },
    [],
  );

  const copyLogs = async () => {
    try {
      await navigator.clipboard.writeText(lines.join("\n"));
      setCopied(true);
      if (copyTimer.current !== null) clearTimeout(copyTimer.current);
      copyTimer.current = setTimeout(() => {
        copyTimer.current = null;
        setCopied(false);
      }, 2000);
    } catch {
      // fallback
    }
  };

  const share = async () => {
    setShareBusy(true);
    setError(null);
    setShareUrl(null);
    try {
      const { url } = await api.logShareMclogs(instance.id);
      setShareUrl(url);
    } catch (e) {
      setError(apiErrorText(e));
    } finally {
      setShareBusy(false);
    }
  };

  const openLink = async (url: string) => {
    try {
      await openUrl(url);
    } catch (e) {
      setError(apiErrorText(e));
    }
  };

  const toggleHistory = () => {
    const next = !historyOpen;
    setHistoryOpen(next);
    if (next && history === null) {
      api
        .instanceLogsList(instance.id)
        .then(setHistory)
        .catch((e: unknown) => setError(apiErrorText(e)));
    }
  };

  const openArchived = async (name: string) => {
    setReadingName(name);
    setError(null);
    try {
      const text = await api.instanceLogRead(instance.id, name);
      setArchived({ name, text });
    } catch (e) {
      setError(apiErrorText(e));
    } finally {
      setReadingName(null);
    }
  };

  const openFolder = () => {
    void api.instanceOpenDir(instance.id);
  };

  return (
    <div
      className="anim-fade-in fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onClick={(e) => {
        if (e.target === e.currentTarget) requestClose();
      }}
    >
      <div
        ref={dialogRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={t("instances.logs.title", { name: instance.name })}
        className="anim-dialog-in flex h-[80vh] w-[860px] max-w-full flex-col rounded-lg border border-border-app bg-card p-5 shadow-2xl"
      >
        <div className="flex items-center justify-between border-b border-border-app pb-3">
          <div className="flex items-center gap-3">
            <h2 className="text-lg font-semibold text-text">
              {t("instances.logs.title", { name: instance.name })}
            </h2>
            {archived && <span className="chip-mono text-xs">{archived.name}</span>}
            <span className="chip-mono text-xs">
              {lines.length} {plural("instances.logs.lines", lines.length)}
            </span>
          </div>
          <button
            onClick={requestClose}
            aria-label={t("common.close")}
            className="icon-btn text-text-muted hover:text-text"
          >
            <X size={18} aria-hidden />
          </button>
        </div>

        {/* Фильтр уровней + шаринг на mclo.gs (F22) */}
        <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
          <div className="flex flex-wrap items-center gap-1">
            {FILTERS.map((f) => (
              <button
                key={f}
                aria-pressed={filter === f}
                onClick={() => setFilter(f)}
                className={`h-8 rounded-lg px-3 text-xs font-medium transition-colors ${
                  filter === f
                    ? "bg-accent text-on-accent"
                    : "text-text-muted hover:bg-surface-2 hover:text-text"
                }`}
              >
                {t(`logs.filter.${f}`)}
              </button>
            ))}
            {archived && (
              <button
                onClick={() => setArchived(null)}
                className="btn-ghost flex h-8 items-center gap-1 px-2 text-xs"
              >
                <ArrowLeft size={14} aria-hidden />
                {t("onboarding.back")}
              </button>
            )}
          </div>
          <button
            onClick={() => void share()}
            disabled={shareBusy}
            aria-busy={shareBusy}
            className="btn-ghost flex h-8 items-center gap-2 px-3 text-xs"
          >
            {shareBusy ? (
              <LoaderCircle size={14} aria-hidden className="animate-spin" />
            ) : (
              <Upload size={14} aria-hidden />
            )}
            {t("logs.share")}
          </button>
        </div>

        {/* Прошлые (ротированные) логи — F23, грузим лениво при раскрытии */}
        <div className="mt-2">
          <button
            onClick={toggleHistory}
            aria-expanded={historyOpen}
            className="flex w-full items-center justify-between rounded-md border border-border-app bg-bg px-3 py-1.5 text-xs font-semibold text-text-muted hover:text-text"
          >
            <span>{t("logs.history")}</span>
            {historyOpen ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
          </button>
          {historyOpen && (
            <div className="mt-1 max-h-32 overflow-y-auto rounded-md border border-border-app">
              {history === null ? (
                <div className="flex justify-center p-2">
                  <LoaderCircle size={14} aria-hidden className="animate-spin text-text-muted" />
                </div>
              ) : history.length === 0 ? (
                <p className="p-2 text-xs text-text-muted">{t("logs.archive.empty")}</p>
              ) : (
                history.map((f) => (
                  <button
                    key={f.name}
                    onClick={() => void openArchived(f.name)}
                    disabled={readingName !== null}
                    aria-busy={readingName === f.name}
                    className={`flex w-full items-center justify-between gap-3 px-3 py-1.5 text-left text-xs hover:bg-surface-2 ${
                      archived?.name === f.name ? "text-accent" : "text-text"
                    }`}
                  >
                    <span className="truncate font-mono">{f.name}</span>
                    <span className="shrink-0 text-text-muted">
                      {fmtBytes(f.bytes)} ·{" "}
                      {new Date(f.modified * 1000).toLocaleDateString(currentLanguage())}
                    </span>
                  </button>
                ))
              )}
            </div>
          )}
        </div>

        {/* Статусная строка: ссылка mclo.gs / ошибка */}
        {(shareUrl || error) && (
          <div className="mt-2 flex flex-wrap items-center gap-2 text-xs">
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
            {error && <span className="text-error">{error}</span>}
          </div>
        )}

        {/* Логи */}
        <div
          ref={containerRef}
          className="mt-3 flex-1 overflow-y-auto rounded-md border border-border-app bg-bg p-3 font-mono text-xs leading-relaxed text-text selection:bg-accent selection:text-on-accent"
        >
          {lines.length === 0 ? (
            <p className="text-text-muted">{t("instances.logs.empty")}</p>
          ) : (
            lines.map((line, idx) => (
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
              disabled={lines.length === 0}
              className="btn-primary flex h-10 items-center gap-2 px-4 text-sm"
            >
              {copied ? (
                <Check key="copied" size={16} aria-hidden className="anim-pop-in" />
              ) : (
                <Copy size={16} aria-hidden />
              )}
              {copied ? t("common.copied") : t("common.copy")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
