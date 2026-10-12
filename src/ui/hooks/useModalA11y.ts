// Единый хук модалок (A33/D21): Escape, ловушка фокуса (Tab-цикл внутри
// диалога), возврат фокуса на триггер при закрытии + анимация закрытия
// (D41: requestClose вешает .anim-closing на контейнер и зовёт onClose через
// таймаут-фолбэк; при prefers-reduced-motion — мгновенно, 0 мс). Реф навесить
// на контейнер диалога (role=dialog). Открытие = наличие компонента в DOM.
import { useCallback, useEffect, useRef } from "react";

const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** Длительность .anim-closing в theme/tokens.css. */
const CLOSE_MS = 140;

// D71: стек смонтированных модалок. Слушатели keydown висят на document у
// КАЖДОЙ модалки в capture-фазе, а stopPropagation не останавливает соузловые
// слушатели того же узла — поэтому при двух открытых модалках один Escape
// закрывал обе сразу, а Tab-ловушки сражались за фокус (ping-pong). Нажатие
// обрабатывает только ВЕРХНЯЯ модалка (последняя в стеке), нижние игнорируют.
const modalStack: symbol[] = [];

export function useModalA11y(
  onClose: () => void,
  opts?: { /** CSS-селектор элемента, получающего фокус первым (например поле ввода). */
    initialFocus?: string;
    /** D69-ревизия: закрытие временно запрещено (идёт установка) —
     * requestClose НЕ запускает анимацию и не расходует одноразовое
     * закрытие: раньше guard в onClose при busy оставлял модалку
     * смонтированной с anim-closing (opacity:0) — «кирпич» до рестарта. */
    canClose?: () => boolean },
) {
  const containerRef = useRef<HTMLDivElement>(null);
  const restoreRef = useRef<HTMLElement | null>(null);
  // D41: закрытие одноразовое (повторные requestClose/Escape — no-op).
  const closingRef = useRef(false);
  const closeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // onClose в ref: колбэк пересоздаётся каждый рендер, а requestClose стабильна
  // (Escape-слушатель ставится один раз на всё время жизни модалки).
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const canCloseRef = useRef(opts?.canClose);
  canCloseRef.current = opts?.canClose;

  /** Закрыть модалку с exit-анимацией (.anim-closing на контейнере), затем
   * вызвать onClose. Повторный вызов до завершения игнорируется. Таймаут —
   * фолбэк вместо animationend: при reduced-motion событие не придёт. */
  const requestClose = useCallback(() => {
    if (closingRef.current) return;
    // Проверка ДО анимации: занятую модалку не схлопываем впустую.
    if (canCloseRef.current && !canCloseRef.current()) return;
    closingRef.current = true;
    containerRef.current?.classList.add("anim-closing");
    const reduce =
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    closeTimerRef.current = setTimeout(
      () => {
        closeTimerRef.current = null;
        onCloseRef.current();
      },
      reduce ? 0 : CLOSE_MS,
    );
  }, []);

  useEffect(() => {
    // D71: регистрация в стеке — до возможного раннего выхода, а снятие ТОКЕНА
    // (по indexOf, а не shift/pop) — всегда в cleanup: StrictMode double-mount
    // и размонтирование средней модалки не ломают порядок остальных.
    const stackToken = Symbol("modal");
    modalStack.push(stackToken);

    const container = containerRef.current;
    const onKey = (e: KeyboardEvent) => {
      // D71: не верхняя модалка — молча пропускаем: Escape закрывает только
      // верхнюю, Tab-ловушка работает только у верхней.
      if (modalStack[modalStack.length - 1] !== stackToken) return;
      if (e.key === "Escape") {
        e.stopPropagation();
        requestClose();
        return;
      }
      if (e.key !== "Tab") return;
      if (!container) return;
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

    if (container) {
      restoreRef.current = document.activeElement as HTMLElement | null;
      // Начальный фокус: первый доступный элемент или сам контейнер.
      const preferred = opts?.initialFocus
        ? container.querySelector<HTMLElement>(opts.initialFocus)
        : null;
      const first = container.querySelector<HTMLElement>(FOCUSABLE);
      (preferred ?? first ?? container).focus();
    }

    return () => {
      document.removeEventListener("keydown", onKey, true);
      const tokenIdx = modalStack.indexOf(stackToken);
      if (tokenIdx >= 0) modalStack.splice(tokenIdx, 1);
      // Модалку размонтировали до срабатывания таймаута закрытия — не зовём
      // onClose вслепую после того, как ушёл DOM диалога.
      if (closeTimerRef.current !== null) {
        clearTimeout(closeTimerRef.current);
        closeTimerRef.current = null;
      }
      // Возврат фокуса на триггер после закрытия.
      restoreRef.current?.focus?.();
    };
    // requestClose стабильна (useCallback([])), повторные подписки не нужны.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return { containerRef, requestClose };
}
