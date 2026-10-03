// D49: в Tauri (wry/WebView2) window.confirm — асинхронный нативный диалог:
// он возвращает Promise<boolean>, а не boolean. Синхронная проверка
// `if (window.confirm(...))` видит «truthy» промис и пропускает действие
// мгновенно, не дожидаясь ответа — подтверждение не работает вовсе.
// Единственный честный паттерн — await. Ошибка показа диалога трактуется
// как отказ (безопаснее не выполнить разрушительное действие).
export async function askConfirm(message: string): Promise<boolean> {
  try {
    return (await window.confirm(message)) === true;
  } catch {
    return false;
  }
}
