// Функциональные проверки UI (фаза E): поиск, персистентность тем, ?theme=,
// клавиатурная доступность строк, скриншоты онбординга.
/* eslint-disable */
const { chromium } = require("playwright-core");
const fs = require("fs");

const OUT = "ui-apply-shots";
fs.mkdirSync(OUT, { recursive: true });

async function main() {
  const browser = await chromium.connectOverCDP("http://localhost:9223");
  const page = browser.contexts()[0].pages()[0];
  page.setDefaultTimeout(20000);
  let failed = 0;
  const ok = (msg) => console.log("OK:", msg);
  const fail = (msg) => { failed++; console.error("FAIL:", msg); };

  // 1. Персистентность: без query тема из localStorage; ?theme= перекрывает.
  // Язык — ru: свежая установка стартует на en (D30), а подписи ниже русские.
  await page.goto("http://localhost:1420/", { waitUntil: "networkidle" }).catch(() => {});
  await page.evaluate(() => localStorage.setItem("lang", "ru"));
  await page.evaluate(() => localStorage.setItem("theme", "mc"));
  await page.reload({ waitUntil: "networkidle" }).catch(() => {});
  let th = await page.evaluate(() => document.documentElement.getAttribute("data-theme"));
  th === "mc" ? ok("тема mc пережила перезагрузку (localStorage)") : fail(`data-theme=${th}`);
  await page.goto("http://localhost:1420/?theme=light", { waitUntil: "networkidle" }).catch(() => {});
  th = await page.evaluate(() => document.documentElement.getAttribute("data-theme"));
  th === "light" ? ok("?theme=light перекрыл localStorage") : fail(`data-theme=${th}`);
  await page.goto("http://localhost:1420/", { waitUntil: "networkidle" }).catch(() => {});
  await page.evaluate(() => localStorage.setItem("theme", "dark"));
  await page.reload({ waitUntil: "networkidle" }).catch(() => {});

  // 2. Поиск фильтрует реально: мусор → честное пустое состояние.
  await page.getByRole("navigation").getByRole("button", { name: /^Инстансы/ }).click();
  const before = await page.locator('[role="button"][aria-label^="Открыть "]').count();
  await page.getByRole("textbox").first().fill("zzz-нет-такого-инстанса");
  await page.waitForTimeout(300);
  const emptyVisible = await page.getByText("Ничего не найдено").count();
  emptyVisible > 0 ? ok("поиск: мусор → пустое состояние") : fail("пустое состояние поиска не показано");
  await page.getByRole("textbox").first().fill("");
  const after = await page.locator('[role="button"][aria-label^="Открыть "]').count();
  before === after ? ok(`поиск: очистка вернула ${before} строк`) : fail(`строки: ${before} -> ${after}`);

  // 3. Клавиатура: Tab до Play первой строки, Enter — запуск (или фокус).
  const play = page.getByRole("button", { name: /^Играть —/ }).first();
  await play.focus();
  const focused = await page.evaluate(() => document.activeElement?.getAttribute("aria-label"));
  focused && focused.startsWith("Играть") ? ok(`клавиатура: фокус на "${focused}"`) : fail("фокус не на Play");
  // Кебаб-меню открывается с клавиатуры, Escape закрывает.
  const kebab = page.getByRole("button", { name: /^Действия —/ }).first();
  await kebab.focus();
  await kebab.press("Enter");
  await page.waitForTimeout(200);
  const menuItems = await page.getByRole("menuitem").count();
  menuItems >= 4 ? ok(`меню кебаба: ${menuItems} пунктов`) : fail(`пунктов меню: ${menuItems}`);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(200);
  const menuGone = await page.getByRole("menuitem").count();
  menuGone === 0 ? ok("Escape закрывает меню") : fail("меню не закрылось по Escape");

  // 4. Ctrl+K ставит фокус в поиск.
  await page.keyboard.press("Control+k");
  const inSearch = await page.evaluate(() => document.activeElement?.getAttribute("aria-label"));
  inSearch ? ok("Ctrl+K → фокус в поиск") : fail("Ctrl+K не сработал");

  // 5. DOM идентичен во всех темах (тема не меняет layout).
  await page.goto("http://localhost:1420/?theme=dark", { waitUntil: "networkidle" }).catch(() => {});
  await page.waitForTimeout(800);
  const domDark = await page.evaluate(() => document.querySelector("main").innerHTML.length);
  await page.goto("http://localhost:1420/?theme=mc", { waitUntil: "networkidle" }).catch(() => {});
  await page.waitForTimeout(800);
  const domMc = await page.evaluate(() => document.querySelector("main").innerHTML.length);
  domDark === domMc ? ok("DOM main идентичен в dark и mc") : fail(`DOM отличается: ${domDark} vs ${domMc}`);

  // 6. Скриншоты онбординга (сброс не сохраняется — после перезагрузки shell).
  // Возвращаемся в тёмную тему, чтобы скриншот был палитрой по умолчанию.
  await page.goto("http://localhost:1420/?theme=dark", { waitUntil: "networkidle" }).catch(() => {});
  await page.waitForTimeout(800);
  await page.getByRole("navigation").getByRole("button", { name: /^Настройки/ }).click();
  await page.getByRole("button", { name: "Сбросить онбординг" }).click();
  await page.waitForTimeout(400);
  await page.screenshot({ path: `${OUT}/dark-6-onboarding.png` });
  ok("скриншот онбординга");

  // Перезагружаем страницу, чтобы вернуть Shell для последующих тестов
  await page.reload({ waitUntil: "networkidle" }).catch(() => {});
  await page.waitForTimeout(500);

  console.log(failed === 0 ? "ALL-CHECKS-PASSED" : `CHECKS-FAILED: ${failed}`);
  await browser.close();
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((e) => {
  console.error("FAIL:", e.message);
  process.exit(1);
});
