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

  // 1. Главная-каталог (D39): табы Модпаки/Моды переключаются.
  const homeTabs = page.getByRole("tablist").first();
  (await homeTabs.getByRole("tab").count()) === 2
    ? ok("главная: 2 таба каталога (Модпаки/Моды)")
    : fail(`табов каталога ${await homeTabs.getByRole("tab").count()}, ожидалось 2`);
  await homeTabs.getByRole("tab", { name: "Моды" }).click();
  await page.waitForTimeout(2000);
  (await homeTabs.getByRole("tab", { name: "Моды" }).getAttribute("aria-selected")) === "true"
    ? ok("вкладка «Моды» переключается")
    : fail("вкладка «Моды» не выбирается");
  await homeTabs.getByRole("tab", { name: "Модпаки" }).click();
  await page.waitForTimeout(1500);

  // 2. Страница инстанса: «Установленные» + добавление контента (4 типа).
  await page.getByRole("navigation").getByRole("button", { name: /^Инстансы/ }).click();
  await page.waitForTimeout(600);
  const irow = page.locator('[role="button"][aria-label^="Открыть "]').first();
  await irow.locator("img").first().click({ position: { x: 10, y: 60 } });
  await page.waitForTimeout(700);
  await page.getByRole("tab", { name: /^Моды/ }).click();
  await page.waitForTimeout(600);
  (await page.getByText("Установленные").count()) >= 1
    ? ok("инстанс: панель «Установленные» на месте")
    : fail("нет панели «Установленные» на вкладке Моды");
  const rows = await page.locator("ul li button[role=switch]").count();
  ok(`список установленного: ${rows} записей`);
  if (rows > 0) {
    const sw = page.locator("button[role=switch]").first();
    const before = await sw.getAttribute("aria-checked");
    await sw.click();
    await page.waitForTimeout(600);
    const after = await page.locator("button[role=switch]").first().getAttribute("aria-checked");
    before !== after ? ok(`тумблер вкл/выкл работает (${before} → ${after})`) : fail(`тумблер не сработал (${before})`);
    await page.locator("button[role=switch]").first().click();
    await page.waitForTimeout(400);
  }
  // Добавление контента: 4 вкладки типа (моды/ресурспаки/шейдеры/датапаки).
  await page.getByRole("button", { name: "Добавить моды" }).click();
  await page.waitForSelector("[role='dialog']");
  await page.waitForTimeout(2000);
  const tabs = page.locator("[role='dialog']").first().getByRole("tablist", { name: "Тип контента" });
  (await tabs.getByRole("tab").count()) === 4
    ? ok("добавление: 4 вкладки типа контента")
    : fail(`вкладок типа ${await tabs.getByRole("tab").count()}, ожидалось 4`);
  await tabs.getByRole("tab", { name: "Шейдеры" }).click();
  await page.waitForTimeout(1500);
  (await tabs.getByRole("tab", { name: "Шейдеры" }).getAttribute("aria-selected")) === "true"
    ? ok("вкладка «Шейдеры» переключается")
    : fail("вкладка «Шейдеры» не выбирается");
  await page.keyboard.press("Escape");
  await page.waitForTimeout(400);

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
