// ---- update check: Clauracle's latest GitHub release vs this build; a toast offers it ----
async function checkUpdate(asked) { // asked = Settings › Check now: says so when there's nothing, ignores Later
  let u; try { u = await call('update_check'); } catch (e) { return asked && toast(`Update check failed: ${e}`, 'err'); }
  if (!u.newer) return asked && toast(`Clauracle ${u.current} is the latest`, 'ok');
  if (!asked && sessionStorage.skipUpdate === u.latest) return;
  const t = Object.assign(document.createElement('div'), {className: 'toast glass', popover: 'manual'});
  t.innerHTML = `<div class="row"><svg class="i ok"><use href="#i-sparkle"/></svg><b class="grow">Clauracle ${esc(u.latest)} is available</b></div>
    <span class="dim">You have ${esc(u.current)}. ${/Windows/.test(navigator.userAgent) ? 'Update downloads the installer and restarts; sessions and unsent input come back.' : 'Opens the release page.'}</span>
    <div class="row" style="justify-content:flex-end"><button class="btn ghost sm" data-k="notes">What's new</button><button class="btn ghost sm" data-k="later">Later</button><button class="btn primary sm" data-k="go">Update</button></div>`;
  document.body.append(t); t.showPopover();
  t.querySelector('[data-k=notes]').onclick = () => T.opener.openUrl(u.url);
  t.querySelector('[data-k=later]').onclick = () => { try { sessionStorage.skipUpdate = u.latest; } catch {} t.remove(); };
  t.querySelector('[data-k=go]').onclick = async e => {
    if (!/Windows/.test(navigator.userAgent)) return T.opener.openUrl(u.url);
    e.target.disabled = true; e.target.textContent = 'Downloading…';
    try { await call('update_run'); } catch (err) { e.target.textContent = 'Failed'; t.querySelector('.dim').textContent = String(err); }
  };
}
// yours first; repos owned by someone else (collaborations) in their own group
function drawProjects() {
  const mine = S.projects.filter(p => !p.shared), shared = S.projects.filter(p => p.shared);
  const row = p => projRow(p).replace(`onclick="openProject('${p.id}')"`, `onclick="openProject('${p.id}');setSide('files')"`); // #401 pick → Files tab
  $('projects').innerHTML = (mine.map(row).join('') || (shared.length ? '' : empty('No projects yet', startProj)))
    + (shared.length ? `<div class="label side-sub" title="GitHub repos owned by someone else that you can push to">Collaborations</div>${shared.map(row).join('')}` : '');
  drawAcct();
}

// header, ready bar, toolbar toggles and the status row under the input
function drawChrome() {
  drawRails();
  const p = P();
  $('pname').textContent = p.name;
  $('tCont').title = 'Clear & continue: ' + MODES[p.cont][0];
  $('tWeb').classList.toggle('on', S.web !== false);
  $('tWeb').title = `Web search: ${S.web !== false ? 'on' : 'off'}, in every project (click to switch)`;
  const next = nextUp(cur);
  // idle without a fresh handover: still offer the next task (sent in this session, no clear) or, queue empty, presets
  const idle = !busy[cur] && !reading[cur] /* #145: Continue sent, turn not up yet → no bar flash */ && !p.ready && p.cont !== 'off' && !queue[cur]?.length && !!logs[cur]?.length;
  const wasHid = $('ready').hidden; if (wasHid) $('ready').classList.add('tuck'); $('ready').hidden = !(p.ready || idle || triaging[cur]);
  if (wasHid && !$('ready').hidden) readyPeek();
  // #63: task done, Continue waiting → pop up once per ready; tucks back by the usual rules (4s timer, click elsewhere, focus)
  if (p.ready && !readyWas[cur] && !$('ready').hidden) { const r = $('ready'); r.classList.remove('tuck', 'shine'); void r.offsetWidth; r.classList.add('shine'); readyLater(9000); } /* #70: 9s ≠ 4s ∵ easy to miss; reflow restarts the shine */
  readyWas[cur] = !!p.ready;
  $('contBtn').hidden = !next.length && !triaging[cur]; /* #87: ready + no task ≠ Continue (fresh turn would find nothing) */
  // plan being re-sorted → Wait…; turn running → Queue (contQ: Continue runs when it ends) → Queued
  $('contBtn').disabled = !!triaging[cur] || !!contQ[cur]; $('contBtn').textContent = triaging[cur] ? 'Wait…' : contQ[cur] ? 'Queued' : busy[cur] ? 'Queue' : 'Continue';
  $('contBtn').title = busy[cur] ? 'Runs once the current turn ends' : '';
  // questions never block a Continue: they open in the fresh session (qsOpen), the bar keeps showing the next task
  const dis = busy[cur] ? ' disabled title="Clauracle is working: wait for this turn to end"' : ''; // #117 presets faded while a turn runs
  $('readytext').innerHTML = next.length ? `<input type="checkbox" disabled style="vertical-align:-2px;margin:0 6px 0 0;opacity:.55">Next: <b>${esc(next[0])}</b>${next.length > 1 ? ` <span class="pill dim">+${next.length - 1}</span>` : ''}`
    : `What next? ${(([l, ic, js]) => `<button class="btn primary"${dis} onclick="${js}" title="${l}" aria-label="${l}">${icon(ic)}<span class="vl">${l}</span></button>`)(nextStep(p))}${NEXT_IDEAS.map(([l, ic], i) => `<button class="btn sm"${dis} onclick="askNext(${i})" title="${l}" aria-label="${l}">${icon(ic)}<span class="vl">${l}</span></button>`).join('')}`;
  readyFit();
  $('ready').title = ''; // #95: click toggles .full only when truncated (readyMore), title set there
  $('handpill').hidden = !(reading[cur] || busy[cur] && hand[cur]);
  $('handpill').textContent = reading[cur] ? 'Reading handover…' : 'Preparing handover…';
  $('workpill').hidden = !busy[cur] || !$('handpill').hidden || shRun(); // #124: shell running → its terminal card shows it
  const sk = $('skel'), skOn = busy[cur] || !$('handpill').hidden;
  if (sk.hidden && skOn) { $('skelts').textContent = tm(new Date().toISOString()); const stick = atEnd();
    sk.hidden = false; if (stick) toEnd(true); }
  sk.hidden = !skOn;
  workTick();
  $('status').innerHTML = `<button onclick="menuAt(this, acctItems())" title="AI account, model and effort: ${esc(mName(p.model))}${p.effort ? ', ' + esc(EFFORTS[p.effort] || p.effort) + ' effort' : ''}">${icon('user')}<b class="lb">${esc(nick(p.account))}</b><span class="lb">${esc(mName(p.model))}${p.effort ? ' · ' + esc(EFFORTS[p.effort] || p.effort) : ''}</span>${icon('down')}</button>`
    + `<button onclick="menuAt(this, permItems())" title="Permissions: level ${permN(p.mode)} of 5, ${esc(PERMS[p.mode] || p.mode)}">${PERM_C[permN(p.mode) - 1] ? icon('shield').replace('<svg', `<svg style="color:${PERM_C[permN(p.mode) - 1]}"`) : icon('shield')}<span class="lb">${PERM_S[p.mode] || p.mode}</span><span class="ab">${permN(p.mode)}</span></button>`
    + (p.questions.length ? `<button onclick="showQuestions(cur)" class="pill" style="color:var(--accent)" title="${p.questions.length} question${p.questions.length > 1 ? 's' : ''} waiting">${p.questions.length}?</button>` : '');
  drawGbar();
  const sendBtn = $('send');
  sendBtn.innerHTML = busy[cur] && !hasInput() ? icon('stop') : icon('up');
  sendBtn.title = busy[cur] && !hasInput() ? 'Stop' : 'Send (Enter)';
}
// session timers per project (this computer): you = time active in the app (input within the last 2 min, window focused),
// ai = time Claude worked, total = time either was going. Reset with a fresh session. Shown at the end of the status row.
const TIMES = (() => { try { return JSON.parse(localStorage.times || '{}'); } catch { return {}; } })();
const IDLE = 120e3;
let lastAct = Date.now(), lastTick = Date.now(), tickN = 0;
['keydown', 'pointermove', 'pointerdown', 'wheel'].forEach(t => addEventListener(t, () => lastAct = Date.now(), {passive: true, capture: true}));
const dur = ms => { const m = Math.floor(ms / 60e3); return m < 60 ? `${m}m` : `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, '0')}m`; };
function drawTimes() {
  const x = cur && TIMES[cur], el = $('stime'); if (!el || !x) return;
  // #200 one stacked bar: whole bar = total; You / AI segments by share (they overlap in time ∴ share of you+ai, not of total)
  const sum = x.you + x.ai || 1, C = {you: 'var(--ok)', ai: 'var(--accent)', any: 'var(--dim)'};
  // #201 skeleton once per project/session, then only text + widths change ∵ innerHTML each tick killed the hover tooltip (flash)
  if (el._k !== cur + x.start) { el._k = cur + x.start;
    const it = (k, ic, l, t) => /* #207 label only in tip ∵ row overflowed card */ `<span class="row" style="gap:6px;white-space:nowrap" data-tip="${t}"><span class="tic" style="background:${C[k]}">${icon(ic)}</span><b data-k="${k}"></b></span>`;
    el.innerHTML = `<div class="row" style="font-size:12px;gap:14px;margin-top:4px">${it('you', 'user', 'You', 'You: active (2 min without input counts as idle)')}${it('ai', 'sparkle', 'AI', 'AI: working')}<span class="grow"></span>${it('any', 'clock', 'Total', 'Total: either going, since ' + esc(tm(new Date(x.start).toISOString())))}</div>
      <div class="ubar"><i style="background:${C.you}"></i><i style="background:${C.ai}"></i></div>`; }
  el.querySelectorAll('[data-k]').forEach(b => { const v = dur(x[b.dataset.k]); if (b.textContent !== v) b.textContent = v; });
  const [i1, i2] = el.querySelectorAll('.ubar i'); i1.style.width = x.you / sum * 100 + '%'; i2.style.width = x.ai / sum * 100 + '%';
}
setInterval(() => {
  const now = Date.now(), dt = now - lastTick; lastTick = now;
  if (dt > 10e3) return; // the computer slept
  const you = document.hasFocus() && now - lastAct < IDLE;
  for (const p of S.projects) {
    const x = TIMES[p.id] ||= {start: now, you: 0, ai: 0, any: 0}, me = you && p.id === cur;
    if (me) x.you += dt; if (busy[p.id]) x.ai += dt; if (me || busy[p.id]) x.any += dt;
  }
  if (++tickN % 10 === 0) try { localStorage.times = JSON.stringify(TIMES); } catch {}
  drawTimes();
}, 1000);
// Working… pill = what's really going on: the running tool (or Thinking) + elapsed, ticking each second
const doing = {}; // pid → {text, id, t}: set by tool lines / busy, back to Thinking when that tool's result lands
const setDoing = (pid, text, id) => { doing[pid] = {text, id, t: Date.now(), sh: /^Running: (Bash|PowerShell)/.test(text)}; }; // #124 sh → the terminal card is the pill
let workT = null; const TURN0 = {}; // #225 pid → turn start (head avatar timer)
/* ONE working indicator: every terminal row whose call has no result yet (PEND; parallel calls too) wears the shimmer + its
   own timer; the Working pill shows only when no running row is visible (Thinking, or a call outside the chat cards) */
const PEND = {}; // pid → Map(tool id → start ms), cleared when the turn ends
const mmss = ms => { const s = Math.max(0, Math.floor(ms / 1000)); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`; };
const runRows = () => !busy[cur] || view !== 'chat' || !$('handpill').hidden || !PEND[cur]?.size ? []
  : [...$('log').querySelectorAll('.termcard:not(.old) .tl > div')].filter(r => r._es?.some(e => PEND[cur].has(e.id)));
const shRun = () => runRows().length > 0;
function workTick() {
  const on = busy[cur] && $('handpill').hidden;
  const draw = () => { const rows = runRows(), now = Date.now();
    $('log').querySelectorAll('.tl > div.run').forEach(x => !rows.includes(x) && x.classList.remove('run'));
    for (const r of rows) { r.classList.add('run'); r.dataset.t = mmss(now - Math.min(...r._es.filter(e => PEND[cur].has(e.id)).map(e => PEND[cur].get(e.id)))); }
    const d = doing[cur] || {text: 'Thinking', t: now}, hd = turnHead();
    let sa = $('stickav'); // #232 one sticky avatar for the newest head row
    if (hd && !sa) { sa = Object.assign(document.createElement('div'), {id: 'stickav'}); sa.innerHTML = '<span class="tav"><svg class="av"><use href="#i-logo"/></svg></span>'; }
    if (hd && sa.nextElementSibling !== hd) { hd.before(sa); // overlay the row's own avatar (reply rows sit indented + raised): measure unstuck, once per new head
      sa.style.position = 'static'; const s0 = sa.getBoundingClientRect(), a0 = hd.querySelector('.av').getBoundingClientRect(), dy = a0.top - s0.top; sa.style.position = '';
      sa.firstElementChild.style.top = dy + 'px'; sa.style.top = -14 - dy + 'px'; } // stuck: avatar at 48px whatever dy
    else if (!hd) sa?.remove();
    const av = hd && sa.firstElementChild;
    $('skel').classList.toggle('hd', !!hd); // #189 the turn's head row already shows avatar + name
    $('log').querySelectorAll('.crow.stick').forEach(x => x !== hd && x.classList.remove('stick')); hd?.classList.add('stick');
    // #225 live head: avatar ring + turn timer while busy; pill always centred in #dockpill
    $('log').querySelectorAll('.tav.on').forEach(x => x !== av && x.classList.remove('on'));
    av?.classList.toggle('on', !!busy[cur]); if (av) av.dataset.t = mmss(now - (TURN0[cur] || d.t)); // #225 ring + turn timer whenever busy
    $('workpill').hidden = !busy[cur] || !$('handpill').hidden;
    $('worktext').textContent = `${rows.length ? 'Thinking' : d.text} · ${mmss(now - (TURN0[cur] || d.t))}`; jumpTxt(); }; // #260 pill = same turn total as the avatar badge, never resets per step
  if (on && !workT) { draw(); workT = setInterval(draw, 1000); }
  if (!on && workT) { clearInterval(workT); workT = null; }
  if (on) draw(); else { $('log').querySelectorAll('.tl > div.run, .tav.on').forEach(x => x.classList.remove('run', 'on')); $('skel').classList.remove('hd'); $('log').querySelectorAll('.crow.stick').forEach(x => x.classList.remove('stick')); $('stickav')?.remove(); jumpTxt(); }
}
// #189 live turn's Clauracle head after the last user msg (chat view, busy) → skeleton drops its own avatar
function turnHead() {
  const c = $('log').firstElementChild; if (!busy[cur] || view !== 'chat' || !c || c.id === 'skel') return null;
  const r = [...c.querySelectorAll(':scope > .crow.user, :scope > .crow.claude')], u = r.findLastIndex(x => x.classList.contains('user'));
  // #196/#232 newest head row (thead or text row with its own avatar) after the user msg ∴ only the bottom-most one sticks + rings
  return u < 0 ? null : r.slice(u + 1).findLast(x => !x.matches('.cont, .reply')) || null; // #233 .reply rows have no avatar (hang under their card)
}
// tooltips: native ones look bad and sit under dialogs; [title] moves to data-tip, shown in the top layer (popover) after a hover pause
const tip = Object.assign(document.createElement('div'), {id: 'tip', popover: 'manual'}); document.body.append(tip);
let tipEl = null, tipT, tipX = 0, tipY = 0, tipOver = null;
const hideTip = () => { clearTimeout(tipT); tipEl = null; tip.classList.remove('rich'); if (tip.matches(':popover-open')) tip.hidePopover(); };
const infoTip = el => { hideTip(); tip.classList.add('rich'); tip.innerHTML = `<span>${icon('info')}</span><span></span>`; tip.lastChild.textContent = el.dataset.info; tip.showPopover();
  const b = el.getBoundingClientRect(), w = tip.offsetWidth, h = tip.offsetHeight;
  tip.style.left = Math.max(6, Math.min(b.left + b.width / 2 - w / 2, innerWidth - w - 6)) + 'px';
  tip.style.top = (b.bottom + 6 + h <= innerHeight - 6 ? b.bottom + 6 : Math.max(6, b.top - h - 6)) + 'px'; };
const tipFor = el => { hideTip(); if (!el?.dataset.tip || document.body.classList.contains('dragging')) return; /* #399 no tip mid-drag (stayed behind the moving composer) */
  tipEl = el;
  tipT = setTimeout(() => {
    if (!el.isConnected) return;
    const mx = tipX, my = tipY;
    tip.textContent = el.dataset.tip; tip.showPopover(); // shown last = above an open dialog/menu
    const b = el.getBoundingClientRect(), r = b.height > 160 ? {top: my - 10, bottom: my + 18, left: b.left, width: b.width} : b, w = tip.offsetWidth, h = tip.offsetHeight, x = r.width > 160 ? mx : r.left + r.width / 2; // wide rows: at the pointer
    tip.style.left = Math.max(6, Math.min(x - w / 2, innerWidth - w - 6)) + 'px';
    tip.style.top = (r.top - h - 6 >= 6 ? r.top - h - 6 : r.bottom + 6) + 'px'; // above (the pointer covers below), else below; #347 tall els (grips) = at the pointer, not their far edge
  }, 450); };
document.addEventListener('mousemove', e => { tipX = e.clientX; tipY = e.clientY; }, {passive: true});
document.addEventListener('mouseover', e => {
  // #248 every ancestor's title → data-tip ∵ an outer [title] left behind showed the OS tooltip too (after a long hover)
  for (let a = e.target; a?.removeAttribute; a = a.parentElement) if (a.hasAttribute('title')) { a.dataset.tip ||= a.title; a.removeAttribute('title'); }
  tipOver = e.target; tipX = e.clientX; tipY = e.clientY;
  const el = e.target.closest?.('[data-tip]');
  if (el === tipEl || (!el && tip.classList.contains('rich'))) return;
  tipFor(el);
});
// #389 title set later (readyMore, lint hover, drawChrome…) = native tip beside ours ∴ convert every title write at once; tip change on the hovered el → re-show
new MutationObserver(ms => { for (const {target: n, attributeName: k} of ms) {
  if (k === 'title') { if (n.hasAttribute('title')) { n.dataset.tip = n.title; n.removeAttribute('title'); } continue; }
  if (n === tipEl || (n.dataset.tip && tipOver?.isConnected && tipOver.closest('[data-tip]') === n)) tipFor(n);
} }).observe(document.documentElement, {subtree: true, attributes: true, attributeFilter: ['title', 'data-tip']});
['mousedown', 'wheel', 'keydown'].forEach(t => document.addEventListener(t, hideTip, true));
// accounts; each row has two labelled click dropdowns: model (sparkle), effort (brain); picking one also picks that account.
// model/effort are project-level (kept on account switch) → every row shows the project's current values
const acctItems = () => [...S.accounts.map(a => { const on = P().account === a.id, m = P().model || '', ef = P().effort || '';
  return {check: on, label: a.nick, hint: usageText(a.id), run: () => setP({account: a.id}), acts: [
    {icon: 'sparkle', label: mName(m), tip: 'Model', items: [{title: 'Model'},
      ...mList().map(([k, l, d]) => ({check: m === k, label: l, hint: d, run: () => setP({account: a.id, model: k})}))]},
    {icon: 'brain', label: EFFORTS[ef] || ef, tip: 'Effort', items: [{title: 'Effort'},
      ...effOf(m).map(k => ({check: ef === k, label: EFFORTS[k] || k, run: () => setP({account: a.id, effort: k})}))]}]}; }), '-',
  {icon: 'gear', label: 'Manage accounts…', run: () => openSettings('ai')}];
const permItems = () => [{title: 'Permissions (level)'}, ...Object.entries(PERMS).map(([k, l], i) => ({check: P().mode === k, label: `${i + 1}  ${l}`, run: () => setP({mode: k})}))];
const usageText = id => { const u = S.usage[id] || {}; return [u.five_hour != null && `5h ${u.five_hour}%`, u.seven_day != null && `week ${u.seven_day}%`].filter(Boolean).join(' · ') || 'usage not seen yet'; };

const LOADED = new Set(); // pids whose transcript is in logs[]
/* #243 the page's log on disk (log_save → open_project `saved`) ∵ transcripts lack UI-only notes (task reports…) + only 2 sessions.
   saved + transcript lines newer than its last ts (turn ran on after the last save). No save yet → transcripts as before. */
function restoreLog(info) {
  const h = info.history, sv = Array.isArray(info.saved) ? info.saved : null;
  if (!sv) return h.length ? [...h, {kind: 'note', text: 'Resumed session'}] : [];
  const last = Math.max(0, ...sv.map(e => Date.parse(e.ts) || 0)), cur = h.filter(e => !e.old), i = cur.findLastIndex(e => (Date.parse(e.ts) || 0) && Date.parse(e.ts) <= last);
  const add = last ? cur.slice(i + 1) : [];
  return [...sv, ...add, ...(sv.at(-1)?.text === 'Resumed session' && !add.length ? [] : [{kind: 'note', text: 'Resumed session', ts: new Date().toISOString()}])];
}
// asks = live (Rust re-sends); strings cut at 8k (big Write/diff bodies) ∵ file size. Only after LOADED ∵ an early save would overwrite the disk log
const LOGT = {}, saveLog = pid => { if (!LOADED.has(pid)) return; clearTimeout(LOGT[pid]); LOGT[pid] = setTimeout(() =>
  call('log_save', {pid, log: JSON.parse(JSON.stringify((logs[pid] || []).filter(x => x.kind !== 'ask'), (k, v) => typeof v === 'string' && v.length > 8000 && !v.startsWith('data:') ? v.slice(0, 8000) + '…' : v /* #438 data: = image, cut = broken thumb */))}).catch(() => {}), 2000); };
async function openProject(pid) {
  if (!P(pid)) return;
  if (cur) texts[cur] = $('prompt').value;
  const prev = cur; cur = pid;
  if (prev && prev !== pid && editing[prev]) flushQueue(prev); // left mid-edit → its other queued messages go on
  $('prompt').value = texts[pid] || '';
  $('head').hidden = $('dock').hidden = $('right').hidden = false; $('suggest').hidden = true; grow(); // measure once visible
  drawProjects(); drawChrome(); drawAtts(); drawQueue(); drawEditBar();
  const info = await call('open_project', {pid});
  // transcript once per run, BEFORE whatever arrived meanwhile (resumed-turn lines, flushed queue) ∵ `!logs[pid]` skipped it → chat looked lost after every restart
  if (!LOADED.has(pid)) { LOADED.add(pid); logs[pid] = [...restoreLog(info), ...(logs[pid] || [])]; }
  if (info.busy) busy[pid] = true; // never → false here: Rust busy is false during armed build steps while the UI must stay busy (status events own that)
  if (pid !== cur) return; // switched again while loading
  $('branch').hidden = !info.branch; $('branch').textContent = info.branch;
  drawLog(); drawChrome(); drawRight(); $('prompt').focus(); if (view === 'edit') applyView();
  suggest(pid); pollTeam(); drawSide(); askLegacy(pid);
}
// the repo's owner decides (by code): its gh account and the AI account tied to it, if they're set up here
async function suggest(pid) {
  const r = await call('who', {pid}).catch(() => null), box = $('suggest');
  await refresh();
  if (pid !== cur) return;
  box.hidden = !r?.differs;
  if (!r?.differs) return;
  const w = r.who;
  box.innerHTML = `${icon('gh')}<span class="grow">${w.shared ? `Shared repo owned by <b>${esc(w.owner)}</b>.` : `This repo belongs to <b>@${esc(w.owner)}</b>.`}
    Suggested: GitHub <b>@${esc(w.gh)}</b> with AI account <b>${esc(nick(w.account))}</b>.</span>
    <button class="btn ghost" onclick="setP({keep:true}); $('suggest').hidden=true">Keep current</button>
    <button class="btn primary" onclick='setP({gh:${esc(JSON.stringify(w.gh))}, account:${esc(JSON.stringify(w.account))}}); $("suggest").hidden=true'>Use these</button>`;
}
const setP = changes => call('update_project', {pid: cur, changes});

