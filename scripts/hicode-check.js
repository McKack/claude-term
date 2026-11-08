// #248 hiCode check: colouring never changes the text (strip tags = escaped source). Run: node scripts/hicode-check.js
const fs = require('fs'), src = fs.readFileSync('ui/index.html', 'utf8');
const a = src.indexOf('const HKW'), b = src.indexOf('const codeHtml');
const esc = s => s.replace(/&/g, '&amp;').replace(/</g, '&lt;');
eval(src.slice(a, b).replace('const HKW', 'var HKW'));
const t0 = Date.now(), out = hiCode(src, 'html'); console.log('html lines', out.length, Date.now() - t0, 'ms');
const strip = h => h.replace(/<i class="\w*">|<\/i>/g, '');
const lines = src.split('\n'); if (!(out.every((h, k) => strip(h) === esc(lines[k])))) throw new Error('roundtrip html');
const rs = fs.readFileSync('src-tauri/src/lib.rs', 'utf8'), ro = hiCode(rs, 'rs'); if (!(ro.every((h, k) => strip(h) === esc(rs.split('\n')[k])))) throw new Error('roundtrip rs');
console.log(hiCode('let x = "a" // hi\n/* a\nb */ fn 42', 'rs').join('\n'));
