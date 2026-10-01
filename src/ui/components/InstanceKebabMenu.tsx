// Кебаб-меню действий инстанса (переиспользуется карточкой инстанса; вынесено
// из InstanceRow при переходе на карточки D35). Все действия — с клавиатуры.
import { useEffect, useRef, useState } from "react";
import {
  Sparkles,
  FolderOpen,
  Copy,
  Trash2,
  MoreVertical,
  Settings,
  Pencil,
  FileText,
  Archive,
  Share,
  Image,
  ImageOff,
  MonitorSmartphone,
  LockOpen,
  ShieldCheck,
} from "lucide-react";
import { t } from "../../i18n";
import type { Instance } from "../../api/types";

export type KebabCallbacks = {
  onOptimize: () => void;
  onFolder: () => void;
  onDuplicate: () => void;
  onDelete: () => void;
  onSettings?: () => void;
  onRename?: () => void;
  onLogs?: () => void;
  onBackup?: () => void;
  onExport?: () => void;
  /** F27: снять зависшую .lock-блокировку (не блокируется busy). */
  onForceUnlock?: () => void;
  /** F2: проверить/починить игровые файлы (saves/mods/config не трогаются). */
  onRepair?: () => void;
  onShortcut?: () => void;
  onIconSet?: () => void;
  onIconClear?: () => void;
};

export default function InstanceKebabMenu({
  inst,
  busy,
  callbacks,
}: {
  inst: Instance;
  busy: boolean;
  callbacks: KebabCallbacks;
}) {
  const [menuOpen, setMenuOpen] = useState(false);
  const firstItemRef = useRef<HTMLButtonElement>(null);
  const kebabRef = useRef<HTMLButtonElement>(null);

  // Фокус на первый пункт меню при открытии — навигация стрелками/Tab.
  useEffect(() => {
    if (menuOpen) firstItemRef.current?.focus();
  }, [menuOpen]);

  const closeMenu = (refocus = false) => {
    setMenuOpen(false);
    if (refocus) kebabRef.current?.focus();
  };

  const {
    onSettings,
    onRename,
    onLogs,
    onBackup,
    onExport,
    onForceUnlock,
    onRepair,
    onIconSet,
    onIconClear,
    onShortcut,
    onOptimize,
    onFolder,
    onDuplicate,
    onDelete,
  } = callbacks;

  return (
    <div className="relative">
      <button
        ref={kebabRef}
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
          <div className="fixed inset-0 z-40" onClick={() => closeMenu()} />
          <div
            role="menu"
            onKeyDown={(e) => {
              if (e.key === "Escape") closeMenu(true);
            }}
            className="anim-menu-in origin-top-right absolute right-0 top-11 z-50 min-w-48 rounded-lg border border-border-strong bg-card p-1 shadow-lg"
          >
            {onSettings && (
              <MenuItem
                ref={firstItemRef}
                icon={Settings}
                label={t("instances.settings.menu")}
                onClick={() => {
                  closeMenu();
                  onSettings();
                }}
              />
            )}
            {onRename && (
              <MenuItem
                icon={Pencil}
                label={t("instances.rename.menu")}
                disabled={busy}
                onClick={() => {
                  closeMenu();
                  onRename();
                }}
              />
            )}
            {onLogs && (
              <MenuItem
                icon={FileText}
                label={t("instances.logs.menu")}
                onClick={() => {
                  closeMenu();
                  onLogs();
                }}
              />
            )}
            <MenuItem
              icon={Sparkles}
              label={t("instances.optimize")}
              onClick={() => {
                closeMenu();
                onOptimize();
              }}
            />
            {onBackup && (
              <MenuItem
                icon={Archive}
                label={t("instances.backup.menu")}
                disabled={busy}
                onClick={() => {
                  closeMenu();
                  onBackup();
                }}
              />
            )}
            {onExport && (
              <MenuItem
                icon={Share}
                label={t("instances.export.menu")}
                disabled={busy}
                onClick={() => {
                  closeMenu();
                  onExport();
                }}
              />
            )}
            {onIconSet && (
              <MenuItem
                icon={Image}
                label={t("instances.icon.menu")}
                disabled={busy}
                onClick={() => {
                  closeMenu();
                  onIconSet();
                }}
              />
            )}
            {onIconClear && (
              <MenuItem
                icon={ImageOff}
                label={t("instances.icon.clearMenu")}
                disabled={busy}
                onClick={() => {
                  closeMenu();
                  onIconClear();
                }}
              />
            )}
            <MenuItem
              icon={FolderOpen}
              label={t("instances.folder")}
              onClick={() => {
                closeMenu();
                onFolder();
              }}
            />

            {onShortcut && (
              <MenuItem
                icon={MonitorSmartphone}
                label={t("instances.shortcut")}
                disabled={busy}
                onClick={() => {
                  closeMenu();
                  onShortcut();
                }}
              />
            )}
            {/* F27: блокировка бывает именно у незапущенной игры — busy не мешает. */}
            {onForceUnlock && (
              <MenuItem
                icon={LockOpen}
                label={t("instances.forceUnlock")}
                onClick={() => {
                  closeMenu();
                  onForceUnlock();
                }}
              />
            )}
            {/* F2: проверка файлов идёт движком загрузок — во время busy не копим. */}
            {onRepair && (
              <MenuItem
                icon={ShieldCheck}
                label={t("instances.repair.menu")}
                disabled={busy}
                onClick={() => {
                  closeMenu();
                  onRepair();
                }}
              />
            )}
            <MenuItem
              icon={Copy}
              label={t("instances.duplicate")}
              disabled={busy}
              onClick={() => {
                closeMenu();
                onDuplicate();
              }}
            />
            <MenuItem
              icon={Trash2}
              danger
              label={t("instances.delete")}
              disabled={busy}
              onClick={() => {
                closeMenu();
                onDelete();
              }}
            />
          </div>
        </>
      )}
    </div>
  );
}

function MenuItem({
  icon: Icon,
  label,
  onClick,
  danger,
  disabled,
  ref,
}: {
  icon: typeof FolderOpen;
  label: string;
  onClick: () => void;
  danger?: boolean;
  disabled?: boolean;
  ref?: React.Ref<HTMLButtonElement>;
}) {
  return (
    <button
      ref={ref}
      role="menuitem"
      disabled={disabled}
      onClick={onClick}
      className={`flex h-10 w-full items-center gap-2 rounded-md px-3 text-left text-sm transition-colors ${
        disabled
          ? "cursor-not-allowed text-text-muted/40"
          : danger
            ? "text-error hover:bg-error/10"
            : "text-text hover:bg-surface-2"
      }`}
    >
      <Icon size={16} aria-hidden />
      {label}
    </button>
  );
}
