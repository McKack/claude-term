// ---- image viewer (chat, terminal, composer attachments) + editor: pen and numbered boxes with notes ----
let VW = null; // {imgs: [{url, name}], i, map: composer attachment index per image | null (from chat → added as new)}
// big pastes → chip: more than this many lines or characters (ponytail: fixed numbers; a setting if someone asks)
const PASTE_LINES = 12, PASTE_CHARS = 1200;
const tsize = t => { const n = t.split('\n').length; return n > 1 ? n + ' lines' : t.length + ' characters'; };
const attText = i => { const a = atts[cur][i]; textView(stem(a.name), a.text); };
// read-only text in a thin frame: title + X; Esc or a click outside closes it
function textView(title, text) { document.querySelector('dialog.tview')?.remove();
  const d = Object.assign(document.createElement('dialog'), {className: 'tview glass'});
  d.innerHTML = `<div class="tvh"><span>${esc(title)}</span><span class="faint">${tsize(text)}</span><button class="ib x" title="Close (Esc)">${icon('x')}</button></div><pre></pre>`;
  d.querySelector('pre').textContent = text; d.querySelector('.ib.x').onclick = () => d.close();
  d.onclick = e => { if (e.target === d) d.close(); }; d.onclose = () => d.remove();
  document.body.appendChild(d); d.showModal(); }
function viewAtt(i) {
  const a = atts[cur] || [], map = a.map((x, k) => x.url ? k : -1).filter(k => k >= 0);
  openViewer(map.map(k => a[k]), map.indexOf(i), map);
}
function openViewer(imgs, i, map) { VW = {imgs, i, map}; drawViewer(); }
const edPending = () => ED && !ED.tab && VW?.map && $('edC') && $('edSave') && !$('edSave').disabled; // composer image with unsaved marks
function drawViewer() { if (edPending()) try { edDone(1); } catch {} edStash(); ED = null; editImage(); } // #232 opens straight in edit mode; stepping applies unsaved marks first
const stepViewer = d => { VW.i = (VW.i + d + VW.imgs.length) % VW.imgs.length; drawViewer(); };
/* #217 vector editor: ops = shapes (pen pts | line | box+note), selectable/movable/resizable/erasable; flattened to one PNG on save
   (PNG ∵ tokens ∝ pixels, not format; JPG blurs text). Attachment keeps a.ed = {orig, ops, nmax} → re-edit stays vector + box numbers
   never repeat (nmax persists); a.notes = ["1. …"] → sent inside the message (sendNow), never typed into the composer. */
/* #294 EDC2 = [primary, secondary]; 'transparent' = valid canvas colour (≠ input value). Left btn draws color=primary fill=secondary, right swaps.
   fill only shows as the box dot's number colour (note boxes/lines/pen ignore it). Colour change → recolours ED.sel, else ED.act (last focused note) */
const EDC0 = ['#ff4d4f', '#ffffff'];
let ED = null, EDC2 = (() => { try { return [localStorage.penColor || EDC0[0], localStorage.penColor2 || EDC0[1]]; } catch { return [...EDC0]; } })();
const edCols = a => a.forEach((v, k) => edSetCol(k, v, 1)); /* #455 reset/swap: only the swatches, never recolours the selection */
function edSetCol(k, v, q) { EDC2[k] = v; try { localStorage[k ? 'penColor2' : 'penColor'] = v; } catch {}
  const s = $('edCol' + k); s.classList.toggle('tr', v === 'transparent'); if (v !== 'transparent') s.value = v;
  if (q) return;
  const T = ED?.sel.length ? ED.sel : ED?.act && ED.ops.includes(ED.act) ? [ED.act] : [];
  if (T.length) { if (!ED.csnap) { ED.csnap = 1; edSnap(); } T.forEach(o => { if (!(k && EDSH.test(o.t) && !o.fill)) o[k ? 'fill' : 'color'] = v; }); /* unfilled shape stays unfilled */ edDraw(); edNotes(); } }
const ETOOLS = [['sel', 'cursor', 'Select: click picks (again = next one below), drag empty space = box/freehand, Shift/Ctrl+click adds/removes, drag inside the box moves, handles scale, Delete removes'], ['pen', 'pen', 'Pen: draw freehand'], ['line', 'line', 'Line: straight line'], ['rect', 'rect', 'Rectangle: drag (Shift = square)'], ['ellipse', 'ellipse', 'Ellipse: drag (Shift = circle)'], ['text', 'text', 'Text: click to type, click a text to edit it, Enter = done'], ['box', 'boxnote', 'Box + note: drag a box, then type a note for it'], ['erase', 'eraser', 'Eraser: click a shape to remove it']]; // #238 icons + data-tip
function editImage() {
  const m = VW.imgs[VW.i], n = VW.imgs.length; VW.ei = VW.i; // ei = image the editor holds (VW.i moves first on a switch)
  // canvas fits the stage (any window size); notes re-placed ∵ scale changed
  // #379 every image = a tab (Edit-view look; ← → still step). ponytail: no ✕ per tab ∵ images belong to the chat/composer
  vWin(`<div class="vtabs" onpointerdown="event.stopPropagation()">${VW.imgs.map((x, k) => `<div class="et${k === VW.i ? ' on' : ''}" title="${esc(x.name)}" onclick="${k === VW.i ? '' : `VW.i = ${k}; drawViewer()`}"><img src="${x.url}" alt=""><span>${esc(x.name)}</span></div>`).join('')}</div>`,
    `<button class="btn primary sm" id="edSave" onclick="edDone()"${VW.map ? ' disabled title="Nothing changed yet"' : ''}>${VW.map ? 'Save' : 'Add to message'}</button>`,
    edToolsHtml(), '<div id="edWrap"><canvas id="edC"></canvas></div>', () => { if (ED) edNotes(); }, 'img', ' ');
  edInit(m.ed?.orig || m.url, m.ed);
}
const edToolsHtml = () => `<span class="seg" id="edTools">${ETOOLS.map(([k, l, t]) => `<button data-tool="${k}" class="${k === 'pen' ? 'on' : ''}" data-tip="${t}">${icon(l)}</button>`).join('')}</span>
    <span class="edcols"><span class="edpair">${EDC2.map((v, k) => `<input type="color" id="edCol${k}" class="${v === 'transparent' ? 'tr' : ''}" value="${v === 'transparent' ? '#ffffff' : v}" data-tip="${k ? 'Secondary colour: right mouse button (left button: number in the note dot)' : 'Primary colour: left mouse button'}. Also recolours the selection / the note being typed">`).join('')}<button class="edsw2" data-tip="Swap primary and secondary" onclick="edCols([EDC2[1], EDC2[0]])">${icon('swap')}</button><button class="edrs" data-tip="Reset to default colours" onclick="edCols(EDC0)"></button></span>
      <button class="edtr" data-tip="Transparent: click = primary, right-click = secondary"></button></span>`;
/* #435 editor wiring shared by the File viewer dialog + Edit-view image tabs (tab = Edit tab → ED.tab). ids (#edC, #vctx…) ∴ one editor at a time: edStash parks the tab's marks */
function edStash() { if (!ED?.tab) return; if (VZ === ED.tab.z) VZ = null; ED.tab.ed = {ops: ED.ops, nmax: ED.nmax}; ED = null; const b = $('ebody'); b._t = null; b.innerHTML = ''; }
function edInit(src, ed, tab) {
  $('edTools').querySelectorAll('button').forEach(b => b.onclick = () => { if (!ED) return; ED.tool = b.dataset.tool; ED.sel = []; $('edTools').querySelectorAll('button').forEach(x => x.classList.toggle('on', x === b)); edCursor(); edDraw(); edCtx(); });
  [0, 1].forEach(k => { const s = $('edCol' + k); s.oninput = () => edSetCol(k, s.value); s.onchange = () => { if (ED) ED.csnap = 0; }; }); // one undo snap per picker session
  const tr = document.querySelector('.edtr'); tr.onclick = () => { edSetCol(0, 'transparent'); if (ED) ED.csnap = 0; };
  tr.oncontextmenu = e => { e.preventDefault(); e.stopPropagation(); edSetCol(1, 'transparent'); if (ED) ED.csnap = 0; };
  const img = new Image(), c = $('edC');
  img.onload = () => { if (!c.isConnected) return; // tab switched / dialog closed before load
    c.width = img.naturalWidth; c.height = img.naturalHeight;
    // #436 no clamp: start at (stage − padding)·95%, ↓ to 10% steps, ≤100%; window resize keeps it. Viewer: VZ.s = shown %, transform = s/b
    const S = c.closest('.vstage'), z = tab ? (tab.z ||= {}) : ((VW.z ||= [])[VW.i] ||= {}); // #438 zoom kept: Edit tab = for good, viewer = per image till closed
    if (!z.b) Object.assign(z, {b: Math.max(1, Math.floor(Math.min(1, (S.clientWidth - 28) * .95 / c.width, (S.clientHeight - 28) * .95 / c.height) * 10)) / 10, x: 0, y: 0});
    z.s ||= z.b; VZ = Object.assign(z, {k: 'img', st: S}); c.style.width = c.width * z.b + 'px'; vZoom(1);
    ED = {img, ops: structuredClone(ed?.ops || []), nmax: ed?.nmax || 0, tool: 'pen', cur: null, sel: [], drag: null, undo: [], redo: [], w: Math.max(3, img.naturalWidth / 300), tab};
    edCursor(); edDraw(); edNotes(); edCtx();
    const pt = e => { const r = c.getBoundingClientRect(); return [(e.clientX - r.left) * c.width / r.width, (e.clientY - r.top) * c.height / r.height]; };
    c.oncontextmenu = e => { e.preventDefault(); e.stopPropagation(); }; // right btn = secondary colour, no menu
    c.onpointerdown = e => { if (e.button === 1) return; c.setPointerCapture(e.pointerId); const [x, y] = pt(e), t = ED.tool; ED.act = null;
      const [color, fill] = e.button === 2 ? [EDC2[1], EDC2[0]] : EDC2;
      if (t === 'text') { c.releasePointerCapture(e.pointerId); const o = edHit(x, y); return edText(o?.t === 'text' ? o : null, x, y, color); }
      if (t === 'erase') { const o = edHit(x, y); if (o) { edSnap(); ED.ops.splice(ED.ops.indexOf(o), 1); edDraw(); edNotes(); } return; }
      if (t === 'sel') { // #240 handle → scale | shape/inside box → move all | empty → marquee (Shift/Ctrl = add/toggle)
        const add = e.shiftKey || e.ctrlKey || e.metaKey, h = add ? -1 : edHandle(x, y), o = h < 0 && edHit(x, y);
        if (add && o) { const k = ED.sel.indexOf(o); k < 0 ? ED.sel.push(o) : ED.sel.splice(k, 1); return edDraw(); }
        if (h >= 0 || o || (ED.sel.length && edIn(x, y))) { const prev = ED.sel.length === 1 ? ED.sel[0] : null; if (o && !ED.sel.includes(o)) ED.sel = [o]; edSnap();
          ED.drag = {x, y, h, o, prev, bb: edSelBB(), src: structuredClone(ED.sel), moved: false}; return edDraw(); }
        const base = add ? [...ED.sel] : []; ED.sel = [...base]; ED.lc = null; // #452 empty space: shape mode = deselect only
        if (edSM !== 'shape') ED.drag = edSM === 'lasso' ? {lz: [[x, y]], base} : {mq: [x, y, x, y], base}; return edDraw(); }
      edSnap(); ED.cur = t === 'pen' ? {t, color, lw: edLW, ds: edDS, pts: [[x, y]]} : t === 'line' ? {t, color, lw: edLW, ds: edDS, a0: edArr[0], a1: edArr[1], x1: x, y1: y, x2: x, y2: y} : EDSH.test(t) ? {t, color, fill: edFill ? fill : null, lw: edLW, x, y, w: 0, h: 0} : {t: 'box', color, fill, lw: edLW, x, y, w: 0, h: 0, note: '', n: ++ED.nmax}; ED.ops.push(ED.cur); };
    c.onpointermove = e => { const [x, y] = pt(e);
      const D = ED.drag;
      if (D?.mq) { D.mq[2] = x; D.mq[3] = y; const [a, b, c2, d] = D.mq, l = Math.min(a, c2), r = Math.max(a, c2), tp = Math.min(b, d), bt = Math.max(b, d); // touches = bbox overlap
        ED.sel = [...new Set([...D.base, ...ED.ops.filter(o => { const [p, q, s, u] = edBB(o); return p <= r && s >= l && q <= bt && u >= tp; })])]; return edDraw(); }
      if (D?.lz) { D.lz.push([x, y]); const P = D.lp = new Path2D(), g = c.getContext('2d'); D.lz.forEach(([a, b], k) => k ? P.lineTo(a, b) : P.moveTo(a, b)); P.closePath(); // #452 lasso: any shape point inside (pen pts | line ends | box corners+centre)
        ED.sel = [...new Set([...D.base, ...ED.ops.filter(o => (o.t === 'pen' ? o.pts : o.t === 'line' ? [[o.x1, o.y1], [o.x2, o.y2]] : [[o.x, o.y], [o.x + o.w, o.y], [o.x, o.y + o.h], [o.x + o.w, o.y + o.h], [o.x + o.w / 2, o.y + o.h / 2]]).some(([a, b]) => g.isPointInPath(P, a, b)))])]; return edDraw(); }
      if (D) { D.moved = true;
        if (D.h >= 0) { const [fx, fy] = EDH[D.h], [a, b, c2, d] = D.bb, ax = a + (1 - fx) * (c2 - a), ay = b + (1 - fy) * (d - b), hx = a + fx * (c2 - a), hy = b + fy * (d - b); // anchor = opposite handle
          const sx = fx === .5 || Math.abs(hx - ax) < 1 ? 1 : (x - ax) / (hx - ax), sy = fy === .5 || Math.abs(hy - ay) < 1 ? 1 : (y - ay) / (hy - ay); // flat bbox axis → no scale (÷0)
          ED.sel.forEach((o, k) => edScale(o, D.src[k], ax, ay, sx, sy)); }
        else { ED.sel.forEach(o => edMove(o, x - D.x, y - D.y)); D.x = x; D.y = y; }
        edDraw(); edNotes(); return; }
      if (ED.tool === 'sel') { const h = edHandle(x, y); c.style.cursor = h >= 0 ? EDC[h] + '-resize' : edHit(x, y) || (ED.sel.length && edIn(x, y)) ? 'move' : EDCUR.sel; }
      const o = ED.cur; if (!o) return;
      if (o.t === 'pen') { const m = o.sm ||= [...o.pts[0]], [px, py] = o.pts.at(-1); m[0] += (x - m[0]) * .35; m[1] += (y - m[1]) * .35; o.raw = [x, y]; if (Math.hypot(m[0] - px, m[1] - py) > ED.w / 2) o.pts.push([...m]); } // #292 EMA smoothing ∵ raw pointer jitters; ↑ factor = less lag, more jitter
      else if (o.t === 'line') { let dx = x - o.x1, dy = y - o.y1; if (e.shiftKey) { const L = Math.hypot(dx, dy), a = Math.round(Math.atan2(dy, dx) / (Math.PI / 8)) * Math.PI / 8; dx = L * Math.cos(a); dy = L * Math.sin(a); } o.x2 = o.x1 + dx; o.y2 = o.y1 + dy; } /* #292 Shift = 22.5° steps */ else { o.w = x - o.x; o.h = y - o.y; if (e.shiftKey && o.t !== 'box') { const m = Math.max(Math.abs(o.w), Math.abs(o.h)); o.w = Math.sign(o.w || 1) * m; o.h = Math.sign(o.h || 1) * m; } } edDraw(); };
    c.onpointerup = e => { const D = ED.drag; ED.drag = null;
      if (D) { if (!D.mq && !D.lz && !D.moved) { edUnsnap(); let o = D.o; const L = ED.lc; ED.lc = [e.clientX, e.clientY]; // plain click: no undo entry, picks just that shape
        if (o && edCT && L && Math.abs(e.clientX - L[0]) <= 2 && Math.abs(e.clientY - L[1]) <= 2) { const H = edHits(D.x, D.y), i = H.indexOf(D.prev); if (i >= 0) o = H[(i + 1) % H.length]; } // #452 click-through: same spot → next shape below (wraps)
        if (o) ED.sel = [o]; } return edDraw(); }
      const o = ED.cur; ED.cur = null; if (!o) return;
      if (o.t === 'box') { if (Math.abs(o.w) < 8 && Math.abs(o.h) < 8) o.w = o.h = 0; // #248 click = numbered point (note, no box)
        if (o.w < 0) { o.x += o.w; o.w = -o.w; } if (o.h < 0) { o.y += o.h; o.h = -o.h; } edDraw(); return edNotes(o); }
      if (o.t === 'line' && Math.hypot(o.x2 - o.x1, o.y2 - o.y1) < 2) { ED.ops.pop(); edUnsnap(); }
      if (EDSH.test(o.t)) { if (Math.abs(o.w) < 2 || Math.abs(o.h) < 2) { ED.ops.pop(); edUnsnap(); } if (o.w < 0) { o.x += o.w; o.w = -o.w; } if (o.h < 0) { o.y += o.h; o.h = -o.h; } }
      if (o.t === 'pen') { if (o.raw) o.pts.push(o.raw); delete o.sm; delete o.raw; } // EMA lags → end exactly at the release point
      edDraw(); };
    c.onpointercancel = c.onpointerup; // #237 box numbered at pointerdown (was pointerup → 'undefined' tag when the up got lost)
  };
  img.src = src;
}
let VBOX = (() => { try { return JSON.parse(localStorage.vbox || 'null'); } catch { return null; } })(), VF = null; // VBOX = window size kept across steps/opens/restarts (localStorage.vbox); VF = file in the File viewer {name, path?, text?}
/* #247 one File viewer window (image editor template): head = left of the bar, act = right buttons (before ✕),
   tools = left toolbar (contextual per file type), stage = content; fit(stage) on every resize */
function vWin(head, act, tools, stage, fit, zoom, ctx = '') {
  VZ = zoom ? {k: zoom, s: 1, x: 0, y: 0} : null; // #248 zoom: 'img' = transform (pan too) | 'text' = CSS zoom; pdf = its own viewer's zoom
  dlg(`<div class="vbar" onpointerdown="vDrag(event)">${head}<span class="grow"></span><button class="ib x" title="Close" onclick="closeDlg()">${icon('x')}</button></div>
    <div class="vctx" id="vctx">${ctx}${zoom === 'text' ? `<span class="vur">${zoomCtl('Math.round(VZ.s * 100)', 'vZp(p)', ', Ctrl+wheel, Ctrl + / −')}</span>` : ''}</div><div class="vtools">${tools}</div><div class="vstage">${stage}</div>${act ? `<div class="vfoot">${act}</div>` : ''}`, 'viewer'); /* #437 zoom right in the bar, act = footer bottom-right */
  const V = $('dlg').querySelector('.viewer'), S = V.querySelector('.vstage');
  if (VBOX) { V.style.width = VBOX[0] + 'px'; V.style.height = VBOX[1] + 'px'; } // #298 .viewer max-* (< dialog's UA max = 100% − 2em − 6px) clamps ∴ ✕ + resize corner never clipped, follows window shrink
  // #261 offsetWidth 0 = dlg closing (display:none) → don't keep, else reopens at min size
  new ResizeObserver(() => { if (!V.isConnected || !V.offsetWidth) return; if (V.style.width) { VBOX = [V.offsetWidth, V.offsetHeight]; try { localStorage.vbox = JSON.stringify(VBOX); } catch {} } fit?.(S); vClamp(); }).observe(V);
  const d = $('dlg'); if (!d.style.left) { const r = d.getBoundingClientRect(); Object.assign(d.style, {margin: '0', right: 'auto', bottom: 'auto', left: r.left + 'px', top: r.top + 'px'}); } // pinned where it opened ∴ resize corner tracks the mouse (centred = grows both ways)
  if (VZ) VZ.st = S; vWire(S, zoom); }
/* #438 stage zoom/pan, shared by the viewer + Edit-view image tabs: img = plain wheel + middle-drag pan, text = Ctrl+wheel. stopPropagation ∵ #ebody's Ctrl+wheel = UI zoom */
function vWire(S, zoom) { if (!zoom) return;
  S.onwheel = e => { if (zoom === 'text' && !e.ctrlKey) return; e.preventDefault(); e.stopPropagation(); vZp(zStep(Math.round(VZ.s * 100), e.deltaY < 0 ? 1 : -1), e); }; // text: plain wheel scrolls. #393 10% snap steps
  S.onmousedown = e => { if (e.button !== 1 || zoom !== 'img') return; e.preventDefault(); let px = e.clientX, py = e.clientY; // middle-drag pan (preventDefault = no autoscroll)
    const mv = ev => { VZ.x += ev.clientX - px; VZ.y += ev.clientY - py; px = ev.clientX; py = ev.clientY; vZoom(1); }, up = () => { removeEventListener('mousemove', mv); removeEventListener('mouseup', up); };
    addEventListener('mousemove', mv); addEventListener('mouseup', up); };
}
let VZ = null; const vZp = (p, e) => VZ && vZoom(Math.min(800, Math.max(10, p)) / 100 / VZ.s, e);
// f = factor; e = pointer → that point stays put (transform-origin 0 0: screen = layout + t + s·u ∴ t' = P − O − s'/s·(P − O − t))
function vZoom(f, e) {
  if (!VZ) return; const S = VZ.st || $('dlg').querySelector('.vstage'), el = S?.firstElementChild; if (!el) return;
  const s2 = f === 0 ? VZ.b || 1 : Math.min(8, Math.max(.1, VZ.s * f)); // 0 = reset to the #436 fit
  if (f === 0) VZ.x = VZ.y = 0;
  else if (VZ.k === 'img') { const r = el.getBoundingClientRect(), ox = r.left - VZ.x, oy = r.top - VZ.y, px = e ? e.clientX : r.left + r.width / 2, py = e ? e.clientY : r.top + r.height / 2;
    VZ.x = px - ox - s2 / VZ.s * (px - ox - VZ.x); VZ.y = py - oy - s2 / VZ.s * (py - oy - VZ.y); }
  VZ.s = s2;
  if (VZ.k === 'img') { el.style.transformOrigin = '0 0'; el.style.transform = `translate(${VZ.x}px, ${VZ.y}px) scale(${VZ.s / (VZ.b || 1)})`; } else el.style.zoom = VZ.s;
  const z = (S.closest('#edv') || S.parentElement)?.querySelector('.vctx .zctl .zp'); if (z) z.textContent = Math.round(VZ.s * 100) + '%';
}
// read-only files → "Save as…" (editable ones = images → Save in editImage). f = {name, path?(copied as is), text?(saved), url?(pdf blob), html?(text view)}
function fileView(f) {
  VF = f; const pdf = /\.pdf$/i.test(f.path || ''), wrap = localStorage.diffWrap !== '0', md = !!fmtKind(f.name) && f.text != null;
  if (md) f.html = docHtml(f.text, f.name, wrap); // #371/#392 formatted/raw per file type (fmtOn)
  vWin(`${icon(pdf ? 'pdf' : 'term')}<span class="vname">${esc(f.name)}</span>`, `<button class="btn primary sm" onclick="vSaveAs()">Save as…</button>`,
    (pdf ? '' : (md ? `<button class="ib${fmtOn(f.name) ? ' on' : ''}" title="Formatted view" onclick="fmtFlip(VF.name); fileView(VF)">${icon('eye')}</button>` : '') + (/class="code/.test(f.html) || md ? hlBtn('', isMd(f.name) && mdOn()) : '') + `<button class="ib${wrap ? ' on' : ''}" title="Wrap long lines" onclick="const s=$('dlg').querySelector('.vtext');try{localStorage.diffWrap=s.classList.toggle('wrap')?'1':'0'}catch{}this.classList.toggle('on',s.classList.contains('wrap'))">${icon('wrap')}</button>
      <button class="ib" title="Copy all" onclick="navigator.clipboard.writeText(VF.text);toast('Copied','ok')">${icon('copy')}</button>`)
    + (f.path ? `<button class="ib" title="Show in folder" onclick="T.opener.revealItemInDir(VF.path).catch(e=>toast(String(e),'err'))">${icon('folder')}</button>` : ''),
    pdf ? `<iframe class="vdoc" src="${f.url}"></iframe>` : `<div class="dscroll vtext sel${wrap ? ' wrap' : ''}">${f.html}</div>`, null, pdf ? null : 'text');
}
async function vSaveAs() {
  const to = await T.dialog.save({defaultPath: VF.name}); if (!to) return;
  try { await call('save_as', {to, from: VF.path || null, text: VF.path ? null : VF.text}); toast('Saved ' + to.split(/[\\/]/).pop(), 'ok'); } catch (e) { toast(String(e), 'err'); }
}
async function openPdf(path) {
  try { const b = await call('read_image', {path}); fileView({name: path.split(/[\\/]/).pop(), path, url: URL.createObjectURL(new Blob([b], {type: 'application/pdf'}))}); }
  catch (e) { toast(String(e), 'err'); }
}
// #239 window may hang ≤50% past the left/right/bottom app edges; top ≥ 0 ∴ the header (drag handle) stays reachable
function vClamp(dx = 0, dy = 0) { const d = $('dlg'); if (!d.style.left) return; const r = d.getBoundingClientRect();
  d.style.left = Math.max(-r.width / 2, Math.min(innerWidth - r.width / 2, r.left + dx)) + 'px'; d.style.top = Math.max(0, Math.min(innerHeight - r.height / 2, r.top + dy)) + 'px'; }
function vDrag(e) { if (e.target.closest('button, input')) return; let px = e.clientX, py = e.clientY;
  dragOn(e, ev => { vClamp(ev.clientX - px, ev.clientY - py); px = ev.clientX; py = ev.clientY; }); }
const edDirty = () => { const b = $('edSave'); if (b) { b.disabled = false; b.removeAttribute('title'); } if (ED?.tab) { ED.tab.dirty = 1; $('etabs').querySelector('.et.on')?.classList.add('dirty'); } }; // #232 Save greyed till a change
const edSnap = () => { edDirty(); ED.undo.push(JSON.stringify([ED.ops, ED.nmax])); ED.undo.splice(0, ED.undo.length - 50); ED.rk = ED.redo; ED.redo = []; edUR(); }; // rk: no-op snaps (plain click) give redo back via edUnsnap
const edUnsnap = () => { ED.undo.pop(); ED.redo = ED.rk || []; edUR(); };
const edMove = (o, dx, dy) => { if (o.t === 'pen') o.pts = o.pts.map(([x, y]) => [x + dx, y + dy]); else if (o.t === 'line') { o.x1 += dx; o.y1 += dy; o.x2 += dx; o.y2 += dy; } else { o.x += dx; o.y += dy; } };
const segD = (x, y, a, b, c, d) => { const l = (c - a) ** 2 + (d - b) ** 2, t = l ? Math.max(0, Math.min(1, ((x - a) * (c - a) + (y - b) * (d - b)) / l)) : 0; return Math.hypot(x - a - t * (c - a), y - b - t * (d - b)); };
// #240 selection gizmo: bbox [x0,y0,x1,y1] of all selected; 8 handles EDH (fractions of the box), cursors EDC; scale = from drag-start copy (src) ∴ no drift, flips ok
const edBB = o => { const xs = o.t === 'pen' ? o.pts.map(p => p[0]) : o.t === 'line' ? [o.x1, o.x2] : [o.x, o.x + o.w], ys = o.t === 'pen' ? o.pts.map(p => p[1]) : o.t === 'line' ? [o.y1, o.y2] : [o.y, o.y + o.h];
  return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)]; };
const edSelBB = () => ED.sel.map(edBB).reduce((a, b) => [Math.min(a[0], b[0]), Math.min(a[1], b[1]), Math.max(a[2], b[2]), Math.max(a[3], b[3])]);
const EDH = [[0, 0], [.5, 0], [1, 0], [1, .5], [1, 1], [.5, 1], [0, 1], [0, .5]], EDC = ['nwse', 'ns', 'nesw', 'ew', 'nwse', 'ns', 'nesw', 'ew'];
const edHandle = (x, y) => { if (!ED.sel.length) return -1; const [a, b, c, d] = edSelBB(), r = ED.w * 4; return EDH.findIndex(([fx, fy]) => Math.abs(x - a - fx * (c - a)) < r && Math.abs(y - b - fy * (d - b)) < r); };
const edIn = (x, y) => { const [a, b, c, d] = edSelBB(); return x >= a && x <= c && y >= b && y <= d; };
const edScale = (o, s, ax, ay, sx, sy) => { const X = v => ax + (v - ax) * sx, Y = v => ay + (v - ay) * sy;
  if (o.t === 'pen') o.pts = s.pts.map(([x, y]) => [X(x), Y(y)]); else if (o.t === 'line') Object.assign(o, {x1: X(s.x1), y1: Y(s.y1), x2: X(s.x2), y2: Y(s.y2)});
  else { const x1 = X(s.x), x2 = X(s.x + s.w), y1 = Y(s.y), y2 = Y(s.y + s.h); Object.assign(o, {x: Math.min(x1, x2), y: Math.min(y1, y2), w: Math.abs(x2 - x1), h: Math.abs(y2 - y1)}); } };
const edHit = (x, y) => edHits(x, y)[0] || null;
function edHits(x, y) { const r = ED.w * 4; // shapes within r of their stroke, topmost first (box/rect/ellipse/text: inside its bbox too; ponytail: ellipse = bbox hit)
  return [...ED.ops].reverse().filter(o => o.t === 'box' || o.t === 'text' || EDSH.test(o.t) ? x > o.x - r && x < o.x + o.w + r && y > o.y - r && y < o.y + o.h + r
    : o.t === 'line' ? segD(x, y, o.x1, o.y1, o.x2, o.y2) < r : o.pts.some((p, k) => segD(x, y, ...p, ...(o.pts[k + 1] || p)) < r)); }
function edDraw(flat) {
  if (!flat && ED.ck !== edCk()) edCtx(); // #451 selection/tool changed → context bar follows
  const c = $('edC'), g = c.getContext('2d');
  g.drawImage(ED.img, 0, 0); g.lineCap = g.lineJoin = 'round';
  for (const o of ED.ops) {
    const lw = ED.w * (o.lw || 1); g.strokeStyle = g.fillStyle = o.color; g.lineWidth = lw; g.setLineDash(EDDASH(o, lw)); g.beginPath();
    if (o.t === 'pen' && o.pts.length === 1) { g.arc(...o.pts[0], lw / 2, 0, 7); g.fill(); continue; } // #248 click = dot (zero-length strokes don't paint)
    if (o.t === 'line') { const a = Math.atan2(o.y2 - o.y1, o.x2 - o.x1), L = lw * 2.5 + ED.w * 2; // #248 arrowheads: filled, line stops at the head's base
      const head = (x, y, d) => { g.save(); g.setLineDash([]); g.beginPath(); g.moveTo(x, y); g.lineTo(x - d * L * Math.cos(a - .45), y - d * L * Math.sin(a - .45)); g.lineTo(x - d * L * Math.cos(a + .45), y - d * L * Math.sin(a + .45)); g.fill(); g.restore(); };
      const k = L * .8, sx = o.a0 ? o.x1 + k * Math.cos(a) : o.x1, sy = o.a0 ? o.y1 + k * Math.sin(a) : o.y1, ex = o.a1 ? o.x2 - k * Math.cos(a) : o.x2, ey = o.a1 ? o.y2 - k * Math.sin(a) : o.y2;
      g.moveTo(sx, sy); g.lineTo(ex, ey); g.stroke(); if (o.a0) head(o.x1, o.y1, -1); if (o.a1) head(o.x2, o.y2, 1); continue; }
    if (EDSH.test(o.t)) { if (o.t === 'rect') g.rect(o.x, o.y, o.w, o.h); else g.ellipse(o.x + o.w / 2, o.y + o.h / 2, Math.abs(o.w / 2), Math.abs(o.h / 2), 0, 0, 7); if (o.fill) { g.fillStyle = o.fill; g.fill(); } g.stroke(); continue; }
    if (o.t === 'text') { g.font = edFont(o, o.h); g.textAlign = 'left'; g.textBaseline = 'top'; o.w = g.measureText(o.text).width; if (flat || !o.ed || !$('edWrap')?.querySelector('.edtxt')) { g.fillText(o.text, o.x, o.y); if (o.u) g.fillRect(o.x, o.y + o.h * .92, o.w, Math.max(1, o.h / 14)); } continue; } // #444 h = font px; w measured here ∴ edBB/edScale treat it like a box (scale = h)
    if (o.t === 'pen') { const p = o.pts; g.moveTo(...p[0]); for (let k = 1; k < p.length - 1; k++) g.quadraticCurveTo(...p[k], (p[k][0] + p[k + 1][0]) / 2, (p[k][1] + p[k + 1][1]) / 2); g.lineTo(...p.at(-1)); g.stroke(); continue; } // smoothed: curves through midpoints
    if (o.w || o.h) g.strokeRect(o.x, o.y, o.w, o.h); g.setLineDash([]);
    const r = ED.w * 4; // numbered tag on the corner = the note's number in the message
    g.beginPath(); g.arc(o.x, o.y, r, 0, 7); g.fill();
    g.fillStyle = o.fill || '#fff'; g.font = `bold ${r * 1.2}px system-ui`; g.textAlign = 'center'; g.textBaseline = 'middle'; g.fillText(o.n, o.x, o.y);
  }
  if (flat) return; const r = ED.w * 4, box = ([a, b, c2, d]) => { g.setLineDash([ED.w * 2, ED.w * 2]); g.lineWidth = ED.w / 2; g.strokeStyle = '#4096ff'; g.strokeRect(a, b, c2 - a, d - b); g.setLineDash([]); };
  if (ED.drag?.mq) { const [a, b, c2, d] = ED.drag.mq; g.fillStyle = '#4096ff22'; g.fillRect(a, b, c2 - a, d - b); box(ED.drag.mq); } // marquee
  if (ED.drag?.lp) { g.fillStyle = '#4096ff22'; g.fill(ED.drag.lp); g.setLineDash([ED.w * 2, ED.w * 2]); g.lineWidth = ED.w / 2; g.strokeStyle = '#4096ff'; g.stroke(ED.drag.lp); g.setLineDash([]); } // #452 lasso
  if (ED.sel.length) { const [a, b, c2, d] = edSelBB(); box([a, b, c2, d]); g.fillStyle = '#fff';
    EDH.forEach(([fx, fy]) => { const hx = a + fx * (c2 - a) - r / 2, hy = b + fy * (d - b) - r / 2; g.fillRect(hx, hy, r, r); g.strokeRect(hx, hy, r, r); }); }
}
// note per box under it: multiline (Enter = new line), ✓ confirms, Ctrl+Enter folds it to a notepad icon (click = open)
function edNotes(focus) {
  const c = $('edC'), s = c.clientWidth / c.width, W = $('edWrap');
  W.querySelectorAll('.ednote').forEach(x => x.remove());
  ED.ops.filter(o => o.t === 'box').forEach(o => {
    const d = Object.assign(document.createElement('div'), {className: 'ednote' + (o.col ? ' col' : '')});
    d.style.left = o.x * s + 'px'; d.style.top = (o.y + o.h) * s + 6 + 'px'; d.style.borderColor = o.color;
    if (o.col) { d.innerHTML = icon('doc'); d.title = o.n + '. ' + (o.note || 'Empty note'); d.onclick = () => { o.col = false; edNotes(o); }; return W.append(d); }
    d.innerHTML = `<textarea rows="1" placeholder="${o.n}. Note (Ctrl+Enter folds)" title="Enter = new line · ✓ or Ctrl+Enter = done (folds it to an icon)"></textarea><button class="ib" title="Done">${icon('check')}</button>`;
    const t = d.querySelector('textarea'), fit = () => { t.style.height = 'auto'; t.style.height = t.scrollHeight + 'px'; }; // #232 grows on soft wraps too
    t.value = o.note; t.onfocus = () => ED.act = o; t.oninput = () => { o.note = t.value; fit(); edDirty(); };
    t.onkeydown = e => { e.stopPropagation(); if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) { e.preventDefault(); o.col = o.ok = true; edNotes(); }
      if (e.key === 'z' && (e.ctrlKey || e.metaKey) && !t.value && !o.ok) { e.preventDefault(); ED.ops.splice(ED.ops.indexOf(o), 1); if (o.n === ED.nmax) ED.nmax--; ED.sel = ED.sel.filter(x => x !== o); edDraw(); edNotes(); } }; // #292 fresh (never confirmed) empty note → Ctrl+Z drops it
    d.querySelector('button').onclick = () => { o.col = o.ok = true; edNotes(); }; // ✓ = done → folds like Ctrl+Enter (#232: blur alone looked dead)
    W.append(d); fit(); if (o === focus) setTimeout(() => t.focus());
  });
}
/* #248 global width (× base ED.w) + line arrows, remembered; menu also restyles the selection */
let edFill = (() => { try { return !!localStorage.penFill; } catch { return false; } })(), edLW = +(localStorage.penW || 1) || 1, edArr = (() => { try { return JSON.parse(localStorage.penArr || '[0,0]'); } catch { return [0, 0]; } })();
// #452 select bar = mode only: shape (click) | rect (marquee) | lasso (freehand); edCT = click-through (same spot ±2px → next shape below). edDS = pen/line dash ''|'dash'|'dot'
let edSM = localStorage.edSM || 'rect', edCT = localStorage.edCT !== '0', edDS = localStorage.penDS || '';
const EDDASH = (o, lw) => o.ds === 'dash' ? [lw * 3, lw * 2] : o.ds === 'dot' ? [0, lw * 2] : []; // dot = 0-length dash + round cap
/* #293 width 1–250 = edLW × 8 (8 = old Medium = 1× ED.w, so saved o.lw multipliers keep working); arrows = 2 toggles; applies live to ED.sel too.
   #379 lives in the viewer's context bar (#vctx) per tool: erase = hint only, arrows for line/select; one undo snap per slider drag */
/* #451 selection of one shape kind → that tool's settings, values read from the first selected shape; mixed = generic Select bar. ED.ck = tool+sel indices → edDraw re-renders on change */
const EDFONT = {sans: 'system-ui,sans-serif', serif: 'Georgia,serif', mono: 'ui-monospace,Consolas,monospace'}, edFont = (o, px) => `${o.i ? 'italic ' : ''}${o.b ? 'bold ' : ''}${px}px ${EDFONT[o.f] || EDFONT.sans}`;
let edTS = (() => { try { return JSON.parse(localStorage.penTS || '{}'); } catch { return {}; } })(); // text style {b,i,u,f} for new texts
const edCk = () => ED.tool + ED.sel.map(o => ED.ops.indexOf(o)).join();
function edCtx() { const el = $(ED?.tab ? 'ectx' : 'vctx'); if (!el || !ED) return; ED.ck = edCk(); const S = ED.sel, ks = new Set(S.map(o => o.t)), o0 = S[0];
  const t = ED.tool === 'sel' && ks.size === 1 ? o0.t : ED.tool, one = t !== 'sel' && S.length > 0; // one = editing selected shapes of tool t
  const ds = one ? o0.ds || '' : edDS, lw = one ? o0.lw || 1 : edLW, w = Math.round(lw * 8), arr = one ? [o0.a0 ? 1 : 0, o0.a1 ? 1 : 0] : [...edArr], fl = one ? !!o0.fill : edFill, ts = one ? {b: o0.b, i: o0.i, u: o0.u, f: o0.f} : {...edTS};
  const [, ic, tip] = ETOOLS.find(x => x[0] === t), [nm, hint] = tip.split(': '), head = `<span class="vtn" role="img" aria-label="${nm}" data-tip="${nm}">${icon(ic)}</span><span class="sep"></span>`; // #453 tool icon (was #411 name); name = tip + aria
  const ur = `<span class="vur">${VZ ? zoomCtl('Math.round(VZ.s * 100)', 'vZp(p)', ', wheel, Ctrl + / −. Middle-drag pans') + '<span class="sep"></span>' : ''}<button class="ib" data-u="0" title="Undo (Ctrl+Z)" onclick="edUndo()">${icon('undo')}</button><button class="ib" data-u="1" title="Redo (Ctrl+Y, Ctrl+Shift+Z)" onclick="edRedo()">${icon('redo')}</button></span>`; // #437
  if (t === 'erase') { el.innerHTML = head + `<span class="faint">${hint}</span>` + ur; return edUR(); }
  if (t === 'sel') { // #452 mode only (mixed/empty selection); click-through toggle
    el.innerHTML = head + [['shape', 'cursor', 'Shape: click a shape to select it'], ['rect', 'marquee', 'Rectangle: drag a box over shapes'], ['lasso', 'lasso', 'Freehand: draw around shapes']].map(([k, i, n]) => `<button class="ib${edSM === k ? ' on' : ''}" data-sm="${k}" data-tip="${n}">${icon(i)}</button>`).join('')
      + `<span class="sep"></span><button class="ib${edCT ? ' on' : ''}" data-ct data-tip="Click-through: clicking the same spot again selects the next shape below">${icon('layers')}</button>` + ur; edUR();
    el.querySelectorAll('.ib[data-sm]').forEach(b => b.onclick = () => { edSM = b.dataset.sm; try { localStorage.edSM = edSM; } catch {} edCtx(); });
    el.querySelector('.ib[data-ct]').onclick = () => { edCT = !edCT; try { localStorage.edCT = edCT ? 1 : 0; } catch {} edCtx(); };
    return; }
  el.innerHTML = head + `<span class="faint">${t === 'text' ? 'Size' : 'Width'}</span><input type="range" min="1" max="100" value="${w}"><b>${w}</b>`
    + (t === 'pen' || t === 'line' ? `<span class="sep"></span><select data-ds data-tip="Line style">${options({'': 'Solid', dash: 'Dashed', dot: 'Dotted'}, ds)}</select>` : '')
    + (t === 'line' ? `<span class="sep"></span><span class="faint">Arrows</span>${[0, 1].map(k => `<button class="ib${arr[k] ? ' on' : ''}" data-a data-tip="Arrow at the ${k ? 'end' : 'start'}">${icon(k ? 'go' : 'back')}</button>`).join('')}` : '')
    + (EDSH.test(t) ? `<span class="sep"></span><span class="faint">Fill</span><button class="ib${fl ? ' on' : ''}" data-fl data-tip="Fill with the secondary colour"><svg class="i" style="fill:currentColor"><use href="#i-${ic}"/></svg></button>` : '')
    + (t === 'text' ? `<span class="sep"></span>${[['b', 'Bold', '<b>B</b>'], ['i', 'Italic', '<i style="font-family:Georgia,serif">I</i>'], ['u', 'Underline', '<u>U</u>']].map(([k, n, g]) => `<button class="ib${ts[k] ? ' on' : ''}" data-ts="${k}" data-tip="${n}">${g}</button>`).join('')}<select data-ts="f">${options({sans: 'Sans-serif', serif: 'Serif', mono: 'Monospace'}, ts.f || 'sans')}</select>` : '')
    + (S.length ? `<span class="sep"></span><span class="faint">Applies to the selected shape${S.length > 1 ? 's' : ''}</span>` : '') + ur; edUR();
  let snap = 0;
  const set = (k, v) => { if (k === 'lw') edLW = v; else if (k === 'f') edFill = v; else if (k === 'ds') edDS = v; else if (typeof k === 'string') edTS[k] = v; else edArr[k] = v;
    try { localStorage.penW = edLW; localStorage.penArr = JSON.stringify(edArr); localStorage.penFill = edFill ? 1 : ''; localStorage.penTS = JSON.stringify(edTS); localStorage.penDS = edDS; } catch {}
    if (S.length) { if (!snap++) edSnap(); S.forEach(o => { if (k === 'lw') { o.lw = v; if (o.t === 'text') o.h = ED.w * 6 * v; } else if (k === 'f') { if (EDSH.test(o.t)) o.fill = v ? EDC2[1] : null; } else if (k === 'ds') { if (o.t === 'pen' || o.t === 'line') o.ds = v; } else if (typeof k === 'string') { if (o.t === 'text') o[k] = v; } else if (o.t === 'line') o[k ? 'a1' : 'a0'] = v; }); edDraw(); } };
  const r = el.querySelector('input'); r.oninput = () => { el.querySelector('b').textContent = r.value; set('lw', r.value / 8); }; r.onchange = () => snap = 0;
  el.querySelector('.ib[data-fl]')?.addEventListener('click', e => { snap = 0; set('f', e.currentTarget.classList.toggle('on')); });
  el.querySelectorAll('.ib[data-a]').forEach((x, k) => x.onclick = () => { snap = 0; set(k, x.classList.toggle('on') ? 1 : 0); });
  el.querySelectorAll('.ib[data-ts]').forEach(x => x.onclick = () => { snap = 0; set(x.dataset.ts, x.classList.toggle('on') || undefined); });
  const ds2 = el.querySelector('select[data-ds]'); if (ds2) ds2.onchange = () => { snap = 0; set('ds', ds2.value); };
  const f = el.querySelector('select[data-ts]'); if (f) f.onchange = () => { snap = 0; set('f', f.value); }; }
// #248 cursor = the tool's icon (white halo for any background); hotspot = where the tool touches (user-marked)
const EDHOT = {sel: [5, 3], pen: [4, 20], line: [5, 19], rect: [3, 6], ellipse: [3, 12], text: [12, 12], box: [3, 5], erase: [4, 20]}, EDCUR = {};
function edCursor() { const c = $('edC'); if (!c || !ED) return; const t = ED.tool;
  EDCUR[t] ||= (() => { const k = ETOOLS.find(x => x[0] === t)[1], p = document.getElementById('i-' + k).innerHTML;
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke-linecap="round" stroke-linejoin="round"><g stroke="#fff" stroke-width="4">${p}</g><g stroke="#111" stroke-width="1.8">${p}</g></svg>`;
    return `url("data:image/svg+xml,${encodeURIComponent(svg)}") ${EDHOT[t].join(' ')}, crosshair`; })();
  c.style.cursor = EDCUR[t]; }
/* #444 text tool: input over the canvas at the text's spot (canvas px × s; stage transform scales it with the image); Enter/Esc/blur = done, empty = removed */
const EDSH = /^(rect|ellipse)$/;
function edText(o, x, y, color) { const c = $('edC'), W = $('edWrap'), s = c.clientWidth / c.width; W.querySelector('.edtxt')?.blur();
  edSnap(); const nw = !o, old = o?.text; if (nw) { o = {t: 'text', color, lw: edLW, x, y: y - ED.w * 3 * edLW, h: ED.w * 6 * edLW, w: 0, text: '', ...edTS}; ED.ops.push(o); }
  o.ed = 1; ED.sel = []; edDraw();
  const i = Object.assign(document.createElement('input'), {className: 'edtxt', value: o.text, placeholder: 'Text'}), done = () => { if (!i.isConnected || !ED) return; delete o.ed; o.text = i.value.trim() ? i.value : ''; i.remove();
    if (!o.text) { ED.ops.splice(ED.ops.indexOf(o), 1); if (nw) edUnsnap(); } else if (o.text === old) edUnsnap(); edDraw(); };
  Object.assign(i.style, {left: o.x * s + 'px', top: o.y * s + 'px', font: edFont(o, o.h * s), textDecoration: o.u ? 'underline' : '', color: o.color === 'transparent' ? '#888' : o.color});
  i.onkeydown = e => { e.stopPropagation(); if (e.key === 'Enter' || e.key === 'Escape') i.blur(); }; i.onblur = done; i.onfocus = () => ED.act = o;
  W.append(i); setTimeout(() => i.focus()); }
const edState = () => JSON.stringify([ED.ops, ED.nmax]);
function edUndo() { const u = ED?.undo.pop(); if (u) { ED.redo.push(edState()); [ED.ops, ED.nmax] = JSON.parse(u); ED.sel = []; edDirty(); edDraw(); edNotes(); } edUR(); }
function edRedo() { const u = ED?.redo.pop(); if (u) { ED.undo.push(edState()); [ED.ops, ED.nmax] = JSON.parse(u); ED.sel = []; edDirty(); edDraw(); edNotes(); } edUR(); }
const edUR = () => document.querySelectorAll('#vctx [data-u], #ectx [data-u]').forEach(b => b.disabled = !ED?.[+b.dataset.u ? 'redo' : 'undo'].length); // #437 greyed when empty
function edDone(keep) { // keep = apply the marks but leave the viewer to the caller (tab/arrow switch, dialog close)
  const k = VW.ei ?? VW.i, m = VW.imgs[k]; ED.sel = []; edDraw(true);
  const url = $('edC').toDataURL('image/png'), name = VW.map ? m.name.replace(/\.\w+$/, '') + '.png' : attName(m.name.replace(/\.\w+$/, '') + '.png'); // same stem ∴ [refs] in the text still match
  const notes = ED.ops.filter(o => o.t === 'box' && o.note.trim()).map(o => `${o.n}. ${o.note.trim()}`);
  const a = {name, url, media_type: 'image/png', data: url.split(',')[1], ed: {orig: m.ed?.orig || m.url, ops: ED.ops, nmax: ED.nmax}, notes};
  if (VW.map) { atts[cur][VW.map[k]] = a; VW.imgs[k] = a; } else { (atts[cur] ||= []).push(a); attRef(atts[cur].length - 1); }
  ED = null; if (!keep) closeDlg(); drawAtts(); grow(); if (!keep) $('prompt').focus();
}
document.addEventListener('keydown', e => {
  if (e.key === 'F11') { e.preventDefault(); return call('fullscreen'); }
  if (e.altKey && !e.ctrlKey && e.code === 'KeyM' && view === 'edit') { e.preventDefault(); return evMenuBar(); } // #447
  if (($('dlg').open || (view === 'edit' && ED?.tab)) && VZ && (e.ctrlKey || e.metaKey) && ['+', '=', '-', '0'].includes(e.key)) { e.preventDefault(); return e.key === '0' ? vZoom(0) : vZp(zStep(Math.round(VZ.s * 100), e.key === '-' ? -1 : 1)); }
  const inl = !$('dlg').open && view === 'edit' && ED?.tab; // #435 editor inside an Edit-view image tab
  if (inl && (e.ctrlKey || e.metaKey) && e.key === 's') { e.preventDefault(); return evSave(ED.tab); }
  if (!inl && (!$('dlg').open || !VW)) return;
  const typing = /^(INPUT|TEXTAREA)$/.test(document.activeElement?.tagName);
  if (ED && (e.ctrlKey || e.metaKey) && /^[zy]$/i.test(e.key) && !typing) { e.preventDefault(); e.key === 'y' || e.key === 'Y' || e.shiftKey ? edRedo() : edUndo(); } // #437 Ctrl+Y / Ctrl+Shift+Z = redo
  if (ED?.sel.length && !typing && (e.key === 'Delete' || e.key === 'Backspace')) { e.preventDefault(); edSnap(); ED.ops = ED.ops.filter(o => !ED.sel.includes(o)); ED.sel = []; edDraw(); edNotes(); }
  if (!typing && VW && $('dlg').querySelector('.viewer') && VW.imgs.length > 1 && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) stepViewer(e.key === 'ArrowLeft' ? -1 : 1);
});
$('dlg').addEventListener('close', () => { if (edPending()) try { edDone(1); } catch {} VW = VF = VZ = null; if (!ED?.tab) ED = null; $('dlg').style.cssText = ''; if (view === 'edit' && cur && !$('ebody')._t) evDraw(true); }); // #435 Edit-view image editor survives dialogs; re-mounts after the viewer parked it // #239 drop the viewer's pinned spot (dlg is shared)
function addImage(file) {
  const r = new FileReader();
  const rid = () => Array.from({length: 3}, () => Math.random().toString(36).slice(2, 5).padEnd(3, '0')).join('-');
  const name = !file.name || /^image\.\w+$/i.test(file.name) ? rid() + '.' + ((file.type.split('/')[1] || 'png').replace('jpeg', 'jpg')) : file.name;
  r.onload = () => { (atts[cur] ||= []).push({name: attName(name), url: r.result, media_type: file.type, data: r.result.split(',')[1]}); attRef(atts[cur].length - 1); drawAtts(); };
  r.readAsDataURL(file);
}
const addPaths = paths => { for (const p of paths) { const a = {name: attName(p.split(/[\\/]/).pop()), path: p}; (atts[cur] ||= []).push(a); if (view !== 'chat') attRef(atts[cur].length - 1); if (/\.(png|jpe?g|gif|webp)$/i.test(p)) imgLoad(a); } drawAtts(); }; // #325 term: no chips ∴ ref = only sign of it
async function pickFiles() { const f = await T.dialog.open({multiple: true}); if (f) addPaths([].concat(f)); }
$('prompt').addEventListener('paste', e => {
  const imgs = [...e.clipboardData.items].filter(i => i.type.startsWith('image/'));
  const txt = imgs.length ? '' : e.clipboardData.getData('text/plain'), n = txt.split('\n').length;
  if (cur && (n > PASTE_LINES || txt.length > PASTE_CHARS)) { e.preventDefault(); const a = atts[cur] ||= [];
    const k = 1 + a.filter(x => x.text).length; a.push({name: `Pasted text ${k}.txt`, text: txt}); drawAtts(); return; }
  if (!imgs.length) return;
  e.preventDefault();
  imgs.forEach(i => addImage(i.getAsFile()));
});
T.event.listen('tauri://drag-enter', () => $('composer').classList.add('drop'));
T.event.listen('tauri://drag-leave', () => $('composer').classList.remove('drop'));
T.event.listen('tauri://drag-drop', e => { $('composer').classList.remove('drop'); if (cur) addPaths(e.payload.paths || []); });

// sent while a turn runs → queued here (editable, saved with the draft); the oldest goes out when the turn ends
// /janitor: the timed Storage Janitor pass, run now; the result lands as a note in this chat
async function janitorNow() { const pid = cur; addEntry(pid, {kind: 'note', text: 'Storage Janitor is running…'});
  const n = await call('janitor').catch(e => (toast(`Storage Janitor failed: ${e}`, 'err'), -1)); if (n < 0) return;
  addEntry(pid, {kind: 'note', text: n ? `Storage Janitor freed ${mb(n)} (old build caches, Claude Code versions, temp files).` : 'Storage Janitor found nothing to remove.'}); }
function send() {
  let t = unpad($('prompt').value); const a = atts[cur] || [];
  const ask = /^\s*\/ask\b/i.test(t); if (ask) t = t.replace(/^\s*\/ask\b\s*/i, ''); // #227 pill pads display-only; #320 /ask = forced question (QNOTE), never filed
  if (!t.trim() && !a.length) return;
  if (!a.length && !editing[cur] && (/^\s*\/build\b/i.test(t) || BUILD_RE.test(t))) { $('prompt').value = ''; grow(); keepDraft(); return buildLocal(cur, (t.match(/^\s*\/build\s+(all|windows|linux|macos|ios|android)\b/i)?.[1] || '').toLowerCase()); }
  if (!a.length && !editing[cur] && /^\s*\/janitor\s*$/i.test(t)) { $('prompt').value = ''; grow(); keepDraft(); return janitorNow(); }
  const ed = editing[cur]; delete editing[cur];
  $('prompt').value = ed ? ed.prev.text : ''; atts[cur] = ed ? ed.prev.atts : []; grow(); drawAtts(); drawEditBar();
  const pid = cur;
  // #56: every direct ask (issue mode) is filed by Clauracle before it goes anywhere ∴ never lost, never unfiled.
  // Chained per project → order kept while gh runs. Edited queued ones were filed already (text carries the note)
  // #93: a plain question (ends "?", not "can you…/please"; or "btw …") → not filed: answered + ```tasks cards (sugCards)
  // #212: doubt → file (a lost ask costs more than a filed question): Q only if ONE short sentence, no change/bug words
  // No guessing here: everything goes to Rust task_do, which keeps a local copy of the ask and decides task / question /
  // remark / status (anything with an image is always a task). Only /ask, slash commands and one- or two-word replies skip it.
  const isQ = ask, slash = !a.length && /^\s*\/[\w:-]+/.test(t); // a /command goes to the CLI as typed: never wrapped, triaged or slipped into a running turn
  const file = issueOn(pid) && !isQ && !/^\s*\//.test(t) && (a.length || t.trim().split(/\s+/).length >= 3) && !FILED_RE.test(t) && !QNOTE_RE.test(t);
  // #101: bubble shows now (⏱ while filing); the chain re-checks below
  // #103: queued (turn running) → bubble now too, ⏱ Queued; QB ties it to its queue item
  const hold = qWait(pid); /* #422 open questions (≠ auto_answer) → queued only, flushed once they're answered */ if (hold) toast('Queued: answer the open question first', 'info');
  const opt = userEntry(pid, t, a, {st: hold || busy[pid] || RESTARTING ? 'queue' : 'pend'}); // #460 always at once (Continue-waiting path used to wait for the Sonnet rewrite + gh)
  // saved to disk while gh files it (seconds): a crash/quit in that window used to lose it (composer already cleared)
  const fm = {text: t, atts: a}; (FILING[pid] ||= []).push(fm); keepDraft(pid, true);
  sendChain[pid] = (sendChain[pid] || Promise.resolve()).then(async () => {
    // One path: file first, then deliver (the bubble already shows). cls = not a task (question / remark / status): never filed
    const f = file ? await fileAsk(pid, t, a) : 0, cls = typeof f === 'string' ? f : '', n = cls ? 0 : f;
    const now = n && fileNow(pid, t, n), text = isQ || cls === 'question' ? t + QNOTE : cls === 'remark' ? t + RNOTE : cls === 'status' ? t + SNOTE : n ? t + filedNote(n, now, URGENT_RE.test(t))
      : file ? t + UNFILED : t;
    const q = queue[pid] ||= [], qw = qWait(pid);
    // Continue waiting: a typed prompt goes to Claude to triage into the Queue, and the work runs (toTasks)
    if (!qw && !slash && contWaiting(pid) && !ed && !a.length) { keepDraft(pid, true); if (opt.log) { reBub(opt, text, a); tick(opt, 'sent'); } return toTasks(pid, [text], opt.log); } // #460 keeps the bubble already shown
    const m = {text, atts: a, work: now ? n : 0};
    if (!qw && !slash && busy[pid] && !RESTARTING && !a.length && !ed && await call('send_mid', {pid, text, work: m.work || null}).catch(() => false)) { // CLI feel: reaches Clauracle mid-turn
      if (m.work) P(pid).work = m.work; if (opt?.log) { reBub(opt, text, a); tick(opt, 'read'); } }
    else if (busy[pid] || RESTARTING || qw) { q.splice(ed ? ed.i : q.length, 0, m); /* an edited one goes back to its place */ if (opt?.log) { reBub(opt, text, a); tick(opt, 'queue'); QB.set(m, opt); } }
    else sendNow(pid, m, opt?.log && opt);
    if (pid === cur) drawQueue();
  }).catch(err => { // a throw used to leave the chain rejected → this and every later message for the project silently dropped
    (queue[pid] ||= []).push(fm); if (pid === cur) drawQueue();
    toast(`Message kept in the queue (send hit an error: ${err})`, 'warn');
  }).finally(() => { FILING[pid] = (FILING[pid] || []).filter(x => x !== fm); keepDraft(pid, true); });
}
