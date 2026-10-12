// Приёмка волны D36 (bugs B* + phase-1 фич из PARITY_AUDIT_REPORT): тумблер
// «Работать офлайн», кнопка «Импорт», пункт «Снять блокировку», Forge в
// диалоге создания, Quick Play/профиль/пресеты JVM/подсказка Java в
// настройках инстанса, быстрый ник в меню профиля.
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

  // 1. Тумблер «Работать офлайн» в титлбаре: switch, клики переключают.
  const offline = page.getByRole("switch", { name: "Работать офлайн" });
  if ((await offline.count()) === 1) {
    const before = await offline.getAttribute("aria-checked");
    await offline.click();
    await page.waitForTimeout(400);
    const mid = await offline.getAttribute("aria-checked");
    await offline.click();
    await page.waitForTimeout(400);
    const after = await offline.getAttribute("aria-checked");
    before !== mid && mid !== after
      ? ok(`тумблер офлайн переключается (${before}→${mid}→${after})`)
      : fail(`тумблер офлайн не переключается (${before}→${mid}→${after})`);
  } else {
    fail("нет тумблера «Работать офлайн» в титлбаре");
  }

  // 2. Инстансы: кнопка «Импорт» (нативный диалог не автоматизируем — presence).
  await page.getByRole("navigation").getByRole("button", { name: /^Инстансы/ }).click();
  await page.waitForTimeout(600);
  const importBtn = page.getByRole("button", { name: "Импорт", exact: true });
  (await importBtn.count()) >= 1
    ? ok("кнопка «Импорт» на странице Инстансы")
    : fail("нет кнопки «Импорт»");

  // 3. Кебаб карточки: пункт «Снять блокировку» (F27).
  const card = page.locator('[role="button"][aria-label^="Открыть "]').first();
  if ((await card.count()) > 0) {
    await card.getByRole("button", { name: /Действия —/ }).click();
    await page.waitForTimeout(300);
    (await page.getByRole("menuitem", { name: "Снять блокировку" }).count()) === 1
      ? ok("кебаб: «Снять блокировку» на месте")
      : fail("в кебабе нет «Снять блокировку»");

    // 4. Настройки инстанса: Quick Play + профиль + пресеты JVM + Java-подсказка.
    await page.getByRole("menuitem", { name: /Настройки/ }).click();
    await page.waitForSelector("role=dialog");
    const dlg = page.locator("div[role='dialog']");
    (await dlg.getByText("Быстрый запуск").count()) >= 1
      ? ok("настройки инстанса: блок «Быстрый запуск»")
      : fail("нет блока «Быстрый запуск»");
    (await dlg.getByText("Профиль для запуска").count()) >= 1
      ? ok("настройки инстанса: «Профиль для запуска»")
      : fail("нет «Профиль для запуска»");
    (await dlg.getByText("Пресет флагов JVM").count()) >= 1
      ? ok("настройки инстанса: пресеты JVM")
      : fail("нет пресетов JVM");
    (await dlg.getByText(/Рекомендуется Java \d+/).count()) >= 1
      ? ok("настройки инстанса: подсказка Java")
      : fail("нет подсказки рекомендуемой Java");
    await dlg.getByRole("button", { name: "Закрыть" }).click();
    await page.waitForSelector("role=dialog", { state: "detached", timeout: 5000 });
  } else {
    fail("нет карточек инстансов для проверки кебаба/настроек");
  }

  // 5. Диалог создания: Forge в списке загрузчиков (DISC-A01).
  await page.getByRole("button", { name: /^Создать|^Новый/ }).first().click();
  await page.waitForSelector("role=dialog");
  (await page.locator("div[role='dialog']").getByText("Forge", { exact: true }).count()) >= 1
    ? ok("диалог создания: Forge доступен")
    : fail("Forge отсутствует в диалоге создания");
  await page.locator("div[role='dialog']").getByRole("button", { name: "Закрыть" }).click();
  await page.waitForSelector("role=dialog", { state: "detached", timeout: 5000 });

  // 6. Меню профиля: быстрый офлайн-ник (F12).
  await page.getByRole("button", { name: /, сменить$/ }).click();
  await page.waitForTimeout(300);
  (await page.getByPlaceholder("Новый ник…").count()) === 1
    ? ok("меню профиля: поле быстрого ника")
    : fail("нет поля быстрого ника в меню профиля");
  await page.keyboard.press("Escape");
  await page.waitForTimeout(200);

  console.log(failed === 0 ? "DONE: parity1_features зелёная" : `DONE с ошибками: ${failed}`);
  await browser.close();
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((e) => {
  console.error("FAIL:", e.message);
  process.exit(1);
});
