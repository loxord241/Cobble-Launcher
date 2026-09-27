// Shell: титлбар 52px + сайдбар 216px + контент — каркас «Сланца»
// (variant-01-slate.html). Компоненты — только презентация; данные из сторов/IPC.
import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getVersion } from "@tauri-apps/api/app";
import {
  House,
  FolderOpen,
  Settings as SettingsIcon,
  Puzzle,
  Box,
  Search,
  Minus,
  Square,
  X,
  ChevronDown,
  Check,
} from "lucide-react";
import { t } from "../../i18n";
import { useInstances } from "../../state/instances";
import { useSettings } from "../../state/settings";
import { useAccounts, accountKindLabelKey } from "../../state/accounts";
import HomePage from "../pages/HomePage";
import InstancesPage from "../pages/InstancesPage";
import ModsPage from "../pages/ModsPage";
import SettingsPage from "../pages/SettingsPage";

type Page = "home" | "instances" | "mods" | "settings";

const NAV: { id: Page; icon: typeof House; label: string }[] = [
  { id: "home", icon: House, label: "nav.home" },
  { id: "instances", icon: FolderOpen, label: "nav.instances" },
  { id: "mods", icon: Puzzle, label: "nav.mods" },
  { id: "settings", icon: SettingsIcon, label: "nav.settings" },
];

const win = () => getCurrentWindow();

export default function Shell({ onLogout }: { onLogout: () => void }) {
  const [page, setPage] = useState<Page>("home");
  const [query, setQuery] = useState("");
  const [version, setVersion] = useState("0.1.0");
  const [accountOpen, setAccountOpen] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);
  const { load, subscribeEvents, list } = useInstances();
  const { settings, load: loadSettings } = useSettings();
  const { list: accounts, load: loadAccounts, setActive } = useAccounts();

  useEffect(() => {
    void loadSettings().then(() => void load());
    void subscribeEvents();
    void loadAccounts();
    getVersion().then(setVersion).catch(() => setVersion("0.1.0"));
  }, [load, loadSettings, subscribeEvents, loadAccounts]);

  // Ctrl+K — фокус в поиск (спека фазы B); Esc чистит внутри инпута.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key.toLowerCase() === "k") {
        e.preventDefault();
        searchRef.current?.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const onSearchKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Escape") {
      setQuery("");
      e.currentTarget.blur();
    }
  };

  const active =
    accounts.find((a) => a.id === settings?.accountsActiveId) ?? accounts[0] ?? null;

  const pickAccount = useCallback(
    (id: string) => {
      setAccountOpen(false);
      void setActive(id).catch(() => undefined);
    },
    [setActive],
  );

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-bg text-text">
      <header
        data-tauri-drag-region
        className="flex h-[52px] flex-none items-center gap-4 border-b border-border-app pl-4 pr-3"
      >
        <div data-tauri-drag-region className="flex items-center gap-2 text-[15px] font-semibold">
          <span className="grid size-6 place-items-center rounded-md bg-accent text-on-accent">
            <Box size={15} aria-hidden />
          </span>
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
              <span
                aria-hidden
                className="grid size-8 place-items-center rounded-md bg-accent-soft text-sm font-bold text-accent"
              >
                {active.name.slice(0, 1).toUpperCase()}
              </span>
              <span className="text-left leading-tight">
                <span className="block text-sm font-semibold">{active.name}</span>
                <span className="block text-xs text-text-muted">
                  {t(accountKindLabelKey[active.kind])}
                </span>
              </span>
              <ChevronDown size={14} aria-hidden className="text-text-muted" />
            </button>
            {accountOpen && (
              <>
                <div className="fixed inset-0 z-40" onClick={() => setAccountOpen(false)} />
                <div
                  role="menu"
                  aria-label={t("shell.account.profile")}
                  className="absolute right-0 top-11 z-50 min-w-48 rounded-lg border border-border-strong bg-card p-1 shadow-lg"
                >
                  {accounts.map((a) => (
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
                      <span className="font-semibold">{a.name}</span>
                      <span className="ml-auto text-xs text-text-muted">
                        {t(accountKindLabelKey[a.kind])}
                      </span>
                    </button>
                  ))}
                </div>
              </>
            )}
          </div>
        ) : (
          <button
            onClick={onLogout}
            aria-label={t("shell.account.profile")}
            className="flex h-10 items-center gap-2 rounded-lg px-2 text-sm font-semibold text-text-muted hover:bg-surface-2 hover:text-text"
          >
            {t("shell.account.profile")}
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

      <div className="grid min-h-0 flex-1 grid-cols-[216px_1fr]">
        <nav
          aria-label={t("app.name")}
          className="flex min-h-0 flex-col gap-0.5 border-r border-border-app p-3"
        >
          {NAV.map(({ id, icon: Icon, label }) => (
            <button
              key={id}
              onClick={() => setPage(id)}
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
                  className="absolute -left-3 top-2 bottom-2 w-[3px] rounded-r bg-accent"
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
            {t("shell.footer.version")} <span className="font-mono">{version}</span>
          </div>
        </nav>

        <main className="min-h-0 overflow-y-auto p-6">
          {page === "home" && (
            <HomePage query={query} onCreate={() => setPage("instances")} />
          )}
          {page === "instances" && <InstancesPage query={query} />}
          {page === "mods" && <ModsPage query={query} />}
          {page === "settings" && <SettingsPage onOnboardingReset={onLogout} />}
        </main>
      </div>
    </div>
  );
}
