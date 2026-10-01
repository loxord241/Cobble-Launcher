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

  // 1. Главная: вход страницы + каскад строк
  await sleep(3500);
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
  // Тема: клик по невыбранной карточке (галочка-пружинка), затем возврат.
  const offCard = page.locator('button[aria-pressed="false"]').first();
  await offCard.click();
  await sleep(1200);
  await page.locator('button[aria-pressed="true"]').first().click();
  await sleep(1200);
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
  // Финал: вернуть тёмную тему (сценарий оставляет выбранной последнюю кликнутую).
  await page.locator('button[aria-pressed="false"]', { hasText: "Тёмная" }).first().click();
  await sleep(1200);

  await cdp.send("Page.stopScreencast").catch(() => {});
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
