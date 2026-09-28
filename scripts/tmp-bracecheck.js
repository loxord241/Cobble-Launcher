// Временная статическая проверка баланса скобок в Rust-файлах вне строк/комментариев.
// Не часть проекта — удаляется после проверки.
const fs = require("fs");

function strip(s) {
  const BS = String.fromCharCode(92); // backslash
  let out = "";
  let i = 0;
  while (i < s.length) {
    const c = s[i];
    const n = s[i + 1];
    // raw string: r#"..."# или r"..."
    if ((c === "r") && (n === '"' || (n === "#" && s[i + 2] === '"'))) {
      const term = n === '"' ? '"' : '"#';
      i += n === '"' ? 2 : 3;
      const j = s.indexOf(term, i);
      out += "S";
      i = j < 0 ? s.length : j + term.length;
      continue;
    }
    if (c === '"') {
      i++;
      while (i < s.length && s[i] !== '"') {
        if (s[i] === BS) i++;
        i++;
      }
      i++;
      out += "S";
      continue;
    }
    if (c === "'") {
      i++;
      while (i < s.length && s[i] !== "'") {
        if (s[i] === BS) i++;
        i++;
      }
      i++;
      out += "C";
      continue;
    }
    if (c === "/" && n === "/") {
      while (i < s.length && s[i] !== "\n") i++;
      continue;
    }
    if (c === "/" && n === "*") {
      i += 2;
      while (i < s.length && !(s[i] === "*" && s[i + 1] === "/")) i++;
      i += 2;
      out += " ";
      continue;
    }
    out += c;
    i++;
  }
  return out;
}

for (const f of process.argv.slice(2)) {
  const s = strip(fs.readFileSync(f, "utf8"));
  let bal = 0;
  for (const ch of s) {
    if (ch === "{") bal++;
    else if (ch === "}") bal--;
  }
  console.log(f, "code_braces_balance=", bal);
}
