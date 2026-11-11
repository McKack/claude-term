// Own window bar where the native title bar is off (tauri.windows.conf.json). ponytail: Windows only; macOS and Linux keep theirs.
const WIN = () => window.__TAURI__.window.getCurrentWindow();
if (/Windows/.test(navigator.userAgent)) { const wb = document.getElementById('wbar'), mx = document.getElementById('wmax'); wb.hidden = false; let t;
  const sync = async () => { const m = await WIN().isMaximized(); if (m === wb.classList.contains('max')) return; wb.classList.toggle('max', m);
    mx.title = m ? 'Restore Down' : 'Maximize'; mx.firstChild.firstChild.setAttribute('href', m ? '#i-wrest' : '#i-wmax'); };
  addEventListener('resize', () => { clearTimeout(t); t = setTimeout(sync, 60); }); sync(); // one check after the size settles, not one per drag step
  const act = () => wb.classList.toggle('off', !document.hasFocus()); addEventListener('focus', act); addEventListener('blur', act); }
const T = window.__TAURI__, call = (cmd, args) => T.core.invoke(cmd, args);
let S = {projects: [], accounts: [], usage: {}, busy: []}, cur = null, ghUsers = [], SET = null, tab = 'gh', page = 'ai', mp = null;
const ctx = (() => { try { return JSON.parse(localStorage.ctx || '{}'); } catch { return {}; } })();
const saveCtx = () => { try { localStorage.ctx = JSON.stringify(ctx); localStorage.tok = JSON.stringify(TOK); } catch {} };
// token estimates per pid: now = context tokens (get_context_usage), drop = Σ context cleared by clear-continue,
// saved += what each finished turn would re-read without the clears: the cleared context, but no more than fits the window
// (a full window gets compacted, it never re-reads everything ever cleared). ponytail: per turn, not per API call → low-ball
const TOK = (() => { try { const t = JSON.parse(localStorage.tok || '{}');
  for (const k in t) if (!t[k].v) t[k] = {now: t[k].now || 0, drop: 0, saved: 0, v: 2}; /* old totals added every clear ever, every turn: reset */ return t; } catch { return {}; } })();
const tk = pid => TOK[pid] ||= {now: 0, drop: 0, saved: 0, v: 2};
const kt = n => n >= 1e9 ? (n / 1e9).toFixed(1) + 'B' : n >= 1e6 ? (n / 1e6).toFixed(1) + 'M' : Math.round(n / 1e3) + 'k';
// AI-button cost: one read of the current context + the prompt (chars/4); tool calls repeat it → "+"
const estTok = (txt = '') => `≈ ${kt(tk(cur).now + txt.length / 4 + 2e3)}+ tokens (current context + prompt)`;
const hand = {}, team = {}, logs = {}, busy = {}, live = {}, builds = {}, hiddenQs = {}, qsOpen = {}, atts = {}, texts = {}, queue = {};
let draftsLoaded = false;
const $ = id => document.getElementById(id);
const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const icon = n => `<svg class="i"><use href="#i-${n}"/></svg>`;
const P = (id = cur) => S.projects.find(p => p.id === id);
const nick = id => (S.accounts.find(a => a.id === id) || {nick: id}).nick;
// #216 named by what they do (clearing between tasks = how, always on); context-limit handover = AI card slider, every mode
const MODES = {off: ['Just chat', 'No task list: tasks run only when you ask'], manual: ['One at a time', 'Finishes a task, then waits for your Start'],
  auto: ['Keep going', 'Works through the task list, fresh context per task, until a limit, a question or an empty list'], auto_answer: ['Keep going + answer', 'Like Keep going, and answers what it can itself; holds only real decisions']};
const MODELS = {'': 'Default', fable: 'Fable', opus: 'Opus', sonnet: 'Sonnet', haiku: 'Haiku'};
const EFFORTS = {'': 'Default', low: 'Low', medium: 'Medium', high: 'High', xhigh: 'Extra high', max: 'Max'};
// #280 the CLI's own /model list (initialize answer `models`), cached ∴ menus work before the 1st connect; MODELS = fallback.
// rows [value ('' = CLI default, no --model), name, description, effort levels|null]
let CMODELS = (() => { try { return JSON.parse(localStorage.cmodels || 'null'); } catch { return null; } })();
const mList = () => CMODELS?.length ? CMODELS.map(x => [x.value === 'default' ? '' : x.value, x.value === 'default' ? 'Default' : x.displayName, x.description || '', x.supportedEffortLevels || null])
  : Object.entries(MODELS).map(([k, l]) => [k, l, '', null]);
const mName = k => mList().find(x => x[0] === (k || ''))?.[1] || MODELS[k] || k;
const effOf = k => ['', ...(mList().find(x => x[0] === (k || ''))?.[3] || Object.keys(EFFORTS).filter(Boolean))];
// short forms for the status row; full names in the menus. PERMS order = level 1..4 (least → most freedom)
const EFF_S = {low: 'L', medium: 'M', high: 'H', xhigh: 'XH', max: 'Max'};
const PERMS = {plan: 'Plan only', default: 'Ask first', acceptEdits: 'Accept edits', auto: 'Auto (classifier)', bypassPermissions: 'Bypass'};
const permN = m => Object.keys(PERMS).indexOf(m) + 1 || '?';
const PERM_C = ['', '#4ade80', '#60a5fa', '#c084fc', '#fb923c']; // #300 RPG rarity by level 1–5: common grey (default) → uncommon green → rare blue → epic purple → legendary orange
const PERM_S = {plan: 'Plan', default: 'Ask', acceptEdits: 'Edits', auto: 'Auto', bypassPermissions: 'Bypass'}; // #116 status label; number when narrow (.lb hidden)

// ponytail: tiny markdown (fences, inline code, bold, headings, lists, links); a library when tables matter
function md(src) {
  return src.split(/```([^\n]*)\n?/).map((part, i, a) => i % 2 ? '' /* fence lang */ : i % 4 ? (a[i - 1].trim() === 'tasks' ? sugCards(part) : `<pre><code>${esc(part)}</code></pre>`) :
    esc(part).replace(/`([^`]+)`/g, '<code>$1</code>').replace(/\*\*([^*]+)\*\*/g, '<b>$1</b>')
      .replace(/\[([^\]]+)\]\((https?:[^)\s]+)\)/g, '<a href="#" data-url="$2">$1</a>')
      .replace(/^#{1,4} (.*)$/gm, '<b>$1</b>')
      .replace(/(?:^[ \t]*(?:[-*]|\d+[.)]) .*(?:\n|$))+/gm, l => `<div class="tlist">${l.trim().split('\n').map(x => tli(x.replace(/^\s*(?:[-*]|\d+[.)]) /, ''))).join('')}</div>\n`)
      .split(/\n{2,}/).map(p => p.trim() && (p.startsWith('<div class="tlist"') ? p : `<p>${p.trim().replace(/\n/g, '<br>')}</p>`)).join('')).join('');
}
/* #371 document markdown (Notes tab, Code tab/File viewer for .md): headings, lists (+ [ ]/[x]), quotes, hr, tables, fences (hiCode colour), inline code/bold/italic/strike/links/images.
   ponytail: line-based, no nested lists/quotes beyond indent → margin; add a real parser only if docs need it. Check: node scripts/mddoc-check.js */
function mdDoc(src) {
  const inl = s => { const cs = []; return esc(s).replace(/`([^`]+)`/g, (m, c) => `\0${cs.push(c) - 1}\0`)
    .replace(/!\[([^\]]*)\]\((https?:[^)\s]+)\)/g, '<img alt="$1" src="$2">').replace(/\[([^\]]+)\]\((https?:[^)\s]+)\)/g, '<a href="#" data-url="$2">$1</a>')
    .replace(/\*\*(.+?)\*\*|__(.+?)__/g, '<b>$1$2</b>').replace(/(^|[^\w*])[*_]([^*_\s][^*_]*?)[*_](?![\w*])/g, '$1<i>$2</i>').replace(/~~(.+?)~~/g, '<s>$1</s>').replace(/&lt;u&gt;(.+?)&lt;\/u&gt;/g, '<u>$1</u>')
    .replace(/\0(\d+)\0/g, (m, i) => `<code>${cs[i]}</code>`); };
  const L = src.replace(/\r/g, '').split('\n'), out = []; let i = 0;
  while (i < L.length) { const l = L[i]; let m;
    if ((m = l.match(/^\s*(```|~~~)\s*(\S*)/))) { const b = []; while (++i < L.length && !L[i].trim().startsWith(m[1])) b.push(L[i]); i++;
      out.push(`<div class="code" data-lang="${esc(m[2])}"><pre style="padding:8px 12px">${hiCode(b.join('\n'), (m[2] || 'txt').toLowerCase()).join('\n')}</pre></div>`); continue; }
    if ((m = l.match(/^(#{1,6})\s+(.*?)\s*#*$/))) { out.push(`<h${m[1].length}>${inl(m[2])}</h${m[1].length}>`); i++; continue; }
    if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(l)) { out.push('<hr>'); i++; continue; }
    if (/^\s*>/.test(l)) { const b = []; while (i < L.length && /^\s*>/.test(L[i])) b.push(L[i++].replace(/^\s*>\s?/, '')); out.push(`<blockquote>${mdDoc(b.join('\n'))}</blockquote>`); continue; }
    if (/^\s*\|.*\|\s*$/.test(l) && /^\s*\|[\s:|-]+\|\s*$/.test(L[i + 1] || '')) { const row = r => r.trim().replace(/^\||\|$/g, '').split('|').map(c => inl(c.trim()));
      const h = row(l); i += 2; const rs = []; while (i < L.length && /^\s*\|/.test(L[i])) rs.push(row(L[i++]));
      out.push(`<table><tr>${h.map(c => `<th>${c}</th>`).join('')}</tr>${rs.map(r => `<tr>${r.map(c => `<td>${c}</td>`).join('')}</tr>`).join('')}</table>`); continue; }
    if (/^\s*([-*+]|\d+[.)])\s/.test(l)) { const ol = /^\s*\d/.test(l), it = [];
      while (i < L.length && (m = L[i].match(/^(\s*)([-*+]|\d+[.)])\s+(\[[ xX]\]\s+)?(.*)$/))) { i++;
        it.push(`<li style="margin-left:${Math.floor(m[1].length / 2) * 18}px"${m[3] ? ' class="task"' : ''}>${m[3] ? `<input type="checkbox" disabled${/x/i.test(m[3]) ? ' checked' : ''}> ` : ''}${inl(m[4])}</li>`); }
      out.push(`<${ol ? 'ol' : 'ul'}>${it.join('')}</${ol ? 'ol' : 'ul'}>`); continue; }
    if (!l.trim()) { i++; continue; }
    const p = []; while (i < L.length && L[i].trim() && !/^(\s*(```|~~~|>|[-*+]\s|\d+[.)]\s|\|)|#{1,6}\s)/.test(L[i])) p.push(L[i++]);
    if (!p.length) p.push(L[i++]);
    out.push(`<p>${p.map(inl).join('<br>')}</p>`); }
  return out.join('');
}
// one task row everywhere (chat lists, Plan tab, Next pill, team tasks): h = HTML; checkable only when `on` (onchange code)
const tli = (h, {done, on, cls = '', tip, end = '', lead} = {}) => `<div class="ti ${cls}"${tip ? ` title="${esc(tip)}"` : ''}>${lead /* face of the agent working on it */ || (/\bsug\b/.test(cls) ? icon('sparkle') /* AI suggestion mark ≠ checkbox */ : `<input type="checkbox"${done ? ' checked' : ''}${on ? ` onchange="${on}"` : ' disabled'}>`)}<span class="tt">${h}</span>${end}</div>`; // end: unclamped tail (buttons)
// #93 question answers: ```tasks fence ("- Title: summary" lines) → cards w/ Accept|Dismiss switch (re-click clears) + Note;
// Confirm files accepted ones (unpicked = dismissed). State per fence content hash, localStorage ∴ survives reload/redraw
const SUGS = (() => { try { return JSON.parse(localStorage.sugs || '{}'); } catch { return {}; } })();
const saveSugs = () => { const k = Object.keys(SUGS); k.slice(0, -300).forEach(x => delete SUGS[x]); /* newest 300 ∵ grew forever */ try { localStorage.sugs = JSON.stringify(SUGS); } catch {} };
const sugKey = code => [...code].reduce((h, c) => (h * 31 + c.charCodeAt(0)) | 0, 7).toString(36);
// #337: unanswered ```tasks fences of the old session → re-posted after "Fresh session" ∴ the card stays in view
const sugPend = es => es.filter(x => x.kind === 'claude' && !x.old).flatMap(x => [...(x.text || '').matchAll(/```tasks[^\n]*\n?([\s\S]*?)```/g)])
  .filter(m => !SUGS[sugKey(m[1])]?.done).map(m => ({kind: 'claude', text: '```tasks\n' + m[1] + '```', ts: new Date().toISOString()}));
function sugCards(code) {
  const t = code.split('\n').map(l => l.match(/^\s*(?:[-*]|\d+[.)])\s+(.+)/)?.[1]).filter(Boolean);
  if (!t.length) return '';
  const k = sugKey(code);
  SUGS[k] ||= {t, p: {}, n: {}};
  return `<div class="sugs" data-k="${k}">${sugBox(k)}</div>`;
}
function sugBox(k) {
  const s = SUGS[k], done = s.done, any = Object.keys(s.p).length;
  const b = (i, v, l) => `<button class="btn sm${s.p[i] === v ? ' on' : ''}" onclick="sugPick(this, ${i}, '${v}')">${l}</button>`;
  return s.t.map((t, i) => { const off = done ? !done[i] : s.p[i] === 'd';
    return tli(esc(t) + (done?.[i] ? ` <span class="pill dim">#${done[i]}</span>` : ''), {cls: 'sug' + (off ? ' completed' : ''), end: done ? '' : b(i, 'a', 'Accept') + b(i, 'd', 'Dismiss')})
      + (done ? '' : `<input class="sugnote" placeholder="Note (optional)" value="${esc(s.n[i] || '')}" oninput="SUGS['${k}'].n[${i}]=this.value; saveSugs()">`); }).join('')
    + (done ? '' : `<div class="btns"><button class="btn sm primary" onclick="sugGo(this)">${any ? 'Confirm' : 'Dismiss all'}</button></div>`);
}
function sugPick(el, i, v) { const box = el.closest('.sugs'), s = SUGS[box.dataset.k];
  if (s.p[i] === v) delete s.p[i]; else s.p[i] = v; saveSugs(); box.innerHTML = sugBox(box.dataset.k); }
async function sugGo(el) {
  const box = el.closest('.sugs'), k = box.dataset.k, s = SUGS[k], done = {}, pid = cur; // pid fixed ∵ switching mid-loop filed into the other project
  el.disabled = true;
  for (const [i, t] of s.t.entries()) if (s.p[i] === 'a') { const n = (s.n[i] || '').trim();
    try { done[i] = +(String(await call('task_do', {pid, action: 'new', arg: t + (n ? `\n\nNote: ${n}` : '')})).match(/\/issues\/(\d+)/)?.[1] || 0) || '?'; }
    catch (e) { toast(`Couldn't file "${t}": ${e}`, 'warn'); } }
  s.done = done; saveSugs(); box.innerHTML = sugBox(k); pollTeam();
}
document.addEventListener('click', e => { const r = e.target.closest('.ti'); if (r && !e.target.closest('input,a,button') && !getSelection().toString()) r.classList.toggle('open'); });
// shell cmd → parts at ; && || | newline OUTSIDE quotes (same as claude::split_cmd; sed 's/a;b/' stays one part)
function splitCmd(c) {
  const out = []; let cur = '', q = '';
  for (let k = 0; k < c.length; k++) { const ch = c[k];
    if (q) { cur += ch; if (ch === q) q = ''; else if (ch === '\\' && q === '"') cur += c[++k] ?? ''; continue; }
    if (ch === '\\') { cur += ch + (c[++k] ?? ''); continue; }
    if (ch === "'" || ch === '"') { q = ch; cur += ch; continue; }
    const two = c.slice(k, k + 2);
    if (two === '&&' || two === '||') { out.push(cur); cur = ''; k++; continue; }
    if (ch === ';' || ch === '|' || ch === '\n') { out.push(cur); cur = ''; continue; }
    cur += ch;
  }
  out.push(cur); return out.map(s => s.trim()).filter(Boolean);
}
// app dialogs instead of WebView2's Win7-looking confirm()/alert(); opened last → on top of dlg/dlg2. Esc = cancel.
// html: body as HTML (already escaped) instead of text's rest
let SURE = Promise.resolve(); // one at a time
const sure = (...a) => (SURE = SURE.then(() => sure1(...a)));
function sure1(text, ok = 'OK', cancel = 'Cancel', html, danger) { // danger: the OK button removes or discards something
  const d = $('dlgc'), [head, ...rest] = String(text).split('\n\n'), body = html ?? (rest.length ? esc(rest.join('\n\n')) : '');
  d.innerHTML = `<div class="small-dlg"><h2 style="white-space:pre-wrap">${esc(head)}</h2>${body ? `<p class="hint" style="white-space:pre-wrap">${body}</p>` : ''}
    <div class="btns">${cancel ? `<button class="btn ghost" value="">${esc(cancel)}</button>` : ''}<button class="btn ${danger ? 'danger' : 'primary'}" value="1">${esc(ok)}</button></div></div>`;
  dlgShell(d.firstElementChild);
  return new Promise(res => { d.onclose = () => res(d.returnValue === '1'); d.returnValue = '';
    d.querySelectorAll('button').forEach(b => b.onclick = () => d.close(b.value)); d.showModal(); d.querySelector(danger ? '.ghost, .danger' : '.primary').focus(); }); // danger: Enter lands on Cancel
}
const say = text => sure(String(text), 'OK', '');
document.addEventListener('click', e => { const u = e.target.closest('[data-url]')?.dataset.url; if (u && (e.ctrlKey || !e.target.closest('[contenteditable=true]'))) { /* in an editor a click places the caret; Ctrl+click opens */ e.preventDefault(); T.opener.openUrl(u); } });

