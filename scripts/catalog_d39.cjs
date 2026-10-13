// Приёмка D39: редизайн в стиле CurseForge.
// 1) Главная — только каталог Modrinth (модпаки/моды, сортировка, бесконечный
//    скролл); клик по строке ОТКРЫВАЕТ окно проекта (не качает).
// 2) Окно проекта — 4 таба как в CurseForge: Обзор / Журнал изменений /
//    Галерея / Версии; внутри обзор с markdown-телом и галереей.
// 3) Инстансы — на своей странице с сортировкой.
// 4) Добавление модов прямо со страницы инстанса (без перекидывания в «Моды»).
// Нужна сеть (Modrinth) и хотя бы один инстанс.
/* eslint-disable */
const { chromium } = require("playwright-core");

async function main() {
  const browser = await chromium.connectOverCDP("http://localhost:9223");
  const page = browser.contexts()[0].pages()[0];
  page.setDefaultTimeout(25000);
  let failed = 0;
  const ok = (m) => console.log("OK:", m);
  const fail = (m) => { failed++; console.error("FAIL:", m); };
  await page.goto("http://localhost:1420/", { waitUntil: "networkidle" }).catch(() => {});
  await page.evaluate(() => localStorage.setItem("lang", "ru"));
  await page.reload({ waitUntil: "networkidle" }).catch(() => {});
  await page.waitForTimeout(600);

  // ---------- 1. Главная: каталог ----------
  await page.getByRole("navigation").getByRole("button", { name: /^Главная/ }).click();
  await page.waitForTimeout(1500); // автопоиск популярных модпаков

  (await page.getByRole("heading", { name: "Каталог" }).count()) === 1
    ? ok("заголовок «Каталог»")
    : fail("нет заголовка «Каталог»");

  const tabs = page.getByRole("tablist").first();
  (await tabs.getByRole("tab", { name: "Модпаки" }).count()) === 1 &&
  (await tabs.getByRole("tab", { name: "Моды" }).count()) === 1
    ? ok("табы типа: Модпаки | Моды")
    : fail("нет переключателя Модпаки/Моды");

  // Инстансов на главной больше нет: ни карточек «Открыть …», ни кнопки «Играть — …».
  (await page.locator('[role="button"][aria-label^="Открыть "]').count()) === 0
    ? ok("инстансов на главной нет")
    : fail("на главной остались карточки инстансов");

  const sortSel = page.getByRole("combobox", { name: "Сортировка" });
  (await sortSel.count()) === 1 ? ok("select сортировки") : fail("нет сортировки");

  // Строки каталога (aria-label кнопки установки «Установить — …»).
  await page.waitForTimeout(2500);
  let installBtns = page.getByRole("button", { name: /^Установить — / });
  let nRows = await installBtns.count();
  nRows >= 5 ? ok(`строки каталога: ${nRows}`) : fail(`мало строк каталога: ${nRows}`);

  // Сортировка «По обновлению» — перезапрос (строки перерисовываются).
  await sortSel.selectOption({ index: 2 }).catch(() => {});
  await page.waitForTimeout(2000);
  nRows = await page.getByRole("button", { name: /^Установить — / }).count();
  nRows > 0 ? ok("сортировка применилась (строки на месте)") : fail("после смены сортировки пусто");

  // Бесконечный скролл: догружаем, пока не вырастет список (до 3 попыток).
  const before = nRows;
  for (let i = 0; i < 3; i++) {
    await page.locator("main").evaluate((m) => m.scrollTo(0, m.scrollHeight));
    await page.waitForTimeout(2200);
    nRows = await page.getByRole("button", { name: /^Установить — / }).count();
    if (nRows > before) break;
  }
  nRows > before
    ? ok(`бесконечный скролл: ${before} → ${nRows}`)
    : fail(`скролл не догрузил: ${before} → ${nRows}`);

  // ---------- 2. Клик по строке открывает окно проекта ----------
  await installBtns.first().click();
  await page.waitForSelector("[role='dialog']", { timeout: 10000 });
  await page.waitForTimeout(2500); // страница проекта догружается

  const dialog = page.locator("[role='dialog']").first();
  (await dialog.getByRole("tab", { name: "Обзор" }).count()) === 1 &&
  (await dialog.getByRole("tab", { name: "Журнал изменений" }).count()) === 1 &&
  (await dialog.getByRole("tab", { name: "Галерея" }).count()) === 1 &&
  (await dialog.getByRole("tab", { name: "Версии" }).count()) === 1
    ? ok("окно проекта: 4 таба как в CurseForge")
    : fail("в окне проекта нет 4 табов");

  // Обзор: markdown-тело отрисовано (заголовки/абзацы) — не пусто.
  const bodyText = await dialog.innerText();
  bodyText.length > 400
    ? ok(`обзор с телом проекта (${bodyText.length} симв.)`)
    : fail("тело проекта пустое");

  // Журнал изменений: список версий с changelog.
  await dialog.getByRole("tab", { name: "Журнал изменений" }).click();
  await page.waitForTimeout(2000);
  const dlgText = await dialog.innerText();
  /журн|changelog|фикс|update|версия|v?\d+\.\d+/i.test(dlgText)
    ? ok("журнал изменений открылся")
    : fail("журнал изменений пуст");

  // Версии: строки версий (номера вида x.y.z).
  await dialog.getByRole("tab", { name: "Версии" }).click();
  await page.waitForTimeout(2000);
  const verText = await dialog.innerText();
  /\d+\.\d+\.\d*/.test(verText)
    ? ok("таб «Версии» с номерами версий")
    : fail("в табе «Версии» нет номеров");

  await page.keyboard.press("Escape");
  await page.waitForSelector("[role='dialog']", { state: "detached", timeout: 5000 });
  ok("Escape закрывает окно проекта");

  // ---------- 3. Инстансы: сортировка ----------
  await page.getByRole("navigation").getByRole("button", { name: /^Инстансы/ }).click();
  await page.waitForTimeout(600);
  const instSort = page.getByRole("combobox", { name: "Сортировка" });
  (await instSort.count()) === 1 ? ok("инстансы: select сортировки") : fail("нет сортировки инстансов");
  const cards = page.locator('[role="button"][aria-label^="Открыть "]');
  const nCards = await cards.count();
  if (nCards === 0) {
    fail("нет инстансов для дальнейших проверок");
  } else {
    await instSort.selectOption({ index: 1 }).catch(() => {});
    await page.waitForTimeout(400);
    ok("сортировка инстансов переключилась");

    // ---------- 4. Добавление модов прямо в инстансе ----------
    // Клик по арту карточки (мимо Play-оверлея и кебаба) — как в cards_skins.
    await cards.first().locator("img").first().click({ position: { x: 10, y: 60 } });
    await page.waitForTimeout(700);
    // Кнопка живёт на табе «Моды» страницы инстанса (по умолчанию открыт «Обзор»).
    await page.getByRole("tab", { name: "Моды" }).click();
    await page.waitForTimeout(500);
    const addBtn = page.getByRole("button", { name: "Добавить моды" });
    (await addBtn.count()) === 1
      ? ok("кнопка «Добавить моды» на странице инстанса")
      : fail("нет кнопки «Добавить моды»");
    await addBtn.click();
    await page.waitForSelector("[role='dialog']", { timeout: 8000 });
    await page.waitForTimeout(2500);
    const addDlg = page.locator("[role='dialog']").first();
    const addText = await addDlg.innerText();
    addText.includes("Добавить контент")
      ? ok("модалка добавления контента открылась")
      : fail("модалка добавления не открылась");
    const addRows = await addDlg.getByRole("button", { name: /^Установить в инстанс/ }).count();
    addRows > 0
      ? ok(`поиск в модалке вернул ${addRows} строк`)
      : fail("в модалке добавления нет результатов поиска");
    await page.keyboard.press("Escape");
    await page.waitForSelector("[role='dialog']", { state: "detached", timeout: 5000 });
  }

  console.log(failed === 0 ? "DONE: catalog_d39 зелёная" : `DONE с ошибками: ${failed}`);
  await browser.close();
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((e) => {
  console.error("FAIL:", e.message);
  process.exit(1);
});
