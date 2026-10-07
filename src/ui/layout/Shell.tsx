// Shell: титлбар 52px + сайдбар 216px + контент — каркас «Сланца»
// (variant-01-slate.html). Компоненты — только презентация; данные из сторов/IPC.
import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getVersion } from "@tauri-apps/api/app";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useShallow } from "zustand/react/shallow";
import {
  House,
  FolderOpen,
  Settings as SettingsIcon,
  Shirt,
  Box,
  Command,
  Search,
  Minus,
  Square,
  X,
  ChevronDown,
  Check,
  Download,
  LoaderCircle,
  UserPlus,
  Wifi,
  WifiOff,
  AlertTriangle,
} from "lucide-react";
import { apiErrorText, t } from "../../i18n";
import { api } from "../../api/client";
import { useInstances, unsubscribeEvents } from "../../state/instances";
import { useSettings } from "../../state/settings";
import {
  useAccounts,
  accountKindLabelKey,
  accountSortPref,
  sortAccounts,
} from "../../state/accounts";
import { useSkins } from "../../state/skins";
import { useUpdateStore } from "../../state/updates";
import { resolveLaunchTarget } from "../../state/launch";
import { SkinHead } from "../components/SkinViews";
import HomePage from "../pages/HomePage";
import InstancesPage from "../pages/InstancesPage";
import SkinsPage from "../pages/SkinsPage";
import SettingsPage from "../pages/SettingsPage";
import InstanceDetailPage from "../pages/InstanceDetailPage";
import AccountsModal from "../components/AccountsModal";
import CrashReportModal from "../components/CrashReportModal";
import CommandPalette from "../components/CommandPalette";
import { ConfirmHost } from "../confirm";

type Page = "home" | "instances" | "skins" | "settings";

const NAV: { id: Page; icon: typeof House; label: string }[] = [
  { id: "home", icon: House, label: "nav.home" },
  { id: "instances", icon: FolderOpen, label: "nav.instances" },
  { id: "skins", icon: Shirt, label: "nav.skins" },
  { id: "settings", icon: SettingsIcon, label: "nav.settings" },
];

const win = () => getCurrentWindow();

/** F12: ник офлайн-профиля — латиница/цифры/подчёркивание, 3–16 символов. */
const NICK_RE = /^[A-Za-z0-9_]{3,16}$/;

/** Сколько живёт баннер результата drop-установки .mrpack (D5b). */
const NOTICE_MS = 6000;

export default function Shell({ onLogout }: { onLogout: () => void }) {
  const [page, setPage] = useState<Page>("home");
  // Открытый инстанс: пока задан — вместо страницы рендерится его страница
  // в стиле CurseForge (запрос владельца). Навигация слева закрывает её.
  const [openInstanceId, setOpenInstanceId] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [version, setVersion] = useState("");
  const [accountOpen, setAccountOpen] = useState(false);
  // F12: черновик быстрого офлайн-ника в открытом меню профиля.
  const [quickNick, setQuickNick] = useState("");
  const [accountsModalOpen, setAccountsModalOpen] = useState(false);
  // F19: палитра команд (Ctrl+P / кнопка в титлбаре).
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [notice, setNotice] = useState<{ kind: "ok" | "error"; text: string } | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const updateInfo = useUpdateStore((s) => s.info);
  const silentUpdateCheck = useUpdateStore((s) => s.silentCheck);
  // D54: попап установки у бейджа в футере (состояние общее со стором).
  const [updateOpen, setUpdateOpen] = useState(false);
  const updateInstalling = useUpdateStore((s) => s.installing);
  const updateProgress = useUpdateStore((s) => s.progress);
  const updateInstallError = useUpdateStore((s) => s.installError);
  const updateInstall = useUpdateStore((s) => s.install);
  const noticeTimer = useRef<number | null>(null);
  const dropBusy = useRef(false);
  // A8: узкий селектор — сброс лог-буфера каждые 200 мс не должен
  // перерисовывать всё дерево Shell (как в страницах, D8).
  const { load, subscribeEvents, list, crashReport, clearCrashReport, launch } = useInstances(
    useShallow((s) => ({
      load: s.load,
      subscribeEvents: s.subscribeEvents,
      list: s.list,
      crashReport: s.crashReport,
      clearCrashReport: s.clearCrashReport,
      launch: s.launch,
    })),
  );
  // D64: узкие селекторы вместо подписки на сторы целиком — тик слайдера
  // (update кладёт новый объект settings) больше не перерисовывает каркас:
  // читаем ровно те поля, что использует Shell.
  const loadedSettings = useSettings((s) => s.loaded);
  const updateSettings = useSettings((s) => s.update);
  const loadSettings = useSettings((s) => s.load);
  const accountsActiveId = useSettings((s) => s.settings?.accountsActiveId);
  const workOffline = useSettings((s) => !!s.settings?.workOffline);
  const logo = useSettings((s) => s.settings?.logo ?? "");
  // D62: degradedIds — профили с упавшим фоновым refresh токена ядра
  // (account_refresh_failed); непустой набор включает плашку «перелогин».
  const accounts = useAccounts((s) => s.list);
  const degradedIds = useAccounts((s) => s.degradedIds);
  const loadAccounts = useAccounts((s) => s.load);
  const setActive = useAccounts((s) => s.setActive);
  const addOffline = useAccounts((s) => s.addOffline);
  // D64: готовность сторов — ярлычный запуск ждёт её (см. launchFromShortcut).
  const instancesLoaded = useInstances((s) => s.loaded);
  const accountsLoaded = useAccounts((s) => s.loaded);
  // D64: активные загрузки ядра (чип в титлбаре); null — ничего не качается.
  const dlQueue = useInstances((s) => s.dlQueue);

  const showNotice = useCallback((kind: "ok" | "error", text: string) => {
    setNotice({ kind, text });
    if (noticeTimer.current !== null) window.clearTimeout(noticeTimer.current);
    noticeTimer.current = window.setTimeout(() => setNotice(null), NOTICE_MS);
  }, []);

  useEffect(
    () => () => {
      if (noticeTimer.current !== null) window.clearTimeout(noticeTimer.current);
    },
    [],
  );

  // D5b: .mrpack, брошенный на окно, ставится тем же ядерным вызовом, что и из
  // каталога. Файловые drop'ы нативно принимает Tauri (dragDropEnabled=true),
  // поэтому пути берём из onDragDropEvent — HTML5-события их не содержат.
  // D64: файлов в drop'е может быть несколько — обрабатываем ВСЕ последовательно
  // (очередь одного drop'а); drop во время незавершённой установки больше не
  // молчит — показываем баннер shell.drop.busy.
  const installDroppedMrpack = useCallback(
    async (paths: string[]) => {
      if (dropBusy.current) {
        showNotice("error", t("shell.drop.busy"));
        return;
      }
      dropBusy.current = true;
      try {
        for (const path of paths) {
          try {
            // Контракт D64: ядро возвращает { instance, group }.
            const { instance: inst } = await api.mrpackInstall(path);
            showNotice(
              "ok",
              t("shell.drop.installed", { name: inst.name, version: inst.mcVersion }),
            );
            void load(); // новый инстанс — обновляем счётчик в навигации и страницы
          } catch (e) {
            showNotice("error", t("shell.drop.error", { message: apiErrorText(e) }));
          }
        }
      } finally {
        dropBusy.current = false;
      }
    },
    [load, showNotice],
  );

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    let disposed = false;
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type !== "drop") return;
        // Не наш drop (нет .mrpack) — не перехватываем: файл просто игнорируется.
        // D64: собираем ВСЕ .mrpack одного drop'а, а не только первый.
        const paths = event.payload.paths.filter((p) => p.toLowerCase().endsWith(".mrpack"));
        if (paths.length === 0) return;
        void installDroppedMrpack(paths);
      })
      .then((un) => {
        if (disposed) un();
        else unlisten = un;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [installDroppedMrpack]);

  useEffect(() => {
    void loadSettings().then(() => void load());
    void subscribeEvents();
    void loadAccounts();
    // D64: при сбое getVersion версии в футере просто нет — выдумывать
    // «0.2.0» нечестно.
    getVersion().then(setVersion).catch(() => setVersion(""));
    // D46: тихая проверка обновлений — один раз за сессию (задержка внутри
    // стора); результат — бейдж в футере и раздел в Настройках, без тостов.
    silentUpdateCheck();
    // A32: слушатели событий ядра живут ровно пока смонтирован Shell.
    return () => unsubscribeEvents();
  }, [load, loadSettings, subscribeEvents, loadAccounts, silentUpdateCheck]);

  // D64: «открыта модалка» — известные модальные флаги каркаса (модалка
  // аккаунтов + crash-репорт). Палитру поверх открытой модалки не открываем:
  // две ловушки Tab сражались за фокус (ping-pong). Сама палитра закрывается
  // по Esc как раньше.
  const anyModalOpen = accountsModalOpen || crashReport !== null;

  // Ctrl+K — фокус в поиск (спека фазы B); Esc чистит внутри инпута.
  // F19: палитра команд живёт на Ctrl+P, а не на Ctrl+K — приёмка
  // scripts/ui_checks.cjs (#4) ждёт, что Ctrl+K ставит фокус в поле поиска,
  // поэтому перехватывать его палитрой нельзя.
  // D64: сверяем e.code (физическая клавиша), а не e.key — на русской
  // раскладке K/P дают «л»/«з», и оба хоткея были мертвы. Модификатор —
  // как раньше, Ctrl.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey) return;
      if (e.code === "KeyK") {
        e.preventDefault();
        searchRef.current?.focus();
      } else if (e.code === "KeyP") {
        // preventDefault гасит нативную «печать» вебвью.
        e.preventDefault();
        if (anyModalOpen) return;
        setPaletteOpen(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [anyModalOpen]);

  const onSearchKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Escape") {
      setQuery("");
      e.currentTarget.blur();
    }
  };

  /** Выбор страницы в навигации: открытая страница инстанса при этом закрывается. */
  const goToPage = (id: Page) => {
    setPage(id);
    setOpenInstanceId(null);
  };

  const active =
    accounts.find((a) => a.id === accountsActiveId) ?? accounts[0] ?? null;

  // D48: ярлык рабочего стола — single-instance пересылает `--id` вторым
  // процессом: либо событием shortcut_launch, либо через shortcut_take_pending
  // (окно открылось позже колбэка). Запуск — тем же потоком, что и кнопка:
  // статусы и прогресс придут через launch_state как обычно.
  // D64: ярлычный запуск откладывается до загрузки сторов (instances+accounts):
  // resolveLaunchTarget резолвит привязку профиля по их спискам, и до загрузки
  // запуск молча игнорировал её. Механизм shortcut_take_pending сохранён —
  // «pending» просто держится в ref, пока сторы не готовы.
  const launchFromShortcut = useCallback(
    async (id: string) => {
      try {
        // F11: привязанный к инстансу профиль активируется и идёт в запуск.
        const player = await resolveLaunchTarget(id);
        await launch(id, player);
      } catch {
        // launch сам кладёт ошибку в стор статусов — тут глушить нечего.
      }
    },
    [launch],
  );

  const pendingShortcutRef = useRef<string | null>(null);
  // Готовность читаем по getState(): колбэк стабилен, слушатели ниже не
  // переподписываются при каждом флипе loaded.
  const queueShortcutLaunch = useCallback(
    (id: string) => {
      const ready = useInstances.getState().loaded && useAccounts.getState().loaded;
      if (ready) void launchFromShortcut(id);
      else pendingShortcutRef.current = id;
    },
    [launchFromShortcut],
  );

  // Флеш отложенного ярлычного запуска: оба стора дозагрузились — запускаем.
  useEffect(() => {
    if (!instancesLoaded || !accountsLoaded) return;
    const id = pendingShortcutRef.current;
    if (id === null) return;
    pendingShortcutRef.current = null;
    void launchFromShortcut(id);
  }, [instancesLoaded, accountsLoaded, launchFromShortcut]);

  useEffect(() => {
    let un: UnlistenFn | null = null;
    let disposed = false;
    void api
      .shortcutTakePending()
      .then((id) => {
        if (id) queueShortcutLaunch(id);
      })
      .catch(() => undefined);
    void listen<string>("shortcut_launch", (e) => {
      queueShortcutLaunch(String(e.payload));
    })
      .then((u) => {
        if (disposed) u();
        else un = u;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      un?.();
    };
  }, [queueShortcutLaunch]);


  // Скины профилей (D34): голова персонажа в чипе титлбара и в меню профилей.
  const skinsById = useSkins((s) => s.byId);
  const loadSkin = useSkins((s) => s.load);
  useEffect(() => {
    for (const acc of accounts) void loadSkin(acc);
  }, [accounts, loadSkin]);

  // D38: логотип (иконка окна применяет ядро; data-URL — для бренд-чипа).
  const [brandLogoUrl, setBrandLogoUrl] = useState<string | null>(null);
  useEffect(() => {
    if (!loadedSettings) return;
    let alive = true;
    api
      .applyLogo(logo)
      .then((r) => {
        if (alive) setBrandLogoUrl(r.dataUrl);
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [logo, loadedSettings]);

  const pickAccount = useCallback(
    (id: string) => {
      setAccountOpen(false);
      void setActive(id).catch(() => undefined);
    },
    [setActive],
  );

  // F12: быстрый офлайн-профиль прямо из меню чипа; успех — активируем и
  // закрываем меню, ошибка — баннером (error) через apiErrorText.
  const addQuickNick = () => {
    const nick = quickNick.trim();
    if (!NICK_RE.test(nick)) return;
    void addOffline(nick)
      .then((acc) => setActive(acc.id))
      .then(() => {
        setQuickNick("");
        setAccountOpen(false);
      })
      .catch((e) => showNotice("error", apiErrorText(e)));
  };

  return (
    <div
      className="flex h-screen flex-col overflow-hidden bg-bg text-text"
      // Гасим дефолт webview на файловый drop (иначе он открыл бы файл); сама
      // установка .mrpack идёт через onDragDropEvent выше.
      onDragOver={(e) => e.preventDefault()}
      onDrop={(e) => e.preventDefault()}
    >
      <header
        data-tauri-drag-region
        className="flex h-[52px] flex-none items-center gap-4 border-b border-border-app pl-4 pr-3"
      >
        <div data-tauri-drag-region className="flex items-center gap-2 text-[15px] font-semibold">
          {/* D38: выбранный логотип; встроенный — дефолтная плашка. */}
          {brandLogoUrl ? (
            <img src={brandLogoUrl} alt="" className="size-6 rounded-md object-cover" />
          ) : (
            <span className="grid size-6 place-items-center rounded-md bg-accent text-on-accent">
              <Box size={15} aria-hidden />
            </span>
          )}
          {t("app.name")}
        </div>

        <div className="ml-2 flex h-9 w-[360px] max-w-[36vw] items-center gap-2 rounded-lg border border-border-app bg-card px-3 text-text-muted">
          <Search size={16} aria-hidden />
          <input
            ref={searchRef}
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onSearchKeyDown}
            placeholder={t("shell.search.instances")}
            aria-label={t("shell.search.instances")}
            className="min-w-0 flex-1 bg-transparent text-text outline-none placeholder:text-text-muted"
          />
          <kbd className="rounded border border-border-strong px-1.5 font-mono text-xs leading-[18px] text-text-muted">
            Ctrl K
          </kbd>
        </div>

        <div data-tauri-drag-region className="flex-1" />

        {/* F19: палитра команд (Ctrl+P); обычная кнопка — под drag-region
            не попадает, клики работают. */}
        <button
          onClick={() => setPaletteOpen(true)}
          aria-label={t("shell.palette.open")}
          title={t("shell.palette.open")}
          className="icon-btn"
        >
          <Command size={16} aria-hidden />
        </button>

        {/* F16: «Работать офлайн» — сеть ядра отключается, запуск из кэша.
            Обычная кнопка в flex-ряду — под drag-region не попадает. */}
        <button
          role="switch"
          aria-checked={workOffline}
          aria-label={t("shell.offline")}
          title={t("shell.offline.hint")}
          onClick={() =>
            void updateSettings({ workOffline: !workOffline }).catch((e) =>
              showNotice("error", apiErrorText(e)),
            )
          }
          className="icon-btn"
        >
          {workOffline ? (
            <WifiOff key="off" size={16} aria-hidden className="anim-fade-in" />
          ) : (
            <Wifi key="on" size={16} aria-hidden className="anim-fade-in" />
          )}
        </button>

        {/* D64: активные загрузки ядра — маленький чип «иконка + сколько»
            (pending+downloading из dlQueue стора инстансов), без кликов и
            переходов. Нечего качать (dlQueue null) — элемента нет вовсе. */}
        {dlQueue && (
          <div
            role="status"
            title={t("shell.dl.active", { count: dlQueue.pending + dlQueue.downloading })}
            aria-label={t("shell.dl.active", { count: dlQueue.pending + dlQueue.downloading })}
            className="flex h-8 items-center gap-1.5 rounded-lg border border-border-app bg-card px-2 text-xs text-text-muted"
          >
            <Download size={13} aria-hidden className="text-accent" />
            <span className="font-mono">{dlQueue.pending + dlQueue.downloading}</span>
          </div>
        )}

        {/* Чип аккаунта: реальный активный профиль из ядра (M8) */}
        {active ? (
          <div className="relative">
            <button
              onClick={() => setAccountOpen((v) => !v)}
              aria-label={t("account.chip.label", {
                name: active.name,
                kind: t(accountKindLabelKey[active.kind]),
              })}
              aria-expanded={accountOpen}
              className="flex h-10 items-center gap-2 rounded-lg px-1 hover:bg-surface-2"
            >
              <SkinHead name={active.name} skin={skinsById[active.id]} size={32} />
              <span className="text-left leading-tight">
                <span className="block text-sm font-semibold">{active.name}</span>
                <span className="block text-xs text-text-muted">
                  {t(accountKindLabelKey[active.kind])}
                </span>
              </span>
              <ChevronDown
                size={14}
                aria-hidden
                className={`text-text-muted transition-transform ${accountOpen ? "rotate-180" : ""}`}
              />
            </button>
            {accountOpen && (
              <>
                <div className="fixed inset-0 z-40" onClick={() => setAccountOpen(false)} />
                <div
                  role="menu"
                  aria-label={t("shell.account.profile")}
                  className="anim-menu-in origin-top-right absolute right-0 top-11 z-50 min-w-48 rounded-lg border border-border-strong bg-card p-1 shadow-lg"
                >
                  {/* Тот же порядок, что в модалке аккаунтов: активный первый,
                      дальше по предвыбранной сортировке (D53). Список длинный —
                      свой скролл с потолком, «Управление аккаунтами» и быстрый
                      ник закреплены под ним (жалоба владельца: без максимизации
                      окна до действий было не дотянуться). */}
                  <div className="max-h-[45vh] overflow-y-auto overscroll-contain">
                    {sortAccounts(accounts, accountSortPref(), active?.id).map((a) => (
                    <button
                      key={a.id}
                      role="menuitemradio"
                      aria-checked={a.id === active.id}
                      onClick={() => pickAccount(a.id)}
                      className="flex h-10 w-full items-center gap-2 rounded-md px-2 text-left text-sm hover:bg-surface-2"
                    >
                      <span className="w-4">
                        {a.id === active.id && <Check size={14} aria-hidden />}
                      </span>
                      <SkinHead name={a.name} skin={skinsById[a.id]} size={16} />
                      <span className="font-semibold">{a.name}</span>
                      <span className="ml-auto text-xs text-text-muted">
                        {t(accountKindLabelKey[a.kind])}
                      </span>
                    </button>
                  ))}
                  </div>
                  <div className="my-1 border-t border-border-app" />
                  <button
                    role="menuitem"
                    onClick={() => {
                      setAccountOpen(false);
                      setAccountsModalOpen(true);
                    }}
                    className="flex h-10 w-full items-center gap-2 rounded-md px-2 text-left text-sm text-accent hover:bg-surface-2"
                  >
                    <UserPlus size={15} aria-hidden />
                    <span className="font-semibold">{t("shell.account.manage")}</span>
                  </button>
                  {/* F12: быстрый офлайн-ник. Input внутри меню (z-50) — клики
                      по нему не задевают закрывающий overlay (z-40). */}
                  <div className="my-1 border-t border-border-app" />
                  <div className="flex items-center gap-1 p-1">
                    <input
                      type="text"
                      value={quickNick}
                      onChange={(e) => setQuickNick(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") addQuickNick();
                      }}
                      placeholder={t("accounts.quickNick")}
                      aria-label={t("accounts.quickNick")}
                      className="h-8 min-w-0 flex-1 rounded-md border border-border-strong bg-bg px-2 text-sm outline-none focus:border-accent"
                    />
                    <button
                      type="button"
                      onClick={addQuickNick}
                      disabled={!NICK_RE.test(quickNick.trim())}
                      className="flex h-8 shrink-0 items-center rounded-md px-2 text-sm font-semibold text-accent hover:bg-surface-2 disabled:cursor-not-allowed disabled:text-text-muted/40"
                    >
                      {t("accounts.quickNick.add")}
                    </button>
                  </div>
                </div>
              </>
            )}
          </div>
        ) : (
          <button
            onClick={() => setAccountsModalOpen(true)}
            aria-label={t("shell.account.profile")}
            className="flex h-10 items-center gap-2 rounded-lg px-2 text-sm font-semibold text-text-muted hover:bg-surface-2 hover:text-text"
          >
            <UserPlus size={16} aria-hidden />
            {t("shell.account.addProfile")}
          </button>
        )}

        <div className="flex gap-1">
          <button
            onClick={() => void win().minimize()}
            aria-label={t("shell.window.minimize")}
            className="icon-btn"
          >
            <Minus size={16} aria-hidden />
          </button>
          <button
            onClick={() => void win().toggleMaximize()}
            aria-label={t("shell.window.maximize")}
            className="icon-btn"
          >
            <Square size={14} aria-hidden />
          </button>
          <button
            onClick={() => void win().close()}
            aria-label={t("shell.window.close")}
            className="icon-btn hover:!bg-error hover:!text-on-danger"
          >
            <X size={16} aria-hidden />
          </button>
        </div>
      </header>

      {/* D62: «требуется перелогин» — тонкая полоса под титлбаром, тот же
          паттерн, что у notice-баннера ниже. «Нет данных — нет элемента»:
          деградировавших профилей нет — плашка не рендерится вовсе. Клик
          открывает модалку аккаунтов тем же механизмом, что и чип. */}
      {degradedIds.size > 0 && (
        <button
          type="button"
          data-testid="relogin-banner"
          aria-label={t("accounts.reloginBanner")}
          title={t("accounts.reloginBanner")}
          onClick={() => setAccountsModalOpen(true)}
          className="anim-slide-down mx-3 mt-3 flex items-center gap-2 rounded-lg border border-warning/40 bg-warning/10 p-3 text-left text-sm text-warning hover:bg-warning/15"
        >
          <AlertTriangle size={16} aria-hidden className="shrink-0" />
          {t("accounts.reloginBanner")}
        </button>
      )}

      {/* Результат drop-установки .mrpack: тот же инлайн-баннер, что в страницах. */}
      {notice && (
        <div
          role={notice.kind === "error" ? "alert" : "status"}
          className={`anim-slide-down mx-3 mt-3 rounded-lg border p-3 text-sm ${
            notice.kind === "error"
              ? "border-error/40 bg-error/10 text-error"
              : "border-success/40 bg-success/10 text-success"
          }`}
        >
          {notice.text}
        </div>
      )}

      <div className="grid min-h-0 flex-1 grid-cols-[216px_1fr]">
        <nav
          aria-label={t("app.name")}
          className="flex min-h-0 flex-col gap-0.5 border-r border-border-app p-3"
        >
          {NAV.map(({ id, icon: Icon, label }) => (
            <button
              key={id}
              onClick={() => goToPage(id)}
              aria-current={page === id ? "page" : undefined}
              className={`relative flex h-10 items-center gap-3 rounded-lg px-3 text-sm font-medium ${
                page === id
                  ? "bg-accent-soft text-text"
                  : "text-text-muted hover:bg-surface-2 hover:text-text"
              }`}
            >
              {page === id && (
                <span
                  aria-hidden
                  className="anim-fade-in absolute -left-3 top-2 bottom-2 w-[3px] rounded-r bg-accent"
                />
              )}
              <Icon size={18} aria-hidden />
              {t(label)}
              {id === "instances" && list.length > 0 && (
                <span className="ml-auto font-mono text-xs text-text-muted">{list.length}</span>
              )}
            </button>
          ))}
          <div className="mt-auto border-t border-border-app p-3 text-xs text-text-muted">
            {/* D54: клик по бейджу открывает попап установки прямо здесь —
                раньше перекидывал в настройки, владелец просил «не кидать». */}
            {updateInfo?.updateAvailable && updateInfo.latest && (
              <div className="relative mb-2">
                <button
                  type="button"
                  onClick={() => setUpdateOpen((v) => !v)}
                  aria-expanded={updateOpen}
                  className="flex w-full items-center gap-1.5 rounded-md border border-accent/40 bg-accent/10 px-2 py-1.5 text-accent hover:bg-accent/15"
                  title={t("updates.availableBadge")}
                >
                  <Download size={12} aria-hidden />
                  <span className="truncate">{t("updates.availableBadge")}</span>
                  <span className="ml-auto font-mono">{updateInfo.latest}</span>
                </button>
                {updateOpen && (
                  <>
                    {/* Подложка: клик мимо закрывает (во время установки
                        попап не закрываем — не дать сорвать скачивание). */}
                    {!updateInstalling && (
                      <div
                        className="fixed inset-0 z-40"
                        onClick={() => setUpdateOpen(false)}
                        aria-hidden
                      />
                    )}
                    <div
                      role="dialog"
                      aria-label={t("updates.availableBadge")}
                      className="anim-fade-up absolute bottom-full left-0 z-50 mb-2 w-72 rounded-lg border border-border-app bg-card p-4 shadow-2xl"
                    >
                      <div className="flex items-center gap-2 text-sm font-semibold text-text">
                        <Download size={14} aria-hidden className="text-accent" />
                        {t("updates.availableBadge")}
                        <span className="ml-auto font-mono text-xs text-text-muted">
                          {updateInfo.latest}
                        </span>
                      </div>
                      <button
                        type="button"
                        onClick={() => void updateInstall()}
                        disabled={updateInstalling}
                        aria-busy={updateInstalling}
                        className="btn-primary btn-sm mt-3 w-full"
                      >
                        {updateInstalling ? (
                          <LoaderCircle size={14} aria-hidden className="animate-spin" />
                        ) : (
                          <Download size={14} aria-hidden />
                        )}
                        {updateInstalling ? t("updates.installing") : t("updates.installNow")}
                      </button>
                      {updateInstalling && updateProgress !== null && (
                        <div className="mt-2 flex items-center gap-2">
                          <div
                            className="h-1.5 flex-1 overflow-hidden rounded-full bg-surface-2"
                            role="progressbar"
                            aria-valuenow={updateProgress}
                            aria-valuemin={0}
                            aria-valuemax={100}
                          >
                            <div
                              className="h-full rounded-full bg-success transition-[width] duration-300"
                              style={{ width: `${updateProgress}%` }}
                            />
                          </div>
                          <span className="font-mono text-xs">{updateProgress}%</span>
                        </div>
                      )}
                      {updateInstallError && (
                        <p role="alert" className="mt-2 text-xs leading-relaxed text-warning">
                          {updateInstallError}
                        </p>
                      )}
                      <button
                        type="button"
                        onClick={() => {
                          setUpdateOpen(false);
                          goToPage("settings");
                        }}
                        className="mt-2 w-full text-left text-xs text-text-muted underline decoration-dotted underline-offset-2 hover:text-text hover:decoration-solid"
                      >
                        {t("updates.moreInSettings")}
                      </button>
                    </div>
                  </>
                )}
              </div>
            )}
            {version && (
              <span>
                {t("shell.footer.version")} <span className="font-mono">{version}</span>
              </span>
            )}
            {/* A14: обязательный дисклеймер Mojang об неофициальном статусе. */}
            <div className="mt-1 text-balance">{t("shell.disclaimer")}</div>
          </div>
        </nav>

        <main className="min-h-0 overflow-y-auto p-6">
          {/* D41: смена страницы/инстанса — remount по key переигрывает вход.
              Обычный див: main не flex, max-w держат сами страницы. */}
          <div key={openInstanceId ?? page} className="anim-page-in">
            {openInstanceId ? (
              // Добавление модов — прямо на странице инстанса (D39), без
              // перекидывания на страницу «Моды».
              <InstanceDetailPage
                instanceId={openInstanceId}
                onBack={() => setOpenInstanceId(null)}
              />
            ) : (
              <>
                {page === "home" && <HomePage query={query} />}
                {page === "instances" && (
                  <InstancesPage query={query} onOpenInstance={setOpenInstanceId} />
                )}
                {page === "skins" && <SkinsPage />}
                {page === "settings" && <SettingsPage onOnboardingReset={onLogout} />}
              </>
            )}
          </div>
        </main>
      </div>

      {accountsModalOpen && (
        <AccountsModal onClose={() => setAccountsModalOpen(false)} />
      )}

      {/* Глобальный confirm (askConfirm) — всегда смонтирован, поверх z-50 модалок. */}
      <ConfirmHost />

      {/* F19: палитра команд. Открытие инстанса — через список, чтобы
          навигация закрыла прежнюю страницу; затем ставим целевой инстанс. */}
      {paletteOpen && (
        <CommandPalette
          onClose={() => setPaletteOpen(false)}
          onNavigate={goToPage}
          onOpenInstance={(id) => {
            goToPage("instances");
            setOpenInstanceId(id);
          }}
          onLaunchInstance={(id, player) => void launch(id, player)}
          onManageAccounts={() => setAccountsModalOpen(true)}
        />
      )}

      {crashReport && (
        <CrashReportModal report={crashReport} onClose={clearCrashReport} />
      )}
    </div>
  );
}
