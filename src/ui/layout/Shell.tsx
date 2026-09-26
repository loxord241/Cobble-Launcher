// Shell: левая навигация + контент (спека §8). Компоненты — только презентация.
import { useEffect, useState } from "react";
import { House, FolderOpen, Settings as SettingsIcon, Play, Puzzle } from "lucide-react";
import { t } from "../../i18n";
import { useInstances } from "../../state/instances";
import { useSettings } from "../../state/settings";
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

export default function Shell({ onLogout }: { onLogout: () => void }) {
  const [page, setPage] = useState<Page>("home");
  const { load, subscribeEvents, list, status } = useInstances();
  const { load: loadSettings } = useSettings();

  useEffect(() => {
    void loadSettings().then(() => void load());
    void subscribeEvents();
  }, [load, loadSettings, subscribeEvents]);

  const player = "Player"; // офлайн-профиль по умолчанию (M8 добавит аккаунты)
  const running = list.some((i) => {
    const st = status[i.id];
    return st && st.phase !== "exited";
  });

  const launchFirst = async () => {
    const first = list[0];
    if (first) await useInstances.getState().launch(first.id, player);
  };

  return (
    <div className="flex h-screen bg-bg text-text">
      <nav className="flex w-16 flex-col items-center gap-2 border-r border-border-app py-3">
        <div className="mb-2 text-xl font-bold text-accent">M2</div>
        {NAV.map(({ id, icon: Icon, label }) => (
          <button
            key={id}
            onClick={() => setPage(id)}
            title={t(label)}
            aria-label={t(label)}
            className={`flex h-11 w-11 items-center justify-center rounded-md transition-colors ${
              page === id ? "bg-card text-accent" : "text-text-muted hover:bg-card"
            }`}
          >
            <Icon size={20} aria-hidden />
          </button>
        ))}
        <div className="mt-auto">
          <button
            onClick={onLogout}
            title="Сменить профиль"
            aria-label="Сменить профиль"
            className="flex h-9 w-9 items-center justify-center rounded-full bg-card text-xs font-bold uppercase text-text-muted"
          >
            {player.slice(0, 2)}
          </button>
        </div>
      </nav>

      <main className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 items-center gap-3 border-b border-border-app px-5">
          <span className="text-sm text-text-muted">{t("app.name")}</span>
          <button
            onClick={() => void launchFirst()}
            disabled={list.length === 0 || running}
            className="ml-auto flex items-center gap-2 rounded-md bg-accent px-5 py-2 font-semibold text-accent-contrast transition-colors hover:bg-accent-hover disabled:cursor-not-allowed disabled:opacity-40"
          >
            <Play size={16} aria-hidden />
            {running ? t("play.running") : t("play")}
          </button>
        </header>
        <div className="min-h-0 flex-1 overflow-y-auto p-5">
          {page === "home" && <HomePage onCreate={() => setPage("instances")} />}
          {page === "instances" && <InstancesPage />}
          {page === "mods" && <ModsPage />}
          {page === "settings" && (
            <SettingsPage onOnboardingReset={onLogout} />
          )}
        </div>
      </main>
    </div>
  );
}
