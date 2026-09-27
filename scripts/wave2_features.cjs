// Приёмка волны 2: иконки инстансов (set/clear через кебаб-меню),
// статистика запусков в настройках инстанса, масштаб интерфейса.
/* eslint-disable */
const { chromium } = require("playwright-core");
const fs = require("fs");

// Минимальный валидный PNG 1x1.
const PNG_1PX = Buffer.from(
  "89504e470d0a1a0a0000000d4948445200000001000000010806000001f15c4890000000d4944415478da63fcffff3f030005fe02fea72d1e480000000049454e44ae426082",
  "hex",
);

async function main() {
  const browser = await chromium.connectOverCDP("http://localhost:9223");
  const page = browser.contexts()[0].pages()[0];
  page.setDefaultTimeout(20000);
  let failed = 0;
  const ok = (m) => console.log("OK:", m);
  const fail = (m) => { failed++; console.error("FAIL:", m); };
  await page.goto("http://localhost:1420/", { waitUntil: "networkidle" }).catch(() => {});

  const iconPath = "C:/Users/Admin/AppData/Local/Temp/mcl-test-icon.png";
  fs.writeFileSync(iconPath, PNG_1PX);

  // 1. Иконка: смена через кебаб-меню (prompt-диалог), <img> в строке.
  await page.getByRole("navigation").getByRole("button", { name: /^Инстансы/ }).click();
  await page.waitForTimeout(600);
  const row = page.locator(".group.grid").first();
  await row.getByRole("button", { name: /Действия/ }).click();
  await page.waitForTimeout(300);
  page.once("dialog", (d) => d.accept(iconPath));
  await page.getByRole("menuitem", { name: "Сменить иконку…" }).click();
  await page.waitForTimeout(800);
  (await row.locator("img[src^='data:image/png;base64,']").count()) > 0
    ? ok("иконка установлена и видна в строке (data-URL)")
    : fail("иконка не появилась в строке");

  // 2. Убрать иконку: буква вернулась.
  await row.getByRole("button", { name: /Действия/ }).click();
  await page.waitForTimeout(300);
  await page.getByRole("menuitem", { name: "Убрать иконку" }).click();
  await page.waitForTimeout(800);
  (await row.locator("img[src^='data:image']").count()) === 0
    ? ok("иконка убрана — вернулась буква")
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
