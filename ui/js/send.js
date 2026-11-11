const FILING = {}; // pid → messages between send() and their place in queue/session (saved in the draft)
const WSEEN = new Set(), sendChain = {}, QB = new WeakMap() /* queue item → its ⏱ bubble (not saved: lost on restart, pill stays) */, FILED_RE = /\(Filed as issue #(\d+) by Clauracle/, QNOTE_RE = /\(A (question|status update|remark), not a task \(Clauracle\)/;
// filing failed (gh or network down): the session files it itself so the ask is not lost
const UNFILED = '\n\n(Clauracle could not file this as an issue. If it is new work, file it yourself first: `gh issue create --label clauracle --label task`, neutral wording, then follow the task-queue rules.)';
const SNOTE = '\n\n(A status update, not a task (Clauracle): context for the task you are on or the one labelled `needs-user`: retry what was blocked; if it unblocks a needs-user issue, `gh issue edit N --remove-label needs-user` and resume it. File nothing.)';
// #98: the appended note is for the AI only → bubble/queue show the user's own text (+ a small #N chip)
// #100: notes = one line each → strip per note (triage batch keeps later bullets); triage preamble hidden too
const bare = t => t.replace(/^New from the user while a Continue waited\.[^\n]*\n\n/, '').replace(/\s*\((Filed as issue #\d+ by Clauracle|A (question|status update|remark), not a task \(Clauracle\)|Clauracle is filing this as an issue right now)[^\n]*/g, '');
const QNOTE = '\n\n(A question, not a task (Clauracle): not filed. Answer it; it is not a request for new work. If it does ask for a change, it is a task: file it (`gh issue create --label clauracle --label task`) and say so. If follow-up work makes sense, end with a fenced code block of language tasks, one "- Title: one-line summary" line per task; the user gets them as Accept/Dismiss cards.)';
/* user's rule: outstanding tasks first, a new ask goes to the END of the queue unless it says urgent (→ front of P.order, worked now).
   Ask into an empty queue = worked now too. now → send({work}) pins it in Rust (p.work) ∴ a handover/crash resumes it, not the queue head */
const URGENT_RE = /(^|[\s#])(urgent|asap|right now|immediately)\b/i;
function fileNow(pid, t, n) {
  const iq = issueQ(pid), urgent = URGENT_RE.test(t);
  if (urgent) { const order = [n, ...(P(pid).order || []).filter(x => x !== n)]; P(pid).order = order; call('update_project', {pid, changes: {order}}).catch(() => {}); }
  return urgent || !iq /* not loaded → old behaviour */ || !iq.some(x => x.number && x.number !== n); // in-progress work is in iq too
}
// #309 privacy: issue text = Sonnet's neutral rewrite (Rust task_do new); raw ask never goes to GitHub (edit history keeps it)
const NEUT = n => `Its title + summary are Clauracle's neutral rewrite of the ask: fix them (\`gh issue edit ${n}\`) only if wrong; never put the user's raw words on GitHub. The user's exact words and the notes for each image are kept locally in \`.git/clauracle-att/ask-${n}.md\` (never on GitHub): Read it first, it wins over the summary. Attached images: Read each one too, drawn markup on them = instructions; any body edit keeps the Screenshots lines.`; // #459 split/edit dropped them → markup asks lost
// #252: one ask, several separate tasks → split like other AIs (related asks stay together); one line ∵ notes are one line each
const SPLIT = n => ` Several separate asks in it (a wall of text) → split: keep the first (+ closely related ones) in #${n}, file each other separate one right after it (\`gh issue create --label clauracle --label task\`, own neutral summary, never the user's raw words, + its Screenshots lines), say which numbers in one line.`;
// one line each (bare() strips it for display; FILED_RE reads the number)
const filedNote = (n, now, urgent) => now
  ? `\n\n(Filed as issue #${n} by Clauracle${urgent ? ', marked urgent: it goes before the other open tasks' : ''}. Work on it now: ${NEUT(n)} Close it with a one-line summary when done.${SPLIT(n)} Not a task after all (a question, a status/unblock reply, already done) → answer it and \`gh issue close ${n} --reason "not planned"\`; an unblock reply → remove \`needs-user\` from the issue it unblocks and carry on with that.)`
  : `\n\n(Filed as issue #${n} by Clauracle at the END of the task queue: open tasks come first (the user's rule), so don't start it now. ${NEUT(n)} Say it's queued in one line, then carry on with the task you're on.${SPLIT(n)} It only adds to that task → fold it in and close #${n} as a duplicate. Not a task (a question, a status/unblock reply) → answer it and \`gh issue close ${n} --reason "not planned"\`; an unblock reply → remove \`needs-user\` from the issue it unblocks and carry on with that.)`;
// #336 remark = comment/feedback/thanks (Sonnet triage in task_do "ask"): acknowledged, never filed
const RNOTE = '\n\n(A remark, not a task (Clauracle): not filed, it reads as a reply or comment in the conversation. If it answers something you asked or agrees to something you proposed, carry on with that; otherwise reply briefly. If it asks for a change after all, file it (`gh issue create --label clauracle --label task`) and say so.)';
// → issue number, 'question'/'remark' when triage says it's no task (nothing filed), 0 on failure (then it's sent unfiled, with a toast; never blocks the prompt)
async function fileAsk(pid, t, a) {
  // #220: images → Rust saves them + lists their paths in the body (was names only → the session on the issue never saw them)
  const fs = a.filter(x => x.path), body = t + midTxt(pid, t) + noteTxt(a) + (fs.length ? `\n\nAttached files:\n${fs.map(x => '- ' + x.path).join('\n')}` : '');
  // #309 local draft: row in the task list at once (gray "local" dot) while Sonnet writes the neutral issue text, then gh files it
  const loc = {number: 0, local: true, title: t.split('\n')[0].slice(0, 90), url: '', labels: [{name: 'local (being filed)', color: '9aa0a6'}], assignees: [], t: Date.now()};
  (FRESH[pid] ||= []).push(loc); issueRedraw(pid);
  try { const [url = '', title, warn] = String(await call('task_do', {pid, action: 'ask', arg: body, images: a.filter(x => x.data).map(x => ({media_type: x.media_type, data: x.data, name: x.name, notes: x.notes || []}))})).trim().split('\n'); if (warn) toast(warn, 'warn');
    const cls = url.match(/^triage:(\w+)/)?.[1]; if (cls) { toast(`Not filed: Clauracle reads it as a ${cls}`, 'info'); return cls; }
    const n = +(url.match(/\/issues\/(\d+)/)?.[1] || 0);
    if (n) FRESH[pid].push({number: n, title: title || loc.title, url, labels: [{name: 'clauracle'}, {name: 'task', color: '888'}], assignees: [], t: Date.now()});
    pollTeam(); return n; }
  catch (e) { toast(`Couldn't file it as an issue: ${e}`, 'warn'); return 0; }
  finally { FRESH[pid] = FRESH[pid].filter(x => x !== loc); issueRedraw(pid); }
}
// a queued message being edited holds the queue (nothing auto-sends) until it's sent or the edit is cancelled
const editing = {}; // pid → {i: its place, m: the original, prev: what the composer held before}
function cancelEdit(hold) {
  const e = editing[cur]; if (!e) return;
  delete editing[cur]; queue[cur].splice(e.i, 0, e.m); qBub(cur, e.m);
  $('prompt').value = e.prev.text; atts[cur] = e.prev.atts;
  drawQueue(); drawAtts(); grow(); drawEditBar(); keepDraft(); if (!hold) flushQueue(cur);
}
const drawEditBar = () => { $('cancelEdit').hidden = !editing[cur]; };
let RESTARTING = false; // Claude asked for a restart: hold everything in the saved queue until the new app is up
function flushQueue(pid) {
  if (RESTARTING || busy[pid] || qWait(pid) /* #422 */ || editing[pid] && pid === cur || !queue[pid]?.length) return; // an edit holds the queue only while you're on that project (else the rest waited forever)
  if (contWaiting(pid)) { // a Continue is up: text messages become tasks, then the Continue runs; attachments stay queued
    const tasks = queue[pid].filter(m => !m.atts.length); queue[pid] = queue[pid].filter(m => m.atts.length); tasks.forEach(m => qDrop(pid, m)); // toTasks shows its own
    if (pid === cur) drawQueue(); keepDraft(pid, true);
    if (tasks.length) return toTasks(pid, tasks.map(m => m.text));
  }
  const m = queue[pid].shift(); qDrop(pid, m); sendNow(pid, m) /* fresh bubble at the end ∵ Clauracle sees it now */; if (pid === cur) drawQueue(); keepDraft(pid, true);
}
// Continue waiting = handover written (ready), or the idle bar offering the next task (current project only: it's drawn)
const contWaiting = pid => !busy[pid] && !RESTARTING && P(pid)?.cont !== 'off' && (P(pid)?.ready || (pid === cur && !$('ready').hidden && !$('contBtn').hidden));
// sending = confirming the Continue: Claude first files them into PLAN.md (same session, plan only; bar says Wait…),
// then on turn end the plan is re-read (Next updates) and the Continue runs (status busy:false → contGo)
const TRIAGE = 'New from the user while a Continue waited. Only update PLAN.md now: urgent or blocking → reword it as a task at the TOP of the Queue; otherwise reword it into the Queue by priority (merge with a matching task); a plain question → answer it briefly. Don\'t start the work and don\'t call mcp__clauracle__ready: Clauracle continues with the Queue\'s first task when this turn ends.\n\n';
// ponytail: new issues land at the Queue's end (order is local, P.order); user drags urgent ones up
const TRIAGE_ISSUES = 'New from the user while a Continue waited. Only file it now: one marked "Filed as issue #N" is filed already (fix its title/labels if needed, merge into a matching open issue and close the dupe); others → `gh issue create --label clauracle --label <task|feature|fix>` (or add it to a matching open issue with `gh issue comment`); a plain question → answer it briefly. Don\'t start the work and don\'t call mcp__clauracle__ready: Clauracle continues with the first open task when this turn ends.\n\n';
const triaging = {}; // pid → plan-only turn running; Continue follows it
/* ribbon: shows tucked (half under the composer); hover/click = pull up; tucks again on any click elsewhere, composer focus,
   4s off it or 8s on it without moving. Tucked = children pointer-events:none (CSS) → a click only reveals.
   --tk from live height ∵ text/wrap changes it; tucking also folds .full (a kept .full once blocked tucking for good) */
let readyT = 0, readyHold = false; const readyWas = {}; // pid → p.ready at last draw (#63 pop-up edge)
function readyTuck(r = $('ready')) { clearTimeout(readyT); readyHold = false; if ($('log').classList.contains('term')) return; /* #312 term = static bar, ^ toggles .full */ if (r.hidden) return r.classList.add('tuck'); /* height 0 → bad --tk */ $('readytext').classList.remove('full'); r.style.setProperty('--tk', `${17 - r.offsetHeight}px`) /* 25px showing: -(h - 25) - 8px gap; h same tucked or not ∴ same depth from any trigger */; r.classList.add('tuck'); }
const readyLater = ms => { clearTimeout(readyT); readyT = setTimeout(() => !$('ready').hidden && readyTuck(), ms); };
// #95: anything to expand? (nowrap ellipsis cut it, or already .full) → pointer + title; else click = no-op (no jump)
function readyMore(t) { const o = e => e && e.scrollWidth > e.clientWidth + 1, m = t.classList.contains('full') || o(t) || o(t.querySelector(':scope > b')); /* #388 flex: the <b> clips, not #readytext */ t.classList.toggle('more', m); t.title = m ? (t.classList.contains('full') ? 'Click to collapse' : 'Click to show the whole task') : ''; return m; }
function readyPeek() { // drawChrome: just shown
  const r = $('ready');
  if (!r.onmouseleave) { // wire once
    // #109: hover = slide up (no click); click while up = hold (20s timers) until it tucks
    r.onmouseenter = () => { if (r.classList.contains('tuck')) { r.classList.remove('tuck'); readyLater(8000); } };
    r.onmouseleave = () => !r.classList.contains('tuck') && readyLater(readyHold ? 20000 : 4000); r.onmousemove = () => !r.classList.contains('tuck') && readyLater(readyHold ? 20000 : 8000);
    r.onclick = () => { if (!r.classList.contains('tuck')) { readyHold = true; readyLater(20000); } else { r.classList.remove('tuck'); readyLater(8000); } };
    document.addEventListener('pointerdown', e => !r.contains(e.target) && !r.classList.contains('tuck') && readyTuck(r), true);
    $('composer').addEventListener('focusin', () => readyTuck(r));
  }
  readyTuck(r);
}
const contQ = {}; // pid → Continue clicked mid-turn; runs on turn end (status busy:false)
// #102: click → instant 'Continue: <task>' bubble, display only (never sent); idle-bar path skips it ∵ sendNow already shows its Carry-on bubble
// #145: cont:true → `cleared` doesn't fold it into the old log; queued click → bubble when it actually runs (status handler), else cont_ folds it above Fresh session
const qWait = pid => !!P(pid).questions?.length && P(pid).cont !== 'auto_answer'; // #220: questions open → runner paused
const contNote = (pid, t = nextUp(pid)[0]) => t && !qWait(pid) && addEntry(pid, {kind: 'user', text: 'Continue: ' + t, cont: true});
const contClick = pid => { if (busy[pid]) { contQ[pid] = true; return drawChrome(); } const r = P(pid).ready, t = nextUp(pid)[0]; contGo(pid); if (r) contNote(pid, t); };
function toTasks(pid, texts, shown) {
  triaging[pid] = true;
  call('send', {pid, text: (issueOn(pid) ? TRIAGE_ISSUES : TRIAGE) /* issueOn ∵ issueQ = null until team loads → wrongly PLAN.md */ + texts.map(t => '- ' + t.trim().replace(/\n/g, '\n  ')).join('\n'), images: [], files: []})
    .catch(e => { triaging[pid] = false; texts.forEach(t => (queue[pid] ||= []).push({text: t, atts: [], work: 0})); keepDraft(pid, true); if (pid === cur) drawQueue(); toast(`Message kept in the queue (send hit an error: ${e})`, 'warn'); }); // a failed send used to drop them
  if (!shown) texts.forEach(t => addEntry(pid, {kind: 'user', text: t}));
  if (pid === cur) drawChrome();
}
// ready → clear + continue; idle bar → next task in this session
// #220: open questions pause the runner (Rust continue_now refuses too); auto_answer holds them by design
const contGo = pid => qWait(pid) ? (pid === cur && showQuestions(pid), toast('Answer the question first: the next task waits for it', 'info'))
  : P(pid).ready ? (pid === cur ? cont_() : call('continue_now', {pid, extra: null}))
  : nextUp(pid).length && sendNow(pid, {work: issueQ(pid)?.[0]?.number, text: (issueQ(pid) ? 'Carry on with the next task (GitHub issue): ' : S.queues?.[pid]?.length ?'Carry on with the next task in PLAN.md\'s Queue: ' : 'Carry on with the next task: ') + nextUp(pid)[0], atts: []});
// #101 ticks: e.st pend ⏱ → sent ✓ (in the session's inbox) → read blue ✓✓ (its turn started) | fail !
const UNREAD = {}, USEREL = new WeakMap(); // pid → entries awaiting ✓✓; entry → its bubble (WeakMap ∵ logs stay plain data)
const fnames = t => (t.match(/\n📎 (.+)$/)?.[1] || '').split(', ').filter(Boolean); // utext's file list
const utext = (t, a) => { const f = a.filter(x => x.path); return t + (f.length ? `\n📎 ${f.map(x => x.name).join(', ')}` : ''); };
const userEntry = (pid, t, a, e = {}) => { addEntry(pid, Object.assign(e, {kind: 'user', st: e.st || 'pend', text: utext(t, a), images: a.filter(x => x.data).map(x => ({url: x.url, name: x.name})), texts: a.filter(x => x.text).map(x => ({name: x.name, text: x.text}))}));
  e.log = !!logs[pid]?.includes(e); return e; }; // log false = a handover turn hid it
const unEntry = (pid, e) => { logs[pid] = (logs[pid] || []).filter(x => x !== e); saveLog(pid); (USEREL.get(e)?.closest('.crow') || USEREL.get(e))?.remove(); e.log = false; };
const qBub = (pid, m) => { const e = userEntry(pid, m.text, m.atts, {st: 'queue'}); if (e.log) QB.set(m, e); };
const qDrop = (pid, m) => { const e = m && QB.get(m); if (e) { unEntry(pid, e); QB.delete(m); } };
// filed note → #N chip: re-render the bubble with the final text
// task tags: filed by Clauracle (note in the text) or later by the session for this message (e.tasks)
const filedTags = e => [...new Set([...[...e.text.matchAll(new RegExp(FILED_RE, 'g'))].map(m => +m[1]), ...e.tasks || []])].map(n => `<span class="filedtag" data-tip="Task #${n}">#${n}</span>`).join('');
function reBub(e, t, a) { if (t != null) e.text = utext(t, a); const old = USEREL.get(e)?.closest('.crow'), n = old && entry(e); if (n) { n.className = old.className; old.replaceWith(n); } }
const TK = {queue: [icon('clock'), 'Queued: sends when this turn ends'], pend: [icon('clock'), 'Sending…'], sent: [icon('check'), 'Sent'], read: [icon('check2'), 'Received by Clauracle'], fail: [icon('warn'), 'Not sent']};
const tickHtml = e => `<span class="tk ${e.st}" data-tip="${TK[e.st][1]}">${TK[e.st][0]}</span>`;
function tick(e, st) { e.st = st; const k = USEREL.get(e)?.closest('.crow')?.querySelector('.cm .tk'); if (k) k.outerHTML = tickHtml(e); }
const readAll = pid => { (UNREAD[pid] || []).forEach(e => e.st !== 'fail' && tick(e, 'read')); UNREAD[pid] = []; if (INBOX[pid]?.length) { INBOX[pid] = []; keepDraft(pid, true); } };
/* sent but not yet read (busy:true) = only in Rust's in-memory inbox (session connecting/retrying) → kept in the saved draft till then ∴ a crash/quit re-queues it */
const INBOX = {};
function sendNow(pid, {text: t, atts: a, work}, e) {
  const images = a.filter(x => x.data), files = a.filter(x => x.path).map(x => x.path);
  const nt = midTxt(pid, t) + noteTxt(a); // #418 reply refs quoted
  const sc = /^\s*\/([\w:-]+)/.exec(t); if (sc) LASTCMD[pid] = sc[1]; else delete LASTCMD[pid];
  if (e) { reBub(e, t, a); if (e.st !== 'pend') tick(e, 'pend'); }
  else e = userEntry(pid, t, a);
  (UNREAD[pid] ||= []).push(e);
  if (work) P(pid).work = work;
  const sm = {text: t, atts: a, work}; (INBOX[pid] ||= []).push(sm); keepDraft(pid, true);
  call('send', {pid, text: t + nt, images: images.map(x => ({media_type: x.media_type, data: x.data, name: x.name})), files, work: work || null})
    .then(() => e.st === 'pend' && tick(e, 'sent'), err => { tick(e, 'fail'); INBOX[pid] = INBOX[pid].filter(x => x !== sm); (queue[pid] ||= []).push(sm); if (pid === cur) drawQueue(); keepDraft(pid, true); toast(`Message not sent (kept in the queue): ${err}`, 'err'); });
}
// one row of pills (first words; click = edit), ‹ › scroll it when it overflows
function drawQueue() {
  const q = queue[cur] || [], words = m => (bare(m.text).trim().split(/\s+/).slice(0, 5).join(' ') || m.atts.map(x => x.name).join(', ')) + (bare(m.text).trim().split(/\s+/).length > 5 ? '…' : '');
  $('queue').hidden = !q.length;
  $('qrow').innerHTML = q.map((m, i) => `<span class="qp" title="${esc(bare(m.text))}" onclick="editQueued(${i})"><span class="qi">${icon('clock')}</span>${m.atts?.length ? `<span class="qa" data-tip="${esc(m.atts.map(x => x.name).join(', '))}">${icon('file')}${m.atts.length > 1 ? m.atts.length : ''}</span>` : ''}<span class="qt">${esc(words(m))}</span>
    <button class="ib" title="Edit (↑ in an empty box)" onclick="event.stopPropagation(); editQueued(${i})">${icon('pen')}</button>
    <button class="ib" title="Send next: first in line when this turn ends" onclick="event.stopPropagation(); queue[cur].unshift(...queue[cur].splice(${i},1)); drawQueue(); keepDraft(cur, true); flushQueue(cur)">${icon('up')}</button>
    <button class="ib" title="Remove" onclick="event.stopPropagation(); qDrop(cur, queue[cur].splice(${i},1)[0]); drawQueue(); keepDraft(cur, true)">${icon('trash')}</button></span>`).join('');
  requestAnimationFrame(() => { const r = $('qrow'); $('queue').classList.toggle('over', r.scrollWidth > r.clientWidth + 1); });
}
const qscroll = d => $('qrow').scrollBy({left: d * $('qrow').clientWidth * .8, behavior: 'smooth'});
// into the composer; what was there comes back after send/cancel
function editQueued(i) {
  const want = queue[cur][i]; if (!want) return;
  if (editing[cur]) { cancelEdit(true); i = queue[cur].indexOf(want); } // index moved: the cancelled one went back in (+ a flush used to send the clicked one)
  const [m] = queue[cur].splice(i, 1), p = $('prompt'); qDrop(cur, m);
  editing[cur] = {i, m, prev: {text: p.value, atts: atts[cur] || []}};
  p.value = m.text; atts[cur] = [...m.atts];
  drawQueue(); drawAtts(); grow(); drawEditBar(); keepDraft(); p.focus();
}
/* Reply of a CLI /command: the options it names become buttons that send "/cmd option". Read from the shape of the text, never
   from the command's name, so a new command that answers in a known shape works at once. Shapes: "Usage: /x <a|b|c>",
   "Available: a, b, c", lines "key=a|b" (settings: "/x key=a"), and a bullet list under "Available …:" with "(current)". */
const LASTCMD = {}; // pid → name of the /command last sent: its reply is the next CLI-written message
function cmdOpts(name, t) { const o = [], rows = [], add = (v, cur) => { if (v && !o.some(x => x.v === v)) o.push({v, cur}); }; let m;
  if ((m = /Usage: `?\/\S+ <([^<>\n]*\|[^<>\n]*)>/.exec(t))) m[1].replace(/\[[^\]]*\]/g, '').split('|').forEach(v => add(v.trim().split(/\s/)[0]));
  if ((m = /Available: ([^\n]+)/.exec(t))) m[1].split(/,\s*/).forEach(v => { v = v.replace(/^or /, '').replace(/\.$/, '').trim(); if (/^[\w.[\]:-]+$/.test(v)) add(v); });
  if (/^Available [\w ]+:\s*$/m.test(t)) for (m of t.matchAll(/^- (\w[\w -]*?)( \(current\))?(?::|\s*$)/gm)) add(m[1], !!m[2]);
  for (m of t.matchAll(/^\s*([\w.]+)=([\w-]+(?:\|[\w-]+)+)\s*$/gm)) rows.push([m[1], m[2].split('|')]);
  const b = (label, arg, cur) => `<button class="btn sm${cur ? ' on' : ''}" data-cmd="/${esc(name)} ${esc(arg)}" onclick="cmdRun(this.dataset.cmd)">${esc(label)}</button>`;
  return (o.length ? `<div class="cmdo">${o.map(x => b(x.v, x.v, x.cur)).join('')}</div>` : '')
    + (rows.length ? `<div class="cmdo rows">${rows.map(([k, vs]) => `<span>${esc(k)}</span><span>${vs.map(v => b(v, k + '=' + v)).join('')}</span>`).join('')}</div>` : ''); }
function cmdRun(text) { if (!cur) return; if (busy[cur] || RESTARTING) return toast('Wait for the current reply, then pick again', 'info'); sendNow(cur, {text, atts: []}); }
// /commands: list from the session's initialize answer; typing "/x" filters, ↑↓ moves, Tab/Enter fills in, Esc hides
const cmds = {}; let slashSel = 0;
const slashMatches = () => { const m = /^\/(\S*)$/.exec($('prompt').value);
  return m && cur ? (cmds[cur] || []).filter(c => c.name.toLowerCase().startsWith(m[1].toLowerCase())).slice(0, 8) : []; };
// #270 "@x" before the caret → project files (HEAT list = git ls-files, shared w/ the Hot tab cache); name-prefix hits first, then shorter paths
let atLoading = 0;
const atMatches = () => { const p = $('prompt'), q = /(?:^|\s)@([^\s@[\]]*)$/.exec(p.value.slice(0, p.selectionStart))?.[1]?.toLowerCase(); if (q == null || !cur) return [];
  const fs = HEAT[cur]?.d.files, pid = cur; if (!fs) { if (!atLoading) atLoading = call('file_heat', {pid}).then(d => { HEAT[pid] ||= {t: Date.now(), d}; atLoading = 0; drawSlash(); }, () => atLoading = 0); return []; }
  const r = f => (f.split('/').pop().toLowerCase().startsWith(q) ? 0 : 1e4) + f.length;
  return fs.map(f => f[0]).filter(f => f.toLowerCase().includes(q)).sort((a, b) => r(a) - r(b)).slice(0, 8); };
// #280 "/model [q]" → the CLI's models (mList); "/model <value> [q]" → that model's effort levels. Items {opt, label, sub, check, run}.
// ponytail: only /model is mapped; another option-picking command = one more branch fed by its initialize data (e.g. available_output_styles)
const modelSet = o => { setP(o); $('prompt').value = ''; slashSel = 0; drawSlash(); grow(); keepDraft(); };
const modelMatches = () => { const m = /^\/model(?:\s+(\S*))?(?:\s+(\S*))?$/.exec($('prompt').value); if (!m || !cur) return [];
  const ls = mList(), sel = m[2] != null && ls.find(x => x[0] === m[1] || (!x[0] && m[1] === 'default'));
  if (sel) { const q = m[2].toLowerCase(); return effOf(sel[0]).filter(e => (e + ' ' + (EFFORTS[e] || '')).toLowerCase().includes(q))
    .map(e => ({opt: 1, label: EFFORTS[e] || e, sub: `Effort for ${sel[1]}`, check: (P().model || '') === sel[0] && (P().effort || '') === e, run: () => modelSet({model: sel[0], effort: e})})); }
  const q = (m[1] || '').toLowerCase();
  return ls.filter(x => (x[0] + ' ' + x[1]).toLowerCase().includes(q)).map(x => ({opt: 1, label: x[1], sub: x[2] || x[0], check: (P().model || '') === x[0],
    run: () => { if (!x[3]) return modelSet({model: x[0]}); $('prompt').value = `/model ${x[0] || 'default'} `; slashSel = Math.max(0, effOf(x[0]).indexOf(P().effort || '')); drawSlash(); grow(); }})); };
// #345 "/build [q]" → same picker: local copy (this OS + run) | every target | one target (picked targets, else all; HOST first)
const buildMatches = () => { const m = /^\/build(?:\s+(\S*))?$/i.exec($('prompt').value); if (!m || !cur) return []; const q = (m[1] || '').toLowerCase();
  const go = tg => () => { $('prompt').value = ''; slashSel = 0; drawSlash(); grow(); keepDraft(); buildLocal(cur, tg); }, pt = P().targets || [];
  const ts = TGT.flatMap(g => g[2]).filter(x => !pt.length || pt.includes(x[0])).sort((a, b) => (b[0] === HOST) - (a[0] === HOST));
  return [{opt: 1, k: 'local', label: 'Local copy', sub: `Build for ${tgtName(HOST)} and run it`, run: go('')}, {opt: 1, k: 'all', label: 'All targets', sub: 'Every target, no run', run: go('all')},
    ...ts.map(x => ({opt: 1, k: x[0], label: x[2], sub: `${x[3]}, no run`, run: go(x[0])}))].filter(o => (o.k + ' ' + o.label).toLowerCase().includes(q)); };
const menuXs = () => { const v = $('prompt').value; let o = modelMatches(); if (o.length || /^\/model\s/.test(v)) return o; o = buildMatches(); if (o.length || /^\/build\s/i.test(v)) return o; const s = slashMatches(); return s.length ? s : atMatches(); }; // commands = objects, files = strings, options = {opt}
function drawSlash() {
  const xs = menuXs(), el = $('slash');
  slashSel = Math.min(slashSel, Math.max(xs.length - 1, 0));
  el.hidden = !xs.length;
  el.innerHTML = xs.map((c, i) => `<button class="${i === slashSel ? 'on' : ''}" onmousedown="event.preventDefault(); pickSlash(${i})">${c.opt ? `<b>${esc(c.label)}</b>${c.check ? icon('check') : ''}<small>${esc(c.sub)}</small>` : typeof c === 'string' ? `${icon(fIcon(c))}<b>${esc(c.split('/').pop())}</b><small>${esc(c)}</small>` : `<b>/${esc(c.name)}</b>
    ${c.argumentHint ? `<span class="faint">${esc(c.argumentHint)}</span>` : ''}<small>${esc(c.description || '')}</small>`}</button>`).join('');
  el.querySelector('.on')?.scrollIntoView({block: 'nearest'}); // #280 /model lists > 8 rows: ↑↓ keeps the pick visible
}
function pickSlash(i) { const c = menuXs()[i]; if (c?.opt) return c.run(); if (typeof c === 'string') return pickAt(c); if (c) { $('prompt').value = '/' + c.name + ' '; slashSel = 0; drawSlash(); grow(); } }
// #270 picked file → path attachment (sent as "Attached files", like a dropped one) + its @[name] tag in place of "@query"
function pickAt(rel) { const p = $('prompt'), c = p.selectionStart, s = p.value.lastIndexOf('@', c - 1), root = P().path, sep = root.includes('\\') ? '\\' : '/';
  const abs = root.replace(/[\\/]+$/, '') + sep + rel.split('/').join(sep), a = (atts[cur] ||= []); let i = a.findIndex(x => x.path === abs);
  if (i < 0) i = a.push({name: attName(rel.split('/').pop()), path: abs}) - 1;
  p.setSelectionRange(s, c); attRef(i); slashSel = 0; drawAtts(); drawSlash(); }
$('prompt').addEventListener('keydown', e => {
  const xs = $('slash').hidden ? [] : menuXs();
  if (xs.length) {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); slashSel = (slashSel + (e.key === 'ArrowDown' ? 1 : xs.length - 1)) % xs.length; return drawSlash(); }
    if (e.key === 'Escape') return void ($('slash').hidden = true);
    // Enter on a fully typed name sends it; otherwise it fills in the highlighted one
    if (e.key === 'Tab' || (e.key === 'Enter' && !e.shiftKey && $('prompt').value !== '/' + xs[slashSel].name)) { e.preventDefault(); return pickSlash(slashSel); }
  }
  // #289 pill = one char: Backspace after / Delete before a live @[name] → select it, native delete removes it (undo + input event intact); attachment stays
  if ((e.key === 'Backspace' || e.key === 'Delete') && !e.ctrlKey && !e.altKey) { const p = $('prompt'), c = p.selectionStart;
    if (c === p.selectionEnd) { const m = e.key === 'Backspace' ? /@\[([^\]\n]+)\]$/.exec(p.value.slice(0, c)) : /^@\[([^\]\n]+)\]/.exec(p.value.slice(c));
      if (m && attAt(m[1]) >= 0) e.key === 'Backspace' ? p.setSelectionRange(c - m[0].length, c) : p.setSelectionRange(c, c + m[0].length); } }
  if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); $('slash').hidden = true; } // #206 value cleared w/o an input event → popup lingered
  if (e.key === 'Tab' && cur) { e.preventDefault(); const k = Object.keys(PERMS), i = k.indexOf(P().mode); setP({mode: k[(i + (e.shiftKey ? k.length - 1 : 1)) % k.length]}); } // #116 Tab/Shift+Tab cycles permission mode (like Claude Code)
  if (e.key === 'ArrowUp' && !hasInput() && queue[cur]?.length) { e.preventDefault(); editQueued(queue[cur].length - 1); }
});
// #249 measure w/o scrollbar (it narrowed the lines → over-tall + a stray 10px bar that later vanished → mirror wrapped wider/narrower → caret drift); bar only past max-height
let PH = 0; // #378 dragged prompt height (0 = auto-grow to content, CSS max 260)
const grow = () => { const t = $('prompt'), st = t.scrollTop; t.style.overflowY = 'hidden'; t.style.height = 'auto'; t.style.maxHeight = PH ? 'none' : ''; const h = t.scrollHeight; t.style.height = (PH || h) + 'px'; t.style.overflowY = h > (PH || parseFloat(getComputedStyle(t).maxHeight)) ? 'auto' : 'hidden'; t.scrollTop = st; $('composer').style.setProperty('--th', PH ? Math.max(28, Math.min(56, Math.round(PH / 2))) + 'px' : ''); pbDraw(); }; // #419 dragged-small composer (PH < 112) → thumbs PH/2 (28..56) ∴ text keeps room. #258 height:auto reset scrollTop to 0 → restore the spot
// #326 arrow over scrollbars: hit = past clientWidth/Height (padding box minus bar); target = the scroller itself there
let sbEl = null; document.addEventListener('mousemove', e => { const t = e.target, on = t instanceof Element && ((t.offsetWidth - t.clientWidth > 4 && e.offsetX >= t.clientWidth) || (t.offsetHeight - t.clientHeight > 4 && e.offsetY >= t.clientHeight));
  if (sbEl && (sbEl !== t || !on)) { sbEl.classList.remove('onsb'); sbEl = null; } if (on && !sbEl) (sbEl = t).classList.add('onsb'); }, { passive: true });
// #378 drag the composer's top edge: cap = 75% of #main − room for the ribbon (its pull-up never pushes the composer) − composer chrome; under 40px → auto (empty size); term view snaps to the 20px grid
$('cgrip').onpointerdown = e => { const t = $('prompt'), y0 = e.clientY, h0 = t.offsetHeight;
  dragOn(e, m => { const r = $('ready'), cap = $('main').clientHeight * .75 - (r.hidden ? 0 : r.scrollHeight) - ($('composer').offsetHeight - t.offsetHeight);
    let h = Math.min(cap, h0 + y0 - m.clientY); if (view !== 'chat') h = Math.round(h / LH) * LH; PH = Math.max(h, parseFloat(getComputedStyle(t).minHeight) || 20); grow(); }); }; // #405 floor = empty composer height (text scrolls), never auto-fit mid-drag
$('cgrip').ondblclick = () => { PH = 0; grow(); }; // back to auto-fit
new ResizeObserver(() => pbDraw()).observe($('prompt')); // width/scrollbar changes w/o typing (window, side panels) → mirror re-measured
let chromeRAF = 0; // typing: one chrome redraw per frame, not per keystroke
// #325 term view: refs = the attachments (no chips) ∴ deleting its last @[name] drops the attachment; chat keeps it (chip stays)
// only refs present before this edit count (_pv) ∴ an attachment added in Chat view w/o a ref never vanishes on typing
const attSync = () => { const p = $('prompt'), v = p.value, pv = p._pv || '', a = atts[cur] || [], has = (t, x) => new RegExp(`@\\[ ?${reEsc(stem(x.name))}\\]`).test(t), k = a.filter(x => has(v, x) || !has(pv, x)); p._pv = v; if (k.length < a.length) { // term view only (caller): thumbs are hidden, so a ref that vanished = removed; Undo puts it back
    const pid = cur, gone = a.map((x, i) => [x, i]).filter(([x]) => !k.includes(x)); atts[cur] = k; drawAtts();
    gone.forEach(([x, i]) => toast('Removed ' + x.name, 'info', ['Undo', () => { if (cur !== pid) return; const l = atts[cur] ||= []; l.splice(Math.min(i, l.length), 0, x); attRef(l.indexOf(x)); drawAtts(); }])); } };
$('prompt').addEventListener('focus', e => e.target._pv = e.target.value);
$('prompt').addEventListener('input', () => { if (cur && view !== 'chat') attSync(); grow(); slashSel = 0; drawSlash(); if (cur) { if (!chromeRAF) chromeRAF = requestAnimationFrame(() => { chromeRAF = 0; drawChrome(); }); keepDraft(); } });
// Continue/auto-clear → "Reading handover…" pill (same spot as Working…) until Claude's first own line/tool or turn end
const reading = {};
const readNote = (pid, on) => { if (on) reading[pid] = true; else delete reading[pid]; if (pid === cur) drawChrome(); };
// a clear keeps the conversation so far above the new one, faded (ponytail: last 300 entries, older ones drop)
const keepOld = pid => (logs[pid] || []).filter(x => x.kind !== 'ask').slice(-300).map(x => ({...x, old: true}));
const freshLog = pid => { logs[pid] = [...keepOld(pid), {kind: 'note', text: 'Fresh session', ts: new Date().toISOString()}, ...sugPend(logs[pid] || [])]; saveLog(pid); };
function cont_(extra) { call('continue_now', {pid: cur, extra: extra || null}); /* first: a drawing error can't swallow the Continue */
  if (!extra && !nextUp(cur).length) return; // #337: nothing queued → Rust only notes "nothing to continue": no clear, old log stays live
  hiddenQs[cur] = false; qsOpen[cur] = true; P().ready = false; $('ready').hidden = true;
  freshLog(cur); readNote(cur, true); drawLog(); }

