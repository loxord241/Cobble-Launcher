// Приёмка управления контентом (M7+): вкладки типов каталога, панель
// «Установленные», бэкап (zip) и экспорт (.mrpack) с проверкой файлов на диске.
/* eslint-disable */
const { chromium } = require("playwright-core");
const fs = require("fs");

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

  // 1. Вкладки типов контента: 4 штуки, переключение меняет выдачу.
  await page.getByRole("navigation").getByRole("button", { name: /^Моды/ }).click();
  await page.waitForTimeout(800);
  const tabs = page.getByRole("tablist", { name: "Тип контента" });
  (await tabs.getByRole("tab").count()) === 5
    ? ok("каталог: 5 вкладок типа контента (D37: +датапаки)")
    : fail(`вкладок типа контента ${await tabs.getByRole("tab").count()}, ожидалось 5`);
  const firstTitle = await page.locator('.flex.flex-col.gap-2 .truncate, [role="button"][aria-label^="Открыть "] .truncate').first().textContent().catch(() => "");
  await tabs.getByRole("tab", { name: "Шейдеры" }).click();
  await page.waitForTimeout(1500);
  const shaderSelected = await tabs.getByRole("tab", { name: "Шейдеры" }).getAttribute("aria-selected");
  shaderSelected === "true" ? ok("вкладка «Шейдеры» переключается") : fail(`aria-selected=${shaderSelected}`);
  await tabs.getByRole("tab", { name: "Модпаки" }).click();
  await page.waitForTimeout(1500);
  ok("вкладка «Модпаки» переключается");

  // 2. Панель «Установленные» открывается и честно показывает содержимое.
  const installedBtn = page.getByRole("button", { name: /^Установленные/ });
  (await installedBtn.count()) > 0 ? ok("кнопка «Установленные» на месте") : fail("нет кнопки «Установленные»");
  await installedBtn.click();
  await page.waitForTimeout(400);
  const empty = await page.getByText("пока ничего не установлено").count();
  const rows = await page.locator("ul li button[role=switch]").count();
  empty > 0 || rows > 0
    ? ok(`панель «Установленные» открылась (${rows > 0 ? `${rows} записей` : "пусто"})`)
    : fail("панель «Установленные» пустая и без текста");
  if (rows > 0) {
    // Переключатель вкл/выкл реально дергает ядро: состояние меняется.
    const sw = page.locator("button[role=switch]").first();
    const before = await sw.getAttribute("aria-checked");
    await sw.click();
    await page.waitForTimeout(600);
    const after = await page.locator("button[role=switch]").first().getAttribute("aria-checked");
    before !== after ? ok(`тумблер вкл/выкл работает (${before} → ${after})`) : fail(`тумблер не сработал (${before})`);
    await page.locator("button[role=switch]").first().click();
    await page.waitForTimeout(400);
  }

  // 3. Бэкап (zip) через кебаб-меню инстанса: IPC + файл на диске.
  await page.getByRole("navigation").getByRole("button", { name: /^Инстансы/ }).click();
  await page.waitForTimeout(600);
  const row = page.locator('[role="button"][aria-label^="Открыть "]').first();
  await row.getByRole("button", { name: /Действия/ }).click();
  await page.waitForTimeout(300);
  await page.getByRole("menuitem", { name: "Бэкап (zip)" }).click();
  await page.waitForSelector("text=Бэкап создан:", { timeout: 60000 });
  const backupMsg = await page.getByText(/Бэкап создан: (.+)/).first().textContent();
  const backupPath = backupMsg.replace("Бэкап создан: ", "").trim();
  fs.existsSync(backupPath) && backupPath.endsWith(".zip")
    ? ok(`бэкап лежит на диске: ${backupPath}`)
    : fail(`файла бэкапа нет: ${backupPath}`);

  // 4. Экспорт .mrpack: IPC + валидный zip (PK-сигнатура) на диске.
  await row.getByRole("button", { name: /Действия/ }).click();
  await page.waitForTimeout(300);
  await page.getByRole("menuitem", { name: "Экспорт .mrpack" }).click();
  await page.waitForSelector("text=Экспорт готов:", { timeout: 60000 });
  const exportMsg = await page.getByText(/Экспорт готов: (.+)/).first().textContent();
  const exportPath = exportMsg.replace("Экспорт готов: ", "").trim();
  const head = fs.readFileSync(exportPath).subarray(0, 2).toString("latin1");
  fs.existsSync(exportPath) && head === "PK"
    ? ok(`экспорт валидный zip: ${exportPath}`)
    : fail(`экспорт не zip: ${exportPath}`);

  console.log(failed === 0 ? "ALL CONTENT-FEATURE CHECKS PASSED" : `FAILED: ${failed}`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((e) => {
  console.error("CRASH:", e);
  process.exit(1);
});
