// Настройки → Темы: карточки как в CurseForge (slate-pro). Выбор применяется
// мгновенно через theme.ts; премиум — некликабельные карточки с замком.
import { useState } from "react";
import { Check, Lock, Crown } from "lucide-react";
import { getTheme, setTheme, THEMES, PREMIUM_THEMES, type ThemeId, type ThemeGroup } from "../../theme/theme";
import { useSettings } from "../../state/settings";
import { t } from "../../i18n";

const GROUPS: { id: ThemeGroup; labelKey: string; themes: ThemeId[] }[] = [
  { id: "classic", labelKey: "themes.group.classic", themes: ["dark", "light"] },
  { id: "colorblind", labelKey: "themes.group.colorblind", themes: ["cb-dark", "cb-light"] },
  { id: "game", labelKey: "themes.group.game", themes: ["mc"] },
];

export default function ThemeCards() {
  const [current, setCurrent] = useState<ThemeId>(getTheme());
  const { update } = useSettings();

  const pick = (id: ThemeId) => {
    // Кроссфейд через View Transitions API (дефолтный, без своего CSS):
    // тема применяется синхронно внутри колбэка, ровно как без него, —
    // меняется только визуальный переход. Guards: API нет (старый WebView)
    // или пользователь просил меньше движения — применяем как раньше.
    const doc = document as Document & { startViewTransition?: (cb: () => void) => unknown };
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (doc.startViewTransition && !reduce) {
      doc.startViewTransition(() => {
        setTheme(id);
      });
    } else {
      setTheme(id);
    }
    setCurrent(id);
    // dark/light синхронизируем с ядром (settings.theme), остальные — только localStorage.
    if (id === "dark" || id === "light") void update({ theme: id }).catch(() => undefined);
  };

  return (
    <div>
      {GROUPS.map((group) => (
        <section key={group.id} className="mb-6">
          <h3 className="mb-3 flex items-center gap-2 text-[13px] font-semibold uppercase tracking-wider text-text-muted">
            {t(group.labelKey)}
          </h3>
          <div className="grid grid-cols-[repeat(auto-fill,minmax(200px,1fr))] gap-3">
            {THEMES.filter((th) => th.group === group.id).map((th) => (
              <button
                key={th.id}
                onClick={() => pick(th.id)}
                aria-pressed={current === th.id}
                title={th.id === "mc" ? t("themes.mc.hint") : undefined}
                className={`flex min-h-24 flex-col gap-2 rounded-lg border bg-card p-2 text-left ${
                  current === th.id ? "border-accent bg-accent-soft" : "border-border-app hover:border-border-strong hover:bg-card-hover"
                }`}
              >
                <span
                  aria-hidden
                  className="flex h-11 items-center gap-2 rounded-md px-3"
                  style={{ background: th.swatches[0], border: `1px solid ${th.swatches[1]}` }}
                >
                  <span className="size-3 rounded-full" style={{ background: th.swatches[2] }} />
                  <span className="h-1 flex-1 rounded-full" style={{ background: th.swatches[1] }} />
                  <span className="h-1 w-4 rounded-full" style={{ background: th.swatches[1] }} />
                </span>
                <span className="flex items-center gap-2 px-1 pb-1">
                  <span className="text-sm font-semibold leading-tight">{t(th.nameKey)}</span>
                  <span className="ml-auto grid size-5 place-items-center">
                    {current === th.id && (
                      <span key="selected" className="anim-pop-in grid size-5 place-items-center rounded-full bg-accent text-on-accent">
                        <Check size={12} aria-hidden strokeWidth={2.5} />
                      </span>
                    )}
                  </span>
                </span>
              </button>
            ))}
          </div>
        </section>
      ))}

      {/* Премиум: карточки с замком, кликабельны НЕ — фейковой оплаты нет */}
      <section className="mb-6">
        <h3 className="mb-3 flex items-center gap-2 text-[13px] font-semibold uppercase tracking-wider text-text-muted">
          <Crown size={15} aria-hidden />
          {t("themes.group.premium")}
        </h3>
        <div className="grid grid-cols-[repeat(auto-fill,minmax(200px,1fr))] gap-3">
          {PREMIUM_THEMES.map((th) => (
            <div
              key={th.nameKey}
              aria-disabled="true"
              title={t("themes.premium.locked", { name: t(th.nameKey) })}
              className="flex min-h-24 cursor-default flex-col gap-2 rounded-lg border border-border-app bg-card p-2 opacity-90"
            >
              <span
                aria-hidden
                className="flex h-11 items-center gap-2 rounded-md px-3"
                style={{ background: th.swatches[0], border: `1px solid ${th.swatches[1]}` }}
              >
                <span className="size-3 rounded-full" style={{ background: th.swatches[2] }} />
                <span className="h-1 flex-1 rounded-full" style={{ background: th.swatches[1] }} />
                <span className="h-1 w-4 rounded-full" style={{ background: th.swatches[1] }} />
              </span>
              <span className="flex items-center gap-2 px-1 pb-1">
                <span className="text-sm font-semibold leading-tight">{t(th.nameKey)}</span>
                <span className="ml-auto text-text-muted">
                  <Lock size={16} aria-hidden />
                </span>
              </span>
            </div>
          ))}
        </div>
        <div className="mt-4 flex items-center gap-4 rounded-lg border border-border-app bg-card p-4">
          <span>
            <span className="text-[15px] font-semibold">{t("themes.premium.title")}</span>
            <br />
            <span className="text-[13px] text-text-muted">{t("themes.premium.desc")}</span>
          </span>
          {/* URL страницы подписки не согласован — кнопка честно disabled с тултипом */}
          <button disabled className="btn-primary ml-auto" title={t("themes.premium.subscribe.unavailable")}>
            <Crown size={16} aria-hidden />
            {t("themes.premium.subscribe")}
          </button>
        </div>
      </section>
    </div>
  );
}
