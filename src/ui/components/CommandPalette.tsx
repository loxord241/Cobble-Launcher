// F19: палитра команд — модалка над всем UI (z-[60]): навигация по разделам,
// инстансы («Открыть»/«Запустить»), управление аккаунтами. Фильтр — подстрока
// без учёта регистра, максимум 10 результатов. Данные (инстансы, статусы,
// профили) палитра читает из сторов сама — как CrashReportModal; Shell
// передаёт только колбэки-переходы и onClose.
import { useMemo, useState } from "react";
import {
  Box,
  FolderOpen,
  House,
  Play,
  Settings as SettingsIcon,
  UserPlus,
} from "lucide-react";
import { t } from "../../i18n";
import { useInstances } from "../../state/instances";
import { useAccounts } from "../../state/accounts";
import { useSettings } from "../../state/settings";
import { useModalA11y } from "../hooks/useModalA11y";

type PalettePage = "home" | "instances" | "settings";

/** Жёсткий лимит результатов (спека F19). */
const MAX_ITEMS = 10;
/** Инстансов при пустом запросе — не больше 8 (лимит 10 режет итог). */
const EMPTY_QUERY_INSTANCES = 8;

interface PaletteItem {
  key: string;
  icon: typeof House;
  label: string;
  run: () => void;
}

export default function CommandPalette({
  onClose,
  onNavigate,
  onOpenInstance,
  onLaunchInstance,
  onManageAccounts,
}: {
  onClose: () => void;
  onNavigate: (page: PalettePage) => void;
  onOpenInstance: (id: string) => void;
  onLaunchInstance: (id: string, player: string) => void;
  onManageAccounts: () => void;
}) {
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  // A33: Escape закрывает, Tab зациклен внутри диалога, стартовый фокус —
  // в поле ввода, после закрытия фокус возвращается на триггер.
  // D41: requestClose — закрытие с анимацией (для подложки).
  const { containerRef: dialogRef, requestClose } = useModalA11y(onClose, {
    initialFocus: "input",
  });

  const list = useInstances((s) => s.list);
  const status = useInstances((s) => s.status);
  const accounts = useAccounts((s) => s.list);
  const settings = useSettings((s) => s.settings);
  // Профиль запуска — как в Shell и CrashReportModal: активный из настроек,
  // иначе первый в списке.
  const active =
    accounts.find((a) => a.id === settings?.accountsActiveId) ?? accounts[0] ?? null;

  const items = useMemo<PaletteItem[]>(() => {
    // (а) Навигация по разделам (те же ключи, что в сайдбаре Shell).
    const navItems: PaletteItem[] = (
      [
        ["home", House],
        ["instances", FolderOpen],
        ["settings", SettingsIcon],
      ] as const
    ).map(([page, icon]) => ({
      key: `nav-${page}`,
      icon,
      label: t(`nav.${page}`),
      run: () => onNavigate(page),
    }));

    // (б) Инстансы: «Открыть {name}» + «Запустить {name}» (не для running
    // и только если есть профиль — без него запуск невозможен).
    const instanceItems: PaletteItem[] = list.flatMap((inst) => {
      const result: PaletteItem[] = [
        {
          key: `open-${inst.id}`,
          icon: Box,
          label: t("shell.palette.openInstance", { name: inst.name }),
          run: () => onOpenInstance(inst.id),
        },
      ];
      if (active && status[inst.id]?.phase !== "running") {
        result.push({
          key: `launch-${inst.id}`,
          icon: Play,
          label: t("shell.palette.launch", { name: inst.name }),
          run: () => onLaunchInstance(inst.id, active.name),
        });
      }
      return result;
    });

    // (в) Действия. «Создать инстанс» здесь нет: диалог создания живёт
    // в страницах, Shell им не владеет.
    const actionItems: PaletteItem[] = [
      {
        key: "accounts",
        icon: UserPlus,
        label: t("shell.account.manage"),
        run: onManageAccounts,
      },
    ];

    const q = query.trim().toLowerCase();
    const all =
      q === ""
        ? // Пустой запрос: навигация + аккаунты + первые инстансы (лимит 10 общий).
          [...navItems, ...actionItems, ...instanceItems.slice(0, EMPTY_QUERY_INSTANCES)]
        : // Поиск — по подписи пункта (имя инстанса входит в подпись).
          [...navItems, ...instanceItems, ...actionItems].filter((it) =>
            it.label.toLowerCase().includes(q),
          );
    return all.slice(0, MAX_ITEMS);
  }, [
    query,
    list,
    status,
    active,
    onNavigate,
    onOpenInstance,
    onLaunchInstance,
    onManageAccounts,
  ]);

  // Страховка: после сужения списка индекс мог вылезти за пределы.
  const idx = Math.min(activeIndex, Math.max(0, items.length - 1));

  const runItem = (it: PaletteItem) => {
    it.run();
    onClose();
  };

  const onInputKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIndex(Math.min(idx + 1, items.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIndex(Math.max(idx - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const it = items[idx];
      if (it) runItem(it);
    }
  };

  return (
    <div
      ref={dialogRef}
      role="dialog"
      aria-modal="true"
      aria-label={t("shell.palette.open")}
      tabIndex={-1}
      className="anim-fade-in fixed inset-0 z-[60] flex items-start justify-center bg-black/70 p-4 pt-[12vh]"
      onClick={(e) => {
        if (e.target === e.currentTarget) requestClose();
      }}
    >
      <div className="anim-dialog-in flex w-[560px] max-w-full flex-col overflow-hidden rounded-lg border border-border-strong bg-card shadow-2xl">
        {/* Combobox-паттерн ARIA: ввод + listbox с aria-activedescendant. */}
        <input
          type="text"
          role="combobox"
          aria-expanded="true"
          aria-haspopup="listbox"
          aria-autocomplete="list"
          aria-controls="command-palette-list"
          aria-activedescendant={items[idx] ? `command-palette-opt-${idx}` : undefined}
          aria-label={t("shell.palette.placeholder")}
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setActiveIndex(0); // Новый ввод — курсор с начала списка.
          }}
          onKeyDown={onInputKeyDown}
          placeholder={t("shell.palette.placeholder")}
          className="h-12 w-full border-b border-border-app bg-transparent px-4 text-text outline-none placeholder:text-text-muted"
        />

        {items.length === 0 ? (
          <div role="status" className="px-4 py-6 text-center text-sm text-text-muted">
            {t("shell.search.noResults")}
          </div>
        ) : (
          <ul
            id="command-palette-list"
            role="listbox"
            aria-label={t("shell.palette.open")}
            className="max-h-[50vh] overflow-y-auto p-1"
          >
            {items.map((it, i) => (
              <li
                key={it.key}
                id={`command-palette-opt-${i}`}
                role="option"
                aria-selected={i === idx}
                ref={(el) => {
                  // Активная строка всегда видна (block:nearest — без прыжков).
                  if (i === idx) el?.scrollIntoView({ block: "nearest" });
                }}
                // preventDefault: клик не уводит фокус из поля ввода.
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => runItem(it)}
                className={`flex h-10 cursor-pointer items-center gap-3 rounded-md px-3 text-sm transition-colors duration-100 ${
                  i === idx
                    ? "bg-accent-soft text-text"
                    : "text-text-muted hover:bg-surface-2 hover:text-text"
                }`}
              >
                <it.icon size={16} aria-hidden />
                <span className="truncate">{it.label}</span>
              </li>
            ))}
          </ul>
        )}

        <div className="border-t border-border-app px-4 py-2 text-xs text-text-muted">
          {t("shell.palette.hint")}
        </div>
      </div>
    </div>
  );
}
