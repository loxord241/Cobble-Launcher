// E2E-проверка kill-фикса: запуск из UI → «Запущено» → Стоп → кнопка Играть вернулась.
const { chromium } = require("playwright-core");

async function main() {
  const browser = await chromium.connectOverCDP("http://localhost:9223");
  const page = browser.contexts()[0].pages()[0];
  page.setDefaultTimeout(60000);

  // B2: дефолт чистой установки — en (D30); приёмки русскоязычные → ставим ru.
  // D62: goto ДО setItem — иначе lang может быть перезаписан инициализацией приложения.
  await page.goto("http://localhost:1420/", { waitUntil: "networkidle" }).catch(() => {});
  await page.evaluate(() => localStorage.setItem("lang", "ru"));
  await page.reload({ waitUntil: "networkidle" }).catch(() => {});

  // Ждём shell (приложение уже онбордингнуто).
  await page.waitForSelector("text=Инстансы", { timeout: 20000 });
  await page.getByRole("navigation").getByRole("button", { name: /^Инстансы/ }).click().catch(() => {}); // D40: каталог на Главной, кнопки «Играть —» только на Инстансах
  await page.waitForTimeout(500);

  // Кнопка Play в строке инстанса (не в hero — у hero просто «Играть»).
  const playBtn = page.locator('button[aria-label^="Играть —"]').first();
  // D62: при нуле инстансов getAttribute вернёт null — раньше падали TypeError на .replace.
  const playLabel = await playBtn.getAttribute("aria-label");
  if (!playLabel) {
    throw new Error("кнопка «Играть — …» не найдена — инстансов ноль? Создайте инстанс перед приёмкой");
  }
  const rowName = playLabel.replace("Играть — ", "");
  console.log("инстанс для теста:", rowName);
  await playBtn.click();
  console.log("OK: клик Играть");

  // Ждём признак запуска: кнопка Стоп в строке.
  const stopBtn = page.locator(`button[aria-label^="Остановить — ${rowName}"]`);
  await stopBtn.waitFor({ timeout: 180000 }); // может качать/готовить
  console.log("OK: статус «Запущено» (кнопка Стоп появилась)");

  // Стоп: кнопка обязана вернуться в Играть — это и есть фикс события Exited.
  await stopBtn.click();
  console.log("OK: клик Стоп");
  await page.locator(`button[aria-label="Играть — ${rowName}"]`).waitFor({ timeout: 15000 });
  console.log("PASS: после Стопа кнопка Играть вернулась (событие Exited дошло)");
  await page.screenshot({ path: "docs/kill-acceptance/after-stop.png" });
}

main().then(
  () => process.exit(0),
  (e) => {
    console.error("FAIL:", e.message);
    process.exit(1);
  },
);
