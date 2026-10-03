// D46: собрать latest.json для updater-плагина из подписанных артефактов
// релиза. Запуск ПОСЛЕ `scripts/build.ps1 -Bundle` (нужны .sig рядом с
// установщиками) и ПОСЛЕ известного номера версии/заметок.
//
//   node scripts/make_latest_json.cjs 0.2.1 "Заметки релиза"
//
// Выход: dist-latest/latest.json — залить ассетом в GitHub-релиз (вместе с
// установщиками и их .sig). Схема: plugins.updater.endpoints в
// tauri.conf.json указывает на releases/latest/download/latest.json.
/* eslint-disable */
const fs = require("fs");
const path = require("path");

const [, , versionArg, notesArg] = process.argv;
if (!versionArg) {
  console.error("usage: node scripts/make_latest_json.cjs <version> [notes]");
  process.exit(1);
}
const version = versionArg.replace(/^v/, "");
const tag = `v${version}`;
const bundle = path.join("src-tauri", "target", "release", "bundle");

// Ищем установщик NSIS с его подписью (updater на Windows ставит nsis/msi;
// в манифест отдаём NSIS — он умеет перезапуск поверх установленного).
const candidates = [];
for (const kind of ["nsis", "msi"]) {
  const dir = path.join(bundle, kind);
  if (!fs.existsSync(dir)) continue;
  for (const f of fs.readdirSync(dir)) {
    if ((f.endsWith(".exe") || f.endsWith(".msi")) && f.includes(version)) {
      candidates.push({ kind, dir, file: f });
    }
  }
}
const pick =
  candidates.find((c) => c.kind === "nsis" && c.file.endsWith("-setup.exe")) ||
  candidates.find((c) => c.kind === "nsis") ||
  candidates[0];
if (!pick) {
  console.error("no installer found in", bundle, "— собери бандл: scripts/build.ps1 -Bundle");
  process.exit(2);
}
const sigFile = path.join(pick.dir, pick.file + ".sig");
if (!fs.existsSync(sigFile)) {
  console.error("no signature next to installer:", sigFile, "— собери с ключом (build.ps1 -Bundle)");
  process.exit(3);
}
const signature = fs.readFileSync(sigFile, "utf8").trim();

// Имя ассета в GitHub-релизе: GitHub заменяет пробелы точками ("Cobble.Launcher_…"),
// поэтому URL строим от dotted-имени — иначе updater получает 404 (поймано на 0.2.1).
const assetName = pick.file.replace(/ /g, ".");
const url = `https://github.com/loxord241/Cobble-Launcher/releases/download/${tag}/${encodeURIComponent(
  assetName,
)}`;

const manifest = {
  version,
  notes: notesArg || `${tag}`,
  pub_date: new Date().toISOString().replace(/\.\d+Z$/, "Z"),
  platforms: {
    "windows-x86_64": { signature, url },
  },
};

const outDir = "dist-latest";
fs.mkdirSync(outDir, { recursive: true });
const out = path.join(outDir, "latest.json");
fs.writeFileSync(out, JSON.stringify(manifest, null, 2) + "\n");
console.log("OK:", out);
console.log(JSON.stringify(manifest, null, 2));
