// Чистка тяжёлых артефактов дев-сессий (2026-10-09): после волн приёмок и
// локальных сборок cargo-кэш в src-tauri/target дорастал до 14+ ГБ, а кэш
// WebView2 дев-сборки (identifier dev.mclauncher.v2) — до сотен МБ. Релизы
// собирает CI, поэтому локальный target можно смело удалять целиком:
// следующая сборка пересоздаст его с нуля (~5–8 минут).
//
// Запуск: npm run clean:dev
// Данные установленного лаунчера и инстансы (%LOCALAPPDATA%\mc-launcher-v2)
// НЕ трогаются — только пересоздаваемые артефакты разработки.
const fs = require('fs');
const path = require('path');

const root = path.resolve(__dirname, '..');
const targets = [
  path.join(root, 'src-tauri', 'target'),
  path.join(process.env.LOCALAPPDATA || '', 'dev.mclauncher.v2', 'EBWebView'),
];

function dirSize(p) {
  let total = 0;
  let stack = [p];
  while (stack.length > 0) {
    const cur = stack.pop();
    let st;
    try {
      st = fs.lstatSync(cur);
    } catch {
      continue; // файл исчез/недоступен — не считаем
    }
    if (st.isDirectory()) {
      for (const name of fs.readdirSync(cur)) stack.push(path.join(cur, name));
    } else {
      total += st.size;
    }
  }
  return total;
}

let freed = 0;
for (const t of targets) {
  if (!t || !fs.existsSync(t)) {
    console.log('skip (нет): ' + t);
    continue;
  }
  const size = dirSize(t);
  try {
    fs.rmSync(t, { recursive: true, force: true, maxRetries: 3, retryDelay: 500 });
  } catch (err) {
    console.error('НЕ удалено (закрой dev-лаунчер и повтори): ' + t);
    console.error(String(err && err.message ? err.message : err));
    process.exitCode = 1;
    continue;
  }
  freed += size;
  console.log('удалено ' + (size / 1e9).toFixed(2) + ' GB: ' + t);
}
console.log('готово, освобождено ~' + (freed / 1e9).toFixed(2) + ' GB');
