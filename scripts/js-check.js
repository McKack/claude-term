// Syntax check of the page script without building: node scripts/js-check.js
// Takes the biggest <script> block in ui/index.html and runs `node --check` on it.
const fs = require('fs'), os = require('os'), path = require('path'), {spawnSync} = require('child_process');
const html = fs.readFileSync(path.join(__dirname, '../ui/index.html'), 'utf8');
const big = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)].map(m => m[1]).sort((a, b) => b.length - a.length)[0];
const f = path.join(os.tmpdir(), 'clauracle-page.js');
fs.writeFileSync(f, big);
const r = spawnSync(process.execPath, ['--check', f], {encoding: 'utf8'});
console.log(r.status ? r.stderr.split('\n').slice(0, 5).join('\n') : 'JS_OK');
process.exitCode = r.status;
