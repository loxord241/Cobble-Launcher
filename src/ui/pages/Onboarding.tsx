// Онбординг: 3 шага — каталог → профиль → первый инстанс (спека §8).
// Пропускаемый; «Настройки → Сбросить онбординг» возвращает сюда.
import { useEffect, useState } from "react";
import { api } from "../../api/client";
import { t } from "../../i18n";
import { useInstances } from "../../state/instances";
import { useSettings } from "../../state/settings";

export default function Onboarding({ onDone }: { onDone: () => void }) {
  const [step, setStep] = useState(1);
  const [nick, setNick] = useState("Player");
  const [dataDir, setDataDir] = useState("");
  const [versions, setVersions] = useState<string[]>([]);
  const [version, setVersion] = useState("");
  const { settings, update } = useSettings();
  const { create } = useInstances();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.dataDir().then(setDataDir).catch(() => setDataDir("?"));
    api
      .manifestVersions(false, false)
      .then((vs) => {
        setVersions(vs.map((v) => v.id));
        setVersion(vs[0]?.id ?? "");
      })
      .catch((e) => setError(e.message ?? String(e)));
  }, []);

  const finish = async () => {
    setBusy(true);
    setError(null);
    try {
      const cleanNick = nick.trim();
      if (cleanNick) {
        try {
          const acc = await api.accountAddOffline(cleanNick);
          await update({ accountsActiveId: acc.id, onboardingDone: true });
        } catch {
          await update({ onboardingDone: true });
        }
      } else {
        await update({ onboardingDone: true });
      }
      if (version) {
        await create(version, `${version}`);
      }
      onDone();
    } catch (e) {
      setError((e as { message?: string })?.message ?? String(e));
    } finally {
      setBusy(false);
    }
  };

  /** D27: «Пропустить» тоже сохраняет onboardingDone — мастер не вернётся. */
  const skip = async () => {
    setBusy(true);
    setError(null);
    try {
      await update({ onboardingDone: true });
      onDone();
    } catch (e) {
      setError((e as { message?: string })?.message ?? String(e));
    } finally {
      setBusy(false);
    }
  };

  void settings;

  return (
    <div className="flex h-screen items-center justify-center bg-bg text-text">
      <div className="w-[520px] rounded-lg border border-border-app bg-card p-6">
        <p className="text-sm text-text-muted">{t("onboarding.step", { n: step })}</p>
        <h1 className="mt-1 text-2xl font-semibold">{t("onboarding.welcome")}</h1>

        {step === 1 && (
          <div className="mt-5">
            <h2 className="font-semibold">{t("onboarding.dir")}</h2>
            <p className="mt-2 text-sm text-text-muted">{t("onboarding.dir.hint")}</p>
            <code className="mt-3 block truncate rounded-md bg-bg px-3 py-2 text-sm">{dataDir}</code>
          </div>
        )}

        {step === 2 && (
          <div className="mt-5">
            <h2 className="font-semibold">{t("onboarding.account")}</h2>
            <p className="mt-2 text-sm text-text-muted">{t("onboarding.account.hint")}</p>
            <label className="mt-3 block text-sm text-text-muted" htmlFor="ob-nick">
              {t("onboarding.account.nick")}
            </label>
            <input
              id="ob-nick"
              value={nick}
              onChange={(e) => setNick(e.target.value)}
              className="mt-1 w-full rounded-md border border-border-app bg-bg px-3 py-2"
            />
          </div>
        )}

        {step === 3 && (
          <div className="mt-5">
            <h2 className="font-semibold">{t("onboarding.instance")}</h2>
            <p className="mt-2 text-sm text-text-muted">{t("onboarding.instance.hint")}</p>
            <select
              value={version}
              onChange={(e) => setVersion(e.target.value)}
              className="mt-3 w-full rounded-md border border-border-app bg-bg px-3 py-2"
            >
              {versions.map((v) => (
                <option key={v} value={v}>
                  {v}
                </option>
              ))}
            </select>
          </div>
        )}

        {/* Ошибка видна на всех шагах: «Пропустить» тоже пишет в ядро (D27). */}
        {error && <p className="mt-3 text-sm text-error" role="alert">{error}</p>}

        <div className="mt-6 flex items-center gap-3">
          <button
            onClick={() => void skip()}
            disabled={busy}
            className="text-sm text-text-muted hover:text-text"
          >
            {t("onboarding.skip")}
          </button>
          <div className="ml-auto flex gap-2">
            {step > 1 && (
              <button
                onClick={() => setStep(step - 1)}
                className="btn-ghost btn-sm"
              >
                ←
              </button>
            )}
            {step < 3 ? (
              <button
                onClick={() => setStep(step + 1)}
                className="btn-primary"
              >
                {t("onboarding.next")}
              </button>
            ) : (
              <button
                onClick={() => void finish()}
                disabled={busy || !version}
                className="btn-primary"
              >
                {t("onboarding.finish")}
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
