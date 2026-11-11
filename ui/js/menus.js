// ---- popover menus ----
let MENU_A = null, MENU_T = 0, MENU_TUI = false; // #218 anchor of the open menu: clicking it again closes (outside mousedown already did) instead of reopening
function menuAt(anchor, items) {
  if (anchor === MENU_A && Date.now() - MENU_T < 400) return;
  closeMenu(); MENU_A = anchor; MENU_TUI = !!anchor.closest?.('#dock') && $('log').classList.contains('term'); // #312 composer popovers in term view = text-mode
  buildMenu(items, anchor.getBoundingClientRect(), false);
  setTimeout(() => document.addEventListener('mousedown', outside), 0);
}
// item: '-' | {and: text} (divider with a word) | {label, hint, icon, check, run, sub (indented), title (caption, not clickable), items (› submenu on hover)}
function buildMenu(items, r, side, level = 0) {
  document.querySelectorAll('.menu').forEach(x => +x.dataset.level >= level && x.remove());
  const m = document.createElement('div');
  m.className = MENU_TUI ? 'menu tui' : 'menu glass'; m.dataset.level = level; m.popover = 'manual'; // top layer, opened last → over Settings/dialogs
  m.innerHTML = items.map((it, i) => it === '-' ? '<hr>' : it.and ? `<div class="mand">${esc(it.and)}</div>` : it.title ? `<div class="mcap">${esc(it.title)}</div>` : `<button data-i="${i}"${it.sub ? ' class="sub"' : ''}${it.off ? ' disabled' : ''}>${it.check !== undefined ? `<span class="chk">${it.check ? icon('check') : ''}</span>${it.icon ? icon(it.icon) : ''}` : it.icon ? icon(it.icon) : ''}
    <span class="grow">${esc(it.label)}${it.hint ? `<small>${esc(it.hint)}</small>` : ''}</span>${(it.acts || []).map((a, j) => `<span class="mact${a.label ? ' lab' : ''}" data-a="${j}" title="${esc(a.tip)}">${icon(a.icon)}${a.label ? `${esc(a.label)}${icon('down')}` : ''}</span>`).join('')}${it.items ? `<span class="mchev">${icon('chev')}</span>` : ''}</button>`).join('');
  (document.querySelector('dialog:modal') || document.body).appendChild(m); m.showPopover(); // inside a modal: else inert
  m.style.maxHeight = innerHeight - 16 + 'px'; m.style.overflowY = 'auto';
  const h = m.offsetHeight, w = m.offsetWidth;
  // below the anchor, else above; neither fits → beside it (left when the anchor is in the right half, else right),
  // so the menu never lands under the pointer; always kept 8px inside the window
  const below = r.bottom + 6 + h <= innerHeight - 8, above = r.top - 6 - h >= 8;
  if (!side && r.left === r.right && r.top === r.bottom) { // right-click point: a corner sits on the pointer, flipped near an edge
    const fl = r.left + w > innerWidth - 8, up = r.top + h > innerHeight - 8 && r.top - h >= 8;
    m.style.left = Math.max(8, fl ? r.left - w : r.left) + 'px';
    m.style.top = Math.max(8, Math.min(up ? r.top - h : r.top, innerHeight - h - 8)) + 'px';
    m.classList.add(up ? 'sup' : 'sdown');
  } else if (side || (!below && !above)) {
    const left = side ? r.right + w + 4 > innerWidth - 8 : (r.left + r.right) / 2 > innerWidth / 2;
    m.style.left = Math.max(8, left ? r.left - w - 4 : Math.min(r.right + 4, innerWidth - w - 8)) + 'px';
    m.style.top = Math.max(8, Math.min(side ? r.top - 5 : r.top + r.height / 2 - h / 2, innerHeight - h - 8)) + 'px';
  } else {
    m.style.left = Math.max(8, Math.min(r.left, innerWidth - w - 8)) + 'px';
    m.style.top = (below ? r.bottom + 6 : r.top - h - 6) + 'px';
  }
  if (!level) MENU_A?.classList?.toggle('mup', !side && !below && above); // #450 chevron flips when the menu sits above
  m.querySelectorAll('[data-i]').forEach(b => { const it = items[b.dataset.i];
    b.onmouseenter = () => it.items ? buildMenu(it.items, b.getBoundingClientRect(), true, level + 1) : document.querySelectorAll('.menu').forEach(x => +x.dataset.level > level && !x.dataset.click && x.remove()); // click-opened: stays till a click/another submenu
    // acts = click-only dropdowns (hover ones overlapped each other); labelled act → opens below it like a select
    b.onclick = e => { const a = e.target.closest('.mact'); if (a) return buildMenu(it.acts[a.dataset.a].items, a.getBoundingClientRect(), !it.acts[a.dataset.a].label, level + 1).dataset.click = 1;
      if (it.run) { closeMenu(); it.run(); } else if (it.items) buildMenu(it.items, b.getBoundingClientRect(), true, level + 1); }; });
  return m;
}
const outside = e => { if (!e.target.closest('.menu')) closeMenu(); };
let CTXP = null, CTXD = false; // #390 pending editable right-click / Rust holds a spelling deferral (closed w/o pick → release it)
function closeMenu() { if (CTXD) setTimeout(() => CTXD && (CTXD = false, call('ctx_pick', {id: -1}))); if (document.querySelector('.menu')) MENU_T = Date.now(); document.querySelectorAll('.menu').forEach(x => x.remove()); document.querySelectorAll('.mup').forEach(x => x.classList.remove('mup')); document.removeEventListener('mousedown', outside); }
// #450 every <select> opens the app menu, never the native square popup; pick → value + 'change' (inline onchange keeps working)
// optgroup → caption + indented items; keyboard ↑↓ still steps the value natively (no popup)
function selMenu(s) { const o2i = o => ({label: o.text, check: o.selected, off: o.disabled, sub: o.parentElement.tagName === 'OPTGROUP', run: () => { s.value = o.value; s.dispatchEvent(new Event('change', {bubbles: true})); }});
  const it = [...s.children].flatMap(c => c.tagName === 'OPTGROUP' ? [{title: c.label}, ...[...c.children].map(o2i)] : c.hidden ? [] : [o2i(c)]);
  s.focus(); menuAt(s, it); const m = document.querySelector('.menu'); if (m) m.style.minWidth = s.offsetWidth + 'px'; }
document.addEventListener('mousedown', e => { const s = e.target.closest?.('select:not([multiple],[size])'); if (!s || e.button || s.disabled) return; e.preventDefault(); if (MENU_A === s && document.querySelector('.menu')) return closeMenu(); selMenu(s); }, true);
document.addEventListener('keydown', e => { const s = e.target; if (s.tagName !== 'SELECT' || s.multiple || !(e.key === ' ' || e.key === 'Enter' || e.altKey && /^Arrow(Down|Up)$/.test(e.key) || e.key === 'F4')) return; e.preventDefault(); selMenu(s); }, true);
// right-click: own menu on chat msgs, tool rows, Files tree, projects; WebView default only in inputs / over a selection
const copy = t => navigator.clipboard.writeText(t);
const absOf = rel => { const b = P().path.replace(/[\\/]+$/, ''), s = b.includes('\\') ? '\\' : '/'; return b + s + rel.replace(/\//g, s); };
function quote(t) { const ta = $('prompt'); ta.value = (ta.value.trim() ? ta.value.replace(/\s*$/, '\n\n') : '') + t.trim().replace(/^/gm, '> ') + '\n\n';
  ta.dispatchEvent(new Event('input')); ta.focus(); }
document.addEventListener('contextmenu', e => {
  const t = e.target, sel = String(getSelection()).trim(), at = items => { e.preventDefault(); menuAt({getBoundingClientRect: () => ({left: e.clientX, right: e.clientX, top: e.clientY, bottom: e.clientY, height: 0})}, items); };
  // A row with a "more" button: right-click opens that same menu
  const more = t.closest('.att')?.querySelector('.amore');
  if (more) { e.preventDefault(); more.click(); return; }
  const ed = t.closest('input:not([type=checkbox],[type=radio],[type=range],[type=button],[type=file]), textarea, [contenteditable]:not([contenteditable=false])');
  if (ed) { // #253 own Cut/Copy/Paste/Select all; menu click steals focus → refocus + restore the saved selection before each edit
    const ce = !('value' in ed), ro = ed.readOnly || ed.disabled, pw = ed.type === 'password';
    let s = 0, en = 0; try { s = ed.selectionStart; en = ed.selectionEnd; } catch {} // number/email inputs throw
    const rg = ce && getSelection().rangeCount ? getSelection().getRangeAt(0).cloneRange() : null;
    const txt = ce ? sel : (ed.value || '').slice(s, en), back = () => { ed.focus(); if (rg) { getSelection().removeAllRanges(); getSelection().addRange(rg); } else try { ed.setSelectionRange(s, en); } catch {} };
    const its = [txt && !ro && !pw && {icon: 'x', label: 'Cut', run: () => { copy(txt); back(); document.execCommand('delete'); }},
      txt && !pw && {icon: 'copy', label: 'Copy', run: () => copy(txt)},
      !ro && {icon: 'clip', label: 'Paste', run: () => navigator.clipboard.readText().then(v => { back(); document.execCommand('insertText', false, v); }).catch(err => toast('Paste blocked: ' + err, 'err'))},
      {icon: 'list', label: 'Select all', run: () => { ed.focus(); ce ? document.execCommand('selectAll') : ed.select(); }}].filter(Boolean);
    if (HOST !== 'windows' || ro || pw) return at(its);
    // #390 Windows: let WebView2 see it → Rust ctxmenu event brings spelling items (shown on top); ≤300ms or our plain menu
    clearTimeout(CTXP?.t); const p = CTXP = {show: sp => { CTXP = null; at(sp.length ? [...sp.map(s => ({icon: /dictionar/i.test(s.l) ? 'plus' : 'check', label: s.l,
      run: () => { CTXD = 0; back(); call('ctx_pick', {id: s.id}); }})), '-', ...its] : its); CTXD = sp.length > 0; }};
    p.t = setTimeout(() => CTXP === p && p.show([]), 300); return;
  }
  const a = t.closest('a[href], [data-url]'), url = a && (a.dataset.url || a.href), img = t.closest('img');
  const media = [url && !/^(javascript|#)/.test(a.getAttribute('href') || '') && {icon: 'globe', label: 'Open link', run: () => T.opener.openUrl(url)}, url && {icon: 'copy', label: 'Copy link', run: () => copy(url)},
    img && {icon: 'image', label: 'Copy image', run: () => fetch(img.src).then(r => r.blob()).then(b => createImageBitmap(b)).then(bm => { const c = document.createElement('canvas'); c.width = bm.width; c.height = bm.height; c.getContext('2d').drawImage(bm, 0, 0);
      return new Promise(r => c.toBlob(r)); }).then(b => navigator.clipboard.write([new ClipboardItem({'image/png': b})])).then(() => toast('Image copied', 'ok'), err => toast('Copy failed: ' + err, 'err'))}, // → png ∵ clipboard only takes image/png
    img && /^https?:/.test(img.src) && {icon: 'doc', label: 'Copy image address', run: () => copy(img.src)}].filter(Boolean);
  let items;
  const msg = t.closest('.msg'), tool = t.closest('.tl > div'), file = t.closest('#files .tree > div[data-p]'), proj = t.closest('.proj[data-id]'), vt = t.closest('.vtext');
  if (vt) items = [sel && {icon: 'copy', label: 'Copy', run: () => copy(sel)}, // #282 file viewer: Select all = its text only (not the whole page)
    {icon: 'list', label: 'Select all', run: () => { const r = document.createRange(); r.selectNodeContents(vt); getSelection().removeAllRanges(); getSelection().addRange(r); }}].filter(Boolean);
  else if (tool?._es) { const d = tool._es[0].detail || {}, f = d.file_path || d.notebook_path || d.path;
    items = [typeof d.command === 'string' && {icon: 'doc', label: 'Copy command', run: () => copy(d.command)},
      f && {icon: 'doc', label: 'Copy path', run: () => copy(f)}, f && {icon: 'file', label: 'Open file', run: () => openFile(f)},
      {icon: 'term', label: 'Details', run: () => tool.click()}].filter(Boolean); }
  else if (msg) { const txt = sel || msg.innerText;
    items = [{icon: 'doc', label: sel ? 'Copy selection' : 'Copy message', run: () => copy(txt)}, {icon: 'reply', label: 'Quote in prompt', run: () => quote(txt)}]; }
  else if (file) { const p = file.dataset.p, dir = /^filesGo/.test(file.getAttribute('onclick'));
    items = [{icon: dir ? 'folder' : 'file', label: 'Open', run: () => file.click()}, {icon: 'folder', label: 'Show in folder', run: () => T.opener.revealItemInDir(absOf(p)).catch(alert)},
      {icon: 'doc', label: 'Copy path', run: () => copy(absOf(p))}, {icon: 'doc', label: 'Copy relative path', run: () => copy(p)}]; }
  else if (proj) { const id = proj.dataset.id;
    items = [{icon: 'folder', label: 'Open', run: () => openProject(id)}, {icon: 'gear', label: 'Settings', run: () => openManager(id)},
      {icon: 'folder', label: 'Show in folder', run: () => T.opener.revealItemInDir(P(id).path).catch(alert)}, '-',
      {icon: 'x', label: 'Remove from Clauracle', run: () => { mp = id; removeProject(); }}]; }
  if (!items && sel) items = [{icon: 'copy', label: 'Copy', run: () => copy(sel)}];
  // #404 empty space in a tab body → that tab's menu (same as its tab button / … button)
  if (!items && !media.length) items = t.closest('#rbody') && cur && TABREF[tab] ? tabItems(tab) : t.closest('#files') ? sideItems()
    : t.closest('#projects') ? [...addItems(), '-', {icon: 'folder', label: 'Project manager…', run: () => openManager()}] : null;
  items = [...media, ...(media.length && items ? ['-'] : []), ...(items || [])];
  items.length ? at(items) : e.preventDefault(); // nowhere useful → no menu (never the WebView's)
});
const addItems = () => [{icon: 'plus', label: 'New project', hint: 'Local for now; publish to GitHub later', run: newLocal},
  {icon: 'folder', label: 'Open folder', run: addProject}, {icon: 'gh', label: 'Clone from GitHub', run: openClone}];
function newLocal() {
  dlg(`<h2>New project${infoI('A folder in ' + S.projects_dir + '/local, only on this computer. Publish it to GitHub any time from the GitHub tab.')}</h2>
    <input id="nlName" style="width:100%" placeholder="Name" onkeydown="if(event.key==='Enter')makeLocal()"><p id="nlMsg" class="err" style="font-size:13px"></p>
    <div class="btns"><button class="btn ghost" onclick="closeDlg()">Cancel</button><button class="btn primary" onclick="makeLocal()">Create</button></div>`);
  $('nlName').focus();
}
async function makeLocal() {
  try { const p = await call('new_local', {name: $('nlName').value}); closeDlg(); await refresh(); await openProject(p.id); askProject(P(p.id)); }
  catch (e) { $('nlMsg').textContent = e; }
}
/* Composer › Skills: Clauracle's own picks on top (Ponytail = the project's switch; Impeccable = one-click install from its
   source, Apache-2.0 by Paul Bakaus, so it stays current), then one folder per source for every other skill.
   A tick = on for this project. Own skills with a shared first word (hyperframes-…) get a folder of their own. */
// kind + name = how the catalog lists it; a, b = ext_add arguments (skill: repo, skill id; mcp: server name, npm package).
// auto = installs itself on first run; the rest install on a click (Godot only makes sense with Godot on the machine).
// sw = a project switch the row flips instead of the allow-list (Ponytail is added to every prompt while on); more = skills installed along with it.
const APPSKILLS = [
  {kind: 'skill', name: 'ponytail', label: 'Ponytail', icon: 'feather', hint: 'The simplest code that works', a: 'DietrichGebert/ponytail', b: 'ponytail', auto: 1, sw: 'ponytail',
    more: ['ponytail-audit', 'ponytail-debt', 'ponytail-gain', 'ponytail-help', 'ponytail-review']},
  {kind: 'skill', name: 'impeccable', label: 'Impeccable', icon: 'brush', hint: 'Design polish, critique and audit', a: 'pbakaus/impeccable', b: 'impeccable', auto: 1},
  {kind: 'skill', name: 'verification-before-completion', label: 'Verify before done', icon: 'check2', hint: 'Superpowers: run the check before saying it works', a: 'obra/superpowers', b: 'verification-before-completion', auto: 1},
  {kind: 'mcp', name: 'playwright', label: 'Playwright', icon: 'monitor', hint: 'Opens the site it built, clicks through it, takes screenshots', a: 'playwright', b: '@playwright/mcp@latest', auto: 1},
  {kind: 'mcp', name: 'context7', label: 'Context7', icon: 'doc', hint: 'Current library docs on demand', a: 'context7', b: '@upstash/context7-mcp@latest', auto: 1},
  {kind: 'mcp', name: 'godot', label: 'Godot', icon: 'play', hint: 'Run and debug Godot projects', a: 'godot', b: '@coding-solo/godot-mcp@latest'}];
const appItem = a => SET.catalog.find(i => i.kind === a.kind && i.label === a.name); // the one installed from its source, never a copy inside a plugin
const skillOn = (p, id) => onOf(p).includes(id);
async function skillSet(ids, on) { const p = P(), items = on ? [...new Set([...onOf(p), ...ids])] : onOf(p).filter(x => !ids.includes(x));
  p.on = items; await call('set_items', {scope: 'project:' + p.id, items}); drawChrome(); }
async function skillGet(a) { toast(`Installing ${a.label}…`, 'info');
  try { const before = new Set(SET.catalog.map(c => c.id)); for (const b of [a.b, ...(a.more || [])]) await call('ext_add', {kind: a.kind, a: a.a, b});
    SET.catalog = await call('refresh_catalog'); const add = SET.catalog.map(c => c.id).filter(id => !before.has(id));
    if (a.sw) await setP({[a.sw]: true}); else if (add.length) await skillSet(add, true); toast(`${a.label} installed, on for ${P().name}`, 'ok'); }
  catch (e) { toast(`${a.label}: ${e}`, 'err'); } }
function skillItems() { const p = P(), last = i => i.label.split(':').pop(), sk = SET.catalog.filter(i => i.kind === 'skill' && !SET.off.includes(i.id));
  const row = i => ({check: skillOn(p, i.id), label: last(i), run: () => skillSet([i.id], !skillOn(p, i.id))});
  const own = APPSKILLS.map(a => { const i = appItem(a), sw = a.sw && {check: !!p[a.sw], run: () => setP({[a.sw]: !p[a.sw]})};
    return i && !SET.off.includes(i.id) ? {...row(i), ...sw, icon: a.icon, label: a.label, hint: a.hint} : {check: false, icon: 'plus', label: a.label, hint: a.hint + '. Click to install', run: () => skillGet(a)}; });
  const rest = sk.filter(i => !builtin(i.id) && !APPSKILLS.some(a => a.kind === 'skill' && last(i) === a.name)), stem = i => i.label.split('-')[0];
  const folder = i => i.label.includes(':') ? i.label.split(':')[0] : rest.filter(x => !x.label.includes(':') && stem(x) === stem(i)).length > 1 ? stem(i) : 'Your skills';
  const groups = {}; rest.forEach(i => (groups[folder(i)] ||= []).push(i));
  return [{title: 'Clauracle'}, ...own, ...(rest.length ? ['-'] : []),
    ...Object.keys(groups).sort((a, b) => (a === 'Your skills') - (b === 'Your skills') || a.localeCompare(b))
      .map(k => ({check: false, icon: 'folder', label: k, hint: `${groups[k].filter(i => skillOn(p, i.id)).length} of ${groups[k].length} on`, items: groups[k].map(row)})),
    '-', {check: false, icon: 'search', label: 'Find more skills…', run: () => openSettings('cx').then(() => jumpTo('[data-f=g-find]'))}]; }
async function skillMenu(btn) { if (!cur) return; SET ||= await call('settings'); menuAt(btn, skillItems()); }
const contItems = () =>Object.entries(MODES).map(([k, [l, h]]) => ({check: P().cont === k, label: l, hint: h, run: () => setP({cont: k})}));

async function refresh() {
  S = await call('state');
  const first = !draftsLoaded;
  if (first) { // unsent input from before a close, crash or restart
    draftsLoaded = true;
    for (const [pid, d] of Object.entries(S.drafts || {})) { texts[pid] = d.text || ''; atts[pid] = d.atts || []; queue[pid] = [...(d.filing || []), ...(d.queue || [])] /* filing = sent mid-gh-call when the app died: no note ∴ Clauracle files it (rules) */; if (d.editing) editing[pid] = d.editing; }
    for (const id of S.busy) busy[id] = true; // first load only: a later refresh resolving after status busy:false re-set it → stuck Working, queue never flushed
  }
  drawProjects();
  if (P()) drawChrome(); else drawHome();
  if (first) Object.keys(queue).forEach(flushQueue); // held across a restart with no resumed turn: busy:false never comes → send now (after drawChrome: idle bar known)
}
// Claude Code's crab, on the home screen and on every fresh chat
// mascots, one per AI account vibe: pixel grids drawn as whole-pixel SVG rects (crisp at any zoom; text glyphs left seams)
const MASCOTS = [
  ['#7c83ff', ['..#........#..', '..##########..', '..###.##.###..', '##############', '...#.#..#.#...']],
  ['var(--crab)', ['...############...', '...##.######.##...', '.################.', '...############...', '....#.#....#.#....']],
  ['#3fbf9f', ['....######....', '..##########..', '.###.####.###.', '##############', '##.###..###.##']],
];
const pix = ([c, rows], k = 6) => `<svg width="${rows[0].length * k}" height="${rows.length * k}" shape-rendering="crispEdges" style="fill:${c}">${rows.flatMap((r, y) =>
  [...r.matchAll(/#+/g)].map(m => `<rect x="${m.index * k}" y="${y * k}" width="${m[0].length * k}" height="${k}"/>`)).join('')}</svg>`;
const CRAB = `<div class="crab">${MASCOTS.map(m => pix(m)).join('')}</div>`;
const recent = () => [...S.projects].sort((a, b) => b.used - a.used);
// home: Active = working now or finished < 1 min ago (cooldown), then back to Recent; the Working… state shows on its row
const lastBusy = {}, ACTIVE_MS = 60000, isActive = p => busy[p.id] || Date.now() - (lastBusy[p.id] || 0) < ACTIVE_MS;
const actRow = p => projRow(p).replace(/<\/div>\s*$/, `<span class="pill${busy[p.id] ? '' : ' dim'}">${busy[p.id] ? 'Working…' : 'Done'}</span></div>`);
function drawHome() {
  const box = $('log').querySelector('.col'), act = recent().filter(isActive);
  $('skel').hidden = true; // the Working… skeleton belongs to an open project, not home
  clearTimeout(drawHome.t); if (act.some(p => !busy[p.id])) drawHome.t = setTimeout(() => !cur && drawHome(), 5000);
  box.innerHTML = `<div id="empty">${CRAB}<h2 class="brandname" style="color:var(--text);margin:0 0 6px;font-size:30px">Claur<span style="color:var(--accent)">acle</span></h2>
    <p>${S.projects.length ? 'Pick up where you left off, or start something new.' : 'Open a folder or clone a GitHub repo. Each project keeps its own GitHub account, AI account, connections, build and .env.'}</p>
    <p class="row" style="justify-content:center"><button class="btn primary" onclick="newLocal()">${icon('plus')}New project</button><button class="btn" onclick="addProject()">${icon('folder')}Open folder</button><button class="btn" onclick="openClone()">${icon('gh')}Clone from GitHub</button></p>
    ${act.length ? `<div class="recent"><div class="label">Active</div>${act.map(actRow).join('')}</div>` : ''}
    ${S.projects.length > act.length ? `<div class="recent"><div class="label">Recent</div>${recent().filter(p => !isActive(p)).slice(0, 6).map(projRow).join('')}</div>` : ''}
    <p id="missing"></p></div>`;
  $('missing').innerHTML = [!S.claude && `Claude Code isn't installed. <button class="btn" onclick="install('claude', this)">Install Claude Code</button>`,
    !S.gh && `GitHub features need the GitHub CLI. <button class="btn" onclick="install('gh', this)">Install gh</button>`].filter(Boolean).join('<br><br>');
}
function goHome() { cur = null; drawSide(); $('head').hidden = $('dock').hidden = $('right').hidden = true; drawProjects(); drawHome(); }
async function install(what, btn) {
  btn.disabled = true; btn.textContent = 'Installing…';
  try { await call('install', {what}); } catch (e) { btn.textContent = 'Failed: ' + e; return false; }
  refresh();
  return true;
}

const projRow = p => `<div class="proj ${p.id === cur ? 'on' : ''}" data-id="${p.id}" onclick="openProject('${p.id}')" title="${esc(p.path)}">
    <span class="dot ${busy[p.id] ? 'busy' : p.ready ? 'ready' : ''}"></span><span class="grow" style="min-width:0"><div class="name">${esc(p.name)}</div>
    <div class="who">${esc(nick(p.account))}${p.gh ? ' · @' + esc(p.gh) : ''}${p.shared ? ' · ' + esc(p.owner) + "'s" : ''}</div></span>
    ${p.questions.length ? `<span class="pill">${p.questions.length}?</span>` : ''}</div>`;
