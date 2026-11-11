// ---- composer: text, pasted images (previews), attached files ----
const hasInput = () => $('prompt').value.trim() || (atts[cur] || []).length;
const stem = n => n.replace(/\.\w+$/, '');
// a new attachment whose stem is already taken gets -2, -3 … before the extension (refs and notes go by stem)
const attName = name => { const l = atts[cur] || [], s = stem(name), e = name.slice(s.length); let n = name;
  for (let k = 2; l.some(x => stem(x.name) === stem(n)); k++) n = `${s}-${k}${e}`; return n; };
// dropped image file: load its bytes so it can be annotated and saved like a pasted one
async function imgLoad(a) { try { const x = a.path.match(/\.(\w+)$/)[1].toLowerCase(), mt = 'image/' + (x === 'jpg' ? 'jpeg' : x),
  url = await new Promise((ok, no) => { const r = new FileReader(); r.onload = () => ok(r.result); r.onerror = no; call('read_image', {path: a.path}).then(b => r.readAsDataURL(new Blob([b], {type: mt})), no); });
  Object.assign(a, {url, media_type: mt, data: url.split(',')[1]}); drawAtts(); } catch {} }
// #217 notes live on the attachment; #238: also in the filed issue body + file/isQ checks (were session-only → the ask got lost)
const noteTxt = a => a.filter(x => x.notes?.length).map(x => `\n\nNotes on @[${stem(x.name)}] (numbered boxes):\n${x.notes.join('\n')}`).join('')
  + a.filter(x => x.text).map(x => `\n\n${stem(x.name)} (pasted by the user):\n~~~~\n${x.text}\n~~~~`).join(''); // big pastes (chips): to Claude and the filed ask
// #227 file-type icon for pills/thumbs: image | doc | code | file
const fIcon = n => /\.(png|jpe?g|gif|webp|bmp|svg|ico)$/i.test(n) ? 'image' : /\.(md|txt|pdf|docx?|rtf|csv|log)$/i.test(n) ? 'doc' : /\.(js|ts|tsx|jsx|rs|py|go|java|c|cpp|h|cs|rb|php|html|css|json|toml|ya?ml|sh|ps1|kt|swift)$/i.test(n) ? 'code' : 'file';
// #308 Files tree/rail: icon by type + tint by language (dev-icon colours); unknown ext → plain faint file
const FTINT = {js: '#e8c547', mjs: '#e8c547', cjs: '#e8c547', jsx: '#5fc8e6', ts: '#4a90d9', tsx: '#4a90d9', rs: '#de8a5a', py: '#5a9fd4', go: '#4fc3d9',
  java: '#e07a3a', kt: '#a97bff', swift: '#f0743e', c: '#7f9fc9', h: '#7f9fc9', cpp: '#5c8fd6', cs: '#9b6fd6', rb: '#d9534f', php: '#8892bf',
  html: '#e5673a', css: '#5b8fe0', scss: '#cf649a', vue: '#41b883', svelte: '#ff5d2e', json: '#d9a63e', toml: '#9c8c7c', yml: '#cb6b6b', yaml: '#cb6b6b',
  md: '#6aa0d8', pdf: '#e05a4f', sh: '#7cb860', ps1: '#4a8fd6', bat: '#7cb860', cmd: '#7cb860', sql: '#e0a040', lock: '#8a8a8a', env: '#d9b43e'};
const ftIcon = n => { const b = n.split('/').pop().toLowerCase(), e = (b.match(/\.([^.]+)$/)?.[1] || ''),
  k =/^\.env/.test(b) ? 'key' : /^\.(git|npm|docker|editor|prettier|eslint)/.test(b) || /^(dockerfile|makefile|license)$/.test(b) ? 'gear'
    : e === 'pdf' ? 'pdf' : /^(sh|ps1|bat|cmd)$/.test(e) ? 'term' : /^(json|toml|ya?ml|lock|xml|ini)$/.test(e) ? 'braces' : fIcon(b), c =FTINT[/^\.env/.test(b) ? 'env' : e];
  return c ? icon(k).replace('class="i"', `class="i" style="color:${c}"`) : icon(k); };
// #270 composer ref = plain "@[name]" (same text in prompt, logs, issues); mirror draws the icon over "@[". unpad: old drafts' "[  name ]" pads → @[name]
const unpad = t => t.replace(/\[\u2007\u2007([^\]\n]*?)\u2002\]/g, '@[$1]'), reEsc = s => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
// #227 chat bubble: @[ref] (old logs: [ref]) naming one of the message's files → same pill
const refPills = (t, names) => esc(unpad(t)).replace(/(?:#(?=\[\w{4}\])|@)?\[([^\]\n]+)\]/g, (m, n) => { if (m[0] === '#') n = '^' + n; /* #427 #[id] */ if (/^\^\w{4}$/.test(n)) return `<span class="rpill mref" onclick="midJump('${n.slice(1)}')" data-tip="Jump to that message">${icon('reply')}${n.slice(1)}</span>`; const f = names.find(x => stem(x) === n.replace(/&amp;/g, '&').trim() || x === n); return f ? `<span class="rpill">${icon(fIcon(f))}${n}</span>` : m; });
function attRef(i) { // inserts [name w/o extension] at the caret; a space before/after only where the neighbour isn't one already
  const el = $('prompt'), v = el.value, s = el.selectionStart ?? v.length, e = el.selectionEnd ?? s, r = view !== 'chat' ? `@[ ${stem(atts[cur][i].name)}]` : `@[${stem(atts[cur][i].name)}]`; // #325 term: "@[ " ∴ mirror shows "[icon name]" w/ a space (mono slots)
  const ins = (s && !/\s/.test(v[s - 1]) ? ' ' : '') + r + (e < v.length && /\s/.test(v[e]) ? '' : ' ');
  el.setRangeText(ins, s, e, 'end'); el._pv = el.value; el.focus(); grow(); keepDraft();
}
// #270 remove → its @[name] tags go too (+ one trailing space), unless another attachment shares the name
function rmAtt(i) { const n = stem(atts[cur].splice(i, 1)[0].name), p = $('prompt');
  if (!atts[cur].some(a => stem(a.name) === n)) { p.value = p.value.replace(new RegExp(`@\\[ ?${reEsc(n)}\\] ?`, 'g'), ''); grow(); } drawAtts(); }
// #419 one "…" per thumb/chip → View / Refer / Remove (replaced hover × + refer buttons)
function attMenu(b, i) { const a = atts[cur][i], base = (P().path || '').replace(/\\/g, '/').replace(/\/$/, '').toLowerCase(), ap = (a.path || '').replace(/\\/g, '/'),
    pdf = /\.pdf$/i.test(ap), inP = ap.toLowerCase().startsWith(base + '/'), canV = a.url || a.text || pdf || inP; // ponytail: non-pdf files outside the project → disabled (no generic text reader for abs paths)
  menuAt(b, [{label: 'View file', icon: 'eye', off: !canV, hint: canV ? '' : 'Only files inside the project', run: () => a.url ? viewAtt(i) : a.text ? attText(i) : pdf ? openPdf(a.path) : openFile(a.path)},
    {label: 'Refer to this', icon: 'reply', run: () => attRef(i)}, '-', {label: 'Remove', icon: 'trash', run: () => rmAtt(i)}]); }
function drawAtts() {
  $('atts').innerHTML = (atts[cur] || []).map((a, i) => `<div class="att${a.url ? ' img' : ''}" title="${esc(a.path || a.name)}" onmouseenter="markHov(${i})" onmouseleave="markHov(-1)">${a.url ? `<img src="${a.url}" onclick="viewAtt(${i})" onload="this.parentNode.style.aspectRatio = this.naturalWidth + '/' + this.naturalHeight; attArr()">` : icon(fIcon(a.name)) + (a.text ? `<span class="tchip" onclick="attText(${i})">${esc(stem(a.name))} · ${tsize(a.text)}</span>` : `<span>${esc(a.name)}</span>`)}
    <button class="amore" title="View, refer to or remove" onmousedown="event.preventDefault()" onclick="attMenu(this, ${i})">${icon('more')}</button>
    ${a.notes?.length ? `<button class="anote" title="${esc(a.notes.join('\n').slice(0, 160))}&#10;(click to edit the notes)" onclick="viewAtt(${i}); editImage()">${icon('doc')}</button>` : ''}</div>`).join('');
  attArr(); pbDraw(); if (cur) { drawChrome(); keepDraft(); }
}
// #217 [ref] pills: #pback mirrors the textarea text; refs naming an attachment get a <mark> behind them (a textarea can't style
// its own text). Click inside one → highlight + scroll its thumb, image → viewer. Icon drawn over the "@[" chars (same text ∴ mirror stays aligned)
const attAt = name => { name = name.trim().replace(/[   ]/g, ''); return (atts[cur] || []).findIndex(a => stem(a.name) === name || a.name === name); };
function pbDraw() { const p = $('prompt'), b = $('pback'); if (!b) return; b.style.overflowY = p.offsetWidth > p.clientWidth ? 'scroll' : 'hidden'; /* #231/#405 textarea scrollbar narrows its lines → mirror gets the SAME scrollbar (thumb hidden): `right:Npx` was off by fractional px at DPR 2.19 (10px bar ≈ 10.06) → other wraps → caret mid-word */
  b.innerHTML = esc(p.value).replace(/(#|@)\[([^\]\n]+)\]/g, (m, h, n) => { if (h === '#') return /^\w{4}$/.test(n) ? `<mark class="mref"><i class="fic">#[${icon('reply')}</i>${n}<i>]</i></mark>` : m; /* #418/#427 reply ref #[id]: icon over "#[" */ const i = attAt(n.replace(/&amp;/g, '&')); if (i < 0) return m;
    return `<mark data-i="${i}"><i class="fic">@[${icon(fIcon(atts[cur][i].name))}</i>${n}<i>]</i></mark>`; }) + '\n'; pbSync(); fadeReset($('pwrap'), p); }
// Chromium keeps the last scroll-fade value when the scroll timeline goes inactive (text no longer overflows): restart the animation to drop it
function fadeReset(el, sc) { if (sc.scrollHeight > sc.clientHeight || getComputedStyle(el).getPropertyValue('--ft') === '0px') return;
  el.style.animation = 'none'; el.offsetHeight; el.style.animation = ''; }
// #258 sync now + next frame ∵ Chromium reveals the caret (scrolls) after the input event/keys, not always w/ a timely scroll event → text drifted till the user scrolled
// #403 caret line kept out of the #381 top/bottom fades (18px): mirror text = value + '\n' ∴ caret offset → Range in #pback → rect → nudge scrollTop
function caretClear() { const p = $('prompt'), b = $('pback'); if (document.activeElement !== p || p.scrollHeight <= p.clientHeight) return;
  let n, k = p.selectionEnd; const w = document.createTreeWalker(b, NodeFilter.SHOW_TEXT);
  while ((n = w.nextNode()) && k > n.length) k -= n.length; if (!n) return;
  const rg = document.createRange(); rg.setStart(n, k); rg.setEnd(n, k); const c = rg.getClientRects()[0] || rg.getBoundingClientRect(), r = p.getBoundingClientRect(), F = 18;
  // #417 nudge only a caret sitting IN a fade band; fully scrolled away (user scrolled / dragged the thumb) → leave it (typing reveals it natively)
  if (!c.height || c.top >= r.bottom || c.bottom <= r.top) return; const d = c.bottom > r.bottom - F ? c.bottom - r.bottom + F : c.top < r.top + F && p.scrollTop > 0 ? c.top - r.top - F : 0;
  if (d) { p.scrollTop += d; b.scrollTop = p.scrollTop; } }
const pbSync = () => { const f = () => { $('pback').scrollTop = $('prompt').scrollTop; caretClear(); }; f(); requestAnimationFrame(f); };
['keyup', 'mouseup', 'select'].forEach(e => $('prompt').addEventListener(e, pbSync));
$('prompt').addEventListener('scroll', () => { $('pback').scrollTop = $('prompt').scrollTop; }); // #417 user scroll = mirror only, no caret clamp (it pinned the view to a bottom-line caret)
// #223 link cursor over a pill: hit-test the mirror's <mark> rects (#pback is pointer-events:none)
// #341 + hovered pill → its thumb gets .hov (same look as .hl); mark text = "@[name]" (svg has no text)
const attHov = i => [...$('atts').children].forEach((t, k) => t.classList.toggle('hov', k === i));
const markHov = i => $('pback').querySelectorAll('mark').forEach(m => m.classList.toggle('hov', +m.dataset.i === i)); // #405 thumb hover → its refs light up
$('prompt').addEventListener('mousemove', e => { const hit = [...$('pback').querySelectorAll('mark')].find(m => [...m.getClientRects()].some(r => e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom));
  e.target.style.cursor = hit ? 'pointer' : ''; attHov(hit ? attAt(hit.textContent.slice(2, -1)) : -1); });
$('prompt').addEventListener('mouseleave', () => attHov(-1));
$('prompt').addEventListener('click', () => { const p = $('prompt'), c = p.selectionStart, v = p.value, o = v.lastIndexOf('[', c), z = v.indexOf(']', o);
  if (o < 0 || z < c || v.slice(o, z).includes('\n')) return; const rid = v[o - 1] === '#' && /^(\w{4})$/.exec(v.slice(o + 1, z))?.[1]; if (rid) return midJump(rid); const i = attAt(v.slice(o + 1, z)); if (i < 0) return;
  const t = $('atts').children[i]; document.querySelectorAll('.att.hl').forEach(x => x.classList.remove('hl')); t?.classList.add('hl'); t?.scrollIntoView({block: 'nearest', inline: 'nearest', behavior: 'smooth'});
  setTimeout(() => t?.classList.remove('hl'), 2500); if (atts[cur][i].url) viewAtt(i); });
// #192 scroll arrows only toward hidden thumbs; ResizeObserver ∵ composer width changes w/o a redraw
function attArr() { const el = $('atts'), [l, r] = $('attw').querySelectorAll('.aarr'); l.hidden = el.scrollLeft < 1; r.hidden = el.scrollLeft + el.clientWidth >= el.scrollWidth - 1; }
const attScroll = d => $('atts').scrollBy({left: d * $('atts').clientWidth * .8, behavior: 'smooth'});
new ResizeObserver(() => attArr()).observe($('atts'));
// every keystroke/attachment lands on disk (debounced), so nothing typed is ever lost
// now: queue changes save at once ∵ a restart inside the 300ms window lost a queued prompt
function keepDraft(pid = cur, now = false) {
  if (pid === cur) texts[pid] = $('prompt').value;
  const text = texts[pid] || '', a = atts[pid] || [], q = queue[pid] || [], ed = editing[pid] || null; // ed: a queued one being edited
  clearTimeout(keepDraft[pid]);
  const f = [...(FILING[pid] || []), ...(INBOX[pid] || [])];
  const save = () => call('save_draft', {pid, draft: text || a.length || q.length || ed || f.length ? {text, atts: a, queue: q, editing: ed, filing: f} : null});
  if (now) save(); else keepDraft[pid] = setTimeout(save, 300);
}
