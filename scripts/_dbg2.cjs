const { chromium } = require("playwright-core");
(async () => {
  const b = await chromium.connectOverCDP("http://localhost:9223");
  const p = b.contexts()[0].pages()[0];
  const menu = await p.getByRole("menuitem").allTextContents();
  console.log("MENUITEMS:", JSON.stringify(menu));
  const dlg = await p.locator("[role='dialog'],[role='alertdialog']").count();
  console.log("DIALOGS:", dlg);
  const btns = await p.locator("button:visible").allTextContents();
  console.log("VISIBLE_BUTTONS:", JSON.stringify(btns.slice(0, 25)));
  const err = await p.locator("[role='alert']").allTextContents();
  console.log("ALERTS:", JSON.stringify(err));
})().catch((e) => { console.error("ERR", e.message); process.exit(1); });
