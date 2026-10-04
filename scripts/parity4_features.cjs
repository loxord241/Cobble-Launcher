// Приёмка D37-C: палитра команд, конфиги, свой authlib-сервер, лимит
// скорости, Discord RPC. Presence-based.
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

  const btn = page.getByRole("button", { name: "Открыть палитру команд" });
  if ((await btn.count()) === 1) {
    await btn.click();
    await page.waitForTimeout(300);
    const dlg = page.locator("div[role='dialog'][aria-label='Открыть палитру команд']");
    (await dlg.count()) === 1 ? ok("палитра открывается") : fail("палитра не открылась");
    const items = await dlg.getByRole("option").count();
    items > 3 ? ok(`палитра: ${items} команд`) : fail(`в палитре ${items} пунктов`);
    await page.keyboard.press("Escape");
    await page.waitForTimeout(300);
    (await dlg.count()) === 0 ? ok("Esc закрывает палитру") : fail("Esc не закрыл");
  } else fail("нет кнопки палитры в титлбаре");

  await page.getByRole("navigation").getByRole("button", { name: /^Инстансы/ }).click();
  await page.waitForTimeout(600);
  const card = page.locator('[role="button"][aria-label^="Открыть "]').first();
  await card.click();
  await page.waitForTimeout(500);
  const cfgTab = page.getByRole("tab", { name: "Конфиги модов" });
  (await cfgTab.count()) === 1 ? ok("вкладка «Конфиги модов»") : fail("нет вкладки конфигов");
  await cfgTab.click();
  await page.waitForTimeout(800);
  const empty = (await page.getByText("Конфигов пока нет").count()) > 0;
  const list = (await page.locator("ul li button").count()) > 0;
  empty || list ? ok("конфиги: честный empty или список") : fail("конфиги пусты и без empty");
  await page.getByRole("button", { name: /^Назад/ }).first().click();
  await page.waitForTimeout(400);

  await page.getByRole("button", { name: /, сменить$/ }).click();
  await page.waitForTimeout(300);
  await page.getByRole("menuitem", { name: /Управление аккаунтами/ }).click();
  await page.waitForSelector("role=dialog");
  (await page.getByRole("button", { name: "Свой сервер" }).count()) === 1
    ? ok("аккаунты: вкладка «Свой сервер»")
    : fail("нет вкладки «Свой сервер»");
  await page.getByRole("button", { name: "Свой сервер" }).click();
  await page.waitForTimeout(300);
  (await page.getByPlaceholder(/authlib-сервер/).count()) === 1
    ? ok("форма authlib: поле URL")
    : fail("нет поля URL authlib");
  await page.getByRole("button", { name: "Закрыть" }).last().click();
  await page.waitForTimeout(400);

  await page.getByRole("navigation").getByRole("button", { name: /^Настройки/ }).click();
  await page.waitForTimeout(600);
  (await page.getByText("Ограничение скорости загрузки").count()) >= 1
    ? ok("настройки: лимит скорости")
    : fail("нет лимита скорости");
  (await page.getByText("Показывать игру в Discord").count()) >= 1
    ? ok("настройки: Discord RPC")
    : fail("нет Discord RPC");

  console.log(failed === 0 ? "DONE: parity4 зелёная" : `DONE с ошибками: ${failed}`);
  await browser.close();
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((e) => {
  console.error("FAIL:", e.message);
  process.exit(1);
});
