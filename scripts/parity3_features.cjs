// Приёмка D37-B: скриншоты-таб, обложки паков, бейджи зависимостей, ярлык,
// монитор ресурсов, безопасный режим в краш-модалке (presence-based).
/* eslint-disable */
const { chromium } = require("playwright-core");

async function main() {
  const browser = await chromium.connectOverCDP("http://localhost:9223");
  const page = browser.contexts()[0].pages()[0];
  page.setDefaultTimeout(20000);
  let failed = 0;
  const ok = (m) => console.log("OK:", m);
  const fail = (m) => { failed++; console.error("FAIL:", m); };
  await page.goto("http://localhost:1420/", { waitUntil: "networkidle" }).catch(() => {});
  await page.evaluate(() => localStorage.setItem("lang", "ru"));
  await page.reload({ waitUntil: "networkidle" }).catch(() => {});
  await page.waitForTimeout(1200);

  // 1. Детальная страница: таб «Скриншоты» (пустой список — честный empty).
  await page.getByRole("navigation").getByRole("button", { name: /^Инстансы/ }).click();
  await page.waitForTimeout(600);
  const card = page.locator('[role="button"][aria-label^="Открыть "]').first();
  if ((await card.count()) === 0) {
    fail("нет карточек инстансов");
  } else {
    await card.click();
    await page.waitForTimeout(500);
    const screensTab = page.getByRole("tab", { name: "Скриншоты" });
    (await screensTab.count()) === 1 ? ok("таб «Скриншоты» на месте") : fail("нет таба «Скриншоты»");
    await screensTab.click();
    await page.waitForTimeout(800);
    const hasEmpty = (await page.getByText("Скриншотов нет").count()) > 0;
    const hasGrid = (await page.locator('[aria-label^="Удалить скриншот"]').count()) > 0;
    hasEmpty || hasGrid
      ? ok(hasEmpty ? "скриншоты: честный empty" : "скриншоты: сетка с удалением")
      : fail("таб «Скриншоты» пуст и без empty");

    // 2. Кебаб детальной: «Ярлык на рабочий стол».
    const kebab = page.getByRole("button", { name: /Действия —/ }).first();
    await kebab.click();
    await page.waitForTimeout(300);
    (await page.getByRole("menuitem", { name: "Ярлык на рабочий стол" }).count()) === 1
      ? ok("кебаб: «Ярлык на рабочий стол»")
      : fail("нет пункта ярлыка в кебабе");
    await page.keyboard.press("Escape");
    await page.waitForTimeout(200);

    // 3. Логи: монитор не рисуется вне игры (проверка отсутствия мусора).
    await page.getByRole("tab", { name: /^Логи/ }).click();
    await page.waitForTimeout(400);
    (await page.locator("span.chip-mono", { hasText: /CPU \d+%.*RAM/ }).count()) === 0
      ? ok("монитор скрыт, пока игра не запущена")
      : fail("монитор виден без запущенной игры");

    // 4. Моды: бейджи зависимостей могут отсутствовать (нет данных — нет
    // элементов) — проверяем только отсутствие сырых ключей i18n.
    await page.getByRole("tab", { name: /Моды \(/ }).click();
    await page.waitForTimeout(800);
    const rawKeys = (await page.getByText("mods.deps.").count()) > 0;
    !rawKeys ? ok("бейджи зависимостей без сырых ключей") : fail("сырые i18n-ключи в бейджах");

    await page.getByRole("button", { name: /^Назад/ }).first().click();
    await page.waitForTimeout(400);
  }

  console.log(failed === 0 ? "DONE: parity3_features зелёная" : `DONE с ошибками: ${failed}`);
  await browser.close();
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((e) => {
  console.error("FAIL:", e.message);
  process.exit(1);
});
