// Авто-догрузка: после открытия «Модов» счётчик обязан вырасти за пределы первой страницы.
const { chromium } = require("playwright-core");
async function main() {
  const browser = await chromium.connectOverCDP("http://localhost:9223");
  const page = browser.contexts()[0].pages()[0];
  await page.getByRole("button", { name: "Моды", exact: true }).click();
  await page.waitForSelector("text=Показано", { timeout: 30000 });
  await page.waitForFunction(
    () => {
      const m = document.body.innerText.match(/Показано (\d+) из/);
      return m && parseInt(m[1], 10) > 20;
    },
    { timeout: 30000 },
  );
  const after = await page.locator("text=Показано").first().textContent();
  console.log("после автодогрузки:", after.trim(), "— PASS");
}
main().then(() => process.exit(0), (e) => { console.error("FAIL:", e.message); process.exit(1); });
