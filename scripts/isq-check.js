// #212 question-vs-task check: pulls ASK_RE + the isQ rule from ui/index.html. Run: node scripts/isq-check.js
const s = require('fs').readFileSync(__dirname + '/../ui/index.html', 'utf8');
const ASK_RE = eval(s.match(/const ASK_RE = (\/.*\/i);/)[1]);
const isQ = t => !ASK_RE.test(t) && t.trim().split(/\s+/).length <= 30 && !/[.!?:]\s+\S/.test(t.trim()) && (/\?\s*$/.test(t) || /^\s*btw\b/i.test(t));
const task = [ // real asks that were lost as "questions"
  'These are not showing as a terminal under Clauracle + timestamp area, instead of the Working... pill like we planned. Is there a detection error when it should show like we planned?',
  'Also wasn\'t the "Terminal" supposed to show in this Working... pill\'s place instead? with just the $ until grep... line with the working styling?',
  'These buttons have the text and icons a bit misaligned. Fix it. Also make it possible to find/install new skills?',
  'Can you show the version here?', 'btw the sidebar is broken',
];
const q = ['What does the Then row do?', 'Why is the build so slow today?', 'How many tasks are left?', 'btw which model is this running on?'];
task.forEach(t => console.assert(!isQ(t), 'should be a task:', t));
q.forEach(t => console.assert(isQ(t), 'should be a question:', t));
console.log('isq-check done');
