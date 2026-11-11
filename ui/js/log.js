// ---- branches: header pill → switch / new (type/name) / push, as the project's gh account ----
const BTYPES = ['feature', 'fix', 'chore', 'docs', 'refactor', ''];
async function branchMenu(el) {
  const b = await call('git_do', {pid: cur, action: 'list', arg: ''});
  menuAt(el, [...b.branches.map(n => ({check: n === b.current, label: n, run: () => n !== b.current && gitDo('switch', n)})), '-',
    {icon: 'plus', label: 'New branch…', hint: 'feature/, fix/, chore/…', run: newBranch},
    {icon: 'up', label: `Push ${b.current}`, hint: P().gh ? `to GitHub as @${P().gh}` : 'to origin', run: () => gitDo('push', '')}]);
}
async function gitDo(action, arg) {
  if (busy[cur] && action !== 'push') return addEntry(cur, {kind: 'err', text: 'Wait for the turn to finish before switching branches.'});
  const pid = cur;
  try { await call('git_do', {pid, action, arg}); addEntry(pid, {kind: 'note', text: action === 'push' ? 'Pushed' : `On branch ${arg}`}); }
  catch (e) { addEntry(pid, {kind: 'err', text: String(e)}); }
  const b = await call('git_do', {pid, action: 'list', arg: ''});
  if (pid === cur) $('branch').textContent = b.current;
}
function newBranch() {
  dlg(`<h2>New branch</h2><p class="row"><select id="nbType">${BTYPES.map(t => `<option value="${t}">${t ? t + '/' : '(no prefix)'}</option>`).join('')}</select>
    <input id="nbName" class="grow" placeholder="short-name" onkeydown="if(event.key==='Enter')makeBranch()"></p>
    <div class="btns"><button class="btn ghost" onclick="closeDlg()">Cancel</button><button class="btn primary" onclick="makeBranch()">Create and switch</button></div>`);
  $('nbName').focus();
}
function makeBranch() {
  const slug = $('nbName').value.trim().toLowerCase().replace(/[^a-z0-9._/-]+/g, '-').replace(/^[-/.]+|[-/.]+$/g, '');
  const t = $('nbType').value;
  if (!slug) return;
  closeDlg(); gitDo('new', (t ? t + '/' : '') + slug);
}

// ---- the log: chat or terminal ----
let view = (() => { try { return localStorage.view || 'chat'; } catch { return 'chat'; } })();
function setView(v) { view = v; try { localStorage.view = v; } catch {} applyView(); if (cur) drawLog(); }
function applyView() { $('log').classList.toggle('term', view !== 'chat'); requestAnimationFrame(() => { grow(); termPad(); }); document.querySelectorAll('#head .seg button').forEach(b => b.classList.toggle('on', b.dataset.v === view));
  panelLay(); if (view === 'edit') { evDraw(); if (UI.pt === 'shell' && !UI.pc && cur) shellShow(); } }
/* #285 Edit view. Tabs per project: ETABS[pid] = {tabs: [{rel|null, name, text, orig, img, err}], on}; open rels persist (localStorage etabs:<pid>).
   ponytail: plain textarea (no highlight/line numbers); Untitled tabs not persisted. Panel: UI.ph px, UI.pc collapsed, UI.pt term|shell */
function panelLay() { const m = $('main'), ed = view === 'edit' && !!cur;
  m.classList.toggle('edit', ed); m.classList.toggle('shell', ed && UI.pt === 'shell'); m.classList.toggle('pc', ed && !!UI.pc);
  const ph = UI.ph ?? 260; m.style.setProperty('--ph', (ph < 8 ? ph : 8 + Math.floor((ph - 8) / 20) * 20) + 'px'); /* #426 log = 8px pad-top + whole 20px rows ∴ scrolled-up rows never cut at the bottom; ponytail: flex-shrunk log (tiny window) stays fractional */
  document.querySelectorAll('#ptabs > button[data-k]').forEach(b => b.classList.toggle('on', b.dataset.k === (UI.pt || 'term') && !UI.pc));
  try { localStorage.ui = JSON.stringify(UI); } catch {} }
function panelTab(k) { UI.pc = false; UI.pt = k; applyView(); }
const ETABS = {};
function etabs(pid = cur) { if (!ETABS[pid]) { let o = {}; try { o = JSON.parse(localStorage['etabs:' + pid] || '{}'); } catch {}
  const tabs = (o.rels || []).map(rel => ({rel, name: rel.split('/').pop()})); ETABS[pid] = {tabs, on: Math.max(0, tabs.findIndex(t => t.rel === o.on))}; } return ETABS[pid]; }
function evKeep(pid = cur) { const E = ETABS[pid]; if (E) try { localStorage['etabs:' + pid] = JSON.stringify({rels: E.tabs.filter(t => t.rel).map(t => t.rel), on: E.tabs[E.on]?.rel}); } catch {} }
/* #435 flatten marks (box note text not kept: numbers only) → same format; gif/bmp can't be encoded by canvas. ponytail: bytes as JSON array, fine ≤ few MB */
async function evSaveImg(t) { if (ED?.tab !== t) return; const ty = {png: 'png', jpg: 'jpeg', jpeg: 'jpeg', webp: 'webp'}[t.rel.split('.').pop().toLowerCase()];
  if (!ty) return toast('Only PNG, JPG and WebP can be saved', 'warn');
  ED.sel = []; edDraw(true); const bl = await new Promise(r => $('edC').toBlob(r, 'image/' + ty, .92)); edDraw();
  try { await call('write_bytes', {pid: cur, rel: t.rel, data: [...new Uint8Array(await bl.arrayBuffer())]}); } catch (e) { return toast(String(e), 'err'); }
  Object.assign(t, {img: URL.createObjectURL(bl), ed: null, dirty: 0}); ED = null; $('ebody')._t = null; evDraw(true); toast('Saved ' + t.name, 'ok'); }
const evDirty = t => !!t.dirty || (t.text !== undefined && t.text !== t.orig);
function evOpen(rel) { const E = etabs(); let i = E.tabs.findIndex(t => t.rel === rel); if (i < 0) i = E.tabs.push({rel, name: rel.split('/').pop()}) - 1; E.on = i; evKeep(); evDraw(); }
function evNew() { const E = etabs(), n = Math.max(0, ...E.tabs.map(t => +(t.name.match(/^Untitled-(\d+)$/)?.[1] || 0))) + 1;
  E.on = E.tabs.push({rel: null, name: 'Untitled-' + n, text: '', orig: ''}) - 1; evDraw(); }
function evPick(i) { etabs().on = i; evKeep(); evDraw(); }
async function evClose(i) { const E = etabs(), t = E.tabs[i]; if (!t) return;
  if (evDirty(t) && !await sure(`Close ${t.name} without saving?`, 'Discard changes', 'Cancel', undefined, true)) return;
  E.tabs.splice(i, 1); if (E.on >= i && E.on > 0) E.on--; if ($('ebody')._t === t) $('ebody')._t = null; evKeep(); evDraw(); }
async function evLoad(t, pid) { const abs = P(pid).path.replace(/[\\/]+$/, '') + '/' + t.rel;
  try { if (/\.(png|jpe?g|gif|webp|bmp)$/i.test(t.rel)) t.img = URL.createObjectURL(new Blob([await call('read_image', {path: abs})]));
    else t.text = t.orig = await call('read_file', {pid, rel: t.rel}); } catch (e) { t.err = String(e); }
  if (pid === cur) evDraw(); }
async function evSave(t) { if (!t) return;
  if (t.img) return evSaveImg(t);
  try { if (t.rel) await call('write_file', {pid: cur, rel: t.rel, text: t.text});
    else { const to = await T.dialog.save({defaultPath: P().path.replace(/[\\/]+$/, '') + '/' + t.name + '.txt'}); if (!to) return;
      await call('save_as', {to, from: null, text: t.text}); const base = P().path.replace(/\\/g, '/').replace(/\/$/, '') + '/', f = to.replace(/\\/g, '/');
      t.name = f.split('/').pop(); if (f.toLowerCase().startsWith(base.toLowerCase())) { t.rel = f.slice(base.length); evKeep(); } }
    t.orig = t.text; evDraw(); evLint(t, 1); toast('Saved ' + t.name, 'ok'); } catch (e) { toast(String(e), 'err'); } }
function evDraw(re) { if (view !== 'edit' || !cur) return;
  const E = etabs(), t = E.tabs[E.on], b = $('ebody');
  $('etabs').innerHTML = E.tabs.map((x, i) => `<div class="et${i === E.on ? ' on' : ''}${evDirty(x) ? ' dirty' : ''}" title="${esc(x.rel || x.name)}${x.rel ? ' · Ctrl+click: also open in the side Code tab' : ''}" onclick="(event.ctrlKey || event.metaKey) && ${!!x.rel} ? previewFile(${esc(JSON.stringify(x.rel || ''))}, true) : evPick(${i})" onauxclick="if (event.button === 1) evClose(${i})">${icon(/\.(png|jpe?g|gif|webp|bmp)$/i.test(x.rel || '') ? 'image' : 'file')}${esc(x.name)}<button class="ib x" title="Close" onclick="event.stopPropagation(); evClose(${i})">${icon('x')}</button></div>`).join('')
    + `<button class="ib add" title="New tab" onclick="evNew()">${icon('plus')}</button>`;
  evBar(t); b.style.zoom = t?.img ? '' : UI.ez || 1;
  const ta = b.querySelector('.ecode textarea'); if (ta && b._t) Object.assign(b._t, {ss: ta.selectionStart, se: ta.selectionEnd, sy: b.firstChild.scrollTop}); // caret + scroll survive tab switches
  if (ta && b._t === t && !re) return;
  if (t?.img && b._t === t && ED?.tab === t && !re) return;
  edStash(); b._t = null;
  if (!t) { b.innerHTML = empty('No files open', `<button class="btn sm" onclick="evNew()">${icon('plus')}New file</button><button class="btn sm ghost" onclick="UI.lc = false; setSide('files')">${icon('folder')}Open from Files</button>`); return; }
  if (t.err) { b.innerHTML = empty(esc(t.err)); return; }
  if (t.img) { b._t = t; /* #435 image tab = File viewer's editor: context bar + side tools; Save writes the flattened image back */
    b.innerHTML = `<div class="evimg"><div class="vtools">${edToolsHtml()}</div><div class="vstage"><div id="edWrap"><canvas id="edC"></canvas></div></div></div>`;
    const S = b.querySelector('.vstage'); new ResizeObserver(() => { if (ED?.tab === t) edNotes(); }).observe(S); vWire(S, 'img');
    edInit(t.img, t.ed, t); return; }
  if (t.text === undefined) { b.innerHTML = `<div style="flex:1;padding:14px">${skel(60)}</div>`; if (!t.loading) { t.loading = 1; evLoad(t, cur); } return; }
  if (evFm(t)) { let h; try { h = evPv(t) === 'md' ? `<div class="mdoc">${mdDoc(t.text)}</div>` : codeHtml(JSON.stringify(JSON.parse(t.text), null, 2), 'json', !!UI.ew).replace('class="code', 'class="code' + (hlOn() ? '' : ' plain')); } catch (e) { h = `<div class="dim">${esc(String(e))}</div>`; }
    b.innerHTML = `<div class="epv">${h}</div>`; return; } /* #372 read-only preview; edits = View › Preview off */
  // #329 Code-tab look: highlighted .code pre (codeHtml) under a transparent textarea, both sized by the pre ∴ one scroller, no scroll sync
  const ext = (t.rel || t.name).split('.').pop().toLowerCase(), hi = () => { b.querySelector('pre')?.remove(); b.querySelector('.ew').insertAdjacentHTML('afterbegin', codeHtml(t.text, ext).replace(/^<div[^>]*>|<\/div>$/g, '')); evMarks(t, 0); }; /* --lw on .code (CSS) ∴ textarea pads past the gutter */
  b.innerHTML = '<div class="code ecode' + (UI.ew ? ' wrap' : '') + (hlOn() ? '' : ' plain') + '"><div class="ew"><textarea spellcheck="false" autocomplete="off"></textarea></div></div>'; b._t = t;
  const a = b.querySelector('textarea'); a.value = t.text; hi();
  let raf = 0; // ponytail: whole-file re-highlight per frame; fine ≤ ~5k lines, incremental per line if big files lag
  a.oninput = () => { t.text = a.value; $('etabs').querySelector('.et.on')?.classList.toggle('dirty', evDirty(t)); cancelAnimationFrame(raf); raf = requestAnimationFrame(hi); clearTimeout(t.ltm); t.ltm = setTimeout(() => evLint(t), 700); };
  a.onmousemove = e => { let tip = ''; const rows = b.querySelector('pre')?.children; /* pre = pointer-events:none ∴ hover tip via row rects */
    for (const x of UI.nl ? [] : t.lint || []) { const r = rows?.[x.l - 1]?.getBoundingClientRect(); if (r && e.clientY >= r.top && e.clientY < r.bottom) tip += (tip ? '\n' : '') + (x.s === 'e' ? 'Error: ' : 'Warning: ') + x.m; }
    if ((a.dataset.tip || '') !== tip) a.dataset.tip = tip; };
  evLint(t);
  a.onkeydown = e => { if ((e.ctrlKey || e.metaKey) && e.key === 's') { e.preventDefault(); evSave(t); }
    else if (e.key === 'Tab' && !e.ctrlKey) { e.preventDefault(); e.stopPropagation(); document.execCommand('insertText', false, '\t'); } }; /* execCommand keeps native undo */
  if (t.ss != null) { a.setSelectionRange(t.ss, t.se); b.firstChild.scrollTop = t.sy; } a.focus(); }
// #372 menu bar over the tabs: File/Edit/View dropdowns (menuAt) + quick toggles; items depend on the tab (text | md/json preview | image). UI.ew wrap, UI.ez zoom (CSS zoom on #ebody)
const evFm = t => !!evPv(t) && fmtOn(t.rel || t.name); // #392 formatted view on (per file type, not per tab)
const evPv = t => t?.text !== undefined && (isMd(t.rel || t.name) ? 'md' : /\.json$/i.test(t.rel || t.name) ? 'json' : ''); // preview kind or ''
const evTa = () => $('ebody').querySelector('textarea');
// #374 lint: UI.nl = off (default on). t.lint = [{l, s:'e'|'w', m}] for text t.lt. JSON checked here; rest → lib lint. rs = cargo check on the saved file ∴ only when clean (open/save)
async function evLint(t, force) { const ext = (t.rel || t.name).split('.').pop().toLowerCase();
  if (UI.nl || t.text === undefined || !LINTX.test(ext)) return evMarks(t);
  if (ext === 'rs' && (!t.rel || evDirty(t))) return; // keeps last marks while editing
  if (!force && t.lint && t.lt === t.text) return evMarks(t);
  if (t.lbusy) { t.lagain = 1; return; }
  const tx = t.text; t.lbusy = 1; const r = await lintOf(t.rel || t.name, tx); t.lbusy = 0;
  Object.assign(t, {lint: r, lt: tx}); evMarks(t);
  if (t.lagain) { t.lagain = 0; evLint(t); } }
// t.lint shape [{l, s, m}]; shared by Edit view + Code tab (#392)
const LINTX = /^(json|m?js|cjs|jsx|tsx?|py|rs|html?)$/;
async function lintOf(rel, tx) { const ext = rel.split('.').pop().toLowerCase(); let r = [];
  if (ext === 'json') { try { JSON.parse(tx); } catch (e) { const m = e.message, p = +(/position (\d+)/.exec(m) || [])[1]; r = [{l: +(/line (\d+)/.exec(m) || [])[1] || (p >= 0 ? tx.slice(0, p).split('\n').length : 1), s: 'e', m}]; } }
  else { let x = ext, s = tx;
    if (/^html?$/.test(ext)) { x = 'js'; /* inline <script> bodies kept, every other char blanked, newlines kept ∴ line numbers match */
      s = tx.replace(/<script\b([^>]*)>([\s\S]*?)(?=<\/script>)|[^\n]/gi, (m, at, body) => body === undefined || /\bsrc=|type=["']?(?!module|text\/javascript)/i.test(at) ? m.replace(/[^\n]/g, '') : (/type=["']?module/i.test(at) && (x = 'mjs'), m.slice(0, m.length - body.length).replace(/[^\n]/g, '') + body)); }
    try { r = await call('lint', {pid: cur, rel, text: s, ext: x}); } catch { r = []; } }
  return r; }
function evMarks(t, bar = 1) { const b = $('ebody'); if (b._t !== t) return; const rows = b.querySelector('.ecode pre')?.children; if (!rows) return;
  for (const r of b.querySelectorAll('.le, .lw')) r.classList.remove('le', 'lw');
  for (const x of UI.nl ? [] : t.lint || []) rows[x.l - 1]?.classList.add(x.s === 'e' ? 'le' : 'lw');
  if (bar) evBar(t); }
function evGo(l) { const a = evTa(); if (!a) return; const p = a.value.split('\n').slice(0, l - 1).join('\n').length + (l > 1);
  a.focus(); a.setSelectionRange(p, p); $('ebody').querySelector('.ecode pre')?.children[l - 1]?.scrollIntoView({block: 'center'}); }
const evProbs = (btn, t) => menuAt(btn, t.lint.map(x => ({label: `${x.l}: ${x.m}`.slice(0, 140), icon: x.s === 'e' ? 'x' : 'warn', run: () => evGo(x.l)})));
function evCmd(c, v) { const a = evTa(); if (!a) return; a.focus(); document.execCommand(c, false, v); } /* execCommand keeps native undo */
function evSet(k, v, re = true) { if (k) UI[k] = v; try { localStorage.ui = JSON.stringify(UI); } catch {} evDraw(re); if (re && tab === 'code') drawRight(); } // re = rebuild body (wrap/preview); zoom only restyles. #392 Code tab shares fmt/lint/wrap ∴ redrawn too
const evImg = () => !!(cur && etabs().tabs[etabs().on]?.img && VZ?.k === 'img'); // #438 image tab: zoom = the image (VZ), never the UI
const evZp = p => evImg() ? vZp(p) : evSet('ez', Math.min(300, Math.max(50, p)) / 100, false), evZoom = d => evImg() ? (d ? vZp(zStep(Math.round(VZ.s * 100), d)) : vZoom(0)) : evZp(d ? zStep(Math.round((UI.ez || 1) * 100), d) : 100);
/* #393 one zoom control [−] N% [+]: ± snap to next multiple of 10 (66 → 70 → 80), drag on % = 1%/px, dblclick = 100%. get/set = JS exprs (set gets p = absolute %) ∵ bars re-render mid-drag */
const zStep = (p, d) => d > 0 ? Math.floor(p / 10 + 1e-6) * 10 + 10 : Math.ceil(p / 10 - 1e-6) * 10 - 10;
const zoomCtl = (get, set, tip = '') => `<span class="zctl"><button class="ib" title="Zoom out" onclick="(p => ${set})(zStep(${get}, -1))">${icon('minus')}</button><span class="zp" data-tip="Drag left/right to zoom${tip} · double-click: 100%" onpointerdown="zDrag(event, () => ${get}, p => ${set})" ondblclick="(p => ${set})(100)">${(0, eval)(get)}%</span><button class="ib" title="Zoom in" onclick="(p => ${set})(zStep(${get}, 1))">${icon('plus')}</button></span>`;
/* #399 ONE drag model for every divider/handle: press → move(ev) follows pointer x/y wherever it is (window listeners + capture)
   → stops on release/cancel/lost capture/buttons 0/window blur/pointer outside the window. cap false = handle re-renders mid-drag (capture would drop).
   Handle gets .on (hover-style highlight) + body.dragging (cursor = handle's, no text select, iframes inert); tooltip hidden for the drag */
function dragOn(e, move, end, cap = true) { if (e.button) return; e.preventDefault(); hideTip(); const g = e.currentTarget, cur = getComputedStyle(g).cursor; let done = false;
  if (cap) try { g.setPointerCapture(e.pointerId); } catch {}
  const out = ev => ev.clientX < 0 || ev.clientY < 0 || ev.clientX >= innerWidth || ev.clientY >= innerHeight;
  const mv = ev => !ev.buttons || out(ev) ? stop() : move(ev), K = ['pointerup', 'pointercancel', 'blur'];
  const stop = () => { if (done) return; done = true; removeEventListener('pointermove', mv); K.forEach(k => removeEventListener(k, stop)); g.removeEventListener('lostpointercapture', stop);
    g.classList.remove('on'); document.body.classList.remove('dragging'); document.body.style.cursor = ''; end?.(); };
  addEventListener('pointermove', mv); K.forEach(k => addEventListener(k, stop)); if (cap) g.addEventListener('lostpointercapture', stop);
  g.classList.add('on'); document.body.classList.add('dragging'); document.body.style.cursor = cur; }
function zDrag(e, get, set) { const x0 = e.clientX, p0 = get(); dragOn(e, ev => set(p0 + Math.round(ev.clientX - x0)), null, false); }
function evMenu(k, btn) { const E = etabs(), t = E.tabs[E.on], tx = !!t && t.text !== undefined, ed = tx && !evFm(t), pv = evPv(t);
  const M = {
    file: [{label: 'New file', icon: 'plus', run: evNew}, {label: 'Open from Files', icon: 'folder', run: () => { UI.lc = false; setSide('files'); }}, '-',
      ...(tx || t?.img ? [{label: 'Save', icon: 'down', hint: 'Ctrl+S', run: () => evSave(t)}] : []), ...(t?.rel ? [{label: 'Open in Code tab', icon: 'code', run: () => previewFile(t.rel, true)}] : []),
      ...(t ? ['-', {label: 'Close tab', icon: 'x', run: () => evClose(E.on)}] : [])],
    edit: ed ? [{label: 'Undo', icon: 'undo', hint: 'Ctrl+Z', run: () => evCmd('undo')}, {label: 'Redo', icon: 'redo', hint: 'Ctrl+Y', run: () => evCmd('redo')}, '-',
      {label: 'Cut', icon: 'cut', hint: 'Ctrl+X', run: () => evCmd('cut')}, {label: 'Copy', icon: 'copy', hint: 'Ctrl+C', run: () => evCmd('copy')},
      {label: 'Paste', icon: 'paste', hint: 'Ctrl+V', run: () => navigator.clipboard.readText().then(v => evCmd('insertText', v), () => toast('Clipboard blocked: use Ctrl+V', 'warn'))},
      {label: 'Select all', icon: 'expand', hint: 'Ctrl+A', run: () => { evTa()?.focus(); evTa()?.select(); }},
      ...(pv === 'json' ? ['-', {label: 'Format JSON', icon: 'braces', run: () => { try { const v = JSON.stringify(JSON.parse(t.text), null, 2); evTa().select(); evCmd('insertText', v); } catch (e) { toast(String(e), 'err'); } }}] : [])]
      : [{title: evFm(t) ? 'Formatted view: switch it off in View to edit' : 'Nothing to edit'}],
    /* #382: order = evBar icon order (preview, highlight, lint, wrap) */
    view: [...(pv ? [{label: 'Formatted view' + (pv === 'json' ? ' (read-only)' : ''), icon: 'eye', check: evFm(t), run: () => { fmtFlip(t.rel || t.name); evSet(); }}] : []),
      ...(tx ? [{label: 'Syntax highlighting', icon: 'code', check: hlOn(), off: evFm(t) && pv === 'md', run: hlSet}, {label: 'Lint (errors and warnings)', icon: 'bug', check: !UI.nl, run: () => evSet('nl', !UI.nl)}, {label: 'Word wrap', icon: 'wrap', check: !!UI.ew, run: () => evSet('ew', !UI.ew)}] : []), ...(tx || pv ? ['-'] : []),
      {label: 'Show menu', icon: 'list', hint: 'Alt+M', check: !UI.nm, run: evMenuBar}, '-',
      {label: 'Zoom in', icon: 'plus', run: () => evZoom(.1)}, {label: 'Zoom out', icon: 'minus', run: () => evZoom(-.1)}, {label: 'Reset zoom', icon: 'search', hint: Math.round((evImg() ? VZ.s : UI.ez || 1) * 100) + '%', run: () => evZoom(0)}]};
  menuAt(btn, M[k]); }
// #447 Show menu off → bar hidden, Alt+M (keydown below) brings it back
function evMenuBar() { evSet('nm', !UI.nm, false); if (UI.nm) toast('Menu bar hidden: press Alt+M to show it again', 'info'); }
function evBar(t) { const pv = evPv(t), tx = t?.text !== undefined;
  $('emenu').style.display = UI.nm ? 'none' : '';
  // #447 a menu open → hovering another title switches to it (menu bar behaviour)
  $('emenu').innerHTML = ['File', 'Edit', 'View'].map(k => `<button class="mb" onclick="evMenu('${k.toLowerCase()}', this)" onmouseenter="MENU_A !== this && MENU_A?.closest('#emenu') && document.querySelector('.menu') && evMenu('${k.toLowerCase()}', this)">${k}</button>`).join('');
  if (t?.img) { if (ED?.tab === t) edCtx(); else $('ectx').innerHTML = ''; return; } // #439 image: edCtx fills #ectx once loaded
  $('ectx').innerHTML = !t || t.err || t.text === undefined ? '' : `<span class="vtn">${pv ? (evFm(t) ? 'Formatted' : 'Source') : 'Text'}</span><span class="sep"></span>`
    + (pv ? `<button class="ib${evFm(t) ? ' on' : ''}" title="Formatted view" onclick="fmtFlip(etabs().tabs[etabs().on].rel || etabs().tabs[etabs().on].name); evSet()">${icon('eye')}</button>` : '')
    + (tx ? hlBtn('', evFm(t) && pv === 'md') : '') /* #373 same toggle as the Code tab (localStorage.codeHl) */
    + (tx && !evFm(t) && !UI.nl && t.lint?.length ? `<button class="ib ez" title="Problems: click to jump" onclick="evProbs(this, etabs().tabs[etabs().on])">${['e', 'w'].map(s => { const n = t.lint.filter(x => x.s === s).length; return n ? `<span style="color:${s === 'e' ? 'var(--err)' : 'var(--warn)'}">${icon(s === 'e' ? 'x' : 'warn')}${n}</span>` : ''; }).join(' ')}</button>` : '')
    + (tx ? `<button class="ib${UI.nl ? '' : ' on'}" title="Lint: errors/warnings in the gutter (JS/HTML scripts via node or the project's eslint, Python, Rust on save, JSON)" onclick="evSet('nl', !UI.nl)">${icon('bug')}</button>` : '')
    + (tx ? `<button class="ib${UI.ew ? ' on' : ''}" title="Word wrap" onclick="evSet('ew', !UI.ew)">${icon('wrap')}</button>` : '')
    + `<span class="vur">${zoomCtl('Math.round((UI.ez || 1) * 100)', 'evZp(p)', ', Ctrl+wheel')}</span>`; } /* #439 = image tab's bar layout: tool/mode name, toggles, zoom right */
$('ebody').addEventListener('wheel', e => { if (e.ctrlKey) { e.preventDefault(); evZoom(e.deltaY < 0 ? .1 : -.1); } }, {passive: false});
// Shell tab: xterm.js (vendored in ui/xterm, MIT) loaded on first use; SH[pid] = {el, term, fit, dead}; one pty per project (lib shell_open)
let XT; const SH = {};
const xtLoad = () => XT ||= new Promise((ok, no) => { const add = (tag, k, v, then) => { const x = document.createElement(tag); x[k] = v; if (tag === 'link') x.rel = 'stylesheet'; x.onload = then; x.onerror = no; document.head.append(x); };
  add('link', 'href', 'xterm/xterm.css'); add('script', 'src', 'xterm/xterm.js', () => add('script', 'src', 'xterm/addon-fit.js', ok)); });
async function shellShow() { const pid = cur;
  try { await xtLoad(); } catch { XT = null; return toast('Could not load the terminal', 'err'); }
  if (pid !== cur || view !== 'edit') return;
  let s = SH[pid];
  if (!s) { const el = document.createElement('div'), bg = getComputedStyle($('shellbox')).backgroundColor;
    const term = new Terminal({fontFamily: 'ui-monospace,"Cascadia Code",Consolas,monospace', fontSize: 13, cursorBlink: true, theme: {background: bg}}), fit = new FitAddon.FitAddon();
    term.loadAddon(fit); s = SH[pid] = {el, term, fit, dead: true};
    term.onData(d => s.dead ? shellStart(pid) : call('shell_write', {pid, d}).catch(() => {}));
    term.onResize(({cols, rows}) => s.dead || call('shell_resize', {pid, cols, rows})); }
  const box = $('shellbox'); if (s.el.parentNode !== box) box.replaceChildren(s.el);
  if (!s.term.element) s.term.open(s.el);
  s.fit.fit(); if (s.dead) shellStart(pid); s.term.focus(); }
async function shellStart(pid) { const s = SH[pid]; s.dead = false;
  try { await call('shell_open', {pid, cols: s.term.cols, rows: s.term.rows}); } catch (e) { s.dead = true; s.term.write('\r\n\x1b[31m' + e + '\x1b[0m\r\n'); } }
// #317 DH = last visible dock height (hidden in Shell → keep the old one) → --dh pads #shellbox to Terminal's log+dock size
// icoFit: bs = buttons in collapse order (first = drops its label first); stops once tight() is false
function icoFit(bs, tight) { bs.forEach(b => b.classList.remove('ic')); for (const b of bs) { if (!tight()) return; b.classList.add('ic'); } }
// #332 What next? presets: one line, labels drop from the last preset back, primary (Build) last; term view = [text] buttons, no icons ∴ skipped
function readyFit() { const r = $('readytext'); if (!r.querySelector('.btn') || $('log').classList.contains('term') || r.classList.contains('full')) return; const t = () => r.scrollWidth > r.clientWidth; r.classList.remove('wrap'); icoFit([...r.querySelectorAll('.btn')].reverse(), t); r.classList.toggle('wrap', t()); } // icons still too wide → wrap
{ new ResizeObserver(() => readyFit()).observe($('readytext')); }
let DH = 0; new ResizeObserver(() => { const h = $('dock').offsetHeight; if (h) { DH = h; $('main').style.setProperty('--dh', h + 'px'); } }).observe($('dock'));
new ResizeObserver(() => { const s = SH[cur]; if (s?.term.element && $('shellbox').offsetHeight) s.fit.fit(); }).observe($('shellbox'));
// panel grip: drag = height (snaps shut under 60px, like the side rails), double-click = collapse
$('pgrip').ondblclick = () => { UI.pc = !UI.pc; applyView(); };
// #380 pointer capture (mouseup was lost when the panel collapsed under the cursor → stuck drag) + buttons==0 guard;
// total panel = log + composer: log shrinks to 0 first, then the composer (PH, term grid), collapse only when the whole panel < 60px
$('pgrip').onpointerdown = e => { const m = $('main'), d0 = DH, t0 = $('prompt').offsetHeight, rest = d0 - t0, k = t0 / Math.max(1, $('log').offsetHeight + t0);
  dragOn(e, e => { const r = m.getBoundingClientRect(), tabs = $('ptabs').offsetHeight, total = r.bottom - e.clientY - tabs;
    const was = UI.pc; UI.pc = total < 60;
    if (!UI.pc) { const max = r.bottom - $('edv').getBoundingClientRect().top - 80 - tabs - rest, sp = Math.max(0, Math.min(total - rest, max)); /* #344 max = editor keeps 80px; sp = log + prompt */
      if (UI.pt === 'shell') UI.ph = Math.max(0, Math.round(sp - t0));
      else { PH = Math.max(LH, Math.round(sp * k / LH) * LH); UI.ph = Math.max(0, Math.round(sp - PH)); grow(); } } /* #442 log:prompt ratio from drag start, prompt = whole rows (panelLay snaps the log) */
    panelLay(); if (was && !UI.pc && UI.pt === 'shell') shellShow(); }); };
function drawLog() {
  const box = document.createElement('div'), l = logs[cur] || [];
  box.className = 'col';
  if (!l.length) box.innerHTML = `<div id="empty" style="margin-top:16vh">${CRAB}<p>What should we work on in <b>${esc(P().name)}</b>?</p></div>`;
  l.forEach(e => put(box, e));
  if (live[cur]) box.appendChild(liveEl(cur));
  // manual clear-continue: questions show only after the Continue (spec), never next to a waiting Continue; N? pill opens them any time
  if (P().questions.length && !hiddenQs[cur] && (qsOpen[cur] || P().cont === 'auto' || P().cont === 'off')) box.appendChild(qsKeep(cur));
  const lg = $('log'), same = drawLog.pid === cur, stick = !same || atEnd(), top = lg.scrollTop; drawLog.pid = cur;
  const f = document.activeElement?.closest?.('.card.qs') && document.activeElement, sel = f && [f.selectionStart, f.selectionEnd];
  lg.replaceChildren(box, $('skel'));
  if (f?.isConnected) { f.focus({preventScroll: true}); if (sel?.[0] != null) f.setSelectionRange(...sel); } // moved card = blurred → typing goes on
  if (stick) toEnd(true); else lg.scrollTop = top; // redraw while scrolled up: stay put (no jump)
}
// follow new output only when within 50px of the bottom; else count it on the ↓ button (click/scroll down = back to following)
const atEnd = () => { const l = $('log'); return l.scrollHeight - l.scrollTop - l.clientHeight < 50; };
let unseen = 0;
function toEnd(stick, n = 0) {
  const l = $('log'), j = $('jump');
  if (stick) { l.scrollTop = l.scrollHeight; unseen = 0; j.hidden = true; return; }
  unseen += n; j.hidden = false; jumpTxt(); /* no count: Working pill updates inflate it */
}
// #251 jump pill = the live middle status while busy (Working pill's text), else New!
function jumpTxt() { const j = $('jump'), w = busy[cur] && !$('workpill').hidden; j.classList.toggle('on', w); j.lastChild.textContent = w ? $('worktext').textContent : unseen ? 'New!' : ''; }
$('log').addEventListener('scroll', () => { const j = $('jump'), e = atEnd(); if (e && !j.hidden) { unseen = 0; j.hidden = true; } else if (!e && j.hidden && busy[cur] && view === 'chat') { j.hidden = false; jumpTxt(); } }, {passive: true}); // #251 scrolled up mid-turn → status pill floats at the bottom
// #424 log shrinks (composer grew: thumbs, more lines) → scrollTop stays ∴ last msg slid under the dock fade; was at end → stay at end
{ let wasEnd = true; $('log').addEventListener('scroll', () => { wasEnd = atEnd(); }, {passive: true}); new ResizeObserver(() => { if (wasEnd && view === 'chat') $('log').scrollTop = $('log').scrollHeight; }).observe($('log')); }
// #327 term grid: every block child of .col padded (margin-bottom) to a whole 20px row ∴ next block starts on a row; inline notes skipped (line box = 20px already)
const LH = 20, gridRO = new ResizeObserver(es => { const t = view !== 'chat'; for (const e of es) { const h = e.borderBoxSize[0].blockSize; e.target.style.marginBottom = t && h ? Math.ceil(h / LH - .01) * LH - h + 'px' : ''; } });
new MutationObserver(ms => { for (const m of ms) for (const n of m.addedNodes) { if (n.nodeType !== 1) continue; if (n.classList.contains('col')) [...n.children].forEach(c => gridRO.observe(c)); else if (n.parentElement?.classList.contains('col')) gridRO.observe(n); } }).observe($('log'), {childList: true, subtree: true});
// #327 scroll: native momentum, then settle on a whole row (bottom = stays at the end; ponytail: snaps after scrollend, not mid-fling)
// #339 scroll = whole rows, no smooth/snap-after: wheel → own row steps (acc keeps touchpad deltas), any other scroll (drag, keys) quantized instantly
// bottom pad ≡ H − padTop (mod LH) ∴ scrollMax is a whole row too (end = on grid)
// #342 dock pad-bottom = (log h + current pad) mod LH ∴ log height = whole rows → dock top lands on the log's row grid (stable: 2nd pass = same value)
const termPad = () => { const l = $('log'), d = $('dock'); if (view === 'chat') return l.style.paddingBottom = d.style.paddingBottom = '';
  d.style.paddingBottom = view !== 'term' ? '' : ((l.getBoundingClientRect().height + (parseFloat(d.style.paddingBottom) || 0)) % LH).toFixed(2) + 'px'; l.style.paddingBottom = LH + ((l.clientHeight - parseFloat(getComputedStyle(l).paddingTop)) % LH + LH) % LH + 'px'; };
new ResizeObserver(termPad).observe($('log'));
let wAcc = 0; $('log').addEventListener('wheel', e => { if (view === 'chat' || e.ctrlKey || !e.deltaY) return; e.preventDefault(); const l = $('log');
  wAcc += e.deltaY * (e.deltaMode === 1 ? LH : e.deltaMode === 2 ? l.clientHeight : 1); const n = Math.trunc(wAcc / LH); if (n) { wAcc -= n * LH; l.scrollTop = Math.round(l.scrollTop / LH) * LH + n * LH; } }, {passive: false});
$('log').addEventListener('scroll', () => { const l = $('log'); if (view === 'chat') return; const t = Math.round(l.scrollTop / LH) * LH; if (Math.abs(t - l.scrollTop) > .5 && l.scrollHeight - l.scrollTop - l.clientHeight >= 2) l.scrollTop = t; }, {passive: true});
// Claude's reply right after a failed tool: a code-picked lead-in line above it and a closing phrase after it.
// Picked by a hash of the reply, so a redraw shows the same words.
const OOPS = ['Whoops! My bad!', 'Uh-oh!', 'My mistake, sorry!', 'Oh no!', 'Oops!', 'Ah, that went wrong.', 'Hmm, that didn\'t work.', 'Oh dear!', 'Sorry about that!', 'Well, that\'s not right.'],
  OOPS_END = ['I\'ll try to avoid this mistake in the future.', 'I\'ll try not to do this again.', 'I\'ll avoid this issue next time.', 'Noted for next time.', 'I\'ll be more careful with this.', 'Lesson learned.'],
  OOPS_PERM = ['I\'ll try another method...', 'I\'ll try another tool...', 'I\'ll try another way...', 'I\'ll see if I can work around this...'],
  PERM_RX = /permission|denied|not allowed|haven't granted|EACCES|EPERM|not permitted/i;
function oops(el, e, err) {
  const m = el.querySelector('.msg.claude'), h = parseInt(sugKey(e.text + (e.ts || '')), 36) >>> 0, pick = a => a[h % a.length];
  if (!m) return;
  m.insertAdjacentHTML('afterbegin', `<p class="oops">${esc(pick(OOPS))}</p>`);
  if (/\?\s*$/.test(e.text)) return; // asks the user something: no "I'll try" promise after it
  const end = esc(pick(PERM_RX.test(err) ? OOPS_PERM : OOPS_END)), p = m.lastElementChild;
  p?.tagName === 'P' && p.className !== 'oops' ? p.insertAdjacentHTML('beforeend', ' ' + end) : m.insertAdjacentHTML('beforeend', `<p>${end}</p>`);
}
// Group thread (setting UI.gt): what Clauracle and its helper agents say to each other in one turn = one row.
// Out: Agent/Task prompts and SendMessage; back: the agent's result. ids maps call ids, agent ids and names to the agent type.
const GT_RE = /^(Task|Agent|SendMessage)$/;
const gtName = n => n === 'Clauracle' ? n : roleOf({name: n});
const gtAv = n => n === 'Clauracle' ? '<svg class="av"><use href="#i-logo"/></svg>' : face(AGENTS?.find(a => a.name === n)?.face || n, 16);
// who can be @-mentioned: Clauracle plus every enabled agent ('' type = Clauracle)
const gtNames = () => { const off = SET?.off || [];
  return [['Clauracle', ''], ...[...(S.agents_on ? (AGENTS || []).filter(a => a.on) : []), ...(CCAG || []).filter(a => !off.includes('agent:' + a.name))].map(a => [roleOf(a), a.name])]; };
function gtPut(box, e) {
  let t = box._gt;
  if (e.kind === 'tool' && !e.parent && GT_RE.test(toolName(e))) {
    if (!t) { t = box._gt = {ms: [], ids: new Map(), el: document.createElement('div')}; t.el.className = 'gthread' + (e.old ? ' old' : '');
      t.el.innerHTML = `<button class="gth" onclick="this.parentElement.classList.toggle('open')"></button><div class="gtb"><div class="gml"></div>`
        + `<div class="gtr"><input class="grow" placeholder="Reply to Clauracle, or @name for an agent" onkeydown="if(event.key==='Enter')gtSend(this)"><button class="ib" data-tip="Send" onclick="gtSend(this.previousElementSibling)">${icon('reply')}</button></div><div class="gtat"></div></div>`;
      box.appendChild(t.el); }
    const d = e.detail || {}, sm = toolName(e) === 'SendMessage', type = sm ? t.ids.get(d.to) || d.to || 'agent' : d.subagent_type || 'general-purpose';
    t.ms.push({from: 'Clauracle', to: type, text: sm ? String(d.message ?? d.content ?? '') : d.prompt || d.description || '', ts: e.ts});
    if (e.id) t.ids.set(e.id, sm ? '' : type); // '' = a SendMessage call: its result is only a delivery note
    if (d.name) t.ids.set(d.name, type);
    gtDraw(t); return true;
  }
  if (!e.result_of || !t?.ids.has(e.result_of)) return false;
  const type = t.ids.get(e.result_of), txt = e.text || '', aid = txt.match(/agentId:\s*(\w+)/)?.[1];
  if (aid && type) t.ids.set(aid, type);
  if (type && (e.kind === 'err' || !/^Async agent launched/.test(txt))) { t.ms.push({from: type, to: 'Clauracle', text: txt.replace(/\n*agentId:[\s\S]*$/, '').trim(), err: e.kind === 'err', ts: e.ts}); gtDraw(t); }
  return true;
}
function gtDraw(t) {
  const who = [...new Set(t.ms.flatMap(m => [m.from, m.to]))].filter(n => n !== 'Clauracle');
  t.el.querySelector('.gth').innerHTML = `<span class="gtav">${['Clauracle', ...who].map(gtAv).join('')}</span><b>${esc(['Clauracle', ...who].map(gtName).join(', '))}</b>`
    + `<span class="pill dim">${t.ms.length} message${t.ms.length === 1 ? '' : 's'}</span>${icon('chev')}`;
  t.el.querySelector('.gml').innerHTML = t.ms.map(m => `<div class="gm${m.err ? ' err' : ''}">${gtAv(m.from)}<div class="grow"><div class="cm"><b>${esc(gtName(m.from))}</b><span>${esc(tm(m.ts))}</span></div>`
    + `<div class="gmt" onclick="getSelection().isCollapsed && this.classList.toggle('full')"><span class="at">@${esc(gtName(m.to))}</span> ${md(m.text)}</div></div></div>`).join('');
  t.el.querySelector('.gtat').innerHTML = ['Clauracle', ...who].map(n => `<button class="btn ghost sm" onclick="gtAt(this, '${esc(gtName(n))}')">@${esc(gtName(n))}</button>`).join('');
}
const gtAt = (b, n) => { const i = b.closest('.gtb').querySelector('input'); const o = gtNames().map(x => x[0]).find(r => i.value.startsWith('@' + r + ' '));
  i.value = '@' + n + ' ' + (o ? i.value.slice(o.length + 2) : i.value); i.focus(); };
// a reply goes through the normal send path (filing, queue, mid-turn); "@Agent text" asks Clauracle to pass it on
function gtSend(inp) {
  let v = inp.value.trim(); if (!v) return;
  const m = gtNames().find(([r]) => v.toLowerCase().startsWith('@' + r.toLowerCase()));
  if (m) { const rest = v.slice(m[0].length + 1).trim(); v = m[1] ? `To the ${m[1]} agent (SendMessage if it still runs, else start it): ${rest}` : rest; }
  if (!v) return;
  inp.value = '';
  const k = $('prompt').value, ka = atts[cur], ed = editing[cur]; // the composer's own draft stays put
  delete editing[cur]; atts[cur] = []; $('prompt').value = v; send();
  $('prompt').value = k; atts[cur] = ka; if (ed) editing[cur] = ed; grow(); drawAtts(); keepDraft();
}
// consecutive tool lines share one compact group in chat view; the same call again bumps "×N" on its line; click → what it did
function put(box, e) {
  if (e.kind === 'user') box._gt = null; // a new turn starts a new thread
  if (UI.gt && view === 'chat' && gtPut(box, e)) return;
  const el = entry(e);
  if (!el) return;
  if (e.kind === 'tool' && view === 'chat') {
    // a run of tool calls = one terminal window; tabs: shell commands, then one per file / other tool; the newest shows
    // (until a tab is clicked); its lines = each call, click one → just that call's details/output
    let g = box.lastElementChild;
    if (!g?.classList.contains('tools') || !!g._old !== !!e.old) { g = document.createElement('div'); g.className = 'tools termcard' + (e.old ? ' old' : ''); g._old = e.old; g._tabs = new Map();
      if (!box.lastElementChild?.matches('.crow.claude')) { // #189 card opens Claude's turn → its avatar/name row sits above it (fixed; no jump)
        const h = chatRow('claude', document.createDocumentFragment(), e.ts || (e.old ? '' : new Date().toISOString())); h.classList.add('thead');
        const a = document.createElement('span'); a.className = 'tav'; a.append(h.firstElementChild); h.prepend(a); box.appendChild(h); }
      box.appendChild(g); hatch(box, g); }
    const d = e.detail || {}, name = toolName(e), sh = /^(Bash|PowerShell)$/.test(name) && typeof d.command === 'string',
      f = d.file_path || d.notebook_path, key = sh ? 'sh' : f ? 'f:' + f : 't:' + name;
    let t = g._tabs.get(key);
    if (!t) g._tabs.set(key, t = {label: sh ? name : f ? shortPath(f).split(/[\\/]/).pop() : name, title: f ? shortPath(f) : name, es: []});
    t.es.push(e); g._last = key; if (!g._pin) g._on = key;
    if (name === 'Read' && /\.(png|jpe?g|gif|webp|bmp)$/i.test(f || '')) (box._shots ||= []).includes(f) || box._shots.push(f);
    return drawAct(g);
  }
  if (e.kind === 'err' && view === 'chat') { // #107: red dot + bold "what → tool failed" (most recent tool call), else "Error"
    const g = box.lastElementChild?.classList.contains('tools') && box.lastElementChild, t = g && g._tabs.get(g._last);
    el.innerHTML = `<span class="et"><span class="edot"></span>${t ? esc(t.title) + ' → ' + esc(toolName(t.es.at(-1))) + ' failed' : 'Error'}</span>`;
    el.append(e.text); el._err = e.text;
  }
  if (e.kind === 'claude' && view === 'chat') { const pv = box.querySelector('.qs')?.previousElementSibling || box.lastElementChild; if (pv?._err != null) oops(el, e, pv._err); }
  if (e.kind === 'claude' && box._shots?.length) { // images Claude read since its last message → thumbs on this one
    const imgs = box._shots.map(f => ({path: f, name: f.split(/[\\/]/).pop().replace(/^[0-9a-f]{8,}-(?!\d+\.\w+$)/, '')})); box._shots = []; // #239 att_save = <id>-<attachment stem>.<ext> → show the attachment's own name
    const th = document.createElement('div'); th.className = 'thumbs read'; el.querySelector('.msg')?.prepend(th);
    th.innerHTML = `<span class="tcap">Looked at ${imgs.map(x => esc(x.name)).join(', ')} while working</span>`; // #333 caption ∵ bare thumb read as "this message is about that image"
    imgs.forEach((x, k) => { const im = document.createElement('img'); im.title = x.path; th.append(im);
      shotUrl(x.path).then(u => { x.url = im.src = u; }, () => im.remove());
      im.onclick = () => openViewer(imgs.filter(y => y.url), imgs.filter(y => y.url).indexOf(x), null); });
  }
  const tg = box.lastElementChild;
  if (e.kind === 'claude' && tg?.classList.contains('tools')) { // right after a terminal card → a reply to it: curved arrow + what ran
    const t = tg._tabs.get(tg._last), d = t.es.at(-1).detail || {}, r = document.createElement('div');
    r.className = 'rto'; r.title = 'Reply to ' + t.label + ': ' + ((d.command || '').split('\n')[0] || d.file_path || d.pattern || t.title);
    r.innerHTML = icon('reply'); // the card sits right above: no repeated header text
    r.onclick = () => { tg.scrollIntoView({block: 'nearest', behavior: 'smooth'}); tg.classList.remove('flash'); void tg.offsetWidth; tg.classList.add('flash'); };
    el.classList.add('reply'); el.querySelector('.msg.claude')?.prepend(r);
  }
  if (el.classList.contains('crow')) { // same speaker right above → no second avatar/name; #204 after a card/reply note = new bubble → own head
    const prev = box.lastElementChild;
    if (prev?.classList.contains('crow') && prev.classList.contains(e.kind) && !prev.matches('.reply, .thead')) el.classList.add('cont');
  }
  if (e.old) el.classList.add('old');
  const qs = box.querySelector('.qs');
  if (el._txt) { const pv = qs ? qs.previousElementSibling : box.lastElementChild; // #463 same plain note again right after → one row "×N", time range on its head
    if (pv?._txt === el._txt) { pv.dataset.n = '×' + (pv._n = (pv._n || 1) + 1); const h = pv._head || pv;
      if (view === 'chat' && e.ts && h._t0) h.dataset.ts = h._t0 === tm(e.ts) ? h._t0 : `${h._t0} – ${tm(e.ts)}`; return; } }
  if (e.kind === 'note' && view === 'chat') { // #363 status notes in a row = one stack: head shows "first – last" time, See report moves to the bottom
    const pv = qs ? qs.previousElementSibling : box.lastElementChild;
    if (pv?.matches('.msg.note')) { const h = pv._head || pv; el._head = h; el.classList.add('nrun'); delete el.dataset.ts;
      if (e.ts && h._t0) h.dataset.ts = h._t0 === tm(e.ts) ? h._t0 : `${h._t0} – ${tm(e.ts)}`;
      const rb = pv.querySelector('.btn[onclick^="repOpen"]'); if (rb && !el.dataset.rid) { el.dataset.rid = pv.dataset.rid; el.append(' ', rb); } }
    else el._t0 = el.dataset.ts; }
  qs ? box.insertBefore(el, qs) : box.appendChild(el);
  if (e.kind !== 'user' && !box._swap) hatch(box, el);
}
// live log + skeleton showing: the new row grows out of the Working… pill, and the skeleton respawns below it
function hatch(box, el) {
  const s = $('skel').firstElementChild;
  if (!box.isConnected || $('skel').hidden || view !== 'chat') return;
  $('skelts').textContent = tm(new Date().toISOString()); // skeleton's time = the latest post
  el.classList.add('born'); s.classList.remove('respawn'); void s.offsetWidth; s.classList.add('respawn');
  workTick(); // #189 new head row → skeleton drops its avatar now, not on the next 1s tick
}
// file → blob URL, cached per path (ponytail: never revoked; fine for a session's handful of screenshots)
const SHOTS = new Map();
const shotUrl = f => SHOTS.get(f) || (SHOTS.set(f, call('read_image', {path: f}).then(b => URL.createObjectURL(new Blob([b])))
  .catch(e => { SHOTS.delete(f); throw e; })), SHOTS.get(f));
const TICON = {Edit: 'new', MultiEdit: 'new', Write: 'file', NotebookEdit: 'new', Read: 'doc', Grep: 'search', Glob: 'search', LS: 'folder'};
// inside the project → relative; elsewhere the full path
const shortPath = f => { const r = (P()?.path || '').replace(/[\\/]+$/, ''); return r && f.toLowerCase().startsWith(r.toLowerCase()) ? f.slice(r.length + 1) : f; };
// Clauracle's own process: handover files and its MCP tools (shown as the pill instead)
const isProcess = e => e.kind === 'tool' && (/^mcp__clauracle__/.test(e.text) || /(^|[\\/])(HANDOVER|CHANGES|PLAN)\.md$/i.test(e.detail?.file_path || ''));
function entry(e) {
  if (e.kind === 'ask') return askCard(e);
  if (e.kind === 'answered') return answeredCard(e);
  if (isProcess(e)) return null;
  if (e.kind === 'claude' && /^No response requested\.?$/.test(e.text.trim())) return null; // Claude's filler after a restart call
  if (view === 'chat' && (e.kind === 'out' || e.kind === 'think')) return null;
  if (view === 'chat' && e.kind === 'note' && /^Queue is empty/.test(e.text)) return null; // #363 noise in the status stack; still in logs (stop reason, line ~3545)
  const d = document.createElement('div');
  d.className = 'msg ' + e.kind;
  if (e.kind === 'user' && e.images?.length) { // older entries: url strings; newer: {url, name}
    const imgs = e.images.map((x, k) => typeof x === 'string' ? {url: x, name: `image ${k + 1}`} : x);
    if (view !== 'chat') { // #312 term: no thumbs; image refs = accent [stem] tags (click → viewer), unreferenced images listed first
      const st = imgs.map(x => stem(x.name)), used = new Set(), tag = k => `<span class="itag" data-k="${k}">${esc(st[k])}</span>`;
      const body = esc(e.text).replace(/@?\[([^\]\n]+)\]/g, (m, n) => { const k = st.indexOf(n.replace(/&amp;/g, '&').trim()); if (k < 0) return m; used.add(k); return tag(k); });
      d.innerHTML = imgs.map((x, k) => used.has(k) ? '' : tag(k) + ' ').join('') + body;
      d.querySelectorAll('.itag').forEach(t => t.onclick = () => openViewer(imgs, +t.dataset.k, null));
    } else {
    d.innerHTML = `<div class="thumbs">${imgs.map(x => `<img src="${x.url}" title="${esc(x.name)}">`).join('')}</div>`;
    d.querySelectorAll('.thumbs img').forEach((im, k) => im.onclick = () => openViewer(imgs, k, null));
    d.insertAdjacentHTML('beforeend', refPills(bare(e.text), [...imgs.map(x => x.name), ...fnames(e.text)]) + filedTags(e));
  } }
  else if (view !== 'chat') d.textContent = e.kind === 'tool' && e.detail ? e.text.split('  ')[0] + '(' + JSON.stringify(e.detail, null, 2) + ')'
    // tool output: drop reminder blocks + blank-line runs (else a one-liner renders as a tall empty box)
    : e.kind === 'out' ? e.text.replace(/<system-reminder>[\s\S]*?(<\/system-reminder>|$)/g, '').replace(/[ \t]+$/gm, '').replace(/\n{3,}/g, '\n\n').trim() : e.text;
  else if (e.kind === 'claude') d.innerHTML = md(e.text) + (e.cmd ? cmdOpts(e.cmd, e.text) : '');
  else if (e.kind === 'user' && view === 'chat') { d.innerHTML = refPills(bare(e.text), fnames(e.text)) + filedTags(e); }
  else if (e.pill) d.innerHTML = `<span class="npill${e.rb ? ' on' : builds[cur]?.busy && e === logs[cur]?.findLast(x => x.pill && !x.rb) ? ' on' : ''}">${icon(e.pill)}${esc(e.text)}</span>`; // #244 Thinking-pill look; shimmer while its build runs (#247 rb = report being written)
  else { d.textContent = e.text; if (e.kind === 'note' && !e.act && !e.rid) d._txt = e.text; } // #463 _txt = collapsible repeat key
  if (e.kind === 'user' && e.texts?.length) { // big pastes stay chips in the bubble (texts saved in the log entry → open after a restart too)
    d.insertAdjacentHTML('beforeend', `<div class="tpills">${e.texts.map((x, k) => `<button class="rpill tchip" data-k="${k}">${icon('doc')}${esc(stem(x.name))} · ${tsize(x.text)}</button>`).join('')}</div>`);
    d.querySelectorAll('.tpills .tchip').forEach(b => b.onclick = () => textView(stem(e.texts[+b.dataset.k].name), e.texts[+b.dataset.k].text)); }
  // #169 ticks after the chain: as its own if, its `else` overwrote md() replies with plain text
  if (e.kind === 'user' && e.st && view === 'chat') USEREL.set(e, d); // #248 tick sits after the timestamp (row head), not in the bubble
  if (view === 'chat' && e.kind === 'note' && e.ts) d.dataset.ts = tm(e.ts); // shown above via CSS ::before
  if (e.rid) d.dataset.rid = e.rid;
  if (view === 'chat' && e.act === 'pdf') { d.insertAdjacentHTML('beforeend', ` <button class="btn sm">${icon('pdf')}${esc(e.file.split(/[\\/]/).pop())}</button>`); d.lastElementChild.onclick = () => openPdf(e.file); }
  if (view === 'chat' && e.kind === 'note' && NOTEACT[e.act]) d.insertAdjacentHTML('beforeend', ` <button class="btn sm" onclick="${NOTEACT[e.act][1]}">${NOTEACT[e.act][0]}</button>`);
  if (view !== 'chat' && (e.kind === 'out' || e.kind === 'tool')) d.onclick = () => getSelection().isCollapsed && d.classList.toggle('open'); // #312 2-line clamp ⇄ full (selecting text ≠ toggle)
  if (view !== 'chat' && e.kind === 'claude' && e.cmd) d.insertAdjacentHTML('beforeend', cmdOpts(e.cmd, e.text)); // the term branch above sets plain text only: /command option buttons were missing
  if (view !== 'chat' && (e.kind === 'user' || e.kind === 'claude')) return ircRow(e, d);
  if (view !== 'chat' || (e.kind !== 'user' && e.kind !== 'claude')) return d;
  const row = chatRow(e.kind, d, e.ts); if (e.kind === 'user' && e.st) row.querySelector('.cm').insertAdjacentHTML('beforeend', tickHtml(e));
  return row;
}
// note buttons (#170 Built! / Build failed): label, handler (cur = the project shown)
const NOTEACT = {run: [icon('play') + 'Run', "call('run_step', {pid: cur, step: 'run'})"], restart: ['Restart into it', "call('restart_into', {pid: cur})"], rep: [icon('list') + 'See report', 'repOpen(this.parentElement.dataset.rid)'], out: ['See output', "rightTab('build'); setTimeout(() => $('buildOut')?.scrollIntoView({block: 'nearest'}))"]};
// chat look: avatar (Clauracle's icon | your GitHub picture) + name · time over the bubble
const tm = ts => { const d = ts ? new Date(ts) : null; if (!d || isNaN(d)) return '';
  const t = d.toLocaleTimeString([], {hour: '2-digit', minute: '2-digit'});
  return d.toDateString() === new Date().toDateString() ? t : d.toLocaleDateString([], {day: 'numeric', month: 'short'}) + ', ' + t; };
// #316 terminal view: header line [HH:MM] <@you>:/<Clauracle>: then the text below (@ = you're the channel admin)
function ircRow(e, d) {
  const w = document.createElement('div'), t = e.ts && new Date(e.ts), u = e.kind === 'user';
  w.className = 'irc ' + e.kind;
  w.innerHTML = `<div class="ih">${t && !isNaN(t) ? '<span class="its">[' + t.toLocaleTimeString([], {hour: '2-digit', minute: '2-digit', hour12: false}) + ']</span>' : ''}<span class="nk">&lt;${u ? '@' + esc(S.you?.nick || P()?.gh || 'You') : 'Clauracle'}&gt;:</span></div>`;
  w.append(d); return w;
}
// #418 message ID = 4 base36 chars of FNV-1a(ts + kind): stable across reloads (log keeps ts), no stored field. ponytail: 1.7M space, collisions per project ≈ never
const mid = (ts, kind) => { let h = 2166136261; for (const c of ts + kind) h = Math.imul(h ^ c.charCodeAt(0), 16777619); return (h >>> 0).toString(36).slice(-4).padStart(4, '0'); };
const midEntry = (pid, id) => (logs[pid] || []).findLast(e => e.ts && mid(e.ts, e.kind === 'user' ? 'user' : 'claude') === id); // ≠ user → 'claude' ∵ a .thead row's ts = its first tool entry's
function midRef(id) { const el = $('prompt'), v = el.value, s = el.selectionStart ?? v.length, e = el.selectionEnd ?? s, r = `#[${id}]`; // reply ref, like attRef; #427 #[id] (was @[^id]: ^ read as "the one above"; #N alone = task)
  el.setRangeText((s && !/\s/.test(v[s - 1]) ? ' ' : '') + r + (e < v.length && /\s/.test(v[e]) ? '' : ' '), s, e, 'end'); el._pv = el.value; el.focus(); grow(); keepDraft(); }
function midJump(id) { const el = $('log').querySelector(`.crow[data-mid="${id}"]`); if (!el) return toast('That message is no longer in the history', 'info');
  el.scrollIntoView({behavior: 'smooth', block: 'center'}); el.classList.remove('jumphl'); void el.offsetWidth; el.classList.add('jumphl'); }
// sent text: "@[^id]" alone means nothing to Claude → quote what it points at (first 400 chars)
const midTxt = (pid, t) => [...new Set([...t.matchAll(/(?:#|@\[\^)\[?(\w{4})\]/g)].map(m => m[1]))].map(id => { const e = midEntry(pid, id);
  return e ? `\n\n(#[${id}] = reply reference to ${e.kind === 'user' ? 'my' : 'your'} earlier message: "${bare(e.text).trim().slice(0, 400).replace(/\n+/g, ' ')}${e.text.length > 400 ? '…' : ''}")` : ''; }).join('');
function chatRow(kind, inner, ts) {
  const w = document.createElement('div'), me = P()?.gh, id = ts && mid(ts, kind);
  w.className = 'crow ' + kind; if (id) w.dataset.mid = id;
  w.innerHTML = (kind === 'claude' ? '<svg class="av"><use href="#i-logo"/></svg>' : me ? avGh(me) : `<span class="av" style="background:var(--accent)">${icon('user')}</span>`)
    + `<div class="cb"><div class="cm"><b>${kind === 'claude' ? 'Clauracle' : esc(S.you?.nick || me || 'You')}</b>${ts ? `<span>${esc(tm(ts))}</span>` : ''}${id ? `<button class="mid" data-tip="Reply to this message (adds #[${id}] to your message)" onclick="midRef('${id}')">#${id}</button>` : ''}</div></div>`;
  w.querySelector('.cb').append(inner);
  return w;
}
// ponytail: shell colouring by regex (strings, $vars, -flags, | ; && operators, # comments, command words); not a parser
function shHi(src) {
  const re = /("(?:[^"\\]|\\.)*"?|'[^']*'?)|(\$\{[^}]*\}|\$[\w:]+)|((?<=\s)--?[A-Za-z][\w-]*)|(\|\||&&|[|;])|((?:^|(?<=\s))#[^\n]*)/g;
  let out = '', i = 0, cmd = true; // cmd: the next bare word starts a command
  const plain = t => { const m = cmd && /^(\s*)(\S+)/.exec(t);
    if (!m) return esc(t); cmd = false; return esc(m[1]) + `<span class="sh-c">${esc(m[2])}</span>` + esc(t.slice(m[0].length)); };
  for (const m of src.matchAll(re)) {
    out += plain(src.slice(i, m.index)); i = m.index + m[0].length;
    const k = m[1] ? 's' : m[2] ? 'v' : m[3] ? 'f' : m[4] ? 'o' : 'm';
    out += `<span class="sh-${k}">${esc(m[0])}</span>`;
    if (k === 'o') cmd = true; else if (k !== 'm') cmd = false;
  }
  return out + plain(src.slice(i));
}
const toolName = e => e.text.replace(/^↳ /, '').split(' ')[0];
// one line per call: shell → prompt + coloured command; else icon, tool, short gist (pattern / ±lines / …)
function actLine(e) {
  const d = e.detail || {}, n = toolName(e), lc = s => (s || '').split('\n').length;
  if (typeof d.command === 'string') return `<span class="sh-p">${n === 'PowerShell' ? 'PS&gt;' : '$'}</span> ${shHi(d.command)}`;
  const gist = 'old_string' in d ? (([r, a]) => `−${r} +${a}`)(dstat(d.old_string, d.new_string)) : Array.isArray(d.edits) ? `${d.edits.length} edits`
    : typeof d.content === 'string' ? `${lc(d.content)} lines` : d.pattern || d.query || d.url || d.description || d.prompt || (d.offset ? `from line ${d.offset}` : '');
  return `${TICON[n] ? icon(TICON[n]) : icon('term')} <span class="sh-c">${esc(n)}</span>${fpath(d)} <span class="sh-m">${esc(String(gist).slice(0, 200))}</span>`;
}
const fpath = d => (d = d.file_path || d.notebook_path || d.path) ? ` <span class="sh-s">${esc(d)}</span>` : ''; // full path on file rows, like the preview
// consecutive calls of one tool (outside the shell tab) = one row: Edit ±lines summed, ×N; click → all their diffs
function runLine(es) {
  if (es.length === 1) return actLine(es[0]);
  const n = toolName(es[0]), ds = es.map(e => e.detail || {});
  const st = ds.every(d => 'old_string' in d) && ds.map(d => dstat(d.old_string, d.new_string)), gist = st ? `−${st.reduce((s, x) => s + x[0], 0)} +${st.reduce((s, x) => s + x[1], 0)}` : '';
  return `${TICON[n] ? icon(TICON[n]) : icon('term')} <span class="sh-c">${esc(n)}</span>${fpath(ds[0])} <span class="sh-m">${gist}</span> <small class="sh-m">×${es.length}</small>`;
}
function drawAct(g) {
  const on = g._tabs.get(g._on), last = on.es.at(-1), runs = [];
  on.es.forEach(e => { const r = runs.at(-1); g._on !== 'sh' && r && toolName(r[0]) === toolName(e) ? r.push(e) : runs.push([e]); });
  g.innerHTML = `<div class="tbar"><i></i><i></i><i></i><span class="ttabs">${[...g._tabs].map(([k, t]) =>
      `<button data-k="${esc(k)}" class="${k === g._on ? 'on' : ''}" title="${esc(t.title)}">${esc(t.label)}${t.es.length > 1 ? `<small>×${t.es.length}</small>` : ''}</button>`).join('')}</span>
    ${g._on === 'sh' ? '<button class="cp" title="Copy the last command">Copy</button>' : ''}</div>
    <div class="tl">${runs.map((es, i) => `<div data-i="${i}" title="${esc(es[0].detail?.description || 'Details')}">${runLine(es)}</div>`).join('')}</div>`;
  g.querySelectorAll('.ttabs button').forEach(b => b.onclick = () => { g._on = b.dataset.k; g._pin = true; drawAct(g); });
  g.querySelectorAll('.tl > div').forEach(r => { r._es = runs[+r.dataset.i]; r.onclick = () => toolDlg(r._es, on.title); }); // _es: right-click menu
  if (busy[cur]) workTick(); // rebuilt rows → running ones get their shimmer back now, not on the next tick
  const cp = g.querySelector('.cp');
  if (cp) cp.onclick = () => { navigator.clipboard.writeText(last.detail.command); cp.textContent = 'Copied'; setTimeout(() => cp.textContent = 'Copy', 1200); };
  const tl = g.querySelector('.tl'); tl.scrollTop = tl.scrollHeight;
  // scroll only the tab strip; ≠ scrollIntoView ∵ that also scrolls #log → yanked a scrolled-up reader down on every tool update
  const ts = g.querySelector('.ttabs'), ob = ts.querySelector('.on');
  if (ob) { const a = ts.getBoundingClientRect(), b = ob.getBoundingClientRect();
    if (b.left < a.left) ts.scrollLeft += b.left - a.left; else if (b.right > a.right) ts.scrollLeft += b.right - a.right; }
}
// ponytail: diff = common head/tail lines as context, the middle as -/+ blocks (no LCS); enough for Edit's small hunks
// [removed, added] line counts, same head/tail trim as diff() → the row's −/+ matches what the diff shows
function dstat(a = '', b = '') {
  const x = a ? a.split('\n') : [], y = b ? b.split('\n') : [];
  let h = 0, t = 0;
  while (h < x.length && h < y.length && x[h] === y[h]) h++;
  while (t < x.length - h && t < y.length - h && x[x.length - 1 - t] === y[y.length - 1 - t]) t++;
  return [x.length - h - t, y.length - h - t];
}
function diff(a = '', b = '') {
  const x = a ? a.split('\n') : [], y = b ? b.split('\n') : [];
  let h = 0, t = 0;
  while (h < x.length && h < y.length && x[h] === y[h]) h++;
  while (t < x.length - h && t < y.length - h && x[x.length - 1 - t] === y[y.length - 1 - t]) t++;
  const row = (c, l) => `<span class="${c}">${c === 'ctx' ? ' ' : c === 'del' ? '-' : '+'} ${esc(l)}\n</span>`;
  return `<pre class="diff">${[...x.slice(0, h).map(l => row('ctx', l)), ...x.slice(h, x.length - t).map(l => row('del', l)),
    ...y.slice(h, y.length - t).map(l => row('add', l)), ...x.slice(x.length - t).map(l => row('ctx', l))].join('')}</pre>`;
}
function toolDlg(es, title = es[0].text) {
  const body = es.map(e => (e => { const t = e.text.replace(/^↳ /, '').split(' ')[0];
    return `<div class="faint mono row" style="font-size:12px;gap:6px">${TICON[t] ? icon(TICON[t]) : icon('term')}<span style="overflow-wrap:anywhere;min-width:0">${esc(e.text)}</span></div>`; })(e) + (d => {
    if ('old_string' in d) return diff(d.old_string, d.new_string);
    if (Array.isArray(d.edits)) return d.edits.map(x => diff(x.old_string, x.new_string)).join('');
    if (typeof d.content === 'string') return diff('', d.content);
    // a command: coloured, then its output (the tool result paired by id, live sessions only)
    const out = e.id && (logs[cur] || []).find(x => x.result_of === e.id);
    if (typeof d.command === 'string') return `<pre class="diff termout">${shHi(d.command)}</pre>` + (out ? `<pre class="diff ${out.kind === 'err' ? 'errout' : ''}">${esc(out.text)}</pre>` : '');
    return `<pre class="diff">${esc(JSON.stringify(d, null, 2))}</pre>`; })(e.detail || {})).join('');
  // #247 in the File viewer; plain text = what Save as…/Copy write (call, then command + output | new text | args)
  const text = es.map(e => { const d = e.detail || {}, out = e.id && (logs[cur] || []).find(x => x.result_of === e.id);
    return e.text + '\n' + (typeof d.command === 'string' ? d.command + (out ? '\n\n' + out.text : '') : 'new_string' in d ? d.new_string : Array.isArray(d.edits) ? d.edits.map(x => x.new_string).join('\n…\n') : typeof d.content === 'string' ? d.content : JSON.stringify(d, null, 2)); }).join('\n\n');
  fileView({name: (title.split(/[\\/]/).pop().replace(/[^\w.-]+/g, '_').slice(0, 60) || 'output') + '.txt', text, html: body});
}
/* #214 task report: code-built from the log when a turn that called mcp__clauracle__ready ends (no AI tokens).
   Slice = entries since the last report note (old ones too ∵ a task can span handovers). Worked = Σ gaps ≤ 10m.
   REPS[rid] = memory copy; disk copy 7 days (report_save/get) → 7-day PDF. ponytail: diffs cut at 4k chars each. */
const REPS = {}, REPBUSY = {}, REPGAP = 600e3;
const workMs = l => { let s = 0, p = 0; for (const e of l) { const t = Date.parse(e.ts); if (!t) continue; if (p && t > p) s += Math.min(t - p, REPGAP); p = t; } return s; };
function repDone(pid) {
  const l = logs[pid] || [], i = l.findLastIndex(e => e.act === 'rep'), sl = l.slice(i + 1);
  // #237 ready's tool row isn't logged (only its "Armed." result) → match either, else no report ever showed
  if (!sl.some(e => e.kind === 'tool' ? /^mcp__clauracle__ready/.test(e.text) : /^Armed\. When this turn ends/.test(e.text || ''))) return;
  const res = new Map(sl.filter(e => e.result_of).map(e => [e.result_of, e])), cut = s => (s || '').slice(0, 4000);
  const ops = sl.filter(e => e.kind === 'tool' && !isProcess(e)).map(e => { const r = res.get(e.id), d = e.detail || {}, f = d.file_path || d.notebook_path;
    return {t: toolName(e), text: e.text.replace(/^↳ /, '').slice(0, 300), cmd: typeof d.command === 'string' ? d.command.slice(0, 600) : '', ts: e.ts,
      ok: r?.kind !== 'err', err: r?.kind === 'err' ? r.text.slice(0, 800) : '', out: r && r.kind !== 'err' ? r.text.slice(0, 300) : '', f: f ? relOf(pid, f) : '',
      d: 'old_string' in d ? [[cut(d.old_string), cut(d.new_string)]] : Array.isArray(d.edits) ? d.edits.map(x => [cut(x.old_string), cut(x.new_string)]) : typeof d.content === 'string' ? [['', cut(d.content)]] : null}; });
  const now = new Date().toISOString(), segs = [];
  for (const e of sl) { if (e.kind === 'user' || !segs.length) segs.push({ts: e.ts, title: e.kind === 'user' ? bare(e.text).split('\n')[0].slice(0, 120) : 'Started', l: []}); segs.at(-1).l.push(e); }
  for (const s of segs) s.ts ||= s.l.find(e => e.ts)?.ts || now; // #245 restored transcript tool rows carry no ts → first dated entry, not "now"
  const segOut = segs.map((s, k) => { const c = s.l.filter(e => e.kind === 'tool').map(e => e.detail?.command || '').join('\n'), fs = new Set(s.l.map(e => e.detail?.file_path).filter(Boolean));
    return {ts: s.ts, end: segs[k + 1]?.ts || now, title: s.title, ms: workMs(s.l), n: s.l.filter(e => e.kind === 'tool' && !isProcess(e)).length, files: fs.size,
      closed: [...c.matchAll(/gh issue close (\d+)/g)].map(m => +m[1]), filed: [...s.l.filter(e => e.result_of && e.kind !== 'err').map(e => e.text).join('\n').matchAll(/\/issues\/(\d+)\s*$/gm)].map(m => +m[1])}; });
  const rid = Date.now(), rep = {id: rid, project: P(pid)?.name, t0: sl.find(e => e.ts)?.ts || now, t1: now, ms: Math.max(workMs(sl), 60e3) /* dur() floors */, ops, segs: segOut,
    notes: sl.filter(e => e.kind === 'note' && e.ts).map(e => ({ts: e.ts, text: e.text}))}; // #363 status notes → timeline; later ones in the same run appended by addEntry
  REPS[rid] = rep; call('report_save', {pid, rep}).catch(() => {});
  addEntry(pid, {kind: 'note', text: `Done! Worked for ${dur(rep.ms)}.`, act: 'rep', rid, ts: now});
}
async function repOpen(rid, tb = 'done') {
  const pid = cur, r = REPS[rid] ||= await call('report_get', {pid, id: +rid}).catch(() => null);
  if (!r) return toast("That report is older than 7 days, or wasn't saved.", 'warn');
  const ok = r.ops.filter(o => o.ok), bad = r.ops.filter(o => !o.ok), files = {};
  for (const o of r.ops) if (o.f && o.d) (files[o.f] ||= {n: 0, d: [], w: o.t === 'Write'}).d.push(...o.d), files[o.f].n++;
  const closed = r.segs.flatMap(s => s.closed), counts = {}; ok.filter(o => !o.cmd).forEach(o => counts[o.t] = (counts[o.t] || 0) + 1);
  const tile = (n, l, i) => `<div class="rtile" title="${n} ${l}">${icon(i)}<b>${n}</b></div>`;
  const body = tb === 'done' ? `<div class="rtiles">${tile(dur(r.ms), 'worked', 'timer')}${tile(ok.filter(o => o.cmd).length, 'commands', 'term')}${tile(Object.keys(files).length, 'files', 'file')}${tile(closed.length, 'issues closed', 'check')}</div>
      ${ok.filter(o => o.cmd).map(o => `<div class="rrow">${icon('check')}<code class="grow">${esc(o.cmd.split('\n')[0])}</code><small class="faint">${tm(o.ts)}</small></div>`).join('') || '<p class="dim">No commands run.</p>'}
      ${Object.keys(counts).length ? `<p class="faint" style="margin:6px 0">${Object.entries(counts).map(([t, n]) => `${esc(t)} ×${n}`).join(' · ')}</p>` : ''}
      ${Object.keys(files).length ? `<h3>Files touched</h3><div class="rfiles">${Object.entries(files).map(([f, x]) => `<button class="btn sm" data-rf="${esc(f)}">${icon(x.w ? 'file' : 'new')}${esc(f.split('/').pop())}${x.n > 1 ? ` <span class="pill">×${x.n}</span>` : ''}</button>`).join('')}</div><div id="rdiff"></div>` : ''}`
    : tb === 'fail' ? bad.map((o, k) => { const m = missOf(o.err); return `<div class="rfail">${icon('warn')}<div class="grow" style="min-width:0"><code>${esc((o.cmd || o.text).split('\n')[0])}</code>
        <pre class="diff errout">${esc(o.err.trim().split('\n').slice(-6).join('\n'))}</pre><div class="row" style="gap:6px;justify-content:flex-end;margin-top:10px">${m ? `<button class="btn sm primary" data-fix="${k}">${esc(m.label)}</button>` : ''}<button class="btn sm" data-ask="${k}">${icon('chat')}Ask Clauracle to fix</button></div></div></div>`; }).join('') || `<p class="dim">${icon('check')} Nothing failed.</p>`
    : `<div class="rtl">${[...r.segs.map(s => [s.ts, `<div class="rstep"><small class="mono faint">${tm(s.ts)}</small><i></i><div class="grow" style="min-width:0"><b>${esc(s.title)}</b>
        <div class="faint" style="font-size:12px">${s.n} steps · ${s.files} files${s.closed.map(n => ` · <span class="pill dim">closed #${n}</span>`).join('')}${s.filed.map(n => ` · <span class="pill dim">filed #${n}</span>`).join('')}</div></div><small>${dur(s.ms)}</small></div>`]),
        ...(r.notes || []).map(n => [n.ts, `<div class="rstep"><small class="mono faint">${tm(n.ts)}</small><i></i><span class="grow dim" style="font-size:12px">${esc(n.text)}</span></div>`])] /* #363 status notes */
        .sort((a, b) => Date.parse(a[0]) - Date.parse(b[0])).map(x => x[1]).join('')}
        <div class="rstep tot"><small class="mono faint">${tm(r.t1)}</small><i></i><b class="grow">Done</b><b>${dur(r.ms)}</b></div></div>`;
  const tabs = [['done', 'check', 'Completed', ok.length], ['fail', 'warn', 'Failed', bad.length], ['tl', 'clock', 'Timeline', '']];
  dlg(`<div class="rhead"><h2>Done! Worked for ${dur(r.ms)}</h2><small class="dim">${esc(r.project || '')} · ${tm(r.t0)} – ${tm(r.t1)}</small>
      <div class="seg" style="margin-top:10px">${tabs.map(([k, i, l, n]) => `<button class="${k === tb ? 'on' : ''}" data-rt="${k}">${icon(i)}${l}${n !== '' ? ` <span class="pill dim">${n}</span>` : ''}</button>`).join('')}</div></div>
    <div class="dscroll wrap rbody">${body}</div>
    <div class="btns"><button class="btn rpdf" onclick="repPdf('${rid}')">${icon('sparkle')}Write a detailed report (.pdf)</button><button class="btn rpdf" onclick="repPdf()">${icon('cal')}Write a 7-day report (.pdf)</button><span class="grow"></span><button class="btn ghost" onclick="closeDlg()">Close</button></div>`, 'small-dlg wide');
  const d = $('dlg'); repBtns();
  d.querySelectorAll('[data-rt]').forEach(b => b.onclick = () => repOpen(rid, b.dataset.rt));
  d.querySelectorAll('[data-rf]').forEach(b => b.onclick = () => { d.querySelectorAll('[data-rf]').forEach(x => x.classList.toggle('on', x === b)); $('rdiff').innerHTML = `<p class="mono faint" style="margin:10px 0 4px">${esc(b.dataset.rf)}</p>` + files[b.dataset.rf].d.map(([a, z]) => diff(a, z)).join(''); });
  d.querySelectorAll('[data-fix]').forEach(b => b.onclick = async () => { const m = missOf(bad[b.dataset.fix].err); b.disabled = true; toast(m.label + '…');
    try { await call('fix_missing', {pid, kind: m.k, name: m.n, feat: m.f || null, pkg: m.pkg || null}); toast('Done: ' + m.label, 'ok'); DEPSC = null; } catch (e) { toast(String(e), 'err'); b.disabled = false; } });
  d.querySelectorAll('[data-ask]').forEach(b => b.onclick = () => { const o = bad[b.dataset.ask]; closeDlg(); putPrompt(`This failed during the last task; fix it:\n${o.cmd || o.text}\n${o.err.trim().split('\n').slice(-6).join('\n')}`); });
}
const repBtns = () => document.querySelectorAll('.rpdf').forEach(b => { b.disabled = !!REPBUSY[cur]; b.classList.toggle('busy', !!REPBUSY[cur]); });
async function repPdf(rid, pid = cur) { // rid = that task, none = 7-day (#413: task = code-built HTML, 7-day = Sonnet), buttons off meanwhile (any window)
  if (REPBUSY[pid]) return; REPBUSY[pid] = true; repBtns();
  // #247 chat pill (shimmers while REPBUSY) → turns into "Report ready!" + (pdf icon) filename → File viewer
  const e = {kind: 'note', text: rid ? 'Writing the detailed report…' : 'Writing the 7-day report…', pill: 'pdf', rb: 1}; addEntry(pid, e);
  try { const f = await call('report_pdf', {pid, rep: rid ? REPS[rid] : null}); Object.assign(e, {text: 'Report ready!', pill: null, act: 'pdf', file: f});
    if (pid === cur && !$('dlg').open) openPdf(f); else toast('Report ready: ' + f.split(/[\\/]/).pop(), 'ok', ['Open', () => openPdf(f)], true, () => openPdf(f)); }
  catch (x) { Object.assign(e, {text: 'Report failed: ' + String(x).slice(0, 200), pill: 'warn'}); toast(String(x), 'err'); }
  delete e.rb; delete REPBUSY[pid]; repBtns(); saveLog(pid); if (pid === cur) drawLog();
}
// streaming text: #live is the bubble (deltas rewrite it); its row goes when the full entry lands
function liveEl(pid) { const d = document.createElement('div'); d.className = 'msg claude'; d.id = 'live'; d.innerHTML = md(live[pid]);
  return view === 'chat' ? chatRow('claude', d, new Date().toISOString()) : d; }
function addEntry(pid, e) {
  if (hand[pid] === true && !['err', 'ask', 'answered'].includes(e.kind)) return; // a handover turn: only the pill shows
  // writing plan/handover/changes = wrapping up → "Preparing plan…"; reading them (every fresh start) ≠ that, and real work after ends it
  const wrap = isProcess(e) && (/^mcp__clauracle__/.test(e.text) || /^(Edit|MultiEdit|Write)\b/.test(toolName(e)));
  if (busy[pid] && (wrap ? !hand[pid] : e.kind === 'tool' && !isProcess(e) && hand[pid] === 'soft')) { hand[pid] = wrap ? 'soft' : false; if (pid === cur) drawChrome(); }
  e.ts ||= new Date().toISOString(); // all kinds: #214 report timeline/worked time
  (logs[pid] ||= []).push(e); saveLog(pid);
  if (e.kind === 'note' && !e.act) { const l = logs[pid], i = l.findLastIndex(x => x.act === 'rep'), r = i >= 0 && REPS[l[i].rid]; // #363 notes right after Done (commit, stage…) = same work run → into its report
    if (r && l.slice(i + 1).every(x => x.kind === 'note')) { (r.notes ||= []).push({ts: e.ts, text: e.text}); r.t1 = e.ts; call('report_save', {pid, rep: r}).catch(() => {}); } }
  if (e.kind === 'tool' && /^(Edit|MultiEdit|Write|NotebookEdit)\b/.test(toolName(e)) && (e.detail?.file_path || e.detail?.notebook_path)) {
    (EDITS[pid] ||= {})[relOf(pid, e.detail.file_path || e.detail.notebook_path)] = Date.now(); if (pid === cur) badgeFiles();
  }
  if (pid !== cur) return;
  const log = $('log'), box = log.querySelector('.col'), stick = atEnd();
  box.querySelector('#empty')?.remove(); const lv = $('live'); (lv?.closest('.crow') || lv)?.remove();
  box._swap = !!lv && e.kind === 'claude'; put(box, e); box._swap = false; // final text replacing the streamed one: hatched already
  if (live[pid]) box.appendChild(liveEl(pid));
  if (e.kind === 'tool') drawRails();
  toEnd(stick, 1);
}

