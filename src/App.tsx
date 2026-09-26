// Корень: онбординг-гейт → shell. Чистый профиль → мастер онбординга (M4).
import { useEffect, useState } from "react";
import Shell from "./ui/layout/Shell";
import Onboarding from "./ui/pages/Onboarding";
import { useSettings } from "./state/settings";
import { useInstances } from "./state/instances";

export default function App() {
  const { settings, loaded, load } = useSettings();
  const { error, setError, load: loadInstances } = useInstances();
  // onboardingDone читается один раз при старте; сброс из настроек
  // перезагружает состояние через update.
  const [showOnboarding, setShowOnboarding] = useState<boolean | null>(null);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (loaded && showOnboarding === null) {
      setShowOnboarding(!settings?.onboardingDone);
    }
  }, [loaded, settings, showOnboarding]);

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
      <Shell onLogout={() => setShowOnboarding(true)} />
      {error && (
        <div
          role="alert"
          className="fixed bottom-4 right-4 z-50 max-w-96 rounded-md border border-error/40 bg-card p-3 text-sm text-error shadow-lg"
        >
          <button
            onClick={() => setError(null)}
            aria-label="close"
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
