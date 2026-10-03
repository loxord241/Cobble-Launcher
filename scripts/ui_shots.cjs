// Скриншоты экранов в каждой из 5 тем (ui-apply-shots/). Драйвер CDP.
// Тема подаётся через ?theme= — заодно проверяет приоритет query над localStorage.
/* eslint-disable */
const { chromium } = require("playwright-core");
const fs = require("fs");

const OUT = "ui-apply-shots";
const THEMES = ["dark", "light", "cb-dark", "cb-light", "mc"];

async function main() {
  fs.mkdirSync(OUT, { recursive: true });
  const browser = await chromium.connectOverCDP("http://localhost:9223");
  const page = browser.contexts()[0].pages()[0];
  page.setDefaultTimeout(30000);

  const nav = async (pageId) => {
    await page.getByRole("navigation").getByRole("button", { name: new RegExp("^" + pageId) }).first().click();
    await page.waitForTimeout(400);
  };

  // Язык интерфейса — ru: свежая установка стартует на en (D30), подписи ниже русские.
  await page.goto("http://localhost:1420/", { waitUntil: "networkidle" }).catch(() => {});
  await page.evaluate(() => localStorage.setItem("lang", "ru"));
  await page.reload({ waitUntil: "networkidle" }).catch(() => {});
  await page.waitForTimeout(800);

  for (const theme of THEMES) {
    await page.goto(`http://localhost:1420/?theme=${theme}`, { waitUntil: "networkidle" }).catch(() => {});
    await page.waitForTimeout(1200);
    // Онбординг мог остаться открытым — он и нужен как один из экранов.
    const onboarding = await page.locator("text=Добро пожаловать").count().catch(() => 0);
    if (!onboarding) {
      await page.screenshot({ path: `${OUT}/${theme}-1-home.png` });
      await nav("Инстансы");
      await page.screenshot({ path: `${OUT}/${theme}-2-instances.png` });
      // Диалог создания
      await page.getByRole("button", { name: "Создать инстанс" }).first().click().catch(() => {});
      await page.waitForTimeout(600);
      await page.screenshot({ path: `${OUT}/${theme}-3-create-dialog.png` });
      await page.getByRole("dialog").getByRole("button", { name: "Закрыть", exact: true }).click().catch(() => {});
      await nav("Главная");
      await page.waitForTimeout(1200);
      await page.screenshot({ path: `${OUT}/${theme}-4-home-catalog.png` });
      await nav("Настройки");
      await page.waitForTimeout(600);
      await page.screenshot({ path: `${OUT}/${theme}-5-settings-themes.png` });
    } else {
      await page.screenshot({ path: `${OUT}/${theme}-0-onboarding.png` });
    }
    console.log(`OK: ${theme}`);
  }

  await browser.close();
  console.log("DONE");
}

main().catch((e) => {
  console.error("FAIL:", e.message);
  process.exit(1);
});
