// Приёмка «Сервер из инстанса» (D68): создать сервер из инстанса «Ферма»
// (vanilla, без мира), принять EULA, запустить, дождаться живого лога,
// корректно остановить («stop» → мир сохранён) и удалить сервер.
/* eslint-disable */
const { chromium } = require("playwright-core");
const fs = require("fs");
const path = require("path");

const SERVER_NAME = "Приёмка-сервер";
const PORT = 25580;
const DATA = path.join(process.env.LOCALAPPDATA || "", "mc-launcher-v2");
const SERVERS = path.join(DATA, "servers");

async function main() {
  const browser = await chromium.connectOverCDP("http://localhost:9223");
  const page = browser.contexts()[0].pages()[0];
  page.setDefaultTimeout(30000);
  let failed = 0;
  const ok = (m) => console.log("OK:", m);
  const fail = (m) => { failed++; console.error("FAIL:", m); };
  await page.goto("http://localhost:1420/", { waitUntil: "networkidle" }).catch(() => {});
  await page.evaluate(() => localStorage.setItem("lang", "ru"));
  await page.reload({ waitUntil: "networkidle" }).catch(() => {});

  // 1. Страница инстанса «Ферма» → вкладка «Серверы».
  await page.getByRole("navigation").getByRole("button", { name: /^Инстансы/ }).click();
  await page.waitForTimeout(800);
  const tile = page.locator('[role="button"][aria-label^="Открыть "]', { hasText: "Ферма" }).first();
  await tile.first().click();
  await page.waitForTimeout(800);
  await page.getByRole("tab", { name: "Серверы" }).click();
  await page.waitForTimeout(600);
  (await page.getByText("Серверов пока нет").count()) >= 1
    ? ok("вкладка «Серверы»: пустое состояние")
    : ok("вкладка «Серверы»: уже есть серверы (почистим в конце)");

  // 1.5 Самоочистка: карточки прошлых прогонов с тем же именем удаляем.
  async function deleteCardByName() {
    const card = page.locator("section, li, div").filter({ hasText: SERVER_NAME }).getByRole("button", { name: "Удалить сервер" }).first();
    if (await card.count()) {
      await card.click();
      await page.locator('[role="alertdialog"]').waitFor({ timeout: 10000 });
      await page.locator('[role="alertdialog"]').getByRole("button", { name: "Удалить" }).click();
      await page.waitForTimeout(2500);
      ok("карточка прошлого прогона удалена");
    }
  }
  await deleteCardByName();

  // 2. Мастер создания: имя/мир «без мира»/порт/RAM.
  await page.getByRole("button", { name: "Создать сервер из инстанса" }).first().click();
  await page.waitForSelector("[role='dialog']");
  const dialog = page.locator("[role='dialog']").first();
  await dialog.locator("input").first().fill(SERVER_NAME);
  const portInput = dialog.locator('input[type="number"]').first();
  await portInput.fill(String(PORT));
  // RAM-ползунок: оставляем дефолт (2048) — инпут рядом с ползунком.
  await dialog.getByRole("button", { name: "Создать" }).click();
  // Создание качает server.jar (~47 МБ) — до 5 минут.
  await page.getByText("Сервер создан", { exact: false }).or(page.getByText(/Скопировано модов/)).first()
    .waitFor({ timeout: 300000 });
  ok("сервер создан (мастер дошёл до сводки)");
  await dialog.getByRole("button", { name: "Готово" }).click();
  await page.waitForTimeout(800);

  // 3. Карточка сервера: EULA не принят → блок подтверждения.
  await page.getByText("Не принята", { exact: false }).first().waitFor({ timeout: 20000 });
  ok("карточка: EULA не принята — блок на месте");
  await page.getByRole("button", { name: "Прочитать EULA Minecraft" }).count() >= 1
    ? ok("ссылка на текст EULA есть")
    : fail("нет ссылки на текст EULA");
  await page.getByRole("button", { name: "Подтвердить EULA" }).first().click();
  await page.locator('[role="alertdialog"]').waitFor({ timeout: 10000 });
  await page.locator('[role="alertdialog"]').getByRole("button", { name: "Подтвердить EULA" }).click();
  await page.waitForTimeout(1500);
  // После принятия блок EULA исчезает целиком (нет данных — нет элемента):
  // успех = «Не принята» больше не видна.
  (await page.getByText("Не принята", { exact: false }).count()) === 0
    ? ok("EULA принята через подтверждение (блок исчез)")
    : fail("блок «Не принята» не исчез после подтверждения");

  // 4. Запуск: статус «Работает», лог живой.
  await page.getByRole("button", { name: "Запустить" }).click();
  // Первый старт: генерация мира — до 4 минут до «Done».
  await page.getByText(/Работает \(PID/).first().waitFor({ timeout: 30000 });
  ok("сервер запущен (статус «Работает»)");
  await page.getByText("Лог сервера", { exact: false }).first().waitFor({ timeout: 15000 });
  await page
    .locator("pre, code, [class*='font-mono']")
    .filter({ hasText: /Starting minecraft server|Loading properties|Done \(/ })
    .first()
    .waitFor({ timeout: 240000 });
  ok("живой лог сервера (строки старта/готовности)");

  // 5. Файлы на диске: eula.txt, server.properties, server.jar.
  const dir = fs.readdirSync(SERVERS).find((d) => d.startsWith("server-") || d.startsWith("priemka-"));
  if (!dir) fail("каталог сервера не найден в " + SERVERS);
  else {
    const sdir = path.join(SERVERS, dir);
    const has = (f) => fs.existsSync(path.join(sdir, f));
    has("eula.txt") ? ok("eula.txt на диске") : fail("нет eula.txt");
    has("server.properties") ? ok("server.properties на диске") : fail("нет server.properties");
    const props = fs.readFileSync(path.join(sdir, "server.properties"), "utf8");
    props.includes(`server-port=${PORT}`) ? ok(`порт ${PORT} в server.properties`) : fail("порт не записан");
    props.includes("online-mode=true") ? ok("online-mode=true (честный дефолт)") : fail("online-mode не true");
  }

  // 6. Корректная остановка: «stop» в stdin → статус «Остановлен».
  await page.getByRole("button", { name: "Остановить" }).click();
  await page.getByText("Остановлен", { exact: false }).first().waitFor({ timeout: 90000 });
  ok("сервер корректно остановлен (статус «Остановлен»)");
  await page.waitForTimeout(1500);
  // Мир сохранён? У vanilla без мира-источника генерится свой: смотрим world/region.
  if (dir) {
    const world = path.join(SERVERS, dir, "world");
    fs.existsSync(world) ? ok("мир сервера на диске") : fail("каталог world не создан");
  }

  // 7. Удаление сервера (чистим за собой): подтверждение.
  await page.getByRole("button", { name: "Удалить сервер" }).last().click();
  await page.locator('[role="alertdialog"]').waitFor({ timeout: 10000 });
  await page.locator('[role="alertdialog"]').getByRole("button", { name: "Удалить" }).click();
  await page.waitForTimeout(2500);
  (await page.getByText(SERVER_NAME).count()) === 0
    ? ok("сервер удалён из списка")
    : fail("сервер остался в списке после удаления");

  failed === 0 ? console.log("DONE: server_acceptance зелёная") : console.error(`ИТОГО FAIL: ${failed}`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((e) => { console.error("CRASH:", e.message); process.exit(1); });
