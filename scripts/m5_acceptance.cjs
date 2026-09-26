// Драйвер приёмки M5 (UI): страница «Моды» → поиск → установка в инстанс.
/* eslint-disable */
const { chromium } = require("playwright-core");
const fs = require("fs");
const OUT = "docs/m5-acceptance";
fs.mkdirSync(OUT, { recursive: true });

async function main() {
  const browser = await chromium.connectOverCDP("http://localhost:9223");
  const page = browser.contexts()[0].pages()[0];
  page.setDefaultTimeout(20000);

  await page.waitForSelector("text=Главная", { timeout: 15000 });
  await page.screenshot({ path: `${OUT}/1-home.png` });

  // Открыть Моды
  await page.getByRole("button", { name: "Моды" }).click();
  await page.waitForSelector("input[aria-label='Поиск модов на Modrinth…']");
  await page.screenshot({ path: `${OUT}/2-mods-page.png` });

  // Поиск Sodium
  await page.getByPlaceholder("Поиск модов на Modrinth…").fill("sodium");
  await page.getByRole("button", { name: "Найти" }).click();
  await page.waitForSelector("text=Sodium", { timeout: 20000 });
  await page.screenshot({ path: `${OUT}/3-search-results.png` });
  console.log("OK: поиск Modrinth через UI");

  // Установка в выбранный инстанс (первая карточка)
  await page.getByRole("button", { name: "Установить в инстанс" }).first().click();
  await page.waitForSelector("text=Установлено в", { timeout: 60000 });
  const info = await page.locator("text=Установлено в").textContent();
  console.log("OK:", info);
  await page.screenshot({ path: `${OUT}/4-installed.png` });

  console.log("DONE");
  await browser.close();
}

main().catch((e) => {
  console.error("FAIL:", e.message);
  process.exit(1);
});
