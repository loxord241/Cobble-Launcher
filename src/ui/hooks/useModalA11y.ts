// Единый хук модалок (A33/D21): Escape, ловушка фокуса (Tab-цикл внутри
// диалога), возврат фокуса на триггер при закрытии. Реф навесить на
// контейнер диалога (role=dialog). Открытие = наличие компонента в DOM.
import { useEffect, useRef } from "react";

const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function useModalA11y(
  onClose: () => void,
  opts?: { /** CSS-селектор элемента, получающего фокус первым (например поле ввода). */
    initialFocus?: string },
) {
  const containerRef = useRef<HTMLDivElement>(null);
  const restoreRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    restoreRef.current = document.activeElement as HTMLElement | null;

    // Начальный фокус: первый доступный элемент или сам контейнер.
    const preferred = opts?.initialFocus
      ? container.querySelector<HTMLElement>(opts.initialFocus)
      : null;
    const first = container.querySelector<HTMLElement>(FOCUSABLE);
    (preferred ?? first ?? container).focus();

    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
        return;
      }
      if (e.key !== "Tab") return;
      const items = Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE));
      if (items.length === 0) return;
      const firstEl = items[0];
      const lastEl = items[items.length - 1];
      const active = document.activeElement;
      if (e.shiftKey && (active === firstEl || active === container)) {
        e.preventDefault();
        lastEl.focus();
      } else if (!e.shiftKey && (active === lastEl || active === container || !container.contains(active))) {
        e.preventDefault();
        firstEl.focus();
      }
    };
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("keydown", onKey, true);
      // Возврат фокуса на триггер после закрытия.
      restoreRef.current?.focus?.();
    };
    // onClose намеренно не в зависимостях: колбэк пересоздаётся каждый рендер,
    // а слушатель должен ставиться один раз на всё время жизни модалки.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return containerRef;
}
