// Приёмка волны 2: иконки инстансов (set/clear через кебаб-меню),
// статистика запусков в настройках инстанса, масштаб интерфейса.
/* eslint-disable */
const path = require("path");
const { chromium } = require("playwright-core");
const fs = require("fs");

// Валидный PNG 8x8: картинки 1x1 WebView2 <img> не рендерит вовсе
// (найдено в D54 — отсюда «битая» иконка на карточке), а суть приёмки —
// проверить, что иконка видна в строке.
const PNG_1PX = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAIAAABLbSncAAAAEUlEQVR4nGM4YWODFTEMLQkAZZlQAVIPr1MAAAAASUVORK5CYII=',
  "base64",
);

async function main() {
  const browser = await chromium.connectOverCDP("http://localhost:9223");
  const page = browser.contexts()[0].pages()[0];
  page.setDefaultTimeout(20000);
  let failed = 0;
  const ok = (m) => console.log("OK:", m);
  const fail = (m) => { failed++; console.error("FAIL:", m); };
  await page.goto("http://localhost:1420/", { waitUntil: "networkidle" }).catch(() => {});
  // Язык интерфейса — ru: свежая установка стартует на en (D30), проверки ниже русскоязычные.
  await page.evaluate(() => localStorage.setItem("lang", "ru"));
  await page.reload({ waitUntil: "networkidle" }).catch(() => {});

  const iconPath = path.join(require("os").tmpdir(), "mcl-test-icon.png");
  fs.writeFileSync(iconPath, PNG_1PX);

  // 1. Иконка: установка через IPC (нативный диалог автоматизировать нельзя),
  //    отображение в строке и снятие — через живой UI.
  await page.getByRole("navigation").getByRole("button", { name: /^Инстансы/ }).click();
  await page.waitForTimeout(600);
  const row = page.locator('[role="button"][aria-label^="Открыть "]').first();
  const instId = await page.evaluate(async () => {
    const ids = await window.__TAURI__.core.invoke("instance_list");
    return ids[0].id;
  }).catch(() => null);
  if (!instId) {
    fail("window.__TAURI__ недоступен — нужен withGlobalTauri в tauri.conf (временный флаг приёмки)");
    console.log("ALL WAVE2 CHECKS PASSED".replace("PASSED", "SKIPPED-ICON"));
  }
  if (instId) {
    const err = await page.evaluate(async ({ id, p }) => {
      try {
        await window.__TAURI__.core.invoke("instance_icon_set", { id, path: p });
        return null;
      } catch (e) {
        return String(e);
      }
    }, { id: instId, p: iconPath });
    if (err) fail(`instance_icon_set упал: ${err}`);
    // Стор UI узнаёт об иконке только после перечитывания списка —
    // перезагружаем страницу и возвращаемся на Инстансы.
    await page.reload({ waitUntil: "networkidle" }).catch(() => {});
    await page.waitForTimeout(800);
    await page.getByRole("navigation").getByRole("button", { name: /^Инстансы/ }).click();
    await page.waitForTimeout(600);
    const row2 = page.locator('[role="button"][aria-label^="Открыть "]').first();
    (await row2.locator("img[src^='data:image/png;base64,']").count()) > 0
      ? ok("иконка установлена (IPC) и видна в строке (data-URL)")
      : fail("иконка не появилась в строке");
  }

  // 2. Убрать иконку: буква вернулась.
  await row.getByRole("button", { name: /Действия/ }).click();
  await page.waitForTimeout(300);
  await page.getByRole("menuitem", { name: "Убрать иконку" }).click();
  await page.waitForTimeout(800);
  // D35: без своей иконки карточка показывает дефолтный арт (svg data-URL)
  const pngGone = (await row.locator("img[src^='data:image/png;base64,']").count()) === 0;
  const artBack = (await row.locator("img[src^='data:image/svg+xml']").count()) > 0;
  pngGone && artBack
    ? ok("иконка убрана — вернулся дефолтный арт")
    : fail("иконка не убралась");

  // 3. Статистика запусков в настройках инстанса.
  await row.getByRole("button", { name: /Действия/ }).click();
  await page.waitForTimeout(300);
  await page.getByRole("menuitem", { name: /Настройки/ }).click();
  await page.waitForSelector("role=dialog");
  const chips = await page
    .getByText(/Запусков: \d+/)
    .or(page.getByText(/В игре: \d+ ч \d+ мин/))
    .count();
  chips > 0 ? ok("статистика (запусков/время) видна в настройках") : fail("чипов статистики нет");
  await page.keyboard.press("Escape");
  await page.waitForTimeout(300);

  // 4. Масштаб интерфейса: 125% реально меняет font-size <html>.
  await page.getByRole("navigation").getByRole("button", { name: /Настройки/ }).first().click();
  await page.waitForTimeout(500);
  const scaleSel = page.getByRole("combobox", { name: "Масштаб интерфейса" });
  (await scaleSel.count()) > 0 ? ok("селектор масштаба на месте") : fail("нет селектора масштаба");
  await scaleSel.selectOption("125");
  await page.waitForTimeout(500);
  let fs_ = await page.evaluate(() => document.documentElement.style.fontSize);
  fs_ === "125%" ? ok("масштаб 125% применён к <html>") : fail(`font-size=${fs_}`);
  await scaleSel.selectOption("100");
  await page.waitForTimeout(400);
  fs_ = await page.evaluate(() => document.documentElement.style.fontSize);
  fs_ === "100%" ? ok("масштаб вернут к 100%") : fail(`font-size=${fs_}`);

  console.log(failed === 0 ? "ALL WAVE2 CHECKS PASSED" : `FAILED: ${failed}`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((e) => {
  console.error("CRASH:", e);
  process.exit(1);
});
