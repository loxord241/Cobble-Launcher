// Настройки: тема, RAM, параллелизм, снапшоты, сброс онбординга (спека §8).
import { useEffect, useState } from "react";
import { useSettings } from "../../state/settings";
import { api } from "../../api/client";
import { t } from "../../i18n";

export default function SettingsPage({ onOnboardingReset }: { onOnboardingReset: () => void }) {
  const { settings, update } = useSettings();
  const [dataDir, setDataDir] = useState("");

  useEffect(() => {
    api.dataDir().then(setDataDir).catch(() => setDataDir("?"));
  }, []);

  if (!settings) return null;

  return (
    <div className="max-w-xl">
      <h1 className="text-xl font-semibold">{t("nav.settings")}</h1>

      <section className="mt-5 space-y-4">
        <Row label={t("settings.theme")}>
          <select
            value={settings.theme}
            onChange={(e) => void update({ theme: e.target.value as "dark" | "light" })}
            className="w-44 rounded-md border border-border-app bg-bg px-3 py-2"
          >
            <option value="dark">{t("settings.theme.dark")}</option>
            <option value="light">{t("settings.theme.light")}</option>
          </select>
        </Row>

        <Row label={t("settings.ram")}>
          <input
            type="number"
            min={1024}
            max={32768}
            step={512}
            value={settings.defaultRamMb}
            onChange={(e) => void update({ defaultRamMb: Number(e.target.value) || 2048 })}
            className="w-44 rounded-md border border-border-app bg-bg px-3 py-2"
          />
        </Row>

        <Row label={t("settings.parallelism")}>
          <input
            type="number"
            min={1}
            max={64}
            value={settings.downloadParallelism}
            onChange={(e) => void update({ downloadParallelism: Number(e.target.value) || 16 })}
            className="w-44 rounded-md border border-border-app bg-bg px-3 py-2"
          />
        </Row>

        <Row label={t("settings.snapshots")}>
          <Toggle
            checked={settings.showSnapshots}
            onChange={(v) => void update({ showSnapshots: v })}
          />
        </Row>

        <Row label={t("settings.oldVersions")}>
          <Toggle
            checked={settings.showOldVersions}
            onChange={(v) => void update({ showOldVersions: v })}
          />
        </Row>

        <Row label={t("settings.dataDir")}>
          <div className="flex items-center gap-2">
            <code className="max-w-72 truncate rounded-md bg-bg px-2 py-1 text-xs text-text-muted">
              {dataDir}
            </code>
            <button
              onClick={() => void api.dirOpen()}
              className="rounded-md border border-border-app px-3 py-1.5 text-sm hover:bg-card"
            >
              {t("settings.open")}
            </button>
          </div>
        </Row>

        <Row label={t("settings.resetOnboarding")}>
          <button
            onClick={onOnboardingReset}
            className="rounded-md border border-border-app px-3 py-1.5 text-sm hover:bg-card"
          >
            {t("settings.resetOnboarding")}
          </button>
        </Row>
      </section>
    </div>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 rounded-md border border-border-app bg-card px-4 py-3">
      <span className="text-sm">{label}</span>
      {children}
    </div>
  );
}

function Toggle({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className={`h-6 w-11 rounded-full transition-colors ${checked ? "bg-accent" : "bg-border-app"}`}
    >
      <span
        className={`block h-5 w-5 rounded-full bg-white transition-transform ${
          checked ? "translate-x-5" : "translate-x-0.5"
        }`}
      />
    </button>
  );
}
