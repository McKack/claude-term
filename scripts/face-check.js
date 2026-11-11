// Checks the pixel faces in ui/js/boot.js: 1-pixel empty margin, body all round both eyes while they glance, whole-pixel scale.
const fs = require('fs'), src = fs.readFileSync(__dirname + '/../ui/js/boot.js', 'utf8');
const fn = src.slice(src.indexOf('function face('), src.indexOf('\n}\n', src.indexOf('function face(')) + 2);
let bad = 0;
for (const dpr of [1, 1.25, 1.5, 2]) {
  const face = new Function('devicePixelRatio', fn + 'return face;')(dpr);
  for (let i = 0; i < 3000; i++) {
    const svg = face('agent' + i + 'x' + dpr, [16, 32][i % 2]), cells = new Set();
    for (const [, x, y] of svg.matchAll(/M(\d+) (\d+)h1/g)) cells.add(x + ',' + y);
    const w = +svg.match(/width="([\d.]+)"/)[1];
    if (Math.abs(w * dpr / 20 - Math.round(w * dpr / 20)) > 1e-9) { bad++; console.log('scale', w, dpr); }
    for (const c of cells) { const [x, y] = c.split(',').map(Number); if (x < 1 || x > 14 || y < 1 || y > 14) { bad++; console.log('edge', i, c); break; } }
    for (const [, ex] of svg.matchAll(/<rect x="(\d+)" y="6" width="2"/g))
      for (let x = +ex - 2; x <= +ex + 3; x++) for (let y = 5; y <= 8; y++)
        if (!cells.has(x + ',' + y)) { bad++; console.log('eye', i, x, y); x = 99; break; }
  }
}
console.log(bad ? 'FACE_BAD ' + bad : 'FACE_OK');
