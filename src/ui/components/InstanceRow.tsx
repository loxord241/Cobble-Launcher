// Строка инстанса «Сланца»: плитка 44 + имя/статус + чип версии + Play/Stop 40
// + кебаб с меню действий (все действия достижимы с клавиатуры). Спека фаза C.
import { useEffect, useRef, useState } from "react";
import {
  Play,
  Square,
  Sparkles,
  FolderOpen,
  Copy,
  Trash2,
  MoreVertical,
} from "lucide-react";
import { t } from "../../i18n";
import type { Instance } from "../../api/types";
import type { LaunchStatus } from "../../state/instances";

export default function InstanceRow({
  inst,
  status,
  onLaunch,
  onStop,
  onOptimize,
  onFolder,
  onDuplicate,
  onDelete,
}: {
  inst: Instance;
  status?: LaunchStatus;
  onLaunch: () => void;
  onStop: () => void;
  onOptimize: () => void;
  onFolder: () => void;
  onDuplicate: () => void;
  onDelete: () => void;
}) {
  const [menuOpen, setMenuOpen] = useState(false);
  const firstItemRef = useRef<HTMLButtonElement>(null);
  const phase = status?.phase;
  const busy = phase !== undefined && phase !== "exited";

  // Статус запуска инлайн в строке, aria-live для скринридеров.
  const statusText = !phase
    ? null
    : phase === "downloading"
      ? t("play.downloading", { done: status?.doneFiles ?? 0, total: status?.totalFiles ?? 0 })
      : phase === "running"
        ? t("play.running")
        : phase === "preparing" || phase === "launching"
          ? t("play.preparing")
          : null;

  // Фокус на первый пункт меню при открытии — навигация стрелками/Tab.
  useEffect(() => {
    if (menuOpen) firstItemRef.current?.focus();
  }, [menuOpen]);

  const closeMenu = () => {
    setMenuOpen(false);
  };

  return (
    <div className="group grid grid-cols-[44px_1fr_auto_auto] items-center gap-3 rounded-lg border border-border-app bg-card px-3 py-2 transition-colors hover:border-border-strong hover:bg-card-hover">
      <span
        aria-hidden
        className="grid size-11 place-items-center rounded-lg bg-accent-soft text-lg font-bold text-accent"
      >
        {inst.name.slice(0, 1).toUpperCase()}
      </span>

      <div className="min-w-0">
        <div className="truncate text-[15px] font-semibold leading-tight" title={inst.name}>
          {inst.name}
        </div>
        <div className="text-[13px] leading-snug text-text-muted" aria-live="polite">
          {inst.loader ? `${inst.loader} ${inst.loaderVersion ?? ""}` : t("instances.loader.none")}
          {statusText ? ` · ${statusText}` : ""}
        </div>
      </div>

      <span className="chip-mono">{inst.mcVersion}</span>

      <div className="flex items-center gap-1">
        {busy ? (
          <button
            onClick={onStop}
            aria-label={`${t("instances.stop")} — ${inst.name}`}
            className="grid size-10 place-items-center rounded-lg bg-error text-on-danger transition-colors hover:opacity-90"
          >
            <Square size={16} aria-hidden fill="currentColor" />
          </button>
        ) : (
          <button
            onClick={onLaunch}
            aria-label={`${t("play")} — ${inst.name}`}
            className="grid size-10 place-items-center rounded-lg bg-accent text-on-accent transition-colors hover:bg-accent-hover"
          >
            <Play size={18} aria-hidden fill="currentColor" strokeWidth={0} />
          </button>
        )}

        <div className="relative">
          <button
            onClick={() => setMenuOpen((v) => !v)}
            aria-label={t("instances.actions", { name: inst.name })}
            aria-expanded={menuOpen}
            aria-haspopup="menu"
            className="icon-btn"
          >
            <MoreVertical size={18} aria-hidden />
          </button>
          {menuOpen && (
            <>
              <div className="fixed inset-0 z-40" onClick={closeMenu} />
              <div
                role="menu"
                className="absolute right-0 top-11 z-50 min-w-44 rounded-lg border border-border-strong bg-card p-1 shadow-lg"
              >
                <MenuItem
                  ref={firstItemRef}
                  icon={Sparkles}
                  label={t("instances.optimize")}
                  onClick={() => {
                    closeMenu();
                    onOptimize();
                  }}
                />
                <MenuItem
                  icon={FolderOpen}
                  label={t("instances.folder")}
                  onClick={() => {
                    closeMenu();
                    onFolder();
                  }}
                />
                <MenuItem
                  icon={Copy}
                  label={t("instances.duplicate")}
                  onClick={() => {
                    closeMenu();
                    onDuplicate();
                  }}
                />
                <MenuItem
                  icon={Trash2}
                  danger
                  label={t("instances.delete")}
                  onClick={() => {
                    closeMenu();
                    onDelete();
                  }}
                  onKeyDown={(e) => {
                    if (e.key === "Escape") closeMenu();
                  }}
                />
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function MenuItem({
  icon: Icon,
  label,
  onClick,
  danger,
  ref,
  onKeyDown,
}: {
  icon: typeof FolderOpen;
  label: string;
  onClick: () => void;
  danger?: boolean;
  ref?: React.Ref<HTMLButtonElement>;
  onKeyDown?: (e: React.KeyboardEvent) => void;
}) {
  return (
    <button
      ref={ref}
      role="menuitem"
      onClick={onClick}
      onKeyDown={onKeyDown}
      className={`flex h-10 w-full items-center gap-2 rounded-md px-3 text-left text-sm ${
        danger ? "text-error hover:bg-error/10" : "text-text hover:bg-surface-2"
      }`}
    >
      <Icon size={16} aria-hidden />
      {label}
    </button>
  );
}
