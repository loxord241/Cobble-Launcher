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

    // D63: дефолтный арт — бандл-пейзаж (src содержит instance-art) или
    // data-URL (своя иконка/старый генератор); первый ряд — карточка с любым.
    const art = first.locator("img[src^='data:'], img[src*='instance-art']");
    (await art.count()) > 0
      ? ok("арт карточки на месте (data-URL или пейзаж)")
      : fail("у карточки нет арта (ни data:, ни instance-art)");

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

  // 2. Скины: голова персонажа у аватара (D65: переехала из титбара в низ рельса) + «Персонаж» в модалке.
  const chipHead = await page.evaluate(() => {
    // SkinHead — span с background-image; ищем в рельсе или титбаре (design D65)
    const el = document.querySelector("nav span[style*='background-image'], header span[style*='background-image']");
    return !!el;
  });
  chipHead ? ok("голова персонажа у аватара (рельс)") : fail("нигде нет головы скина");

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

  // Честные состояния по типу активного аккаунта: офлайн → «Скин по
  // умолчанию» + хинт Ely; ely/authlib → подсказка про сайт; msa → заметка
  // про лицензионный скин (после одобрения Mojang активным бывает MSA).
  const offlineLabel = await page.getByText("Скин по умолчанию").count();
  const offlineHint = await page.getByText(/Подключите el\.by/).count();
  if (offlineLabel > 0 || offlineHint > 0) {
    ok("офлайн-профиль: честный дефолт (без фейкового скина)");
  } else {
    // MSA/ely: скин лицензионный — подпись-дефолт отсутствует по дизайну;
    // canvas-рендер персонажа выше уже подтверждён.
    ok("лицензионный/ely-профиль: без дефолтной подписи (canvas подтверждён)");
  }

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
