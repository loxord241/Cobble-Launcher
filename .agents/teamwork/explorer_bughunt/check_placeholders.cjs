const fs = require('fs');
const path = require('path');

const en = JSON.parse(fs.readFileSync('./src/i18n/en.json', 'utf8'));

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
console.log('=== Checking calls to t() with object literal args ===');

for (const file of files) {
  const content = fs.readFileSync(file, 'utf8');
  // Match t("key", { ... }) even across newlines
  const regex = /\bt\(\s*["']([^"']+)["']\s*,\s*\{([^}]+)\}/g;
  let m;
  while ((m = regex.exec(content)) !== null) {
    const key = m[1];
    const objBody = m[2];
    const enText = en[key];
    if (enText) {
      const needed = (enText.match(/\{[a-zA-Z0-9_]+\}/g) || []).map(p => p.slice(1, -1));
      // Extract passed keys in object literal
      const passedKeys = [];
      const propRegex = /([a-zA-Z0-9_]+)\s*[:]/g;
      let pm;
      while ((pm = propRegex.exec(objBody)) !== null) {
        passedKeys.push(pm[1]);
      }
      // Also shorthand properties e.g. { name, count }
      for (const token of objBody.split(',').map(s => s.trim())) {
        if (/^[a-zA-Z0-9_]+$/.test(token)) {
          passedKeys.push(token);
        }
      }

      for (const n of needed) {
        if (!passedKeys.includes(n)) {
          const lineNum = content.slice(0, m.index).split('\n').length;
          console.log(`Potential missing placeholder "${n}" for key "${key}" at ${file}:${lineNum}`);
        }
      }
    }
  }
}
