// Запись демо-видео анимаций фазы 1 (D41): CDP-скринкаст кадров приложения
// во время управляемого сценария + склейка через ffmpeg (вызывается отдельно).
// Кадры приходят только при перерисовке — concat-файл с длительностями
// сохраняет реальный тайминг.
/* eslint-disable */
const { chromium } = require("playwright-core");
const fs = require("fs");
const path = require("path");

const OUT = "docs/anim-frames";

async function main() {
  fs.rmSync(OUT, { recursive: true, force: true });
  fs.mkdirSync(OUT, { recursive: true });
  const browser = await chromium.connectOverCDP("http://localhost:9223");
  const page = browser.contexts()[0].pages()[0];
  await page.setViewportSize({ width: 1240, height: 820 }).catch(() => {});
  await page.goto("http://localhost:1420/", { waitUntil: "networkidle" }).catch(() => {});
  await page.evaluate(() => localStorage.setItem("lang", "ru"));
  await page.reload({ waitUntil: "networkidle" }).catch(() => {});

  const cdp = await page.context().newCDPSession(page);
  let idx = 0;
  let lastTs = Date.now();
  const stamps = [];
  cdp.on("Page.screencastFrame", async (e) => {
    const now = Date.now();
    if (stamps.length) stamps[stamps.length - 1].dur = now - lastTs;
    lastTs = now;
    const name = `f${String(idx++).padStart(4, "0")}.jpg`;
    fs.writeFileSync(path.join(OUT, name), Buffer.from(e.data, "base64"));
    stamps.push({ file: name, dur: 120 });
    await cdp.send("Page.screencastFrameAck", { sessionId: e.sessionId }).catch(() => {});
  });
  await cdp.send("Page.startScreencast", {
    format: "jpeg",
    quality: 75,
    maxWidth: 1240,
    maxHeight: 820,
    everyNthFrame: 1,
  });

  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const main = page.locator("main");

  // 1. Главная: вход страницы + каскад строк (перезагрузка выше уже
  //    отыграла вход — длинная статичная пауза в ролике не нужна)
  await sleep(1800);
  // 2. Прокрутка: дозагрузка (stagger) + sticky-панель
  await main.evaluate((m) => m.scrollTo(0, 1400));
  await sleep(3000);
  await main.evaluate((m) => m.scrollTo(0, 1400));
  await sleep(1500);
  await main.evaluate((m) => m.scrollTo(0, 0));
  await sleep(1200);
  // 3. Окно проекта: вход модалки, табы, зум галереи
  await page.getByRole("button", { name: /^Установить — / }).first().click();
  await sleep(2500);
  const dlg = page.locator("[role='dialog']").first();
  await dlg.getByRole("tab", { name: "Журнал изменений" }).click();
  await sleep(1400);
  await dlg.getByRole("tab", { name: "Галерея" }).click();
  await sleep(1400);
  await dlg.locator(".grid img").first().click();
  await sleep(1500); // зум: anim-zoom-in
  await page.keyboard.press("Escape"); // Escape закрывает только зум
  await sleep(900);
  await dlg.getByRole("tab", { name: "Версии" }).click();
  await sleep(1400);
  // 4. Закрытие модалки — анимация выхода, затем повторный вход
  await page.keyboard.press("Escape");
  await sleep(1000);
  await page.getByRole("button", { name: /^Установить — / }).first().click();
  await sleep(2200);
  await page.keyboard.press("Escape");
  await sleep(1000);
  // 5. Переход на Инстансы + hover карточки (Play-оверлей)
  await page.getByRole("navigation").getByRole("button", { name: /^Инстансы/ }).click();
  await sleep(1800);
  await page.locator('[role="button"][aria-label^="Открыть "]').first().hover();
  await sleep(1200);
  // 6. Страница инстанса: переход + табы
  await page.locator('[role="button"][aria-label^="Открыть "]').first().locator("img").first().click({ position: { x: 10, y: 60 } });
  await sleep(1800);
  await page.getByRole("tab", { name: "Моды" }).click();
  await sleep(1200);
  await page.getByRole("tab", { name: "Логи" }).click();
  await sleep(1200);
  await page.getByRole("button", { name: /^Назад/ }).click();
  await sleep(1200);
  // 7. Настройки: галочка выбора темы (пружинка)
  await page.getByRole("navigation").getByRole("button", { name: /^Настройки/ }).click();
  await sleep(1500);
  // Тема: клик по невыбранной КАРТОЧКЕ ТЕМЫ (галочка-пружинка), затем возврат.
  // D62: раньше кликали первый button[aria-pressed=false] на странице — это пресет
  // акцента/логотипа (SettingsPage), перманентно менялся uiAccent/логотип юзера.
  // Названия карточек — из ru-локали (i18n keys themes.*), язык выставлен выше.
  const THEME_RU = {
    "dark": "Сланец · Тёмная",
    "light": "Сланец · Светлая",
    "cb-dark": "Дальтоник · Тёмная",
    "cb-light": "Дальтоник · Светлая",
    "mc": "Глубинный сланец",
  };
  const themeCard = (name) => page.getByRole("button", { name, exact: true });
  // D62: снапшот исходной темы + настроек ядра (theme/uiAccent/logo) для отката.
  const origTheme = await page.evaluate(() => document.documentElement.getAttribute("data-theme") || "dark");
  let snap = null;
  try {
    snap = await page.evaluate(() => (window.__TAURI__ ? window.__TAURI__.core.invoke("settings_get") : null));
  } catch { snap = null; }
  let origCardName = null;
  let offCardName = null;
  for (const [id, name] of Object.entries(THEME_RU)) {
    // D62: короткий таймаут пробы — не висим 30 с на каждой карточке.
    const pressed = await themeCard(name).getAttribute("aria-pressed", { timeout: 3000 }).catch(() => null);
    if (pressed === "true") origCardName = name;
    else if (pressed === "false" && !offCardName) offCardName = name;
  }
  if (origCardName && offCardName) {
    await themeCard(offCardName).click();
    await sleep(1200);
    // Возврат кликом по исходной карточке — пружинка второй раз, состояние на месте.
    await themeCard(origCardName).click();
    await sleep(1200);
  } else {
    console.error("WARN: карточки тем не найдены — демо темы пропущено");
  }
  // 8. Палитра команд (Ctrl+P) — вход панели
  await page.keyboard.press("Control+p");
  await sleep(1500);
  await page.keyboard.press("Escape");
  await sleep(800);
  // 9. Меню профиля: menu-in + шеврон
  const chip = page.getByRole("button", { name: /, сменить$/ });
  await chip.click();
  await sleep(1400);
  // Меню закрыто кликом по его подложке (сам чип перекрыт этой подложкой).
  await page.locator("header div.fixed.inset-0.z-40").click();
  await sleep(800);
  // 10. Оффлайн-переключатель дважды (fade иконки + вдавливание кнопки)
  const offline = page.getByRole("switch", { name: /офлайн/i });
  await offline.click();
  await sleep(1100);
  await offline.click();
  await sleep(1000);
  // D62: финал — вернуть исходную тему (хардкод «Тёмная» перетирал выбор юзера).
  const curTheme = await page.evaluate(() => document.documentElement.getAttribute("data-theme") || "dark");
  if (curTheme !== origTheme && THEME_RU[origTheme]) {
    await themeCard(THEME_RU[origTheme]).click();
  }
  await sleep(1200);

  await cdp.send("Page.stopScreencast").catch(() => {});
  // D62: на всякий случай — откат ядра-настроек из снапшота, если клики по
  // карточкам (dark/light пишут settings.theme) успели что-то изменить.
  if (snap) {
    try {
      const now = await page.evaluate(() => window.__TAURI__.core.invoke("settings_get"));
      if (now && JSON.stringify(now) !== JSON.stringify(snap)) {
        await page.evaluate((s) => window.__TAURI__.core.invoke("settings_set", { settings: s }), snap);
        console.log("OK: настройки ядра восстановлены из снапшота");
      }
    } catch (e) {
      console.error("WARN: не удалось откатить настройки ядра:", e.message || e);
    }
  }
  await browser.close();

  // concat-файл с реальными длительностями кадров
  const lines = ["ffconcat version 1.0"];
  for (const s of stamps) lines.push(`file '${s.file}'`, `duration ${(s.dur / 1000).toFixed(3)}`);
  if (stamps.length) lines.push(`file '${stamps[stamps.length - 1].file}'`);
  fs.writeFileSync(path.join(OUT, "frames.txt"), lines.join("\n") + "\n");
  console.log(`frames: ${stamps.length}`);
}

main().catch((e) => {
  console.error("FAIL:", e.message);
  process.exit(1);
});
