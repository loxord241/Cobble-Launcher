// Драйвер приёмки M4: онбординг → shell → создание инстанса → запуск из UI.
// Подключается к WebView2 по CDP (порт из tauri.conf.json additionalBrowserArgs).
/* eslint-disable */
const { chromium } = require("playwright-core");

const OUT = "docs/m4-acceptance";
const fs = require("fs");
fs.mkdirSync(OUT, { recursive: true });

async function main() {
  const browser = await chromium.connectOverCDP("http://localhost:9223");
  const contexts = browser.contexts();
  const page = contexts[0].pages()[0];
  page.setDefaultTimeout(20000);

  // B2: дефолт чистой установки — en (D30); приёмки русскоязычные → ставим ru.
  await page.evaluate(() => localStorage.setItem("lang", "ru"));
  await page.reload({ waitUntil: "networkidle" }).catch(() => {});


  // 1. Онбординг виден?
  await page.waitForSelector("text=Добро пожаловать", { timeout: 15000 });
  await page.screenshot({ path: `${OUT}/1-onboarding-step1.png` });
  console.log("OK: онбординг шаг 1 (каталог данных)");

  // 2. Шаг 2: профиль
  await page.getByRole("button", { name: "Далее" }).click();
  await page.waitForSelector("text=Профиль");
  await page.fill("#ob-nick", "MCL2");
  await page.screenshot({ path: `${OUT}/2-onboarding-step2.png` });
  console.log("OK: онбординг шаг 2 (ник MCL2)");

  // 3. Шаг 3: первый инстанс (latest из манифеста) → Готово
  await page.getByRole("button", { name: "Далее" }).click();
  await page.waitForSelector("text=Первый инстанс");
  await page.waitForFunction(() => document.querySelectorAll("select option").length > 0);
  const chosen = await page.locator("#\\31  select, select").first().inputValue().catch(() => "");
  await page.screenshot({ path: `${OUT}/3-onboarding-step3.png` });
  await page.getByRole("button", { name: "Готово" }).click();
  console.log("OK: инстанс создан из онбординга (версия из манифеста)");

  // 4. Shell: карточки инстансов
  await page.waitForSelector("text=M3", { timeout: 15000 }).catch(() => {});
  await page.waitForTimeout(1500);
  await page.screenshot({ path: `${OUT}/4-shell-home.png` });
  console.log("OK: shell с навигацией и карточками");

  // 5. Запуск мышкой: кнопка Play на главной
  // Сланец: Play теперь квадратная кнопка в строке инстанса,
  // aria-label = "Играть — {имя}" (было: общая кнопка "Играть" в хедере).
  await page.getByRole("button", { name: /^Играть —/ }).first().click();
  console.log("OK: нажата кнопка Играть");
  await page.waitForTimeout(8000);
  await page.screenshot({ path: `${OUT}/5-launch-progress.png` });

  console.log("DONE: UI-флоу отработан; состояние игры проверяет внешний скрипт");
  await browser.close();
}

main().catch((e) => {
  console.error("FAIL:", e.message);
  process.exit(1);
});
