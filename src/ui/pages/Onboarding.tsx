// Онбординг: 3 шага — каталог → профиль → первый инстанс (спека §8).
// Пропускаемый; «Настройки → Сбросить онбординг» возвращает сюда.
import { useEffect, useRef, useState } from "react";
import { api } from "../../api/client";
import { apiErrorText, t } from "../../i18n";
import { useInstances } from "../../state/instances";
import { useSettings } from "../../state/settings";

// Всего шагов мастера (индикатор прогресса и условие «Далее/Готово»).
const TOTAL_STEPS = 3;

// F12: ник офлайн-профиля — латиница/цифры/подчёркивание, 3–16 символов
// (тот же регекс, что в Shell для быстрого добавления профиля).
const NICK_RE = /^[A-Za-z0-9_]{3,16}$/;

export default function Onboarding({ onDone }: { onDone: () => void }) {
  const [step, setStep] = useState(1);
  // Направление перехода шага: сравниваем новый номер с предыдущим (D41).
  const prevStepRef = useRef(step);
  const direction: "next" | "back" = step >= prevStepRef.current ? "next" : "back";
  useEffect(() => {
    prevStepRef.current = step;
  }, [step]);
  const [nick, setNick] = useState("Player");
  const [dataDir, setDataDir] = useState("");
  const [versions, setVersions] = useState<string[]>([]);
  const [version, setVersion] = useState("");
  const { settings, update } = useSettings();
  const { create } = useInstances();
  const [busy, setBusy] = useState(false);
  // D64: disabled={busy} срабатывает только после ре-рендера — два быстрых
  // клика по «Готово» в один тик успевали дважды войти в finish и плодить
  // дубликаты офлайн-профиля/инстанса. Одноразовость гарантирует ref.
  const inFlightRef = useRef(false);
  const [error, setError] = useState<string | null>(null);
  // Ник: пустой — «без профиля» (мастер пропустит создание), непустой и не по
  // маске — инлайн-ошибка, «Далее» заблокирован (раньше мастер молчал).
  const nickInvalid = nick.trim() !== "" && !NICK_RE.test(nick.trim());

  useEffect(() => {
    api.dataDir().then(setDataDir).catch(() => setDataDir("?"));
    api
      .manifestVersions(false, false)
      .then((vs) => {
        setVersions(vs.map((v) => v.id));
        setVersion(vs[0]?.id ?? "");
      })
      .catch((e) => setError(apiErrorText(e)));
  }, []);

  const finish = async () => {
    if (inFlightRef.current) return;
    inFlightRef.current = true;
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
      setError(apiErrorText(e));
    } finally {
      setBusy(false);
      inFlightRef.current = false;
    }
  };

  /** D27: «Пропустить» тоже сохраняет onboardingDone — мастер не вернётся. */
  const skip = async () => {
    if (inFlightRef.current) return;
    inFlightRef.current = true;
    setBusy(true);
    setError(null);
    try {
      await update({ onboardingDone: true });
      onDone();
    } catch (e) {
      setError(apiErrorText(e));
    } finally {
      setBusy(false);
      inFlightRef.current = false;
    }
  };

  void settings;

  return (
    <div className="flex h-screen items-center justify-center bg-bg text-text">
      <div className="w-[520px] rounded-lg border border-border-app bg-card p-6">
        <div className="flex items-center gap-2">
          <p className="text-sm text-text-muted">{t("onboarding.step", { n: step })}</p>
          <div className="flex justify-center gap-2">
            {Array.from({ length: TOTAL_STEPS }, (_, i) => (
              <span
                key={i}
                className={`size-2 rounded-full transition-all duration-200 ${
                  i + 1 === step ? "bg-accent scale-110" : "bg-surface-2"
                }`}
              />
            ))}
          </div>
        </div>
        <h1 className="mt-1 text-2xl font-semibold">{t("onboarding.welcome")}</h1>

        {step === 1 && (
          <div
            key={step}
            className={`mt-5 ${direction === "back" ? "anim-step-prev" : "anim-step-next"}`}
          >
            <label className="block text-sm font-medium" htmlFor="ob-lang">
              {t("onboarding.language")}
            </label>
            <select
              id="ob-lang"
              value={settings?.language ?? "en"}
              onChange={(e) => void update({ language: e.target.value }).catch(() => undefined)}
              className="mt-1 w-full rounded-md border border-border-app bg-bg px-3 py-2"
            >
              <option value="en">English</option>
              <option value="ru">Русский</option>
              <option value="uk">Українська</option>
              <option value="pl">Polski</option>
            </select>
            <h2 className="mt-4 font-semibold">{t("onboarding.dir")}</h2>
            <p className="mt-2 text-sm text-text-muted">{t("onboarding.dir.hint")}</p>
            <code className="mt-3 block truncate rounded-md bg-bg px-3 py-2 text-sm">{dataDir}</code>
          </div>
        )}

        {step === 2 && (
          <div
            key={step}
            className={`mt-5 ${direction === "back" ? "anim-step-prev" : "anim-step-next"}`}
          >
            <h2 className="font-semibold">{t("onboarding.account")}</h2>
            <p className="mt-2 text-sm text-text-muted">{t("onboarding.account.hint")}</p>
            <label className="mt-3 block text-sm text-text-muted" htmlFor="ob-nick">
              {t("onboarding.account.nick")}
            </label>
            <input
              id="ob-nick"
              value={nick}
              onChange={(e) => setNick(e.target.value)}
              maxLength={16}
              aria-invalid={nickInvalid || undefined}
              aria-describedby={nickInvalid ? "ob-nick-err" : undefined}
              className={`mt-1 w-full rounded-md border bg-bg px-3 py-2 ${
                nickInvalid ? "border-error/60" : "border-border-app"
              }`}
            />
            {/* Инлайн-ошибка без словарных ключей: маска сама по себе интернациональна. */}
            {nickInvalid && (
              <p id="ob-nick-err" role="alert" className="mt-1 flex items-center gap-2 text-[13px] text-error">
                <code className="rounded bg-bg px-1 font-mono">A-Z a-z 0-9 _</code>
                <span className="font-mono">3–16</span>
              </p>
            )}
          </div>
        )}

        {step === 3 && (
          <div
            key={step}
            className={`mt-5 ${direction === "back" ? "anim-step-prev" : "anim-step-next"}`}
          >
            <h2 className="font-semibold">{t("onboarding.instance")}</h2>
            <p className="mt-2 text-sm text-text-muted">{t("onboarding.instance.hint")}</p>
            <select
              value={version}
              onChange={(e) => setVersion(e.target.value)}
              aria-label={t("onboarding.instance")}
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
            aria-busy={busy}
            className="text-sm text-text-muted hover:text-text"
          >
            {t("onboarding.skip")}
          </button>
          <div className="ml-auto flex gap-2">
            {step > 1 && (
              <button
                onClick={() => setStep(step - 1)}
                aria-label={t("onboarding.back")}
                className="btn-ghost btn-sm"
              >
                ←
              </button>
            )}
            {step < TOTAL_STEPS ? (
              <button
                onClick={() => setStep(step + 1)}
                disabled={step === 2 && nickInvalid}
                className="btn-primary"
              >
                {t("onboarding.next")}
              </button>
            ) : (
              <button
                onClick={() => void finish()}
                disabled={busy || !version}
                aria-busy={busy}
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
