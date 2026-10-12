// Приёмка D72-хвоста: «Что нового» (фикс молчаливого проглатывания при
// обновлении со старой версии) + восстановление бэкапа по клику на
// ОДНОРАЗОВОМ инстансе (создаётся и удаляется в конце, zip'ы чистятся) —
// реальные данные владельца не трогаются.
/* eslint-disable */
const { chromium } = require("playwright-core");
const fs = require("fs");
const path = require("path");

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

  // ============ 0. Пре-уборка хвостов прошлого прогона (идемпотентный rerun) ============
  await page.getByRole("navigation").getByRole("button", { name: /^Инстансы/ }).click();
  await page.waitForTimeout(600);
  for (const nm of ["RestoreAccept2", "RestoreAccept"]) {
    const r = page.locator(`[role="button"][aria-label="Открыть ${nm}"]`);
    if ((await r.count()) > 0) {
      console.log(`хвост прошлого прогона: удаляю инстанс ${nm}`);
      await r.getByRole("button", { name: /^Действия/ }).click();
      await page.waitForTimeout(300);
      await page.getByRole("menuitem", { name: "Удалить", exact: true }).click();
      await page.waitForSelector("role=dialog");
      // «Удалить навсегда» — сама кнопка подтверждения (чекбокса в диалоге нет).
      await page
        .locator("[role='dialog']")
        .getByRole("button", { name: "Удалить навсегда, вместе с данными" })
        .click();
      await r.waitFor({ state: "detached", timeout: 60000 });
    }
  }
  const bdir = path.join(process.env.LOCALAPPDATA || "", "mc-launcher-v2", "backups");
  for (const f of fs.existsSync(bdir) ? fs.readdirSync(bdir) : []) {
    if (f.startsWith("RestoreAccept") && f.endsWith(".zip")) fs.unlinkSync(path.join(bdir, f));
  }

  // ============ 1. «Что нового»: профиль-обновление (seen — старая версия) ============
  console.log("==> 1. WhatsNew: обновление со старой версии показывает окно");
  await page.evaluate(() => {
    localStorage.removeItem("changelogFreshProfile");
    localStorage.setItem("changelogSeenVersion", "0.0.1");
  });
  await page.reload({ waitUntil: "networkidle" }).catch(() => {});
  const wnw = page.locator("[role='dialog'][aria-label^='Что нового в']");
  try {
    await wnw.waitFor({ state: "visible", timeout: 10000 });
    const label = await wnw.getAttribute("aria-label");
    const version = (label.match(/[\d.]+$/) || ["?"])[0];
    const items = await wnw.locator("ul li").count();
    items >= 3
      ? ok(`«Что нового» появилось (${label}), пунктов: ${items}`)
      : fail(`в окне мало пунктов: ${items}`);
    await wnw.getByRole("button", { name: "Продолжить" }).click();
    await wnw.waitFor({ state: "detached", timeout: 5000 });
    const seen = await page.evaluate(() => localStorage.getItem("changelogSeenVersion"));
    seen === version
      ? ok(`закрытие записало seen=${seen}`)
      : fail(`seen=${seen}, ожидалось ${version}`);
  } catch {
    fail("окно «Что нового» не появилось для обновившегося профиля");
  }

  // ============ 2. «Что нового»: свежая установка (маркер) — окно НЕ показывается ============
  console.log("==> 2. WhatsNew: свежая установка — окно не показывается");
  await page.evaluate(() => {
    localStorage.removeItem("changelogSeenVersion");
    localStorage.setItem("changelogFreshProfile", "1");
  });
  await page.reload({ waitUntil: "networkidle" }).catch(() => {});
  await page.waitForTimeout(3000);
  const ghost = await page.locator("[role='dialog'][aria-label^='Что нового в']").count();
  ghost === 0
    ? ok("свежая установка: окна нет")
    : fail("свежей установке показалось окно «Что нового»");
  const seen2 = await page.evaluate(() => localStorage.getItem("changelogSeenVersion"));
  const freshLeft = await page.evaluate(() => localStorage.getItem("changelogFreshProfile"));
  seen2 && /^\d+\.\d+\.\d+$/.test(seen2)
    ? ok(`свежая установка: версия зафиксирована (${seen2})`)
    : fail(`свежая установка: seen=${seen2}`);
  freshLeft === null
    ? ok("маркер свежего профиля погашен")
    : fail(`маркер не погашен: ${freshLeft}`);

  // ============ 3. Восстановление бэкапа на одноразовом инстансе ============
  console.log("==> 3. Restore: создание одноразового инстанса RestoreAccept");
  // Секции 1–2 перезагружали страницу — маршрут сброшен на «Главную».
  await page.getByRole("navigation").getByRole("button", { name: /^Инстансы/ }).click();
  await page.waitForTimeout(600);
  await page.getByRole("button", { name: "Создать инстанс" }).first().click();
  await page.waitForSelector("role=dialog");
  await page.fill("#ci-name", "RestoreAccept");
  await page.waitForFunction(() => document.querySelectorAll("#ci-ver option").length > 0);
  await page.getByRole("button", { name: "Создать инстанс" }).last().click();
  await page.waitForSelector("text=RestoreAccept", { timeout: 60000 });
  ok("инстанс RestoreAccept создан");

  // 3а. Бэкап через кебаб.
  const row = page.locator('[role="button"][aria-label="Открыть RestoreAccept"]');
  await row.getByRole("button", { name: /^Действия/ }).click();
  await page.waitForTimeout(300);
  await page.getByRole("menuitem", { name: "Бэкап (zip)" }).click();
  await page.waitForSelector("text=Бэкап создан:", { timeout: 300000 });
  const backupMsg = await page.getByText(/Бэкап создан: (.+)/).first().textContent();
  const backupPath = backupMsg.replace("Бэкап создан: ", "").trim();
  fs.existsSync(backupPath)
    ? ok(`бэкап на диске: ${path.basename(backupPath)}`)
    : fail(`бэкапа нет: ${backupPath}`);

  // 3б. Переименование — текущее состояние расходится с бэкапом.
  await row.getByRole("button", { name: /^Действия/ }).click();
  await page.waitForTimeout(300);
  await page.getByRole("menuitem", { name: "Переименовать" }).click();
  await page.waitForSelector("#rename-input");
  await page.fill("#rename-input", "RestoreAccept2");
  await page.locator("[role='dialog']").getByRole("button", { name: "Сохранить" }).click();
  await page.waitForSelector("text=RestoreAccept2", { timeout: 10000 });
  ok("инстанс переименован в RestoreAccept2");

  // 3в. Настройки → Бэкапы → Восстановить (цель — id из архива: имя вернётся).
  console.log("==> 3в. Restore: Настройки → Бэкапы → Восстановить");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /^Настройки/ })
    .click();
  await page.waitForTimeout(600);
  const restoreBtn = page
    .locator("button[aria-label^='Восстановить: RestoreAccept-']")
    .first();
  (await restoreBtn.count()) >= 1
    ? ok("кнопка «Восстановить» в реестре бэкапов на месте")
    : fail("кнопки «Восстановить» нет в реестре");
  await restoreBtn.click();
  // askConfirm рендерит role="alertdialog" (не dialog) — см. src/ui/confirm.tsx.
  const confirmDialog = page.locator("[role='alertdialog']").filter({ hasText: "бэкапа" }).first();
  await confirmDialog.waitFor({ state: "visible", timeout: 5000 });
  await confirmDialog.getByRole("button", { name: "Восстановить", exact: true }).click();
  await page.waitForSelector("text=Восстановлено: RestoreAccept", { timeout: 300000 });
  ok("ядро отчиталось «Восстановлено: RestoreAccept»");

  // 3г. Инстансы: имя вернулось из бэкапа, «2»-имени нет.
  await page.getByRole("navigation").getByRole("button", { name: /^Инстансы/ }).click();
  await page.waitForTimeout(800);
  (await page.locator('[role="button"][aria-label="Открыть RestoreAccept"]').count()) === 1
    ? ok("инстанс вернулся с именем из бэкапа")
    : fail("инстанса RestoreAccept нет после восстановления");
  (await page.locator("text=RestoreAccept2").count()) === 0
    ? ok("следов RestoreAccept2 нет (состояние заменено целиком)")
    : fail("RestoreAccept2 всё ещё виден");

  // ============ 4. Уборка: удалить инстанс навсегда + zip'ы прогона ============
  console.log("==> 4. Уборка");
  const row2 = page.locator('[role="button"][aria-label="Открыть RestoreAccept"]');
  await row2.getByRole("button", { name: /^Действия/ }).click();
  await page.waitForTimeout(300);
  await page.getByRole("menuitem", { name: "Удалить", exact: true }).click();
  await page.waitForSelector("role=dialog");
  await page
    .locator("[role='dialog']")
    .getByRole("button", { name: "Удалить навсегда, вместе с данными" })
    .click();
  await page
    .locator('[role="button"][aria-label="Открыть RestoreAccept"]')
    .waitFor({ state: "detached", timeout: 60000 })
    .then(() => ok("инстанс удалён"))
    .catch(() => fail("инстанс не удалился"));

  const backupsDir = path.join(process.env.LOCALAPPDATA || "", "mc-launcher-v2", "backups");
  let cleaned = 0;
  for (const f of fs.existsSync(backupsDir) ? fs.readdirSync(backupsDir) : []) {
    if (f.startsWith("RestoreAccept") && f.endsWith(".zip")) {
      fs.unlinkSync(path.join(backupsDir, f));
      cleaned++;
    }
  }
  cleaned >= 2
    ? ok(`zip'ы прогона убраны (${cleaned} шт.: бэкап + авто-бэкап)`)
    : fail(`ожидались 2+ zip прогона, убрано: ${cleaned}`);

  console.log(failed === 0 ? "ALL BACKUP-RESTORE CHECKS PASSED" : `FAILED: ${failed}`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((e) => {
  console.error("CRASH:", e);
  process.exit(1);
});
