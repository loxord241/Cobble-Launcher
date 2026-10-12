// Приёмка волны D37 (фаза 2 аудита): миры, «Проверить файлы», тест Java,
// mclo.gs, история логов, датапаки-вкладка, снапшоты/откат, storage, экспорт
// настроек. Presence-based: тяжёлые/нативные операции не запускаем.
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

  // 1. Детальная страница инстанса: таб «Миры» + содержимое/пусто.
  await page.getByRole("navigation").getByRole("button", { name: /^Инстансы/ }).click();
  await page.waitForTimeout(600);
  const card = page.locator('[role="button"][aria-label^="Открыть "]').first();
  if ((await card.count()) === 0) {
    fail("нет карточек инстансов");
  } else {
    await card.click();
    await page.waitForTimeout(500);
    const worldsTab = page.getByRole("tab", { name: "Миры" });
    (await worldsTab.count()) === 1
      ? ok("детальная страница: таб «Миры» на месте")
      : fail("нет таба «Миры»");
    await worldsTab.click();
    await page.waitForTimeout(800);
    const hasList = (await page.getByRole("button", { name: "Бэкап мира" }).count()) > 0;
    const hasEmpty = (await page.getByText(/Миров пока нет/).count()) > 0;
    hasList || hasEmpty
      ? ok(hasList ? "список миров с бэкап-кнопками" : "честный empty для миров")
      : fail("таб «Миры» пуст и без empty-состояния");

    // 2. Кебаб детальной страницы: «Проверить файлы».
    const kebab = page.getByRole("button", { name: /Действия —/ }).first();
    if ((await kebab.count()) > 0) {
      await kebab.click();
      await page.waitForTimeout(300);
      (await page.getByRole("menuitem", { name: "Проверить файлы" }).count()) === 1
        ? ok("кебаб: «Проверить файлы»")
        : fail("нет «Проверить файлы» в кебабе");
      await page.keyboard.press("Escape");
      await page.waitForTimeout(200);
    } else fail("нет кебаба на детальной странице");

    // 3. Моды-вкладка: секция снапшотов/отката (после интеграции оркестратором).
    const modsTab = page.getByRole("tab", { name: /Моды \(/ });
    if ((await modsTab.count()) === 1) {
      await modsTab.click();
      await page.waitForTimeout(800);
      (await page.getByText("Снапшоты модов").count()) >= 1
        ? ok("моды: секция «Снапшоты модов»")
        : fail("нет секции снапшотов на вкладке Моды");
    }

    // 4. Логи: кнопка mclo.gs + секция «Прошлые логи».
    const logsTab = page.getByRole("tab", { name: /^Логи/ });
    await logsTab.click();
    await page.waitForTimeout(500);
    (await page.getByRole("button", { name: "Отправить на mclo.gs" }).count()) >= 1
      ? ok("логи: кнопка mclo.gs")
      : fail("нет кнопки mclo.gs в логах");
    (await page.getByText("Прошлые логи").count()) >= 1
      ? ok("логи: секция «Прошлые логи»")
      : fail("нет секции истории логов");
    const filterChips = await page.getByRole("button", { name: /^(Все|Ошибки|Предупреждения)$/ }).count();
    filterChips >= 3 ? ok(`фильтры уровней логов (${filterChips})`) : fail(`фильтров логов ${filterChips} < 3`);

    // Назад к списку
    await page.getByRole("button", { name: /^Назад/ }).first().click();
    await page.waitForTimeout(400);
  }

  // 5. Добавление контента (D39, вместо страницы «Моды»): 4 вкладки типа.
  // После «Назад к списку» заново открываем страницу инстанса.
  const irow2 = page.locator('[role="button"][aria-label^="Открыть "]').first();
  await irow2.locator("img").first().click({ position: { x: 10, y: 60 } });
  await page.waitForTimeout(700);
  const modsTab2 = page.getByRole("tab", { name: /^Моды/ });
  await modsTab2.click();
  await page.waitForTimeout(500);
  await page.getByRole("button", { name: "Добавить моды" }).click();
  await page.waitForSelector("[role='dialog']");
  await page.waitForTimeout(1500);
  const tabs = page.locator("[role='dialog']").first().getByRole("tablist", { name: "Тип контента" });
  const nTabs = await tabs.getByRole("tab").count();
  nTabs === 4 ? ok("добавление: 4 вкладки типа (датапаки на месте)") : fail(`вкладок ${nTabs}, ожидалось 4`);
  await tabs.getByRole("tab", { name: "Датапаки" }).click();
  await page.waitForTimeout(1200);
  (await tabs.getByRole("tab", { name: "Датапаки" }).getAttribute("aria-selected")) === "true"
    ? ok("вкладка «Датапаки» переключается")
    : fail("вкладка «Датапаки» не выбирается");
  await page.keyboard.press("Escape");
  await page.waitForTimeout(300);

  // 6. Настройки: «Дисковое пространство» + экспорт/импорт.
  await page.getByRole("navigation").getByRole("button", { name: /^Настройки/ }).click();
  await page.waitForTimeout(600);
  (await page.getByText("Дисковое пространство").count()) >= 1
    ? ok("настройки: секция «Дисковое пространство»")
    : fail("нет секции storage");
  (await page.getByRole("button", { name: "Подсчитать" }).count()) === 1
    ? ok("storage: кнопка «Подсчитать»")
    : fail("нет кнопки «Подсчитать»");
  (await page.getByRole("button", { name: "Экспорт настроек" }).count()) === 1 &&
  (await page.getByRole("button", { name: "Импорт настроек" }).count()) === 1
    ? ok("настройки: экспорт/импорт на месте")
    : fail("нет кнопок экспорта/импорта настроек");

  console.log(failed === 0 ? "DONE: parity2_features зелёная" : `DONE с ошибками: ${failed}`);
  await browser.close();
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((e) => {
  console.error("FAIL:", e.message);
  process.exit(1);
});
