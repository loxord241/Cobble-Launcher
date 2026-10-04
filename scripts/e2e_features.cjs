// E2E-тест новых возможностей: настройки инстанса, переименование, логи, аккаунты.
/* eslint-disable */
const { chromium } = require("playwright-core");

async function main() {
  const browser = await chromium.connectOverCDP("http://localhost:9223");
  const page = browser.contexts()[0].pages()[0];
  page.setDefaultTimeout(20000);
  await page.goto("http://localhost:1420/?theme=dark", { waitUntil: "networkidle" }).catch(() => {});
  // Язык интерфейса — ru: свежая установка стартует на en (D30), проверки ниже русскоязычные.
  await page.evaluate(() => localStorage.setItem("lang", "ru"));
  await page.reload({ waitUntil: "networkidle" }).catch(() => {});
  await page.waitForTimeout(500);

  console.log("==> 1. Открытие страницы Инстансы");
  await page.getByRole("navigation").getByRole("button", { name: /^Инстансы/ }).click();
  await page.waitForTimeout(600);

  // Проверяем, есть ли хотя бы один инстанс. Если нет, создаем тестовый инстанс.
  const rowsCount = await page.locator('[role="button"][aria-label^="Открыть "]').count();
  if (rowsCount === 0) {
    console.log("Создаем тестовый инстанс...");
    await page.getByRole("button", { name: "Создать инстанс" }).first().click();
    await page.waitForSelector("role=dialog");
    await page.fill("#ci-name", "E2E-Instance");
    await page.waitForFunction(() => document.querySelectorAll("#ci-ver option").length > 0);
    await page.getByRole("button", { name: "Создать инстанс" }).last().click();
    await page.waitForSelector("text=E2E-Instance", { timeout: 15000 });
    console.log("OK: тестовый инстанс создан");
  }

  // 2. Проверка кебаб-меню и всех пунктов
  console.log("==> 2. Проверка кебаб-меню инстанса");
  const kebab = page.getByRole("button", { name: /^Действия —/ }).first();
  await kebab.click();
  await page.waitForTimeout(300);

  const menuItems = page.getByRole("menuitem");
  const count = await menuItems.count();
  if (count < 7) {
    throw new Error(`Ожидалось не менее 7 пунктов меню, получено: ${count}`);
  }
  console.log(`OK: кебаб-меню содержит ${count} пунктов`);

  // 3. Настройки инстанса
  console.log("==> 3. Проверка настроек инстанса");
  await page.getByRole("menuitem", { name: "Настройки" }).click();
  await page.waitForSelector("role=dialog");
  await page.waitForSelector("text=Выделение оперативной памяти");
  // Меняем заметки и флаги
  await page.fill("#inst-jvm-flags", "-XX:+UseG1GC -Dlauncher.test=true");
  await page.fill("#inst-notes", "Тестовая заметка E2E");
  await page.getByRole("button", { name: "Сохранить" }).click();
  await page.waitForSelector("role=dialog", { state: "detached", timeout: 5000 });
  console.log("OK: настройки инстанса сохранены");

  // 4. Переименование инстанса
  console.log("==> 4. Проверка переименования инстанса");
  await page.getByRole("button", { name: /^Действия —/ }).first().click();
  await page.waitForTimeout(300);
  await page.getByRole("menuitem", { name: "Переименовать" }).click();
  await page.waitForSelector("role=dialog");
  const newName = `Renamed-${Date.now().toString().slice(-4)}`;
  await page.fill("#rename-input", newName);
  await page.getByRole("button", { name: "Сохранить" }).click();
  await page.waitForSelector("role=dialog", { state: "detached", timeout: 5000 });
  await page.waitForSelector(`text=${newName}`);
  console.log(`OK: инстанс переименован в ${newName}`);

  // 5. Просмотр логов
  console.log("==> 5. Проверка диалога логов");
  const renamedKebab = page.getByRole("button", { name: new RegExp(`^Действия — ${newName}`) }).first();
  await renamedKebab.click();
  await page.waitForTimeout(300);
  await page.getByRole("menuitem", { name: "Логи" }).click();
  await page.waitForSelector("role=dialog");
  await page.waitForSelector("text=Автопрокрутка вниз");
  // Закрываем окно логов кликом по кнопке Закрыть
  await page.locator("role=dialog").getByRole("button", { name: "Закрыть" }).click();
  await page.waitForSelector("role=dialog", { state: "detached", timeout: 5000 });
  console.log("OK: диалог логов открыт и закрыт");

  // 6. Управление аккаунтами
  console.log("==> 6. Проверка модального окна аккаунтов");
  // Кликаем по чипу профиля в титлбаре
  const accountChip = page.getByRole("button", { name: /^Аккаунт/ }).first();
  if (await accountChip.isVisible()) {
    await accountChip.click();
    await page.getByRole("menuitem", { name: "Управление аккаунтами…" }).click();
  } else {
    await page.getByRole("button", { name: /Профиль/ }).click();
  }
  await page.waitForSelector("role=dialog");
  await page.waitForSelector("text=Управление аккаунтами");

  // Добавляем тестовый офлайн-профиль
  const testNick = `Tester_${Date.now().toString().slice(-4)}`;
  await page.fill("input[placeholder='Никнейм игрока']", testNick);
  await page.getByRole("button", { name: "Добавить" }).click();
  await page.waitForTimeout(600);
  await page.waitForSelector(`text=${testNick}`);
  console.log(`OK: добавлен профиль ${testNick}`);

  // Закрываем окно аккаунтов
  await page.getByRole("button", { name: "Готово" }).click();
  await page.waitForTimeout(500);

  // Проверяем, что в титлбаре появился новый ник
  await page.waitForSelector(`text=${testNick}`);
  console.log(`OK: активный профиль ${testNick} отображается в титлбаре`);

  console.log("==> ALL E2E FEATURE CHECKS PASSED!");
  await browser.close();
}

main().catch((e) => {
  console.error("FAIL:", e.message);
  process.exit(1);
});
