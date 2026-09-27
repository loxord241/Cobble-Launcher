const { chromium } = require("playwright-core");
async function main() {
  const b = await chromium.connectOverCDP("http://localhost:9223");
  const p = b.contexts()[0].pages()[0];
  await p.getByRole("button", { name: "Главная", exact: true }).click();
  await p.waitForTimeout(4000); // дать модпакам загрузиться
  await p.screenshot({ path: "docs/kill-acceptance/new-home.png" });
  console.log("shot ok");
}
main().then(() => process.exit(0), (e) => { console.error(e.message); process.exit(1); });
