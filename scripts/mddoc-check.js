// #371 mdDoc check: block types render, no raw markdown left, code spans keep their text. Run: node scripts/mddoc-check.js
const fs = require('fs'), src = fs.readdirSync('ui/js').map(f => fs.readFileSync('ui/js/' + f, 'utf8')).join(String.fromCharCode(10));
const esc = s => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const cut = (a, b) => src.slice(src.indexOf(a), src.indexOf(b, src.indexOf(a)));
eval(cut('const HKW', 'function hiCode').replace('const HKW', 'var HKW') + cut('function hiCode', '// #338 colour toggle') + cut('function mdDoc', '// one task row'));
const h = mdDoc('# Title\n\nSome **bold** and *it* and `a*b*c` [x](https://e.com)\n\n- one\n  - two\n- [x] done\n\n1. a\n2. b\n\n> quote\n\n---\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n```js\nconst x = 1\n```\ntail');
const want = ['<h1>Title</h1>', '<b>bold</b>', '<i>it</i>', '<code>a*b*c</code>', 'data-url="https://e.com"', '<ul>', 'margin-left:18px', 'checked', '<ol>', '<blockquote>', '<hr>', '<th>A</th>', '<td>2</td>', 'class="code"', '<p>tail</p>'];
want.forEach(w => { if (!h.includes(w)) throw new Error('missing ' + w + '\n' + h); });
if (/\*\*|^#|\|---/m.test(h)) throw new Error('raw md left\n' + h);
console.log('mddoc ok');
