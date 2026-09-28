// Приёмка D34/D35: карточки инстансов в стиле CurseForge (сетка, арт по
// умолчанию, бейдж версии, Play-оверлей) и скины (голова персонажа в чипе
// титлбара и в модалке аккаунтов, «Персонаж» активного профиля).
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

  // 1. Карточки инстансов: сетка, у каждой карточки арт (img с data-URL —
  //    своя иконка или процедурная «карта»), бейдж версии, Play-оверлей.
  await page.getByRole("navigation").getByRole("button", { name: /^Инстансы/ }).click();
  await page.waitForTimeout(600);
  const cards = page.locator('[role="button"][aria-label^="Открыть "]');
  const nCards = await cards.count();
  nCards > 0 ? ok(`инстансы: ${nCards} карточек (сетка)`) : fail("карточек инстансов нет");

  if (nCards > 0) {
    const first = cards.first();
    const gridIsGrid = await page.evaluate(() => {
      const el = document.querySelector('[role="button"][aria-label^="Открыть "]')?.parentElement;
      return el ? getComputedStyle(el).display === "grid" : false;
    });
    gridIsGrid ? ok("контейнер карточек — grid") : fail("контейнер карточек не grid");

    const art = first.locator("img[src^='data:']");
    (await art.count()) > 0 ? ok("арт карточки: data-URL (иконка или дефолт)") : fail("у карточки нет арта img[data:]");

    const badge = await first.locator(".chip-mono").first().textContent().catch(() => "");
    /^\d+\.\d+/.test(badge.trim()) ? ok(`бейдж версии на арте: «${badge.trim()}»`) : fail("нет бейджа версии");

    (await first.getByRole("button", { name: /^Играть —/ }).count()) === 1
      ? ok("Play-оверлей с прежним aria-label «Играть — …»")
      : fail("нет кнопки «Играть — …» в карточке");
    (await first.getByRole("button", { name: /Действия —/ }).count()) === 1
      ? ok("кебаб действий в карточке")
      : fail("нет кебаба в карточке");

    // Клик по телу карточки (мимо кнопок) открывает страницу инстанса.
    await first.locator("img").first().click({ position: { x: 10, y: 60 } });
    await page.waitForTimeout(500);
    (await page.getByRole("button", { name: /^Назад/ }).count()) === 1
      ? ok("клик по карточке открывает страницу инстанса")
      : fail("страница инстанса не открылась");
    await page.getByRole("button", { name: /^Назад/ }).click();
    await page.waitForTimeout(400);

    // Главная (D39): каталог Modrinth — больше не инстансы (те живут здесь).
    await page.getByRole("navigation").getByRole("button", { name: /^Главная/ }).click();
    await page.waitForTimeout(800);
    const catalogHead = await page.getByRole("heading", { name: "Каталог" }).count();
    catalogHead > 0 ? ok("главная: заголовок «Каталог»") : fail("на главной нет каталога");
    const typeTabs = await page
      .getByRole("tablist")
      .first()
      .getByRole("tab")
      .count();
    typeTabs >= 2 ? ok(`переключатель типа каталога (${typeTabs} таба)`) : fail("нет табов типа каталога");
  }

  // 2. Скины: голова персонажа в чипе титлбара + «Персонаж» в модалке аккаунтов.
  const chipHead = await page.evaluate(() => {
    // SkinHead — span с двумя вложенными span-слоями и background-image
    const el = document.querySelector("header span[style*='background-image']");
    return !!el;
  });
  chipHead ? ok("голова персонажа в чипе титлбара") : fail("в чипе титлбара нет головы скина");

  await page.getByRole("button", { name: /, сменить$/ }).click();
  await page.waitForTimeout(400);
  await page.getByRole("menuitem", { name: /Управление аккаунтами/ }).click();
  await page.waitForSelector("role=dialog");
  await page.waitForTimeout(800); // скины догружаются

  const listHeads = await page.locator("div[role='dialog'] span[style*='background-image']").count();
  listHeads > 0 ? ok(`аватары-головы в списке профилей (${listHeads})`) : fail("в списке профилей нет голов скинов");

  (await page.getByText("Персонаж").count()) >= 1
    ? ok("секция «Персонаж» на месте")
    : fail("нет секции «Персонаж»");
  const body = await page.locator("div[role='dialog'] canvas").count();
  body > 0 ? ok("тело персонажа (canvas-рендер)") : fail("нет canvas-тела персонажа");

  // Активный профиль оффлайн → честная подпись «Скин по умолчанию» и хинт.
  const defaultLabel = await page.getByText("Скин по умолчанию").count();
  const offlineHint = await page.getByText(/Подключите el\.by/).count();
  defaultLabel > 0 || offlineHint > 0
    ? ok("для офлайн-профиля — честный дефолт (без фейкового скина)")
    : fail("нет подписи дефолтного скина для офлайн-профиля");

  await page.getByRole("button", { name: "Закрыть" }).last().click();
  await page.waitForSelector("role=dialog", { state: "detached", timeout: 5000 });

  console.log(failed === 0 ? "DONE: cards_skins зелёная" : `DONE с ошибками: ${failed}`);
  await browser.close();
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((e) => {
  console.error("FAIL:", e.message);
  process.exit(1);
});
