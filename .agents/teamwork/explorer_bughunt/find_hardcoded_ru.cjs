const fs = require('fs');
const path = require('path');

function scanDir(dir) {
  let results = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      results = results.concat(scanDir(full));
    } else if (entry.isFile() && (entry.name.endsWith('.ts') || entry.name.endsWith('.tsx'))) {
      if (!full.includes('i18n')) {
        results.push(full);
      }
    }
  }
  return results;
}

const files = scanDir('./src');
const cyrillicRegex = /[\u0400-\u04FF]/;

for (const file of files) {
  const content = fs.readFileSync(file, 'utf8');
  // Strip block comments
  let stripped = content.replace(/\/\*[\s\S]*?\*\//g, (m) => ' '.repeat(m.length));
  // Strip line comments
  stripped = stripped.replace(/\/\/.*$/gm, (m) => ' '.repeat(m.length));

  const lines = stripped.split('\n');
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (cyrillicRegex.test(line)) {
      console.log(`${file}:${i + 1}: ${line.trim()}`);
    }
  }
}
