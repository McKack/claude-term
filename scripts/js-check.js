// Syntax check of the page scripts without building: node scripts/js-check.js
// Runs `node --check` on every ui/js/*.js file (each must parse on its own: they load as separate classic scripts).
const fs = require('fs'), path = require('path'), {spawnSync} = require('child_process');
const dir = path.join(__dirname, '../ui/js');
let bad = 0;
for (const f of fs.readdirSync(dir).filter(f => f.endsWith('.js'))) {
  const r = spawnSync(process.execPath, ['--check', path.join(dir, f)], {encoding: 'utf8'});
  if (r.status) { bad = 1; console.log(r.stderr.split('\n').slice(0, 5).join('\n')); }
}
console.log(bad ? 'JS_FAIL' : 'JS_OK');
process.exitCode = bad;
