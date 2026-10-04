// Кебаб-меню действий инстанса (переиспользуется карточкой инстанса; вынесено
// из InstanceRow при переходе на карточки D35). Все действия — с клавиатуры.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  Sparkles,
  FolderOpen,
  Copy,
  Trash2,
  MoreVertical,
  Settings,
  Pencil,
  ArrowLeftRight,
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
  /** Смена версии Minecraft (как в Prism): модалку монтирует вызывающая страница. */
  onChangeVersion?: () => void;
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

// Зазор между кебабом и меню; минимальный отступ меню от краёв окна.
const MENU_GAP = 6;
const VIEWPORT_MARGIN = 8;

// Fixed-координаты меню (рендерится порталом в body): right — отступ правого
// края меню от правого края окна; вниз — top, при флипе вверх — bottom.
type MenuCoords = { right: number; top?: number; bottom?: number };

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
  const menuRef = useRef<HTMLDivElement>(null);
  // Прямоугольник кебаба на момент открытия: меню закрывается на скролл,
  // поэтому координаты не успевают протухнуть, пока меню открыто.
  const kebabRectRef = useRef<DOMRect | null>(null);
  const [coords, setCoords] = useState<MenuCoords | null>(null);

  // Фокус на первый пункт меню при открытии — навигация стрелками/Tab.
  useEffect(() => {
    if (menuOpen) firstItemRef.current?.focus();
  }, [menuOpen]);

  const closeMenu = (refocus = false) => {
    setMenuOpen(false);
    if (refocus) kebabRef.current?.focus();
  };

  const openMenu = () => {
    const rect = kebabRef.current?.getBoundingClientRect();
    if (!rect) return;
    kebabRectRef.current = rect;
    // Правый край меню прижат к правому краю кнопки — за правый край окна
    // меню не вылезает по построению.
    setCoords({ right: window.innerWidth - rect.right, top: rect.bottom + MENU_GAP });
    setMenuOpen(true);
  };

  // Высота меню известна только после монтирования: измеряем в layout-эффекте
  // (до отрисовки кадра). Влезает снизу — открываем вниз; влезает сверху —
  // флип вверх; не влезает ни там ни там (меню выше окна) — прижимаем к нижней
  // кромке с отступом, чтобы был виден максимум пунктов.
  useLayoutEffect(() => {
    if (!menuOpen) return;
    const rect = kebabRectRef.current;
    const menu = menuRef.current;
    if (!rect || !menu) return;
    const height = menu.offsetHeight;
    const fitsBelow = rect.bottom + MENU_GAP + height <= window.innerHeight - VIEWPORT_MARGIN;
    const fitsAbove = rect.top - MENU_GAP - height >= VIEWPORT_MARGIN;
    if (fitsBelow) return;
    if (fitsAbove) {
      setCoords({
        right: window.innerWidth - rect.right,
        bottom: window.innerHeight - rect.top + MENU_GAP,
      });
      return;
    }
    setCoords({
      right: window.innerWidth - rect.right,
      top: Math.max(VIEWPORT_MARGIN, window.innerHeight - VIEWPORT_MARGIN - height),
    });
  }, [menuOpen]);

  // Меню позиционное: при скролле/ресайзе окна координаты устаревают —
  // закрываем. scroll ловим в capture-фазе, чтобы видеть скролл любых
  // прокручиваемых контейнеров, а не только window.
  useEffect(() => {
    if (!menuOpen) return;
    const close = () => closeMenu();
    window.addEventListener("scroll", close, true);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("resize", close);
    };
  }, [menuOpen]);

  const {
    onSettings,
    onRename,
    onChangeVersion,
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
        onClick={() => {
          if (menuOpen) closeMenu();
          else openMenu();
        }}
        aria-label={t("instances.actions", { name: inst.name })}
        aria-expanded={menuOpen}
        aria-haspopup="menu"
        className="icon-btn"
      >
        <MoreVertical size={18} aria-hidden />
      </button>
      {/* Меню и бэкдроп рендерятся порталом в body с fixed-координатами:
          карточка с overflow-hidden больше не срезает меню. */}
      {menuOpen &&
        coords !== null &&
        createPortal(
          <>
            <div className="fixed inset-0 z-40" onClick={() => closeMenu()} />
            <div
              ref={menuRef}
              role="menu"
              onKeyDown={(e) => {
                if (e.key === "Escape") closeMenu(true);
              }}
              style={{
                position: "fixed",
                right: coords.right,
                ...(coords.bottom !== undefined
                  ? { bottom: coords.bottom }
                  : { top: coords.top }),
              }}
              className={`anim-menu-in ${
                coords.bottom !== undefined ? "origin-bottom-right" : "origin-top-right"
              } z-50 min-w-48 rounded-lg border border-border-strong bg-card p-1 shadow-lg`}
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
            {onChangeVersion && (
              <MenuItem
                icon={ArrowLeftRight}
                label={t("instances.changeVersion.menu")}
                disabled={busy}
                onClick={() => {
                  closeMenu();
                  onChangeVersion();
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
          </>,
          document.body,
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
