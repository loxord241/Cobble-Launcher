// Корень: онбординг-гейт → shell. Чистый профиль → мастер онбординга (M4).
import { useCallback, useEffect, useState } from "react";
import Shell from "./ui/layout/Shell";
import Onboarding from "./ui/pages/Onboarding";
import { useSettings } from "./state/settings";
import { t } from "./i18n";
import { useInstances } from "./state/instances";

export default function App() {
  // PERF#1 (аудит 2026-10-03): без селекторов App подписан на ВЕСЬ стор —
  // каждый тик dl_progress (10/с) перерисовывал всё дерево. Только узкие поля.
  const loaded = useSettings((s) => s.loaded);
  const load = useSettings((s) => s.load);
  const loadError = useSettings((s) => s.error);
  const settings = useSettings((s) => s.settings);
  const error = useInstances((s) => s.error);
  const setError = useInstances((s) => s.setError);
  const loadInstances = useInstances((s) => s.load);
  // onboardingDone читается один раз при старте; сброс из настроек
  // перезагружает состояние через update.
  const [showOnboarding, setShowOnboarding] = useState<boolean | null>(null);
  // Стабильный колбэк: инлайн-стрелка каждый рендер App давала Shell новый проп.
  const handleLogout = useCallback(() => setShowOnboarding(true), []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (loaded && showOnboarding === null) {
      setShowOnboarding(!settings?.onboardingDone);
    }
  }, [loaded, settings, showOnboarding]);

  if (loadError) {
    // D6: ошибка начальной загрузки — текст + «Повторить», не вечное «…».
    return (
      <div className="flex h-screen flex-col items-center justify-center gap-4 bg-bg px-6 text-center">
        <div className="text-[15px] font-semibold text-text">{t("app.loadError.title")}</div>
        <div className="max-w-md text-[13px] text-text-muted">{loadError}</div>
        <button onClick={() => void load()} className="btn-primary btn-sm">
          {t("app.loadError.retry")}
        </button>
      </div>
    );
  }

  if (!loaded || showOnboarding === null) {
    return <div className="flex h-screen items-center justify-center bg-bg text-text-muted">…</div>;
  }

  if (showOnboarding) {
    return (
      <Onboarding
        onDone={() => {
          setShowOnboarding(false);
          void loadInstances();
        }}
      />
    );
  }

  return (
    <>
      <Shell onLogout={handleLogout} />
      {error && (
        <div
          role="alert"
          className="fixed bottom-4 right-4 z-50 max-w-96 rounded-md border border-error/40 bg-card p-3 text-sm text-error shadow-lg"
        >
          <button
            onClick={() => setError(null)}
            aria-label={t("common.close")}
            className="float-right ml-2 text-text-muted"
          >
            ✕
          </button>
          {error}
        </div>
      )}
    </>
  );
}
