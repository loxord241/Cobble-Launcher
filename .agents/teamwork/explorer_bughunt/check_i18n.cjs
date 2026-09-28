const fs = require('fs');
const path = require('path');

const en = JSON.parse(fs.readFileSync('./src/i18n/en.json', 'utf8'));
const ru = JSON.parse(fs.readFileSync('./src/i18n/ru.json', 'utf8'));
const uk = JSON.parse(fs.readFileSync('./src/i18n/uk.json', 'utf8'));
const errorsJson = JSON.parse(fs.readFileSync('./src/i18n/errors.json', 'utf8'));

function scanDir(dir) {
  let results = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      results = results.concat(scanDir(full));
    } else if (entry.isFile() && (entry.name.endsWith('.ts') || entry.name.endsWith('.tsx'))) {
      results.push(full);
    }
  }
  return results;
}

const files = scanDir('./src');
const missingT = [];
const missingPlural = [];
const dynamicT = [];

for (const file of files) {
  const content = fs.readFileSync(file, 'utf8');
  // match t("...") or t('...')
  const tMatches = content.matchAll(/\bt\(\s*(["'`])([^"'`]+)\1/g);
  for (const m of tMatches) {
    const key = m[2];
    if (key.includes('${')) {
      dynamicT.push({ file, key });
      continue;
    }
    if (!en[key]) {
      missingT.push({ file, key });
    }
  }

  // match plural("...") or plural('...')
  const plMatches = content.matchAll(/\bplural\(\s*(["'`])([^"'`]+)\1/g);
  for (const m of plMatches) {
    const key = m[2];
    if (!en[`${key}.one`] && !en[key]) {
      missingPlural.push({ file, key });
    }
  }
}

console.log('=== Missing t() keys ===');
console.log(missingT);

console.log('=== Missing plural() keys ===');
console.log(missingPlural);

console.log('=== Dynamic t() keys ===');
console.log(dynamicT);
