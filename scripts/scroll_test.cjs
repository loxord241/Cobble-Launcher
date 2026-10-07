// Авто-догрузка: в каталоге главной (D39) список обязан вырасти за пределы первой страницы.
const { chromium } = require("playwright-core");
async function main() {
  const browser = await chromium.connectOverCDP("http://localhost:9223");
  const page = browser.contexts()[0].pages()[0];
  page.setDefaultTimeout(30000);
  // Самодостаточность: сброс к чистому состоянию (после других приёмок окно
  // может остаться на детальной странице инстанса с перекрытием).
  await page.goto("http://localhost:1420/", { waitUntil: "networkidle" }).catch(() => {});
  await page.evaluate(() => localStorage.setItem("lang", "ru"));
  await page.reload({ waitUntil: "networkidle" }).catch(() => {});
  await page.waitForTimeout(800);
  await page.getByRole("navigation").getByRole("button", { name: /^Главная/ }).click();
  const count = () => page.evaluate(() => document.querySelectorAll("[aria-label^='Установить — ']").length);
  await page.waitForFunction(
    () => document.querySelectorAll("[aria-label^='Установить — ']").length >= 20,
    { timeout: 30000 },
  );
  const before = await count();
  await page.locator("main").evaluate((m) => m.scrollTo(0, m.scrollHeight));
  await page.waitForFunction(
    (n) => document.querySelectorAll("[aria-label^='Установить — ']").length > n,
    before,
    { timeout: 30000 },
  );
  const after = await count();
  console.log(`после автодогрузки: ${before} -> ${after} — PASS`);
}
main().then(() => process.exit(0), (e) => { console.error("FAIL:", e.message); process.exit(1); });
