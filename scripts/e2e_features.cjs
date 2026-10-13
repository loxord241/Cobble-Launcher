// E2E-тест новых возможностей: настройки инстанса, переименование, логи, аккаунты.
/* eslint-disable */
const { chromium } = require("playwright-core");

async function main() {
  const browser = await chromium.connectOverCDP("http://localhost:9223");
  const page = browser.contexts()[0].pages()[0];
  page.setDefaultTimeout(20000);
  // D62: снапшоты для отката после прогона (настройки инстанса, аккаунты).
  let instSnap = null;
  let accSnap = null;
  let testNick = null;
  const invoke = (cmd, args = {}) =>
    page.evaluate(async ({ cmd, args }) => window.__TAURI__.core.invoke(cmd, args), { cmd, args });
  try {
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
  // D62: снапшот инстанса ДО правки — восстановим настройки/имя в finally.
  try {
    const rowLabel = await page
      .locator('[role="button"][aria-label^="Открыть "]')
      .first()
      .getAttribute("aria-label");
    const rowName = (rowLabel || "").replace(/^Открыть\s+/, "");
    const list = await invoke("instance_list");
    instSnap = list.find((i) => i.name === rowName) || list[0] || null;
  } catch (e) {
    console.error("WARN: снапшот инстанса недоступен (нужен withGlobalTauri):", e.message || e);
  }
  await page.getByRole("menuitem", { name: "Настройки" }).click();
  await page.waitForSelector("role=dialog");
  await page.waitForSelector("text=Оперативная память (МБ)");
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
  // D62: снапшот активного аккаунта ДО добавления — вернём его в finally.
  testNick = `Tester_${Date.now().toString().slice(-4)}`;
  try {
    const settings = await invoke("settings_get");
    accSnap = { activeId: settings.accountsActiveId || null };
  } catch (e) {
    console.error("WARN: снапшот аккаунтов недоступен (нужен withGlobalTauri):", e.message || e);
  }
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
  } finally {
    // D62: откат — настройки инстанса и аккаунты возвращаются даже при FAIL.
    // Требуется withGlobalTauri (временный флаг приёмки): без него — только WARN.
    try {
      if (accSnap) {
        const accounts = await invoke("account_list");
        const test = testNick && accounts.find((a) => a.name === testNick);
        if (test) await invoke("account_remove", { id: test.id });
        const settings = await invoke("settings_get");
        if (accSnap.activeId && settings.accountsActiveId !== accSnap.activeId) {
          // account_remove активного сам переназначает первого — задаём исходного явно.
          await invoke("account_active_set", { id: accSnap.activeId });
        }
        console.log("OK: откат аккаунтов выполнен");
      }
    } catch (e) {
      console.error("WARN: не удалось откатить аккаунты:", e.message || e);
    }
    try {
      if (instSnap) {
        const cur = await invoke("instance_settings_get", { id: instSnap.id });
        if (cur && cur.name !== instSnap.name) {
          // Шаг «Переименовать» мог сменить имя — возвращаем исходное.
          await invoke("instance_rename", { id: instSnap.id, name: instSnap.name });
        }
        await invoke("instance_settings_set", { instance: instSnap });
        console.log("OK: настройки инстанса восстановлены из снапшота");
      }
    } catch (e) {
      console.error("WARN: не удалось откатить настройки инстанса:", e.message || e);
    }
    await browser.close().catch(() => {});
  }
}

main().catch((e) => {
  console.error("FAIL:", e.message);
  process.exit(1);
});
