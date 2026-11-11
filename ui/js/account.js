// ---- account card (bottom-left): the open project's AI + GitHub account, others as a stack ----
// GitHub avatars from github.com/<user>.png; Claude accounts have no pictures → initials in the mascot colours
// AI accounts: the service's mark (all Claude for now) in its brand colour
const avAI = (a, cls = '') => `<span class="av ai ${cls}" title="${esc(a.nick)}"><svg><use href="#i-claude"/></svg></span>`;
// avatars: one preloaded Image per user, kept alive → the page's image cache draws re-rendered cards instantly (no reload flash)
/* avatars: Rust `avatar` (disk cache 1d) → blob URL once per run; imgs drawn before it lands get data-av and are filled in.
   fail → plain github URL (onerror → letter) */
const AVIMG = new Map(), avUrl = u => {
  if (!AVIMG.has(u)) AVIMG.set(u, call('avatar', {login: u}).then(b => URL.createObjectURL(new Blob([b], {type: 'image/png'})), () => `https://github.com/${encodeURIComponent(u)}.png?size=88`)
    .then(s => { AVIMG.set(u, s); document.querySelectorAll('img[data-av]').forEach(i => { if (i.dataset.av === u) i.src = s; }); return s; }));
  const s = AVIMG.get(u); return typeof s === 'string' ? s : '';
};
const avGh = (u, cls = '') => `<img class="av ${cls}" data-av="${esc(u)}" src="${avUrl(u) || 'data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7'}" alt="" title="@${esc(u)}" onerror="this.replaceWith(Object.assign(document.createElement('span'),{className:'av ${cls}',textContent:'${esc(u[0] || '?').toUpperCase()}',style:'background:var(--accent)'}))">`;
function drawAcct() {
  const p = P(), a = S.accounts.find(x => x.id === (p?.account || S.default_account)) || S.accounts[0];
  if (!a) return $('acctBtn').innerHTML = `${icon('user')}<span class="nm">Sign in</span>`;
  const gh = p?.gh;
  // stack: only what this project uses (AI mark + its GitHub picture); other accounts live in the card, so none look active here
  $('acctBtn').innerHTML = `<span class="stack">${avAI(a)}${gh ? avGh(gh) : ''}</span>
    <span class="nm grow">${esc(a.nick)}<small>${gh ? '@' + esc(gh) : 'no GitHub account'}</small></span>${icon('search').replace('class="i"', 'class="i sic"')}`;
}
// usage line: icon · title · bar · % on one row; the reset time on its own line underneath, aligned past the icon
const useRow = (v, l, ic, sub) => `<div style="font-size:12px"><div class="row" style="gap:6px">${icon(ic)}<span class="grow">${l}</span><span class="bar" style="width:90px"><i style="width:${v || 0}%"></i></span><span style="width:34px;text-align:right">${v == null ? '?%' : v + '%'}</span></div>${sub ? `<small class="faint" style="display:block;padding-left:22px">${sub}</small>` : ''}</div>`;
// "Reset: 2h 14m · 14:30" for an account's usage window ('' when unknown)
function resetIn(id, k) {
  const r = S.resets?.[id]?.[k]; if (!r) return '';
  const ms = r * 1000 - Date.now(); if (ms <= 0) return 'reset, fresh quota';
  const h = Math.floor(ms / 36e5), m = Math.floor(ms % 36e5 / 6e4);
  return `Reset: ${h >= 24 ? `${Math.floor(h / 24)}d ${h % 24}h` : h ? `${h}h ${m}m` : `${m}m`} · ${tm(new Date(r * 1000).toISOString())}`;
}
// a usage window that reset in the last 10 min → one toast (seen keys kept per page load)
// #366 any passed reset → S.usage 0% locally (no provider call; Rust usage() does the same on state()), redraw once
const RESEEN = new Set();
setInterval(() => { let z = 0; for (const [id, w] of Object.entries(S.resets || {})) for (const [k, r] of Object.entries(w)) {
  const key = id + k + r, ago = Date.now() - r * 1000;
  if (ago > 0 && S.usage?.[id]?.[k]) { S.usage[id][k] = 0; z = 1; }
  if (ago > 0 && ago < 6e5 && !RESEEN.has(key)) { RESEEN.add(key); toast(`${k === 'five_hour' ? '5-hour' : 'Weekly'} limit reset for ${nick(id)}: fresh quota`, 'ok', 0, true, () => openSettings('ai')); }
} if (z && P()) drawChrome(); }, 30000);
// #39 kind → [icon, colour]: info | ok | warn | err
const TOASTK = {info: ['info', 'var(--accent)'], ok: ['check', 'var(--ok)'], warn: ['warn', 'var(--warn)'], err: ['x', 'var(--err)']};
// #284 notification center: every toast → NOTES (newest first, ≤50, localStorage.notes; act fn kept this run only) → bell in #gbar
let NOTES = []; try { NOTES = JSON.parse(localStorage.notes || '[]'); } catch {} NOTES = NOTES.filter((n, i) => NOTES.findIndex(m => m.text === n.text) === i); // #429 old stacked copies → newest only
let NUNREAD = NOTES.filter(n => !n.seen).length;
// #402 snooze = S.notify.snooze unix secs (Rust notify() skips desktop/phone too); ∞ = 9999999999
const snoozed = () => Date.now() / 1000 < (S.notify?.snooze || 0);
const snooze = s => saveNotify({snooze: s && (s > 1e9 ? s : Math.floor(Date.now() / 1000) + s)});
const snoozeMenu = b => { const go = s => () => { snooze(s); document.querySelector('.acard:popover-open')?.hidePopover(); }; // #430 bell card snooze icon
  menuAt(b, [...(snoozed() ? [{label: 'Unsnooze', icon: 'bell', run: go(0)}, '-'] : []), ...[['1h', 3600], ['24h', 86400], ['7d', 604800], ['∞', 9999999999]].map(([l, s]) => ({label: 'Snooze ' + l, icon: 'clock', run: go(s)}))]); };
const notesSave = () => { try { localStorage.notes = JSON.stringify(NOTES.map(({act, go, ...n}) => n)); } catch {} };
// One toast shows at a time; the queue waits behind it as stacked card edges and slides forward when the front one leaves.
const TQ = [];
function toastDraw() { const box = $('toasts'); if (!box) return;
  TQ.forEach((t, i) => { t.style.setProperty('--i', Math.min(i, 3)); t.classList.toggle('back', i > 0); t.classList.toggle('far', i > 2); });
  const top = TQ[0]; if (top && !top._timer) top._timer = setTimeout(() => toastOut(top), top._ms);
  if (!TQ.length) box.hidePopover?.(); }
function toastOut(t, now) { const i = TQ.indexOf(t); if (i >= 0) TQ.splice(i, 1); clearTimeout(t._timer);
  if (now) t.remove(); else { t.classList.add('out'); setTimeout(() => t.remove(), 260); } toastDraw(); }
function toast(text, kind = 'info', act, log = true, go) { // act = [label, fn] → button, stays 20s; log false = popup only (ⓘ help); go = fn that opens the toast's source on click
  // #429 same text = one entry: older copy (list + open popup) replaced, never stacked
  if (log) { const o = NOTES.findIndex(n => n.text === text); if (o >= 0) { if (!NOTES[o].seen) NUNREAD--; NOTES.splice(o, 1); }
    NOTES.unshift({text, kind, ts: Date.now(), act, go}); NOTES.length = Math.min(NOTES.length, 50); NUNREAD++; notesSave(); try { drawGbar(); } catch {} } /* toast before S loaded */
  TQ.filter(x => x._t === text).forEach(x => toastOut(x, 1));
  if (snoozed()) return; /* #402 still logged in the bell list, no popup */
  const [ic, c] = TOASTK[kind] || TOASTK.info, t = Object.assign(document.createElement('div'), {className: 'toast glass in', _t: text, _ms: act ? 20000 : 8000, title: go ? 'Open' : ''});
  t.innerHTML = `<div class="row" style="gap:10px;align-items:flex-start"><span style="color:${c};flex:none;display:inline-flex;padding-top:1px">${icon(ic)}</span><span class="grow">${esc(text)}</span>${act ? `<button class="btn sm primary">${esc(act[0])}</button>` : ''}</div>`;
  let box = $('toasts'); if (!box) document.body.append(box = Object.assign(document.createElement('div'), {id: 'toasts', popover: 'manual'}));
  box.append(t); TQ.push(t); if (box.matches(':popover-open')) box.hidePopover(); box.showPopover(); /* re-show = above any popover opened since */
  requestAnimationFrame(() => requestAnimationFrame(() => t.classList.remove('in'))); toastDraw();
  t.onclick = () => { toastOut(t); go?.(); };
  if (act) t.querySelector('button').onclick = e => { e.stopPropagation(); toastOut(t); act[1](); };
}
/* #167 missing tool/feature/module in a tool result → install suggestion, pure regex (no AI tokens). → {k, n, f?, pkg?, label} | null
   ponytail: TOOLPKG = known tools only [winget id, brew name]; add one = an entry. gh/gcloud/claude use their own installers. */
const TOOLPKG = {node: ['OpenJS.NodeJS.LTS', 'node'], npm: ['OpenJS.NodeJS.LTS', 'node'], npx: ['OpenJS.NodeJS.LTS', 'node'], git: ['Git.Git', 'git'],
  python: ['Python.Python.3.12', 'python'], python3: ['Python.Python.3.12', 'python'], pip: ['Python.Python.3.12', 'python'], cargo: ['Rustlang.Rustup', 'rustup'],
  rustup: ['Rustlang.Rustup', 'rustup'], jq: ['jqlang.jq', 'jq'], go: ['GoLang.Go', 'go'], ffmpeg: ['Gyan.FFmpeg', 'ffmpeg'], gh: [], gcloud: [], claude: []};
function missOf(t) {
  let m = /gated behind the `([\w-]+)` feature/.exec(t), c = m && (/(?:for (?:struct|enum|trait|type) |unresolved import )`([a-z_]\w*)::/.exec(t) || /[\/\\]([a-z][\w-]*?)-\d+\.\d+\.\d+[\/\\]src[\/\\]/.exec(t));
  if (m && c) return {k: 'cargo', n: c[1].replace(/_/g, '-'), f: m[1], label: `Enable the ${m[1]} feature of ${c[1]}`};
  // #295: name must be package-shaped (@scope/pkg/sub) ∵ grep output of this very regex matched "([^"
  if ((m = /Cannot find module '(@?[\w-][\w.\/@-]*)'|Can't resolve '(@?[\w-][\w.\/@-]*)'/.exec(t))) { const n = (m[1] || m[2]).match(/^(@[^/]+\/)?[^/]+/)[0]; return {k: 'npm', n, label: `npm install ${n}`}; }
  if ((m = /No module named '([\w-]+)/.exec(t))) return {k: 'pip', n: m[1], label: `pip install ${m[1]}`};
  m = /(?:^|\n)(?:\S*: )?(?:line \d+: )?([\w.-]+): (?:command )?not found|'([\w.-]+)' is not recognized as an internal|The term '([\w.-]+)' is not recognized/.exec(t);
  const n = m && (m[1] || m[2] || m[3]).replace(/\.(exe|cmd)$/, '');
  return n && TOOLPKG[n] ? {k: 'tool', n, pkg: TOOLPKG[n][navigator.userAgent.includes('Mac') ? 1 : 0], label: `Install ${n}`} : null;
}
const MISSED = new Set(); // ponytail: one suggestion per thing per app run
function missCheck(pid, t) {
  const x = t && missOf(t), key = x && x.k + ':' + x.n + ':' + (x.f || '');
  if (!x || MISSED.has(key)) return; MISSED.add(key);
  toast(`Missing: ${x.n}${x.f ? ' (' + x.f + ' feature)' : ''}`, 'warn', [x.label.split(' ')[0] === 'Enable' ? 'Enable' : 'Install', async () => {
    toast(x.label + '…'); try { await call('fix_missing', {pid, kind: x.k, name: x.n, feat: x.f || null, pkg: x.pkg || null}); toast('Done: ' + x.label, 'ok'); DEPSC = null; }
    catch (e) { toast(String(e), 'err'); } }]);
}
// #218 opener toggles its card: light-dismiss fires on pointerdown, so the same click must not reopen it
const SHUT = {}, reopen = k => Date.now() - (SHUT[k] || 0) < 400, shutOn = (card, k) => card.addEventListener('beforetoggle', e => e.newState === 'closed' && (SHUT[k] = Date.now())); /* #307 beforetoggle = sync; toggle is queued → ran after the click had already reopened */
/* #283 Start menu: account row → morphs (clip-path) into the Clauracle menu; row stays put at its bottom.
   Grid = every side tab (left + right); search → rows: tabs/settings, projects, files (HEAT, file_heat), secret NAMES (env_list, never values), tasks.
   ponytail: plain substring match, ≤ 60 rows; fuzzy ranking when lists get long */
const SMTABS = () => [['projects', 'folder', 'Projects', 'l'], ['files', 'doc', 'Files', 'l'], ['notes', 'notepad', 'Notes', 'l'],
  ...(cur ? [...document.querySelectorAll('#rtabs button[data-t]')].map(b => [b.dataset.t, b.querySelector('use').getAttribute('href').slice(3), b.textContent.trim(), 'r']) : [])];
const SMENV = {}; // pid → {t, keys: [[name, file]]}; 60 s
function smTab(k, side) {
  document.querySelector('.smenu')?.hidePopover();
  if (side === 'l') { UI.lc = false; setSide(k); return jumpTo(`#stabs [data-s="${k}"]`); }
  expandRight(k); jumpTo(`#rtabs [data-t="${k}"]`);
}
function smFind(q) {
  q = q.toLowerCase(); const out = [], pid = cur, has = s => String(s).toLowerCase().includes(q);
  const row = (ic, name, sub, size, date, js) => out.push(`<div class="srow" tabindex="0" onclick="${esc(js)}">${icon(ic)}<span class="grow"><b>${esc(name)}</b>${sub ? `<small>${esc(sub)}</small>` : ''}</span>${size ? `<small>${size}</small>` : ''}${date ? `<small class="sd">${date}</small>` : ''}</div>`);
  const day = t => new Date(t * 1e3).toLocaleDateString(undefined, {day: 'numeric', month: 'short'});
  SMTABS().filter(t => has(t[2])).forEach(t => row(t[1], t[2], 'Tab', '', '', `smTab('${t[0]}', '${t[3]}')`));
  Object.entries(PAGES).filter(([, v]) => has(v[1].replace('&amp;', '&'))).forEach(([k, v]) => row(v[0], v[1].replace('&amp;', '&'), 'Settings', '', '', `document.querySelector('.smenu')?.hidePopover(); openSettings('${k}')`));
  S.projects.filter(p => has(p.name)).forEach(p => row('folder', p.name, 'Project', '', '', `document.querySelector('.smenu')?.hidePopover(); openProject('${p.id}')`));
  if (pid) {
    teamTasks(pid).filter(x => x.number && (has(x.title) || has('#' + x.number))).slice(0, 15).forEach(x => row('check2', '#' + x.number + ' ' + x.title, 'Task', '', '', `document.querySelector('.smenu')?.hidePopover(); editIssue(${x.number})`));
    (SMENV[pid]?.keys || []).filter(([n]) => has(n)).forEach(([n, f]) => row('key', n, 'Secret · ' + f, '', '', `document.querySelector('.smenu')?.hidePopover(); envAt = ${JSON.stringify(f)}; expandRight('svc')`));
    (HEAT[pid]?.d.files || []).filter(f => has(f[0])).slice(0, 40).forEach(f => row('file', f[0].split('/').pop(), f[0].split('/').slice(0, -1).join('/'), mb(f[1]), f[4] ? day(f[4]) : '', `document.querySelector('.smenu')?.hidePopover(); previewFile(${JSON.stringify(f[0])})`));
  }
  return out.slice(0, 60).join('') || '<p class="dim" style="padding:8px">Nothing found.</p>';
}
function startMenu(btn) {
  if (reopen('acct')) return;
  const pid = cur, p = P(), a = S.accounts.find(x => x.id === (p?.account || S.default_account)) || S.accounts[0];
  if (!a) return openSettings('ai');
  // data for search, fetched once per open (HEAT/SMENV cached 60 s)
  if (pid && (!HEAT[pid] || Date.now() - HEAT[pid].t > 60e3)) call('file_heat', {pid}).then(d => { HEAT[pid] = {t: Date.now(), d}; m.querySelector('input').oninput(); }, () => {});
  if (pid && !(Date.now() - (SMENV[pid]?.t || 0) < 60e3)) call('env_list', {pid}).then(fs => { SMENV[pid] = {t: Date.now(), keys: fs.flatMap(f => f.keys.map(k => [k[0], f.file]))}; m.querySelector('input').oninput(); }, () => {});
  const m = Object.assign(document.createElement('div'), {className: 'smenu glass', popover: 'auto'});
  m.innerHTML = `<label class="ssearch">${icon('search')}<input placeholder="Search files, secrets, tasks, settings…" spellcheck="false"></label>
    <div class="sbody"><div class="sgrid">${SMTABS().map(t => `<button onclick="smTab('${t[0]}', '${t[3]}')" title="${esc(t[2])}">${icon(t[1])}<span>${esc(t[2])}</span></button>`).join('')}</div></div>
    <div class="sacct">${btn.innerHTML.replace(/<svg class="i sic">.*?<\/svg>/, '')}
      <button class="ib" title="Settings" data-a="set">${icon('gear')}</button><button class="ib" title="Restart Clauracle: sessions, running turns and unsent input all come back" data-a="re">${icon('loop')}</button><button class="ib" title="Shut down Clauracle" data-a="off">${icon('power')}</button></div>`;
  document.body.append(m);
  let r = btn.getBoundingClientRect(); if (!r.width) r = {left: 20, bottom: innerHeight - 10, width: 0, height: 40}; // sidebar collapsed (rail gear) → bottom-left corner
  m.style.left = r.left - 10 + 'px'; m.style.bottom = innerHeight - r.bottom - 10 + 'px';
  m.showPopover(); shutOn(m, 'acct');
  m.addEventListener('toggle', e => e.newState === 'closed' && m.remove());
  const h = m.offsetHeight, w = m.offsetWidth; // morph: clip from the account row's box out to the full panel
  m.animate([{clipPath: `inset(${h - r.height - 20}px ${Math.max(0, w - r.width - 20)}px 0 0 round 10px)`, opacity: .6}, {clipPath: 'inset(0 0 0 0 round 16px)', opacity: 1}], {duration: 180, easing: 'cubic-bezier(.2,.8,.2,1)'});
  const inp = m.querySelector('input'), body = m.querySelector('.sbody'), grid = body.innerHTML;
  inp.oninput = () => { const q = inp.value.trim(); body.innerHTML = q ? `<div class="slist">${smFind(q)}</div>` : grid; };
  inp.onkeydown = e => { if (e.key === 'Enter') body.querySelector('.srow, .sgrid button')?.click(); if (e.key === 'ArrowDown') { e.preventDefault(); body.querySelector('.srow, .sgrid button')?.focus(); } };
  body.onkeydown = e => { if (e.key === 'Enter' && e.target.matches('.srow')) e.target.click();
    if (/^Arrow(Up|Down)$/.test(e.key)) { e.preventDefault(); const els = [...body.querySelectorAll('.srow, .sgrid button')], i = els.indexOf(e.target) + (e.key === 'ArrowDown' ? 1 : -1); (i < 0 ? inp : els[i])?.focus(); } };
  m.querySelector('.sacct').onclick = e => { const k = e.target.closest('[data-a]')?.dataset.a; m.hidePopover();
    k === 're' ? call('restart') : k === 'off' ? call('quit') : openSettings(k ? undefined : 'ai'); };
  setTimeout(() => inp.focus(), 0);
}
const limTip = v => +v ? `Hand over at ${v}%: when context reaches this, Clauracle writes the handover, clears and continues in a fresh session` : 'Hand over: off (0 = never clear & continue)';
// #195 merged AI card, lower half: account/model/effort (→ acctItems menu), context (+ hand-over), usage, session timers (#stime, live via drawTimes)
function statsHtml(p) {
  const u = S.usage[p.account] || {}, a = S.accounts.find(x => x.id === p.account), c = ctx[cur], n = tk(cur).now;
  // #200 one Usage section: label row, full-width bar under it; 5h + weekly share a line (reset → tooltip)
  const half = (k, l, ic) => `<div class="grow" style="font-size:12px" data-tip=""${l} usage: ${u[k] ?? '?'}%${resetIn(p.account, k) ? '\n' + resetIn(p.account, k) : ''}"><div class="row" style="gap:6px">${icon(ic)}<span class="grow">${l}</span>${u[k] ?? '?'}%</div><div class="ubar"><i style="width:${u[k] || 0}%"></i></div></div>`;
  return `<h5>AI Account</h5><button class="pick" id="scAcct" title="AI account, model and effort"><span class="grow">${esc(nick(p.account))}<small class="dim" style="display:block">${esc(mName(p.model))}${p.effort ? ' · ' + esc(EFFORTS[p.effort] || p.effort) + ' effort' : ''}</small></span>${icon('down')}</button>
    <h5>Usage</h5><div style="font-size:12px"><div class="row" style="gap:6px">${icon('doc')}<span class="grow">Context <span id="scLimV" class="faint"></span></span>${c ?? '?'}%${n ? ` (${kt(n)})` : ''}</div>
      <div class="ubar"><i style="width:${c || 0}%"></i><input id="scLim" type="range" min="0" max="95" value="${p.context_limit}" title="${limTip(p.context_limit)}"></div></div>
    ${tk(cur).saved ? `<div class="row faint" style="font-size:11px;gap:6px" title="Clear-continue: each turn after a clear would otherwise re-read the cleared context. Counted once per turn (tool calls re-read it too), so the real saving is higher.">${icon('loop')}<span class="grow">Saved by Clauracle (est.)</span>${kt(tk(cur).saved)} tokens</div>` : ''}
    <div class="row" style="gap:12px;align-items:flex-start">${half('five_hour', '5-hour', 'clock')}${half('seven_day', 'Weekly', 'cal')}</div>
    <div id="stime"></div>
    <div class="btnrow"><button class="btn sm" id="scReset">Start timers over</button></div>`;
}
/* #179 global statusline (all projects): usage (current project's account, else first), AI working (which + what + agents), builds.
   Redrawn by drawChrome + 1s tick; el._h = last html → no DOM churn. Cards = popovers */
const gbusy = () => S.projects.filter(p => busy[p.id]), gbuilds = () => S.projects.filter(p => builds[p.id]?.busy);
const blast = (pid, n) => { const ls = (builds[pid]?.out || '').slice(builds[pid]?.from || 0).split('\n').filter(l => l.trim()); return n ? (ls.length ? ls.slice(-n) : ['Starting…']) : ls.at(-1) || 'Starting…'; };
// #262 file names a build makes = last build's outputs (GHI builds, newest within 10 min of the top one); '' = none known yet
const bnames = pid => { const b = GHI[pid]?.g?.builds || [], t = b[0]?.mtime; return [...new Set(b.filter(x => t - x.mtime < 600).map(x => x.name))].map(name => ({name})); };
function drawGbar() {
  const el = $('gbar'); if (!el) return;
  const p = cur && P(), acct = p ? p.account : S.accounts[0]?.id, u = S.usage[acct] || {};
  const meter = (k, l, ic) => u[k] == null ? '' : `<span title="${l} usage: ${u[k]}%${resetIn(acct, k) ? ', ' + resetIn(acct, k) : ''}">${icon(ic)}<span class="bar"><i style="width:${u[k]}%"></i></span>${u[k]}%</span>`;
  const wk = gbusy(), bd = gbuilds(), what = () => 'Working…' /* #203 short state only; command → card tooltip */, wait = gwait(), last = glast();
  // #195 ONE AI button (left): activity + context + usage; its card = activity, account/model, context, usage, timers
  const nag = wk.reduce((n, x) => n + agentsOf(x.id).filter(a => a.running).length, 0); // one project: +N after its name; several: a bot count
  const lead = wk.length ? `<i class="gdot"></i><span class="gt">${wk.length > 1 ? `Clauracle working in ${wk.length} projects` : `<b>${esc(wk[0].name)}</b>${nag ? ` +${nag}` : ''} · ${esc(what(wk[0]))}`}</span>${wk.length > 1 && nag ? `<span>${icon('bot')}${nag}</span>` : ''}`
    : wait.length ? `${icon('sparkle')}<span class="gt">${wait.length > 1 ? `${wait.length} projects waiting on you` : `<b>${esc(wait[0].name)}</b> · ${esc(wait[0].why)}`}</span>`
    : `${icon('sparkle')}<span class="gt">Clauracle idle</span>`;
  const ai = `<button class="stats" onclick="gCard(this, 'ai')" title="AI: activity, account, context, usage and session time">${lead}${p ? `<span>${icon('doc')}${ctx[cur] ?? '?'}%</span>` : ''}${meter('five_hour', '5-hour', 'clock')}${meter('seven_day', 'Weekly', 'cal')}</button>`;
  const bl = bd.length ? `<button onclick="gCard(this, 'build')" title="Builds in progress"><span class="spin"></span><span class="gt">Building ${bd.length > 1 ? bd.length + ' projects' : `<b>${esc(bd[0].name)}</b>`}</span></button>`
    : last ? `<button onclick="gCard(this, 'build')" title="Last build"><span class="${last.b.ok ? 'ok' : 'bad'}">${icon(last.b.ok ? 'check' : 'x')}</span><span class="gt">${last.b.ok ? 'Built' : 'Build failed'} <b>${esc(last.p.name)}</b> · ${gago(last.b.at)}</span></button>`
    : `<button onclick="gCard(this, 'build')" title="Builds">${icon('hammer')}<span class="gt">No builds</span></button>`;
  const bell = `<button onclick="gCard(this, 'notes')" title="Notifications${snoozed() ? ' (snoozed)' : ''}${NUNREAD ? ': ' + NUNREAD + ' new' : ''}" style="position:relative">${icon(snoozed() ? 'bell-off' : 'bell')}${NUNREAD ? '<i class="gdot" style="position:absolute;top:2px;right:3px;width:6px;height:6px;animation:none"></i>' : ''}</button>`;
  const clock = `<span title="${new Date().toLocaleDateString([], {weekday: 'long', day: 'numeric', month: 'long'})}">${new Date().toLocaleTimeString([], {hour: 'numeric', minute: '2-digit'})}</span>`; // locale picks 24h / AM-PM
  const h = `<div class="gz">${ai}</div><div class="gz r">${bl}${clock}${bell}</div>`;
  if (el._h !== h) { el._h = h; el.innerHTML = h; }
  drawTimes();
}
setInterval(drawGbar, 1000);
// waiting on the user: ready (Continue up) or open questions; last finished build (this run)
const gwait = () => S.projects.filter(x => !busy[x.id] && (x.questions?.length || x.ready)).map(x => ({...x, why: x.questions?.length ? `${x.questions.length} question${x.questions.length > 1 ? 's' : ''}` : 'ready to continue'}));
const glast = () => S.projects.map(p => ({p, b: builds[p.id]})).filter(x => x.b?.at && !x.b.busy).sort((a, b) => b.b.at - a.b.at)[0];
const gago = t => { const s = (Date.now() - t) / 1000; return s < 60 ? 'just now' : s < 3600 ? Math.round(s / 60) + 'm ago' : Math.round(s / 3600) + 'h ago'; };
function gCard(btn, kind) {
  if (reopen(kind)) return;
  const r = btn.getBoundingClientRect(); // before notes' drawGbar() re-renders btn (detached → 0,0 = top-left, #314)
  const card = Object.assign(document.createElement('div'), {className: 'acard glass', popover: 'auto'});
  const go = (pid, t) => `openProject('${pid}').then(() => ${t ? `rightTab('${t}')` : '0'}); this.closest('[popover]').hidePopover()`;
  const act = (label, js, cls = '') => `<button class="btn sm ${cls}" onclick="event.stopPropagation(); ${js}">${label}</button>`;
  const close = "this.closest('[popover]').hidePopover()";
  const row = (pid, lead, name, tail = '') => `<div class="row" style="gap:6px;cursor:pointer" onclick="${go(pid)}">${lead}<span class="grow">${esc(name)}</span>${tail}</div>`;
  if (kind === 'ai') {
    // #201 short state (Working…/Ready/Idle), full command in the tooltip; acts() re-run each second while open (only on change)
    const acts = () => { const wk = gbusy(), wt = gwait(), why = Object.fromEntries(wt.map(x => [x.id, x.why]));
      // #200 ONE activity list: current project first, then working, then waiting; status on the right; no Continue (the dock has it)
      const ls = [...new Set([...(cur ? [P()] : []), ...wk, ...wt.map(x => S.projects.find(p => p.id === x.id))])].sort((a, b) => (b.id === cur) - (a.id === cur));
      return ls.map(x => { const on = busy[x.id], ag = on ? agentsOf(x.id).filter(a => a.running) : [], w = WORK[x.id];
        return row(x.id, (on ? '<i class="gdot"></i>' : why[x.id] ? icon('sparkle') : '<i class="gdot" style="background:var(--line2);animation:none"></i>') + gtAv('Clauracle'), x.name, `<span class="dim" style="white-space:nowrap"${on ? ` data-tip="${esc(doing[x.id]?.text || 'Thinking')}"` : ''}>${w && on ? '#' + w + ' · ' : ''}${on ? 'Working…' : why[x.id] ? (why[x.id] === 'ready to continue' ? 'Ready' : esc(why[x.id])) : 'Idle'}</span>`
          + (on ? `<button class="ib" title="Stop" onclick="event.stopPropagation(); call('interrupt', {pid: '${x.id}'}); ${close}">${icon('stop')}</button>` : ''))
          + ag.map(a => `<div class="row gav" style="gap:6px;font-size:12px;margin:-4px 0 0 13px">${gtAv(a.type)}<b>${esc(roleOf({name: a.type}))}</b><span class="dim grow" style="min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap" data-tip="${esc(a.desc + (a.last ? ': ' + a.last : ''))}">${esc(a.desc)}${a.last ? ' · ' + esc(a.last) : ''}</span></div>`).join(''); }).join('') || '<p class="dim">Nothing running</p>'; };
    card.innerHTML = `<h5>Activity</h5><div id="gacts">${acts()}</div>` + (cur ? statsHtml(P()) : '');
    const ga = card.querySelector('#gacts'), iv = setInterval(() => { if (!card.isConnected) return clearInterval(iv); const h = acts(); if (ga._h !== h) ga.innerHTML = ga._h = h; }, 1000); ga._h = acts();
  } else if (kind === 'notes') {
    NUNREAD = 0; NOTES.forEach(n => n.seen = 1); window.NSNAP = NOTES.slice(); /* indices stable while card open */ notesSave(); drawGbar();
    card.innerHTML = `<div class="row"><h5 class="grow">Notifications</h5>`
      + `<button class="ib${snoozed() ? ' on' : ''}" title="Snooze" onclick="event.stopPropagation(); snoozeMenu(this)">${icon('clock')}</button>` /* #430 one icon → menu */
      + `${NOTES.length ? act('Clear all', `NOTES = []; notesSave(); drawGbar(); ${close}`, 'ghost') : ''}</div>`
      + (snoozed() ? `<div class="row dim" style="gap:6px;font-size:12px">${icon('bell-off')}<span class="grow">Snoozed${S.notify.snooze < 9e9 ? ' until ' + new Date(S.notify.snooze * 1000).toLocaleString([], {weekday: 'short', hour: '2-digit', minute: '2-digit'}) : ''}: no popups</span></div>` : '') /* #431 own full-width row */
      + (NOTES.map((n, i) => { const [ic, c] = TOASTK[n.kind] || TOASTK.info;
        return `<div class="row" style="gap:8px;align-items:flex-start;font-size:12px${n.go ? ';cursor:pointer' : ''}"${n.go ? ` onclick="NSNAP[${i}].go(); ${close}"` : ''}><span style="color:${c};flex:none;display:inline-flex">${icon(ic)}</span><span class="grow sel">${esc(n.text)}</span><span class="dim" style="white-space:nowrap">${gago(n.ts)}</span></div>`
          + (n.act ? `<div class="gact">${act(esc(n.act[0]), `NSNAP[${i}].act[1](); ${close}`, 'primary')}</div>` : ''); }).join('') || '<p class="dim">No notifications yet: toasts (builds, limits, errors…) land here.</p>');
    card.style.maxHeight = '60vh'; card.style.overflowY = 'auto';
  } else {
    // #262 busy row: output file names (last build's, GHI cache) + last 3 output lines, live (1s); Output → Build tab log, flashed
    const ell = 'font-size:12px;margin:0 0 2px 20px;max-width:420px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap';
    const bhtml = () => { const bd = gbuilds(), done = S.projects.map(p => ({p, b: builds[p.id]})).filter(x => x.b?.at && !x.b.busy).sort((a, b) => b.b.at - a.b.at).slice(0, 4);
      return `<div class="row"><h5 class="grow">Builds</h5>${cur && !builds[cur]?.busy ? act(icon('hammer') + 'Build local copy', `buildLocal(cur); ${close}`, 'ghost') : ''}</div>${bd.map(x => row(x.id, '<span class="spin"></span>', x.name)
        + `<div style="margin:0 0 4px 20px;font-size:12px">${bfiles(x, bnames(x.id), x.id)}</div>` /* #263 same ✓ list as the Build tab */
        + blast(x.id, 3).map(l => `<div class="dim sel mono" style="${ell};font-size:11px">${esc(l)}</div>`).join('')
        + `<div class="gact">${act('Output', `openProject('${x.id}').then(() => { rightTab('build'); jumpTo('#buildOut'); }); ${close}`)}${act(icon('stop') + 'Stop', `call('stop_build', {pid: '${x.id}'}); ${close}`, 'ghost')}</div>`).join('') || '<p class="dim">No builds running</p>'}
      ${done.length ? `<h5>Finished</h5>${done.map(({p, b}) => row(p.id, `<span class="${b.ok ? 'ok' : 'bad'}">${icon(b.ok ? 'check' : 'x')}</span>`, p.name, `<span class="dim">${gago(b.at)}</span>`)
          + `<div class="gact">${act('Output', `openProject('${p.id}').then(() => { rightTab('build'); jumpTo('#buildOut'); }); ${close}`)}${b.ok ? (b.own ? act('Restart into it', `call('restart_into', {pid: '${p.id}'})`, 'primary') : p.build.run ? act(icon('play') + 'Run', `call('run_step', {pid: '${p.id}', step: 'run'}); ${close}`, 'primary') : '')
            : act(icon('hammer') + 'Build again', `call('run_step', {pid: '${p.id}', step: 'build'}); ${close}`, 'primary')}</div>`).join('')}` : ''}`; };
    card.innerHTML = card._h = bhtml();
    const iv = setInterval(() => { if (!card.isConnected) return clearInterval(iv); const h = bhtml(); if (card._h !== h) card.innerHTML = card._h = h; }, 1000);
  }
  document.body.append(card); card.showPopover(); shutOn(card, kind); drawTimes();
  card.addEventListener('toggle', e => e.newState === 'closed' && card.remove());
  const q = id => card.querySelector('#' + id);
  if (q('scLim')) { q('scLim').oninput = e => { e.target.title = limTip(e.target.value); q('scLimV').textContent = '· hand over ' + (+e.target.value ? e.target.value + '%' : 'off'); };
    q('scLim').onchange = e => { q('scLimV').textContent = ''; setP({context_limit: Math.max(0, Math.min(95, +e.target.value || 0))}).then(refresh); };
    q('scReset').onclick = () => { TIMES[cur] = {start: Date.now(), you: 0, ai: 0, any: 0}; drawTimes(); };
    q('scAcct').onclick = e => { const b = e.currentTarget.getBoundingClientRect(); card.hidePopover(); menuAt({getBoundingClientRect: () => b}, acctItems()); }; }
  card.style.left = Math.max(8, Math.min(r.left, innerWidth - card.offsetWidth - 8)) + 'px'; card.style.top = Math.max(8, r.top - card.offsetHeight - 8) + 'px';
}
