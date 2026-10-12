// «Как оптимизировать?» (D54): вместо авто-кнопки «Оптимизировать» — честная
// справка. Советы — стандартные настройки сообщества (прорисовка ≤8 чанков —
// главный рычаг; симуляция ниже; графика «Быстрая»; RAM — не больше половины
// системы); для модифицированных сборок — Sodium-стек, который лаунчер ставит
// из каталога. Кнопки «оптимизировать за меня» у ванили не существует —
// не изобретаем.
import { X } from "lucide-react";
import { useModalA11y } from "../hooks/useModalA11y";
import { t } from "../../i18n";

export default function OptimizeHelpModal({ onClose }: { onClose: () => void }) {
  const { containerRef: dialogRef, requestClose } = useModalA11y(onClose);
  const rows = [
    { title: t("instance.optimizeHelp.settings.title"), body: t("instance.optimizeHelp.settings.body") },
    { title: t("instance.optimizeHelp.ram.title"), body: t("instance.optimizeHelp.ram.body") },
    { title: t("instance.optimizeHelp.mods.title"), body: t("instance.optimizeHelp.mods.body") },
    { title: t("instance.optimizeHelp.system.title"), body: t("instance.optimizeHelp.system.body") },
  ];
  return (
    <div
      className="anim-fade-in fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onClick={(e) => {
        if (e.target === e.currentTarget) requestClose();
      }}
    >
      <div
        ref={dialogRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={t("instance.optimizeHelp.button")}
        className="anim-dialog-in flex max-h-[85vh] w-[540px] max-w-full flex-col overflow-y-auto overscroll-contain rounded-lg border border-border-app bg-card p-6 shadow-2xl"
      >
        <div className="flex shrink-0 items-center justify-between border-b border-border-app pb-3">
          <h2 className="text-lg font-semibold text-text">{t("instance.optimizeHelp.button")}</h2>
          <button
            onClick={requestClose}
            aria-label={t("common.close")}
            className="icon-btn text-text-muted hover:text-text"
          >
            <X size={18} aria-hidden />
          </button>
        </div>
        <div className="flex flex-col gap-4 py-4">
          {rows.map((r) => (
            <section key={r.title}>
              <h3 className="text-sm font-semibold text-text">{r.title}</h3>
              <p className="mt-1 text-sm leading-relaxed text-text-muted">{r.body}</p>
            </section>
          ))}
        </div>
        <div className="mt-auto flex shrink-0 justify-end border-t border-border-app pt-3">
          <button onClick={onClose} className="btn-primary h-10 px-5 text-sm">
            {t("common.ok")}
          </button>
        </div>
      </div>
    </div>
  );
}
