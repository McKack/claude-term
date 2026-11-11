// ---- settings: Accounts (AI + GitHub) · Projects | Extensions (cx, agents, deps) · Appearance · Notifications ----
// #38 order: who + what | what Clauracle gets + Clauracle itself (divider before cx)
// #59: GitHub folded into Accounts, Agents into Extensions; old page keys still open their new home
const PAGES = {ai: ['user', 'Accounts'], projects: ['folder', 'Projects'], build: ['hammer', 'Build &amp; Release'], cx: ['plug', 'Extensions'], look: ['sliders', 'Appearance'], notify: ['bell', 'Notifications']};
let AGENTS = null;
// GitHub accounts / catalog changed: drop the cache and load again
const reloadSettings = p => { SET = null; return openSettings(p); };
async function openSettings(p) {
  const same = !p || p === page; // re-render of the same tab keeps its scroll
  if (p) page = {gh: 'ai', agents: 'cx'}[p] || p;
  if (p === 'gh' || p === 'agents') setTimeout(() => jumpTo(p === 'gh' ? '[data-f=a-gh]' : '[data-f=g-agents]')); // #168 aliases land on their section
  if (!$('dlg').open) dlg(`<div style="padding:20px">${skel(30, 'b100', 'b100', 'b100', 70, 'b100', 'b100')}<small class="dim">The first time asks Claude Code which connections it has</small></div>`, 'prefs');
  if (!SET || !AGENTS) [SET, AGENTS] = await Promise.all([call('settings'), call('agents')]);
  else if (same) await refresh(); // after a change: state is a cheap in-process read
  ghUsers = SET.gh_users;
  const scroll = $('dlg').querySelector('.body')?.scrollTop;
  dlg(`<div class="tbar">${Object.entries(PAGES).map(([k, [ic, l]]) => `${k === 'cx' ? '<i class="rsep"></i>' : ''}<button class="${k === page ? 'on' : ''}" onclick="openSettings('${k}')">${icon(ic)}${l}</button>`).join('')}
      <button class="ib x" title="Close" onclick="closeDlg()">${icon('x')}</button></div>
    <div class="body"><div class="row stitle"><h2 class="grow">${PAGES[page][1]}</h2><input type="search" class="ssearch" placeholder="Search settings" onfocus="sidxFill()" oninput="sSug(this)" onkeydown="if (event.key === 'Enter') document.querySelector('.menu [data-i]')?.click()"><datalist id="sidx"></datalist></div>
    ${SPG[page]()}</div>
    <div class="sfoot"><select id="sjump" title="Jump to a section" onchange="sjumpGo(this)"></select><span class="row grow">${(FOOT[page] || (() => ''))()}</span><button class="btn ghost" onclick="closeDlg()">Done</button></div>`, 'prefs');
  if (same && scroll) $('dlg').querySelector('.body').scrollTop = scroll;
  if (page === 'ai') relaySort();
  sjumpFill(); setTimeout(sjumpFill, 1500); // ponytail: 2nd pass ∵ some pages fill sections async
}
// #420 title-row search: native <datalist> of every page's labels (pages rendered into an inert <template> on focus); pick → open page + jumpTo
const SPG = {look: () => lookPage(), projects: () => projectsPage(), build: () => buildPage(), cx: () => cxPage(), ai: () => aiPage(), notify: () => notifyPage()};
const SSEL = '.opt-row .grow > div:first-child, .label, .grid2 > span:nth-child(odd), summary b';
const sTxt = e => [...e.childNodes].filter(n => n.nodeName !== 'BUTTON').map(n => n.textContent).join('').replace(/\s+/g, ' ').trim();
function sidxFill() { const dl = $('sidx'); if (!dl || dl.options.length) return; const seen = new Set();
  dl.innerHTML = Object.entries(SPG).flatMap(([k, f]) => { const t = document.createElement('template'); try { t.innerHTML = f(); } catch { return []; }
    return [...t.content.querySelectorAll(SSEL)].map(sTxt).filter(x => x.length > 1 && x.length < 60 && !seen.has(k + x) && seen.add(k + x))
      .map(x => `<option value="${esc(x + ' · ' + PAGES[k][1].replace('&amp;', '&'))}"></option>`); }).join(''); }
// #450 suggestions = app menu (native datalist popup = square WebView box); datalist stays the index
function sSug(i) { sidxFill(); const q = i.value.trim().toLowerCase(); if (!q) return closeMenu();
  const m = [...$('sidx').options].map(o => o.value).filter(v => v.toLowerCase().includes(q)).slice(0, 12);
  if (!m.length) return closeMenu(); MENU_T = 0; menuAt(i, m.map(v => ({label: v, run: () => { i.value = v; sFind(i); }}))); }
function sFind(i) { const m = i.value.match(/^(.*) · ([^·]+)$/), k = m && Object.keys(PAGES).find(k => PAGES[k][1].replace('&amp;', '&') === m[2]); i.value = '';
  if (k) openSettings(k).then(() => jumpTo(() => [...$('dlg').querySelectorAll('.prefs .body ' + SSEL.split(', ').join(', .prefs .body '))].find(e => sTxt(e) === m[1]))); }
// #215 footer: Jump to = the page's section titles (folds + secs), live from the DOM; FOOT[page] = contextual actions
const sjumpEls = () => [...$('dlg').querySelectorAll('.prefs .body :is(details.fold, section.sec)')].filter(e => !e.parentElement.closest('details.fold, section.sec'));
function sjumpFill() { const s = $('sjump'); if (!s) return;
  // #219: sections inside an account card → <optgroup> under its name (els stay contiguous per card ∴ index order holds)
  let g = null; s.innerHTML = '<option value="">Jump to…</option>' + sjumpEls().map((e, i) => { const a = e.parentElement.closest('details.acct'), o = `<option value="${i}">${esc(e.querySelector('.label')?.textContent.trim() || '')}</option>`;
    const pre = a === g ? '' : (g ? '</optgroup>' : '') + (a ? `<optgroup label="${esc(a.querySelector('summary b')?.textContent.trim() || '')}">` : ''); g = a; return pre + o; }).join('') + (g ? '</optgroup>' : '');
  s.hidden = s.options.length < 3; }
function sjumpGo(s) { const el = sjumpEls()[s.value]; s.value = ''; if (el) jumpTo(() => el); }
const FOOT = {projects: () => mp ? `<button class="btn rpdf" onclick="repPdf(null, mp)">${icon('cal')}7-day report (.pdf)</button><button class="btn" onclick="removeProject()">Remove project</button>` : ''};
// Clauracle's own (Ponytail, always on) + a separately installed ponytail plugin (duplicate of it): never offered
const builtin = id => /^skill:(clauracle|ponytail):/.test(id);
// what a project/account has on; one from before allow-lists (`on` null) has everything but its `off`
const onOf = x => x.on ?? SET.catalog.map(i => i.id).filter(id => !x.off.includes(id));
// global rows switch an item off everywhere; scoped rows (account id | project:<id>) pick it for that one
const cxItems = (kind, scope) => SET.catalog.filter(i => i.kind === kind && (scope === 'global' || (!SET.off.includes(i.id) && !builtin(i.id))));
const cxCount = (kind, scope, x) => { const xs = cxItems(kind, scope);
  return `${xs.filter(i => scope === 'global' ? !SET.off.includes(i.id) : onOf(x).includes(i.id)).length}/${xs.length}`; };
// settings section that folds; title + "on/total" pill; key unique per page/scope
// All/None pair, one look everywhere (#55: label style, like the sub-section ones); kind 'cx' = mcp + skill
const cxKinds = k => k === 'cx' ? ['mcp', 'skill'] : [k];
const cxAB = (kind, scope) => cxKinds(kind).flatMap(k => cxItems(k, scope)).length > 1
  ? ['All', 'None'].map(l => `<button class="btn ghost sm cxab" onclick="event.preventDefault(); cxAll(this, '${kind}', '${esc(scope)}', ${l === 'All'})">${l}</button>`).join('') : '';
const cxFold = (key, title, kind, scope, x) => fold(key, title, `<div class="box">${cxList(kind, scope, x)}</div>`, cxCount(kind, scope, x), cxAB(kind, scope), scope === 'global');
// #32: Connections + Skills = one fold, a sub-section each (own All/None) + All/None for both; pill = both summed
const cxGroup = (key, title, scope, x) => { const n = ['mcp', 'skill'].map(k => cxCount(k, scope, x).split('/').map(Number));
  return fold(key, title, [['mcp', 'Connections'], ['skill', 'Skills']].map(([k, l]) => `<div class="cxsub">${sec(l, `<div class="box">${cxList(k, scope, x)}</div>`, cxAB(k, scope))}</div>`).join(''), `${n[0][0] + n[1][0]}/${n[0][1] + n[1][1]}`, cxAB('cx', scope) + (scope === 'global' ? `<button class="ib" title="Rescan Claude Code" onclick="cxRescan(this)">${icon('refresh')}</button>` : ''), scope === 'global' || !(n[0][0] + n[1][0])); }; // #257 per project/account: open only while nothing is picked
// fold pill = on/total of its switches, redrawn in place (toggles don't re-render the page)
const cxLive = el => { const d = el.closest('details.fold'), xs = [...d.querySelectorAll('.sc .sw input')], p = d.querySelector(':scope > summary .pill');
  if (p) p.textContent = `${xs.filter(c => c.checked).length}/${xs.length}`; };
async function cxAll(el, kind, scope, on) {
  const ids = cxKinds(kind).flatMap(k => cxItems(k, scope)).map(i => i.id);
  if (scope === 'global') { for (const id of ids) if (SET.off.includes(id) === on) await setGlobalOff(id, !on); }
  else {
    const x = scope.startsWith('project:') ? P(scope.slice(8)) : S.accounts.find(a => a.id === scope);
    await call('set_items', {scope, items: onOf(x).filter(i => !ids.includes(i)).concat(on ? ids : [])}); await refresh();
  }
  const d = el.closest('.cxsub') || el.closest('details.fold'); d.querySelectorAll('.sw input').forEach(c => c.checked = on); cxLive(d);
}
// MCP status from `claude mcp list` (note); not connected → how to fix: claude.ai connectors sign in on claude.ai
// (CLI can't run their OAuth headless), others via /mcp in an interactive CLI. Rescan after.
const cxBad = i => i.kind === 'mcp' && !/^connected$/i.test(i.note);
const cxNote = i => esc(i.note) + (!cxBad(i) ? '' : /^claude\.ai /.test(i.label)
  ? ` · <a href="#" data-url="https://claude.ai/settings/connectors">Authenticate</a>, then Rescan` : ' · run /mcp in the Terminal tab, then Rescan');
const cxList = (kind, scope, x) => (scope.startsWith('project:') && kind === 'mcp' && cxItems(kind, scope).some(i => cxBad(i) && onOf(x).includes(i.id))
  ? `<p class="hint">${icon('info')} Some picked connections aren't connected: Clauracle can't use them until they are.</p>` : '') + cxItems(kind, scope).map(i =>
  optRow(esc(i.label), cxNote(i), scope === 'global' ? sw(!SET.off.includes(i.id), `cxLive(this); setGlobalOff('${esc(i.id)}', !this.checked)`)
    : sw(onOf(x).includes(i.id), `cxLive(this); toggleOn('${esc(scope)}', '${esc(i.id)}', this.checked)`))).join('')
  || empty('Nothing found yet', `<button class="btn sm" title="Ask Claude Code again for its connections and skills" onclick="cxRescan(this)">${icon('loop')}Rescan</button>`);
const cxRescan = b => { b.disabled = true; b.title = 'Asking Claude Code…'; if (!b.classList.contains('ib')) b.innerHTML = '<span class="spin"></span>Asking Claude Code…'; call('refresh_catalog').then(() => reloadSettings()); };
function setGlobalOff(id, off) {
  SET.off = SET.off.filter(i => i !== id).concat(off ? [id] : []);
  return call('set_off', {item: id, off, scope: 'global'});
}
async function toggleOn(scope, id, checked) {
  const x = scope.startsWith('project:') ? P(scope.slice(8)) : S.accounts.find(a => a.id === scope);
  const on = onOf(x).filter(i => i !== id);
  if (checked) on.push(id);
  await call('set_items', {scope, items: on}); await refresh();
}
// the picker: nothing on unless ticked (a shared machine can have plenty that isn't for this account/project)
async function pickItems(scope, title, pre) {
  SET ||= await call('settings');
  const all = SET.catalog.filter(i => !SET.off.includes(i.id) && !builtin(i.id));
  if (!all.length) return;
  const row = i => optRow(esc(i.label), esc(i.note), `<label class="sw"><input type="checkbox" value="${esc(i.id)}" ${pre.includes(i.id) ? 'checked' : ''}><i></i></label>`);
  const part = (k, l) => { const xs = all.filter(i => i.kind === k); return xs.length ? `<div class="label" style="margin-top:12px">${l}</div><div class="box" style="max-height:210px;overflow:auto;margin-bottom:0">${xs.map(row).join('')}</div>` : ''; };
  const tick = f => document.querySelectorAll('#dlg input[type=checkbox]').forEach(c => c.checked = f(c.value));
  let last = []; try { last = JSON.parse(localStorage.lastPick || '[]'); } catch {} // per computer, last Save of any picker
  dlg(`<h2>${esc(title)}${infoI('Only what you switch on loads for this project.')}</h2>
    <p class="row" style="margin:10px 0 0">${last.length ? `<button class="btn sm" id="pickLast" title="${esc(last.length + ' items')}">${icon('loop')}Use last settings</button>` : ''}
      <button class="btn ghost sm" id="pickAll">All</button><button class="btn ghost sm" id="pickNone">None</button></p>
    ${part('mcp', 'Connections')}${part('skill', 'Skills')}${part('plugin', 'Plugins & mods (hooks, their own tools; also on when one of their skills is ticked)')}
    <div class="btns"><button class="btn ghost" id="pickCancel">Cancel</button><button class="btn primary" id="pickSave">Save</button></div>`);
  $('pickAll').onclick = () => tick(() => true); $('pickNone').onclick = () => tick(() => false);
  if ($('pickLast')) $('pickLast').onclick = () => tick(v => last.includes(v));
  $('pickCancel').onclick = closeDlg;
  if (!await new Promise(res => { $('pickSave').onclick = () => res(true); $('pickCancel').addEventListener('click', () => res(false)); })) return;
  const items = [...document.querySelectorAll('#dlg input[type=checkbox]:checked')].map(c => c.value);
  try { localStorage.lastPick = JSON.stringify(items); } catch {}
  await call('set_items', {scope, items});
  closeDlg(); await refresh();
}
const askProject = p => pickItems('project:' + p.id, `What can Clauracle use in ${p.name}?`, p.on || []);
// projects from before allow-lists: ask once, ticked as they are now
const asked = {};
async function askLegacy(pid) {
  const p = P(pid);
  if (!p || p.on != null || asked[pid]) return;
  asked[pid] = true;
  SET ||= await call('settings');
  pickItems('project:' + pid, `Choose what Clauracle can use in ${p.name}`, onOf(p));
}
/* #213 Extensions › Find & install: skills.sh (skills) | MCP registry (servers) → ext_add (user-wide) → rescan → picked for
   the open project at once (allow-lists: else installed-but-off). EXF = last search, kept while Settings reopens. */
const EXF = {k: 'skill', q: '', r: null, busy: {}};
// registry entry → [install arg, why not]: remote http w/o required headers | npm pkg w/o required env; else just a link
const mcpHow = s => { const r = (s.remotes || []).find(r => r.type === 'streamable-http' && !(r.headers || []).some(h => h.isRequired));
  if (r) return [r.url]; const n = (s.packages || []).find(p => p.registryType === 'npm' && !(p.environmentVariables || []).some(e => e.isRequired));
  return n ? [n.identifier] : [null, 'needs keys or setup: see its page']; };
function extRows() {
  const r = EXF.r; if (!r) return EXF.q ? skel(60, 'b100') : '<p class="dim">Search the skills directory or the MCP registry and install what you find.</p>';
  if (r.err) return `<p class="err">${esc(r.err)}</p>`;
  if (!r.length) return '<p class="dim">Nothing found. Try another word.</p>';
  const have = new Set(SET.catalog.map(i => i.label.toLowerCase()));
  return `<div class="box">${r.map((x, i) => optRow(x.url ? `<a href="#" data-url="${esc(x.url)}">${esc(x.name)}</a>` : esc(x.name), esc(x.note),
    have.has(x.name.toLowerCase()) ? '<span class="dim">Installed</span>' : !x.a ? `<span class="dim" title="${esc(x.why)}">Manual</span>`
    : `<button class="btn sm" ${EXF.busy[i] ? 'disabled' : ''} onclick="extAdd(${i}, this)">${EXF.busy[i] ? 'Installing…' : icon('plus') + 'Install'}</button>`)).join('')}</div>`;
}
const extDraw = () => { const el = $('exr'); if (el) el.innerHTML = extRows(); };
async function extFind() {
  EXF.k = $('exk').value; EXF.q = $('exq').value.trim(); EXF.r = null; EXF.busy = {}; if (!EXF.q) return extDraw(); extDraw();
  try { const j = await call('ext_find', {kind: EXF.k, q: EXF.q});
    EXF.r = EXF.k === 'skill' ? (j.skills || []).map(x => ({name: x.name, note: `${x.source} · ${(x.installs || 0).toLocaleString()} installs`, a: x.source, b: x.skillId, url: 'https://skills.sh/' + x.id}))
      : (j.servers || []).filter(x => x._meta?.['io.modelcontextprotocol.registry/official']?.isLatest !== false).map(({server: s}) => { const [b, why] = mcpHow(s);
          return {name: s.name.split('/').pop(), note: s.description || '', a: b && s.name, b, why, url: s.repository?.url || s.websiteUrl}; });
  } catch (e) { EXF.r = {err: String(e)}; }
  extDraw();
}
async function extAdd(i, btn) {
  const x = EXF.r[i], before = new Set(SET.catalog.map(c => c.id)); EXF.busy[i] = 1; extDraw();
  try { await call('ext_add', {kind: EXF.k, a: x.a, b: x.b});
    const cat = await call('refresh_catalog'), add = cat.map(c => c.id).filter(id => !before.has(id)), p = P();
    if (p && add.length) await call('set_items', {scope: 'project:' + p.id, items: [...new Set([...onOf(p), ...add])]});
    toast(`${x.name} installed${p && add.length ? ', on for ' + p.name : ''}`, 'ok'); await reloadSettings('cx');
  } catch (e) { delete EXF.busy[i]; extDraw(); toast(`${x.name}: ${e}`, 'err'); }
}
function cxPage() {
  return `${fold('g-find', 'Find &amp; install', `<p class="row"><select id="exk">${options({skill: 'Skills', mcp: 'Connections (MCP)'}, EXF.k)}</select>
      <input id="exq" class="grow" placeholder="Search, e.g. pdf, postgres, figma" value="${esc(EXF.q)}" onkeydown="if(event.key==='Enter')extFind()">
      <button class="btn" onclick="extFind()">${icon('search')}Search</button></p><div id="exr">${extRows()}</div>`, EXF.q ? 1 : 0)}
    ${cxGroup('g-cx', 'Connections &amp; Skills' + infoI('Everything your Claude Code has. Switch something off here and no project gets it.'), 'global')}${cxFold('g-plugin', 'Plugins & mods', 'plugin', 'global')}
    ${fold('g-agents', 'Agents' + infoI('Helpers the main session hands work to, often on cheaper models.'), ccAgents() + agentsPage(),
      `${(CCAG || []).filter(a => !SET.off.includes('agent:' + a.name)).length + (S.agents_on ? AGENTS.filter(a => a.on).length : 0)}/${(CCAG || []).length + AGENTS.length}`)}
    <p class="hint">Build toolchains (Node, Rust, WSL, Xcode…): <a href="#" onclick="event.preventDefault(); openSettings('build')">Settings › Build &amp; Release</a></p>`;
}
/* #146 Settings › Build & Release: app-wide build options. Mac signing = per-project service → shown for the open project.
   Network Mac = macHtml (#147 SSH; build/deps/signing #148–#150). */
function buildPage() {
  setTimeout(depsFill);
  const p = P(), mac = p && svcOf(p).includes('macsign'), keys = RELC[cur]?.c?.sign || [],
    go = mac ? `rightTab('svc')` : `svcAdd('macsign').then(() => rightTab('svc'))`;
  return sec('CI', `<div class="box" id="set-ci">${optRow('CI builds', 'Let GitHub Actions build the other platforms (costs Actions minutes).', sw(S.ci_builds, `call('set_ci_builds', {on: this.checked}).then(refresh)`))}</div>`)
    + `
    ${fold('g-deps', 'Dependencies (this computer, WSL for Linux)' + infoI('Tools this computer needs to build the platforms your projects pick.'),`<div id="depsBox">${skel(90, 60, 'b100', 'b100', 'b100')}</div>`, 0, `<button class="ib" title="Check again" onclick="DEPSC=null; depsFill()">${icon('loop')}</button>`)}
    ${sec('Mac signing (Apple Developer ID)', `<div class="box">${p ? optRow(esc(p.name), !mac ? 'Not set up: Mac builds stay unsigned' : keys.includes('APPLE_SIGNING_IDENTITY') ? 'Keys in .env: ' + keys.join(', ') : 'Set up · signing keys not in .env yet',
      `<button class="btn sm" onclick="closeDlg(); ${go}">${icon('apple')}${mac ? 'Open' : 'Set up'}</button>`) : empty('Open a project to set up its Mac signing', startProj)}</div>`)}
    ${sec('Network Mac' + infoI('Build, sign and notarize macOS apps on a Mac on your network over SSH.'), macHtml())}`;
}
/* #147 Network Mac: guided SSH setup (#160 one-click key install). Auth = ssh keys (BatchMode) → nothing secret saved. MACT = last test (kept, no auto re-run). */
let MACT = null;
const macIn = (k, ph) => `<input id="mac-${k}" value="${esc(S.mac?.[k] || '')}" placeholder="${ph}" onchange="macSave()" style="width:100%">`;
function macHtml() {
  const t = MACT, chk = (n, v, hint) => optRow(n, v ? esc(v) : hint, v ? `<span class="ok">${icon('check')}</span>` : `<span class="faint">missing</span>`);
  return `<div class="box" id="set-mac">
    ${optRow('1. Turn on Remote Login on the Mac', 'System Settings › General › Sharing › Remote Login', '')}
    ${S.mac?.host && !MACF ? optRow('2. SSH login', esc(macTo()) + (t?.ok ? ' · key login works' : ''), `<button class="btn sm" onclick="MACF=1; $('set-mac').outerHTML = macHtml()">Change</button>`)
      : optRow('2. Set up SSH', 'The Mac\'s name or IP and your login. The password is not saved.',
        `<div style="display:flex;flex-direction:column;gap:6px;width:260px"><input id="macf-host" value="${esc(S.mac?.host || '')}" placeholder="mac-mini.local">
          <div style="display:flex;gap:6px"><input id="macf-user" value="${esc(S.mac?.user || '')}" placeholder="Mac username" style="width:100%"><input id="macf-pw" type="password" placeholder="Mac password" style="width:100%" onkeydown="event.key === 'Enter' && macKey()"></div>
          <button class="btn sm" id="macKeyBtn" onclick="macKey()">${icon('key')}Set up SSH</button></div>`)}
    ${optRow('3. Build folder on the Mac', 'Empty = ~/clauracle-build', `<div style="width:260px">${macIn('dir', '~/clauracle-build')}</div>`)}
    ${optRow('4. Test connection', t ? (t.ok ? esc(t.os) : '<span class="err">' + esc(t.err || 'Failed') + '</span>') : 'Logs in with your key and checks the build tools',
      `<button class="btn sm" id="macTestBtn" onclick="macTest()" ${S.mac?.host ? '' : 'disabled'}>${icon('loop')}Test</button>`)}
    ${t?.ok ? (t.xcode ? chk('Xcode tools', t.xcode) : optRow('Xcode tools', "Opens Apple's installer on the Mac: click Install there, then Test again", `<button class="btn sm" onclick="macXcode(this)">${icon('apple')}Install</button>`)) + chk('Rust', t.rust, 'Set up installs it') + chk('Node.js', t.node, 'Set up installs it')
      + optRow('5. Build tools', MACS.ok ? '<span class="ok">Ready: Node.js, Rust + both Mac targets</span>' : 'Installs what is missing (Homebrew node, rustup, Apple silicon + Intel targets). Safe to rerun.',
        `<button class="btn sm" onclick="macSetup()" ${MACS.busy ? 'disabled' : ''}>${MACS.busy ? '<span class="spin"></span>Installing…' : icon('hammer') + 'Set up'}</button>`)
      + (MACS.lines.length ? `<div id="mac-log" class="mono dim" style="font-size:12px;max-height:10em;overflow:auto;white-space:pre-wrap;margin:4px 0">${MACS.lines.map(macLn).join('')}</div>` : '') : ''}</div>`;
}
/* #148 Set up = store::mac_setup over ssh; lines stream in via event mac_setup (STEP/WARN/ERR prefixes styled), re-tests at end. */
const MACS = {busy: false, ok: false, lines: []};
const macLn = l => { const m = /^(STEP|WARN|ERR|DONE) ?(.*)/.exec(l) || [0, '', l];
  return `<div${m[1] === 'ERR' ? ' class="err"' : m[1] === 'WARN' ? ' style="color:var(--crab)"' : m[1] === 'STEP' ? ' style="color:var(--text)"' : ''}>${esc(m[1] === 'DONE' ? 'Done' : m[2])}</div>`; };
function macLine(l) { MACS.lines.push(l); const g = $('mac-log'); if (!g) return $('set-mac') && ($('set-mac').outerHTML = macHtml());
  g.insertAdjacentHTML('beforeend', macLn(l)); g.scrollTop = g.scrollHeight; }
async function macSetup() {
  Object.assign(MACS, {busy: true, ok: false, lines: []}); $('set-mac').outerHTML = macHtml();
  MACS.ok = await call('mac_setup', {mac: S.mac}).catch(e => (macLine('ERR ' + e), false)); MACS.busy = false;
  toast(MACS.ok ? 'Mac build tools ready' : 'Mac setup failed: see Settings › Build & Release', MACS.ok ? 'ok' : 'err');
  await macTest();
}
/* #160 Set up SSH: lib mac_key = ssh-keygen if needed + key → authorized_keys (password once, askpass) + saves cfg.mac → Test. MACF = form open. */
let MACF = 0;
const macTo = () => (S.mac.user ? S.mac.user + '@' : '') + S.mac.host;
async function macKey() {
  const b = $('macKeyBtn'), mac = {host: $('macf-host').value.trim(), user: $('macf-user').value.trim(), dir: S.mac?.dir || ''}, pw = $('macf-pw').value;
  if (!mac.host || !pw) return toast('Enter the Mac name and password', 'warn');
  b.disabled = true; b.innerHTML = '<span class="spin"></span>Setting up…';
  try { await call('mac_key', {mac, pw}); S.mac = mac; MACF = 0; MACS.ok = false; MACS.lines = []; $('set-mac').outerHTML = macHtml(); await macTest(); }
  catch (e) { toast('SSH setup failed: ' + e, 'err'); b.disabled = false; b.innerHTML = icon('key') + 'Set up SSH'; }
}
async function macXcode(b) {
  b.disabled = true; await call('mac_xcode', {mac: S.mac}).then(() => toast('Click Install in the dialog on the Mac, then Test again', 'info'), e => toast('Xcode install: ' + e, 'err'));
  b.disabled = false;
}
async function macSave() {
  S.mac = {...S.mac, dir: $('mac-dir').value.trim()}; MACT = null; MACS.ok = false; MACS.lines = [];
  await call('set_mac', {mac: S.mac}); $('set-mac').outerHTML = macHtml();
}
async function macTest() {
  const b = $('macTestBtn'); if (b) { b.disabled = true; b.innerHTML = '<span class="spin"></span>Testing…'; }
  MACT = await call('mac_test', {mac: S.mac}).catch(e => ({ok: false, err: String(e)}));
  $('set-mac') && ($('set-mac').outerHTML = macHtml());
}
/* Dependencies (#28): toolchains for local builds of the picked target platforms (store::deps, 60s cache). Missing → AI installs it
   in the current project's session (official installer/package manager) or the download page. Other OSes build in CI (#27). */
const DEPS = {node: ['Node.js', [], 'https://nodejs.org/en/download'], rust: ['Rust', [], 'https://rustup.rs'],
  jdk: ['Java JDK 17+', ['android'], 'https://adoptium.net'], android_sdk: ['Android SDK', ['android'], 'https://developer.android.com/studio'],
  android_ndk: ['Android NDK', ['android'], 'https://developer.android.com/ndk/downloads'], xcode: ['Xcode', ['macos', 'ios'], 'https://developer.apple.com/xcode/'],
  webkit: ['WebKitGTK 4.1 dev', ['linux'], 'https://v2.tauri.app/start/prerequisites/#linux'],
  wsl: ['WSL with Ubuntu (+ Node.js, Rust, WebKitGTK 4.1 dev inside)', ['linux'], 'https://learn.microsoft.com/windows/wsl/install']}; /* #131 */
let DEPSC = null;
const depsGet = async () => (DEPSC && Date.now() - DEPSC.t < 60e3) ? DEPSC.d : (DEPSC = {d: await call('deps').catch(() => ({})), t: Date.now()}).d;
// missing deps for project p's picked targets (only ids this OS reports)
async function depsFill() {
  const d = await depsGet(), box = $('depsBox'); if (!box) return;
  const want = new Set(S.projects.flatMap(p => p.targets || []));
  box.innerHTML = `<div class="box">`
    + Object.keys(DEPS).filter(k => k in d).map(k => { const [n, ts, url] = DEPS[k], need = ts.filter(t => want.has(t));
      return optRow(esc(n), d[k] ? 'Installed' : (ts.length ? (need.length ? 'Needed for ' + need.map(tgtName).join(', ') : 'Only for ' + ts.map(tgtName).join(', ')) : 'Not found'),
        d[k] ? `<span class="ok">${icon('check')}</span>` : `${cur ? `<button class="btn sm" onclick="depsAi('${k}')">${icon('sparkle')}Install with AI</button>` : ''}<button class="btn ghost sm" data-url="${url}">Download</button>`).replace('class="opt-row"', `class="opt-row" data-dep="${k}"`); }).join('') + '</div>';
}
async function depsAi(k) {
  const [n, ts] = DEPS[k], text = `Install ${n} on this computer${ts.length ? ` (to build for ${ts.map(tgtName).join(', ')})` : ''}: official installer or package manager, per user if possible, set any env vars it needs, then check it's found. Ask me before large downloads.`;
  if (!await sure(`Ask Claude in ${P().name}?\n\n${text}\n${estTok()}`)) return;
  closeDlg(); DEPSC = null; sendNow(cur, {text, atts: []});
}
// Custom agents: the user's own; each gets a random face seed on creation, clicking the face rolls a new one
function agentsPage() {
  return '<div data-f="g-own">' + sec('Custom agents' + infoI('Agents you write yourself. Switched off = none of them is given to Clauracle.'),
    (AGENTS.length ? optRow('Give custom agents to Clauracle', '', sw(S.agents_on, 'call(\'set_agents\', {agents: AGENTS, on: this.checked}).then(() => openSettings())')) : '')
    + `<div style="${S.agents_on ? '' : 'opacity:.5'}">` + (AGENTS.length ? '' : empty('No custom agents yet', `<button class="btn sm" onclick="agentAdd()">${icon('plus')}Add agent</button>`)) + AGENTS.map((a, i) => `<div class="agent"><div class="row"><button class="ib agface" title="Roll another face" onclick="agentSet(${i}, 'face', agentSeed())">${face(a.face || a.name, 26)}</button>${sw(a.on, `agentSet(${i}, 'on', this.checked)`)}<input class="mono" value="${esc(a.name)}" style="width:160px" onchange="agentSet(${i}, 'name', this.value.trim())">
        <select onchange="agentSet(${i}, 'model', this.value)">${options({haiku: 'Haiku', sonnet: 'Sonnet', opus: 'Opus', inherit: 'Same as session'}, a.model || 'inherit')}</select>
        <span class="grow"></span><button class="ib" title="Remove agent" onclick="agentDel(${i})">${icon('trash')}</button></div>
      <input value="${esc(a.description)}" placeholder="When Clauracle should use it" onchange="agentSet(${i}, 'description', this.value)">
      <textarea placeholder="Its instructions" onchange="agentSet(${i}, 'prompt', this.value)">${esc(a.prompt)}</textarea></div>`).join('')
    + `</div>`, AGENTS.length ? agentAddBtn() : '') + '</div>'
    + sec('Chat', optRow('Group thread', 'What Clauracle and its agents say to each other shows in the chat as one thread you can reply in', sw(UI.gt, 'UI.gt = this.checked; layout(); cur && drawLog()')));
}
/* #407 Claude Code's own agents (built-ins, ~/.claude/agents, plugins): CCAG ← "commands" event (CLI initialize `agents`);
   off = global cfg.off "agent:<name>" → Rust denies Agent(name)/Task(name); session reconnects (set_off) */
let CCAG; try { CCAG = JSON.parse(localStorage.ccag || 'null'); } catch {}
function ccAgents() {
  const own = new Set(AGENTS.map(a => a.name.trim())), off = SET.off.filter(i => i.startsWith('agent:')).map(i => i.slice(6));
  const xs = [...(CCAG || []).filter(a => !own.has(a.name)), ...off.filter(n => !(CCAG || []).some(a => a.name === n)).map(name => ({name}))];
  return sec('Clauracle agents' + infoI('The fixed agents your AI provider brings (built-in, its agents folder, plugins). Switched off = Clauracle can\'t start it, in every project.'),
    xs.map(a => optRow(`<span class="row" style="gap:8px">${face(a.name, 24)}<b>${esc(roleOf(a))}</b><small class="dim">${esc(a.name)}</small></span>`, esc(agBrief(a.description)), sw(!off.includes(a.name), `setGlobalOff('agent:${esc(a.name)}', !this.checked)`))).join('')
      || empty('Listed once a project\'s session has started', `<button class="btn sm" onclick="closeDlg()">Back to the project</button>`));
}
const agentSeed = () => Math.random().toString(36).slice(2, 10);
const agentAdd = () => { AGENTS.push({name: 'helper', model: 'haiku', description: '', prompt: '', on: true, face: agentSeed()}); saveAgents(); };
const agentAddBtn = () => `<button class="ib" title="Add custom agent" onclick="agentAdd()">${icon('plus')}</button>`;
async function agentDel(i) {
  if (await sure(`Remove the agent "${AGENTS[i].name}"?\n\nIts instructions are deleted.`, 'Remove', 'Cancel', undefined, true)) { AGENTS.splice(i, 1); saveAgents(); }
}
const agentSet = (i, k, v) => { AGENTS[i][k] = v; saveAgents(); };
const saveAgents = () => call('set_agents', {agents: AGENTS}).then(() => openSettings());
// You: nickname (name on your chat messages; never used to address you) and interests/prefs (→ ~/.clauracle/you.md, read by the AI only when needed)
const YOU_TAGS = ['Web apps', 'Websites', 'Mobile apps', 'Desktop apps', 'Games', 'Game engines', 'Backend & APIs', 'DevOps & cloud', 'CLI tools',
  'Machine learning', 'Data & analytics', 'Embedded & hardware', 'Security', 'Open source', 'Art & design', '3D', 'Music & audio', 'Video', 'Writing', 'Automation'];
const you = (ch) => call('set_you', {you: {...{nick: '', tags: [], prefs: ''}, ...S.you, ...ch}}).then(refresh).then(() => openSettings('ai'));
const youTag = t => you({tags: S.you?.tags?.includes(t) ? S.you.tags.filter(x => x !== t) : [...(S.you?.tags || []), t]});
const youFind = el => { const q = el.value.trim().toLowerCase();
  el.closest('.you').querySelectorAll('.chips .pill:not(.on)').forEach(c => c.hidden = !!q && !c.textContent.toLowerCase().includes(q)); };
function youHtml() {
  const y = S.you || {}, tags = y.tags || [], all = [...new Set([...tags, ...YOU_TAGS])];
  return fold('a-you', 'You' + infoI('Optional. The nickname labels your own messages in the chat. Clauracle reads interests and preferences only when a choice depends on your taste.'), `<div class="you">    <div class="grid2"><span>Nickname</span><input value="${esc(y.nick || '')}" placeholder="${esc(P()?.gh || 'e.g. Kim')} (default: your GitHub name)" onchange="you({nick: this.value})">
      <span>Interests</span><div><input placeholder="Search or add your own (Enter)" style="width:100%;margin-bottom:6px" oninput="youFind(this)" onkeydown="if (event.key === 'Enter' && this.value.trim()) youTag(this.value.trim())">
        <div class="chips">${all.map(t => `<span class="pill${tags.includes(t) ? ' on' : ''}" onclick="youTag(${esc(JSON.stringify(t))})">${esc(t)}</span>`).join('')}</div></div>
      <span>Preferences</span><textarea style="width:100%;min-height:60px;font-size:13px" placeholder="e.g. TypeScript over JS, short answers, explain like I'm new to Rust, dark UIs" onchange="you({prefs: this.value})">${esc(y.prefs || '')}</textarea>
    </div></div>`, [y.nick, tags.length && tags.length + ' interests', y.prefs && 'preferences'].filter(Boolean).join(' · ') || 'optional', '', false);
}
/* #257 Accounts 1-2-3 while setup is missing (AI login + GitHub); step 3 (You) optional → never keeps it up */
function acctSteps() {
  const st = [[S.accounts.length > 0, 'Sign in to Claude', `<div class="btnrow"><button class="btn sm" onclick="addAcctDlg()">${icon('plus')}Add account</button></div>`],
    [ghUsers.length > 0, 'Add a GitHub account', `<div class="btnrow">${S.gh ? `<button class="ib" title="Add a GitHub account" onclick="jumpTo('[data-f=a-gh]'); addGh()">${icon('plus')}</button>` : `<button class="btn primary" onclick="install('gh', this).then(() => reloadSettings())">Install the GitHub CLI</button>`}</div>`],
    [!!(S.you?.nick || S.you?.tags?.length), 'Tell Clauracle about you (optional)', `<div class="btnrow"><button class="btn" onclick="jumpTo('[data-f=a-you]')">Open You</button></div>`]];
  if (st[0][0] && st[1][0]) return '';
  return `<ol class="stepper" style="margin-bottom:14px">${st.map(([ok, t, body], i) => `<li class="${ok ? 'done' : ''}"><span class="sn">${ok ? icon('check') : i + 1}</span><b>${t}</b>${ok ? '' : body}</li>`).join('')}</ol>`;
}
/* #383 relays (was "pool"): accounts in one relay take over from each other at their limits. ≥1 relay → outer sections
   (.relay > .rl[data-r]) + "Not in a relay"; drag (Sortable, handle summary) between them → acct relay. RLS = section names this
   render (handlers use the index ∵ names in attrs need escaping); RNEW = added but still empty (UI-only, gone at restart). */
let RLS = [], RNEW = [];
const relays = () => [...new Set([...S.accounts.map(a => a.relay).filter(Boolean), ...RNEW])];
function relayAdd() { const all = relays(); let n = all.length + 1; while (all.includes('Relay ' + n)) n++; RNEW.push('Relay ' + n); openSettings(); }
async function relayRename(i, v) { const old = RLS[i]; v = v.trim(); if (!v || v === old) return openSettings();
  RNEW = RNEW.map(r => r === old ? v : r);
  for (const a of S.accounts.filter(a => a.relay === old)) await call('update_account', {id: a.id, changes: {relay: v}});
  openSettings(); }
const relayDrop = i => { RNEW = RNEW.filter(r => r !== RLS[i]); openSettings(); };
const relaySort = () => $('dlg').querySelectorAll('.rl').forEach(l => Sortable.create(l, {group: 'relay', draggable: '.acct', handle: 'summary', animation: 150,
  onEnd: e => e.from !== e.to && acct(e.item.dataset.id, {relay: RLS[e.to.dataset.r] || ''})}));
function aiPage() {
  RLS = relays();
  const meter = (v, l) => v == null ? '' : `<small class="dim">${l} <span class="bar"><i style="width:${v}%"></i></span> ${v}%</small>`;
  const row = a => { const u = S.usage[a.id] || {}, sub = [a.id === 'Main' ? 'Claude Code login' : a.base_url ? esc(a.base_url) : 'setup-token', a.id === S.default_account && 'default'].filter(Boolean).join(' · ');
    return `<details class="acct" data-id="${esc(a.id)}"><summary>
      ${icon('claude')}<span class="who"><small>${sub}</small><b>${esc(a.nick)}</b></span>
      <span class="um">${meter(u.five_hour, '5h') + meter(u.seven_day, 'week') || '<small class="dim">No usage yet</small>'}</span></summary>
      <div><div class="grid2">
        <span>Name</span><input value="${esc(a.nick)}" onchange="this.value.trim() && acct('${esc(a.id)}', {nick:this.value.trim()})">
        <span>Limits</span><span class="row wrap">5-hour <input type="number" min="1" max="100" value="${a.limits.five_hour}" style="width:70px" onchange="acct('${esc(a.id)}', {limits:{...limitsOf('${esc(a.id)}'), five_hour:+this.value}})">%
          weekly <input type="number" min="1" max="100" value="${a.limits.seven_day}" style="width:70px" onchange="acct('${esc(a.id)}', {limits:{...limitsOf('${esc(a.id)}'), seven_day:+this.value}})">%</span>
        <span></span><small class="dim" style="margin-top:-6px">Auto mode switches account or pauses here; held questions show then.</small>
        <span>Relay</span><span class="row"><input class="grow" value="${esc(a.relay)}" placeholder="None (type a new relay name)" onchange="acct('${esc(a.id)}', {relay:this.value.trim()})">
          ${RLS.length ? `<button class="ib" title="Pick a relay" onclick="relayMenu(this, '${esc(a.id)}')">${icon('chev')}</button>` : ''}</span>
        <span>GitHub for new projects</span><select onchange="acct('${esc(a.id)}', {gh:this.value})">${options({'': 'None', ...Object.fromEntries(ghUsers.map(g => [g, '@' + g]))}, a.gh)}</select>
      </div>
      ${cxGroup('a-cx', 'Connections &amp; Skills for new projects', a.id, a)}${cxFold('a-plugin', 'Plugins & mods for new projects', 'plugin', a.id, a)}
      <div class="btns"><button class="btn" onclick="csAcct('${esc(a.id)}')">${icon('sliders')}Advanced AI settings</button><span class="grow"></span>${a.id === S.default_account ? '' : `<button class="btn" onclick="call('set_default_account', {id:'${esc(a.id)}'}).then(() => openSettings())">Make default</button>`}
        ${a.id === 'Main' ? '' : `<button class="btn ghost" onclick="rmClaude('${esc(a.id)}')">Remove</button>`}</div></div></details>`; };
  const list = !RLS.length ? `<div class="accts">${S.accounts.map(row).join('')}</div>` :[...RLS, ''].map((r, i) => { const as = S.accounts.filter(a => (a.relay || '') === r);
    return `<div class="relay"><div class="rh">${r ? `<input class="rname" value="${esc(r)}" title="Rename this relay" onchange="relayRename(${i}, this.value)">` : '<b>Not in a relay</b>'}
      <small class="dim">${as.length} account${as.length === 1 ? '' : 's'}</small><span class="grow"></span>${r && !as.length ? `<button class="ib" title="Remove this empty relay" onclick="relayDrop(${i})">${icon('trash')}</button>` : ''}</div>
      <div class="accts rl" data-r="${r ? i : ''}">${as.map(row).join('')}</div></div>`; }).join('');
  return acctSteps() + youHtml() + sec('AI accounts' + infoI('Accounts in one relay take over from each other when one hits its limit.' + (RLS.length ? ' Drag an account by its title to move it to another relay.' : '')),
    list, `<button class="btn sm" title="Add relay" onclick="relayAdd()">${icon('plus')}Relay</button><button class="btn sm" title="Add account" onclick="addAcctDlg()">${icon('plus')}Account</button>`)
    + `${fold('a-gh', 'Other accounts' + infoI('GitHub and Google accounts. Projects pick theirs in the project manager, and one account can serve any number of projects.'),
      `<div class="idcards">${otherCards() || empty('No other accounts yet', `<button class="ib" title="Add an account" onclick="otherAdd(this)">${icon('plus')}</button>`)}</div><p id="ghMsg" class="hint"></p>`,
      new Set(ghUsers).size + (GACC || []).length || '', `<button class="ib" title="Add an account" onclick="event.preventDefault(); otherAdd(this)">${icon('plus')}</button>`, true)}
    ${fold('a-lessons', 'Lessons' + infoI('Never-again rules, one per line. Every session in every project gets them.'), `    <textarea id="lessons" class="mono" style="width:100%;min-height:90px;font-size:12px" placeholder="e.g. tauri build: cargo on PATH same cmd, else 'cargo metadata' fails"
      onchange="call('set_lessons', {lessons: this.value.split('\\n')}).then(refresh)">${esc((S.lessons || []).join('\n'))}</textarea>`, (S.lessons || []).filter(Boolean).length || '', '', false)}`;
}
const limitsOf = id => S.accounts.find(a => a.id === id).limits;
// own dropdown ∵ native <datalist> popup = off-style WebView box; typing a new name stays in the input
const relayMenu = (el, id) => { const cur = S.accounts.find(a => a.id === id).relay || '';
  menuAt(el, ['', ...relays()].map(p => ({label: p || 'None', check: p === cur, run: () => acct(id, {relay: p})}))); };
const acct = (id, changes) => call('update_account', {id, changes}).then(() => openSettings());
/* Account cards: provider icon, provider name, account name under it. Same card on the Accounts page, in the first-run
   setup and as the project's palette. GACC = Google accounts from gcloud, loaded once per run (null = not yet, false = no gcloud). */
let GACC = null;
const gAccts = () => GACC !== null ? Promise.resolve(GACC)
  : call('gcloud_accounts').then(r => GACC = r === null ? false : (Array.isArray(r) ? r.map(a => a.account) : []), () => GACC = []);
const acard = (ic, prov, name, attrs = '', on) => `<button class="idcard${on ? ' on' : ''}" ${attrs}>${icon(ic)}<span><small>${prov}</small><b>${esc(name)}</b></span></button>`;
// after an account is added or removed: redraw wherever it happened (setup overlay or Settings)
const acctDraw = async () => { if (!$('onb')) return reloadSettings(); SET = await call('settings'); ghUsers = SET.gh_users; await refresh(); onbDraw(); };
function otherCards() {
  if (GACC === null) gAccts().then(() => $('onb') ? onbDraw() : page === 'ai' && $('dlg').open && openSettings());
  return [...new Set(ghUsers)].map(u => acard('gh', 'GitHub', '@' + u, `title="Account menu" onclick="menuAt(this, [{label: 'Log out', run: () => rmGh('${esc(u)}')}])"`))
    .concat((GACC || []).map(a => acard('google', 'Google', a, 'title="Signed in through gcloud"'))).join('');
}
const otherAdd = el => menuAt(el, [{label: 'GitHub', run: () => S.gh ? addGh() : install('gh', el).then(ok => ok && acctDraw())},
  {label: 'Google', run: () => GACC === false ? install('gcloud', el).then(ok => ok && (GACC = null, acctDraw())) : gAdd()}]);
async function gAdd() { toast('Finish signing in to Google in your browser…', 'info');
  try { await call('gcloud_login'); GACC = null; acctDraw(); } catch (e) { toast(String(e), 'err'); } }
/* First-run setup: one step per slide, sliding sideways, Next at the bottom right. Everything a session would otherwise
   ask for on the fly is asked here once. Shown once per computer (localStorage onb), only while there are no projects yet. */
let ONB = 0, ONOTE = null;
const ONBS = [
  () => `${icon('logo').replace('class="i"', 'class="i ologo"')}<h1>Welcome to Clauracle</h1><p class="dim">Build websites, software and games with Claude Code and GitHub. A few steps and you are ready; you can change all of it later in Settings.</p>`,
  () => `<h1>What should we call you?</h1><p class="dim">Optional. The nickname labels your own messages in the chat.</p>
    <input id="onick" style="width:100%" value="${esc(S.you?.nick || '')}" placeholder="Nickname (default: your GitHub name)" onchange="call('set_you', {you: {...{nick: '', tags: [], prefs: ''}, ...S.you, nick: this.value}}).then(refresh)">`,
  () => `<h1>Your accounts</h1><p class="dim">Sign in once here. Each project then picks which of these it uses.</p>
    <h3>AI accounts</h3><div class="idcards">${S.accounts.map(a => acard('claude', 'Claude', a.nick)).join('')}<button class="btn" onclick="addAcctDlg()">${icon('plus')}Add AI account</button></div>
    <h3>Other accounts</h3><div class="idcards">${otherCards()}<button class="btn" onclick="otherAdd(this)">${icon('plus')}GitHub, Google…</button></div><p id="ghMsg" class="hint"></p>`,
  () => `<h1>Permissions</h1><p class="dim">Asked now, so nothing pops up while you work.</p>
    <h3>Notifications</h3><div class="box">${optRow('Desktop notifications', 'When a question needs you or a limit pauses the work', ONOTE === 'granted'
      ? `<span class="pill">${icon('check')}Allowed</span>` : `<button class="btn" onclick="onbNotify()">Allow</button>`)}</div>
    <h3>Keychain</h3><p class="dim">Sign-in tokens are kept in your system keychain. If your computer asks whether Clauracle may use it, choose Always allow.</p>`,
  () => `${icon('check').replace('class="i"', 'class="i ologo"')}<h1>You're set</h1><p class="dim">Add a project from the side panel to start. Accounts and permissions stay under Settings.</p>`];
function onbOpen() {
  if ($('onb')) return;
  ONB = 0; document.body.insertAdjacentHTML('beforeend', `<div id="onb"><div class="otrack"></div><div class="obar"><button class="btn ghost" id="oskip" onclick="onbEnd()">Skip setup</button>
    <span class="grow"></span><span class="odots"></span><span class="grow"></span><button class="btn ghost" id="oback" onclick="onbGo(-1)">Back</button><button class="btn primary" id="onext" onclick="onbGo(1)"></button></div></div>`);
  if (!SET) call('settings').then(x => { SET ||= x; ghUsers = SET.gh_users; onbDraw(); }).catch(() => {});
  T.notification?.isPermissionGranted?.().then(ok => { ONOTE = ok ? 'granted' : null; onbDraw(); }).catch(() => {});
  onbDraw();
}
function onbDraw() {
  const o = $('onb'); if (!o) return;
  const w = $('wbar'); o.style.top = (w && !w.hidden ? w.offsetHeight : 0) + 'px'; // the own title bar stays usable
  o.querySelector('.otrack').innerHTML = ONBS.map(f => `<div class="oslide"><div>${f()}</div></div>`).join('');
  o.querySelector('.otrack').style.transform = `translateX(-${ONB * 100}%)`;
  o.querySelector('.odots').innerHTML = ONBS.map((_, i) => `<i class="${i === ONB ? 'on' : ''}"></i>`).join('');
  $('oback').hidden = !ONB; $('oskip').hidden = ONB === ONBS.length - 1;
  $('onext').textContent = ONB === ONBS.length - 1 ? 'Get started' : 'Next';
}
const onbGo = d => { if (ONB + d >= ONBS.length) return onbEnd(); ONB = Math.max(0, ONB + d); onbDraw(); };
function onbEnd() { try { localStorage.onb = 1; } catch {} $('onb')?.remove(); }
async function onbNotify() {
  ONOTE = await (T.notification?.requestPermission?.() ?? Promise.resolve('granted')).catch(() => null);
  if (ONOTE === 'granted') call('test_notify'); else toast('Notifications are off. Turn them on for Clauracle in your system settings.', 'info');
  onbDraw();
}
// Appearance: per computer (localStorage), applied before anything draws
const ACCENTS = {Mono: ['#000000', '#ffffff'], Indigo:['#4f5bd5', '#8b93ff'], Blue: ['#2563eb', '#6ea8fe'], Teal: ['#0f8b80', '#3cc9b9'], Green: ['#2b8a3e', '#69db7c'],
  Orange: ['#c8613f', '#e8916f'], Pink: ['#c2255c', '#f783ac'], Graphite: ['#4a4a50', '#b8b8c0']};
let LOOK = {theme: 'system', accent: 'Indigo', tint: '', tintPct: 30, corners: 'soft', font: 'sans-serif', scale: 110, glass: true, motion: true};
// the stylesheet's surface colours per theme (light, dark), the base the tint mixes into; keep in sync with :root
const SURF = [{'--bg': '#ffffff', '--side': '#f7f7f8', '--card': '#ffffff', '--line': '#ececee', '--line2': '#dcdce0', '--hover': '#f0f0f2', '--code': '#f6f6f7', '--text': '#1d1d1f', '--dim': '#6e6e76', '--faint': '#a1a1a8'},
  {'--bg': '#161618', '--side': '#1c1c1f', '--card': '#232326', '--line': '#2c2c30', '--line2': '#3a3a3f', '--hover': '#2a2a2e', '--code': '#1c1c1f', '--text': '#ededef', '--dim': '#a0a0a8', '--faint': '#6c6c74'}];
const MONO = [{'--bg': '#fff', '--side': '#fff', '--card': '#fff', '--line': '#bbb', '--line2': '#000', '--hover': '#e8e8e8', '--code': '#f2f2f2', '--text': '#000', '--dim': '#222', '--faint': '#555'},
  {'--bg': '#000', '--side': '#000', '--card': '#000', '--line': '#444', '--line2': '#fff', '--hover': '#1c1c1c', '--code': '#0d0d0d', '--text': '#fff', '--dim': '#ddd', '--faint': '#aaa'}];
// tint weight per colour: dark theme = contrast-like (darks stay dark, lines + text take the colour); light = surfaces
const TINT_W = [{'--text': .25, '--dim': .4, '--faint': .5},
  {'--bg': .3, '--side': .35, '--code': .3, '--card': .5, '--hover': .6, '--line': 1.1, '--line2': 1.3, '--text': 1.2, '--dim': 1.4, '--faint': 1.3}];
// presets (LOOKS) = theme + accent + tint only (scale/glass/motion stay per computer); user's own in localStorage.presets
const PRESET_KEYS = ['theme', 'accent', 'tint', 'tintPct', 'corners', 'font'];
const PRE_DEF = {corners: 'soft', font: 'sans-serif'}; // key missing in a preset → this
const LOOKS = {Default: {theme: 'system', accent: 'Indigo', tint: '', tintPct: 30, corners: 'soft'},
  'Classic Claude': {theme: 'system', accent: 'Orange', tint: 'Orange', tintPct: 25, corners: 'soft', font: 'serif'},
  'The Matrix': {theme: 'dark', accent: 'Mono', tint: 'Green', tintPct: 100, corners: 'square', font: 'monospace'},
  Paper: {theme: 'light', accent: 'Mono', tint: '', tintPct: 30, corners: 'square'}, Midnight: {theme: 'dark', accent: 'Indigo', tint: 'Indigo', tintPct: 60, corners: 'soft'},
  Synthwave: {theme: 'dark', accent: 'Pink', tint: 'Indigo', tintPct: 80, corners: 'round'}, Ember: {theme: 'dark', accent: 'Orange', tint: 'Orange', tintPct: 45, corners: 'soft'},
  Seaside: {theme: 'light', accent: 'Teal', tint: 'Blue', tintPct: 35, corners: 'round'}};
let MYPRE = {}; try { MYPRE = JSON.parse(localStorage.presets || '{}'); } catch {}
const savePre = () => { try { localStorage.presets = JSON.stringify(MYPRE); } catch {} };
const preMatch = () => Object.entries({...LOOKS, ...MYPRE}).find(([, v]) => PRESET_KEYS.every(k => (k === 'tintPct' && !v.tint) || (v[k] ?? PRE_DEF[k] ?? '') === (LOOK[k] ?? '')))?.[0]; // no tint → amount irrelevant
function presetItems() {
  const m = preMatch(), item = ([n, v]) => ({check: m === n, label: n, run: () => setLook({...PRE_DEF, ...v, preset: n})});
  const mine = Object.entries(MYPRE).map(([n, v]) => ({...item([n, v]), acts: [{icon: 'x', tip: 'Delete', items: [{label: `Delete ${n}`, run: () => { delete MYPRE[n]; savePre(); openSettings('look'); }}]}]}));
  return [{title: 'Presets'}, ...Object.entries(LOOKS).map(item), ...(mine.length ? ['-', {title: 'Yours'}, ...mine] : []),
    ...(!m && MYPRE[LOOK.preset] ? ['-', {icon: 'plus', label: 'Save as new preset…', run: presetSave}] : [])];
}
function presetSave(name) {
  if (typeof name === 'string') { MYPRE[name] = Object.fromEntries(PRESET_KEYS.map(k => [k, LOOK[k]])); savePre(); return setLook({preset: name}); }
  dlg(`<h2>Save preset</h2><input id="preName" style="width:100%" placeholder="Name" onkeydown="if(event.key==='Enter')preOk()">
    <p id="preMsg" class="err" style="font-size:13px"></p><div class="btns"><button class="btn ghost" onclick="closeDlg()">Cancel</button><button class="btn primary" onclick="preOk()">Save</button></div>`);
  $('preName').focus();
}
function preOk() { const n = $('preName').value.trim();
  if (!n || LOOKS[n]) return $('preMsg').textContent = n ? 'That name is a built-in preset' : 'Give it a name';
  closeDlg(); presetSave(n); }
const TINT_MAX = 40; // ponytail: 40% real mix at the slider's 100%; raise if people want louder tints
try { Object.assign(LOOK, JSON.parse(localStorage.look || '{}')); } catch {}
const darkMq = matchMedia('(prefers-color-scheme: dark)');
function applyLook() {
  const h = document.documentElement, dark = LOOK.theme === 'dark' || (LOOK.theme === 'system' && darkMq.matches);
  h.dataset.theme = dark ? 'dark' : 'light';
  const a = ACCENTS[LOOK.accent]?.[dark ? 1 : 0];
  for (const [k, v] of Object.entries(LOOK.accent === 'Indigo' || !a ? {} : {'--accent': a, '--accent2': `color-mix(in srgb, ${a} 85%, ${dark ? '#fff' : '#000'})`,
    '--soft': `color-mix(in srgb, ${a} ${dark ? 22 : 10}%, var(--bg))`})) h.style.setProperty(k, v);
  if (LOOK.accent === 'Indigo') ['--accent', '--accent2', '--soft'].forEach(k => h.style.removeProperty(k));
  const mono = LOOK.accent === 'Mono'; h.classList.toggle('mono', mono);
  ['square', 'round'].forEach(c => h.classList.toggle('c-' + c, LOOK.corners === c));
  h.classList.toggle('f-serif', LOOK.font === 'serif'); h.classList.toggle('f-mono', LOOK.font === 'monospace');
  // theme tint: surfaces mixed with a colour; slider 100% = TINT_MAX real mix, so text stays readable
  const t = ACCENTS[LOOK.tint]?.[dark ? 1 : 0], m = (LOOK.tintPct ?? 30) * TINT_MAX / 100;
  for (const [k, base] of Object.entries((mono ? MONO : SURF)[dark ? 1 : 0])) t && m ? h.style.setProperty(k, `color-mix(in srgb, ${t} ${Math.min(90, m * (TINT_W[dark ? 1 : 0][k] ?? 1))}%, ${base})`) : h.style.removeProperty(k);
  // fixed-colour parts (terminal cards, terminal view) read these
  if (t && m) { h.style.setProperty('--tint', t); h.style.setProperty('--tintm', m * .6 + '%'); } else ['--tint', '--tintm'].forEach(k => h.style.removeProperty(k));
  h.classList.toggle('noglass', !LOOK.glass); h.classList.toggle('nomotion', !LOOK.motion);
  zoom(LOOK.scale);
}
// on top of the OS display scaling (CSS px are already DPI-independent), so 110% looks the same on any screen
const zoom = pct => call('set_zoom', {scale: pct / 100}).catch(() => {});
function setLook(c) { Object.assign(LOOK, c); try { localStorage.look = JSON.stringify(LOOK); } catch {} applyLook(); openSettings('look'); }
darkMq.addEventListener('change', applyLook);
const choice = (key, opts) => `<span class="seg">${opts.map(o => `<button class="${LOOK[key] === o ? 'on' : ''}" onclick="setLook({${key}:'${o}'})">${o[0].toUpperCase() + o.slice(1)}</button>`).join('')}</span>`;
function lookPage() {
  const dk = document.documentElement.dataset.theme === 'dark' ? 1 : 0, pm = preMatch(), mine = !pm && MYPRE[LOOK.preset];
  return sec('Look', `<div class="box">${optRow('Theme', '', choice('theme', ['system', 'light', 'dark']))}
    ${optRow('Preset', 'Theme, accent, tint, corners and font together', `<span class="row" style="gap:6px">${pm ? '' : `<button class="btn sm" onclick="presetSave(${mine ? 'LOOK.preset' : ''})">${mine ? `Update ${esc(LOOK.preset)}` : 'Save as preset'}</button>`}
      <button class="selbtn" onclick="menuAt(this, presetItems())">${esc(pm || 'Custom')}</button></span>`)}
    ${optRow('Corners', '', choice('corners', ['square', 'soft', 'round']))}
    ${optRow('Font', '', choice('font', ['sans-serif', 'serif', 'monospace']))}
    ${optRow('Accent', 'Mono: pure black and white, high contrast', `<span class="row" style="gap:8px">${Object.entries(ACCENTS).map(([n, c]) => `<button class="swatch ${LOOK.accent === n ? 'on' : ''}" title="${n}" style="background:${c[dk]}" onclick="setLook({accent:'${n}'})"></button>`).join('')}</span>`)}
    ${optRow('Theme tint', 'Colours the whole app a little', `<span style="display:flex;flex-direction:column;gap:8px"><span class="row" style="gap:8px"><button class="swatch ${!LOOK.tint ? 'on' : ''}" title="None" style="background:var(--bg);display:grid;place-items:center;color:var(--faint)" onclick="setLook({tint:''})">${icon('x')}</button>${Object.entries(ACCENTS).filter(([n]) => n !== 'Mono').map(([n, c]) => `<button class="swatch ${LOOK.tint === n ? 'on' : ''}" title="${n}" style="background:${c[dk]}" onclick="setLook({tint:'${n}'})"></button>`).join('')}</span>
      ${LOOK.tint ? `<span class="row" style="gap:8px"><input type="range" min="5" max="100" step="5" value="${LOOK.tintPct}" style="flex:1;padding:0;border:0;background:none" title="Amount"
        oninput="LOOK.tintPct=+this.value; this.nextElementSibling.textContent=this.value+'%'; applyLook()" onchange="setLook({tintPct:+this.value})"><b style="width:40px;text-align:right">${LOOK.tintPct}%</b></span>` : ''}</span>`)}
    ${optRow('UI scale', 'Scales text and everything else together, on top of Windows display scaling',
      `<span class="row" style="gap:10px"><input type="range" min="80" max="150" step="5" value="${LOOK.scale}" style="width:160px;padding:0;border:0;background:none"
        oninput="this.nextElementSibling.textContent=this.value+'%'" onchange="setLook({scale:+this.value})"><b style="width:40px">${LOOK.scale}%</b>
        <button class="btn sm" onclick="setLook({scale:100})">Reset</button></span>`)}</div>`)
    + sec('Performance', `<div class="box">${optRow('Frosted glass', 'Blur behind menus and dialogs. Off is lighter to draw', sw(LOOK.glass, 'setLook({glass:this.checked})'))}
    ${optRow('Animations', 'Shimmer, pulses and fades', sw(LOOK.motion, 'setLook({motion:this.checked})'))}</div>`);
}
function notifyPage() {
  const n = S.notify;
  return sec('Desktop' + infoI('When questions need you in auto mode, or the loop pauses at a limit.'), `<div class="box">${optRow('Desktop notifications', '', sw(n.desktop, 'saveNotify({desktop:this.checked})'))}</div>`)
    + sec('Phone' + infoI('Install the free ntfy app (ntfy.sh) and subscribe to this topic. Anyone who knows the topic can read the pushes.'), `<div class="row"><input id="topic" class="grow sel mono" value="${esc(n.ntfy)}" placeholder="Off" onchange="saveNotify({ntfy:this.value.trim()})">
      <button class="btn sm" onclick="$('topic').value='clauracle-'+crypto.randomUUID().slice(0,13); saveNotify({ntfy:$('topic').value})">Generate</button>
      <button class="btn sm" onclick="call('test_notify')">${icon('bell')}Send a test</button></div>
      <p class="hint">Get the app at <a href="#" data-url="https://ntfy.sh" style="color:var(--accent)">ntfy.sh</a>.</p>`)
    + sec('Updates', `<div class="box">${optRow('Check for updates', S.update_at ? `Last checked ${gago(S.update_at * 1e3)}` : 'Not checked yet',
      `<span class="row" style="gap:6px"><span class="seg">${[[0, 'Never'], [24, 'Every 24h'], [168, 'Weekly'], [720, 'Monthly']].map(([h, t]) => `<button class="${S.update_every === h ? 'on' : ''}" onclick="call('set_update_every', {hours: ${h}}).then(() => openSettings())">${t}</button>`).join('')}</span>
      <button class="btn sm" onclick="this.disabled = true; checkUpdate(true).then(() => openSettings())">Check now</button></span>`)}</div>`);
}
const saveNotify = c => call('set_notify', {notify: {...S.notify, ...c}}).then(refresh);
// Claude Code settings files (user / project / project-local): permissions lists, defaultMode, env; raw JSON fallback.
// ponytail: whole-file write after a fresh read per edit; a CLI write in between the read and the click is lost (rare).
// #29: opened per AI account from Accounts (modal in dlg2); scope account:<id> = ~/.clauracle/account-<id>.json, layered via --settings
let CS = {scope: 'user'};
function csAcct(id) {
  const d = $('dlg2'), a = S.accounts.find(x => x.id === id);
  d.innerHTML = `<div class="small-dlg" style="width:min(640px,92vw)"><h2>Advanced AI settings · ${esc(a.nick)}</h2>
    <div id="csBody">${skel(40, 'b100', 'b100', 'b100', 'b100')}</div><div class="btns"><button class="btn ghost" onclick="$('dlg2').close()">Close</button></div></div>`;
  dlgShell(d.firstElementChild); d.showModal(); CS = {scope: 'account:' + id, acct: id}; csLoad();
}
async function csLoad(scope = CS.scope) {
  if ((scope === 'project' || scope === 'local') && !cur) scope = 'account:' + CS.acct;
  const acct = CS.acct, a = S.accounts.find(x => x.id === acct);
  CS = {scope, acct, ...await call('claude_settings', {scope, pid: cur || ''})};
  const j = CS.json, perm = j?.permissions || {}, el = $('csBody'); if (!el) return;
  const sc = {['account:' + acct]: `${a?.nick || acct} only (Clauracle sessions on this account)`, user: 'Every account (+ plain claude in a terminal)',
    ...(cur ? {project: `${P().name}: shared (committed)`, local: `${P().name}: only you (not committed)`} : {})};
  const list = k => sec({allow: 'Allow without asking', ask: 'Always ask', deny: 'Never allow'}[k], `<div class="row" style="flex-wrap:wrap;gap:6px">${(perm[k] || []).map((r, i) => `<span class="pill mono">${esc(r)} <a style="cursor:pointer" title="Remove" onclick="csEdit(j => j.permissions.${k}.splice(${i}, 1))">${icon('trash')}</a></span>`).join('') || '<span class="hint">None yet: add a rule below.</span>'}</div>
    <div class="row" style="margin-top:6px"><input class="grow mono" placeholder="e.g. Bash(npm test:*), Read(./src/**), WebFetch(domain:github.com)"
      onkeydown="if (event.key === 'Enter' && this.value.trim()) csAdd('${k}', this.value.trim())"><button class="ib" title="Add" onclick="const v = this.previousElementSibling.value.trim(); if (v) csAdd('${k}', v)">${icon('plus')}</button></div>`);
  const env = Object.entries(j?.env || {});
  el.innerHTML = sec('Claude Code settings' + infoI('Claude Code settings for permissions and environment.'), `<div class="row"><select class="grow" onchange="csLoad(this.value)">${options(sc, scope)}</select></div>
    <p class="faint mono" style="font-size:12px">${esc(CS.path)}</p>`) + `
    ${j ? sec('Default permission mode', `<select onchange="csEdit(j => this.value ? j.permissions.defaultMode = this.value : delete j.permissions.defaultMode)">${options({'': 'Not set', default: 'Ask', acceptEdits: 'Accept edits', plan: 'Plan', auto: 'Auto (classifier)', dontAsk: "Don't ask (deny unless allowed)", bypassPermissions: 'Bypass (no checks)'}, perm.defaultMode || '')}</select>`)
      + ['allow', 'ask', 'deny'].map(list).join('')
      + sec('Environment variables', `<div class="box">${env.map(([k, v]) => optRow(`<span class="mono">${esc(k)}</span>`, '', `<input class="mono" value="${esc(String(v))}" onchange="csEdit(j => j.env['${esc(k)}'] = this.value)"><button class="ib" title="Remove" onclick="csEdit(j => delete j.env['${esc(k)}'])">${icon('trash')}</button>`)).join('') || '<p class="hint">None yet: add one below.</p>'}</div>
      <div class="row"><input id="csEk" class="mono" placeholder="NAME" style="width:180px"><input id="csEv" class="grow mono" placeholder="value">
        <button class="ib" title="Add" onclick="const k = $('csEk').value.trim(); if (k) csEdit(j => (j.env ||= {})[k] = $('csEv').value)">${icon('plus')}</button></div>`)
    : '<p class="warn">This file is not valid JSON. Fix it below; nothing else here writes to it until it parses.</p>'}
    ${fold('cs-raw', 'Raw JSON', `<textarea id="csRaw" class="mono" style="width:100%;min-height:220px">${esc(CS.text)}</textarea>
      <p class="row"><button class="btn" onclick="csRaw()">Save JSON</button><span id="csMsg" class="hint"></span></p>`)}`;
}
// one edit = fresh read → change → write → redraw; refuses while the file doesn't parse
async function csEdit(fn) {
  const r = await call('claude_settings', {scope: CS.scope, pid: cur || ''});
  if (!r.json) return toast('Settings file is not valid JSON: fix it in Raw JSON', 'warn');
  r.json.permissions ||= {};
  fn(r.json);
  for (const k of ['allow', 'ask', 'deny']) if (r.json.permissions[k]?.length === 0) delete r.json.permissions[k];
  if (!Object.keys(r.json.permissions).length) delete r.json.permissions;
  if (r.json.env && !Object.keys(r.json.env).length) delete r.json.env;
  await call('claude_settings_set', {scope: CS.scope, pid: cur || '', value: r.json});
  csLoad();
}
const csAdd = (k, rule) => csEdit(j => { const a = j.permissions[k] ||= []; if (!a.includes(rule)) a.push(rule); });
async function csRaw() {
  let v; try { v = JSON.parse($('csRaw').value); } catch (e) { return $('csMsg').textContent = 'Not valid JSON: ' + e.message; }
  if (!v || typeof v !== 'object' || Array.isArray(v)) return $('csMsg').textContent = 'Must be a JSON object { … }';
  await call('claude_settings_set', {scope: CS.scope, pid: cur || '', value: v}); csLoad();
}
// Add account: a small window over Settings; sign in, or paste a setup-token
function addAcctDlg() {
  const d = $('dlg2');
  d.innerHTML = `<div class="small-dlg"><h2>Add an AI account</h2>
    <input id="nick" style="width:100%" placeholder="Name (optional), e.g. Work">
    <button class="btn primary" style="width:100%;justify-content:center;margin-top:10px" onclick="addClaude()">Sign in with Claude</button>
    <div class="ordiv">or</div>
    <div class="row"><input id="token" class="grow mono" placeholder="Token from claude setup-token: sk-ant-oat01-…"><button class="ib" title="Add" onclick="pasteClaude()">${icon('plus')}</button></div>
    <div class="ordiv">or a custom endpoint (proxy, gateway)</div>
    <input id="baseUrl" class="mono" style="width:100%" placeholder="Base URL, e.g. https://gateway.example.com">
    <div class="row" style="margin-top:6px"><input id="apiKey" type="password" class="grow mono" placeholder="API key"><button class="ib" title="Add" onclick="pasteClaude(1)">${icon('plus')}</button></div>
    <p id="loginMsg" class="hint"></p><div class="btns"><button class="btn ghost" onclick="$('dlg2').close()">Cancel</button></div></div>`;
  dlgShell(d.firstElementChild); d.showModal(); $('nick').focus(); // opened after Settings → the topmost modal, so it's the active one
}
async function addClaude() {
  $('loginMsg').textContent = 'Starting claude setup-token…';
  try { await call('claude_login', {nick: $('nick').value}); $('dlg2').close(); await askNewAccount(); } catch (e) { $('loginMsg').textContent = e; }
}
async function pasteClaude(custom) {
  try { await call('claude_add_token', custom ? {nick: $('nick').value, token: $('apiKey').value, baseUrl: $('baseUrl').value.trim()} : {nick: $('nick').value, token: $('token').value});$('dlg2').close(); await askNewAccount(); } catch (e) { $('loginMsg').textContent = e; }
}
async function askNewAccount() {
  const known = S.accounts.map(a => a.id);
  await refresh();
  const a = S.accounts.find(x => !known.includes(x.id));
  if (a) await pickItems(a.id, `What should ${a.nick}'s new projects get?`, []);
  acctDraw();
}
async function rmClaude(id) { if (await sure('Remove this account? Its projects fall back to Main.', 'Remove', 'Cancel', undefined, true)) { await call('claude_remove', {id}); openSettings(); } }
async function addGh() { $('ghMsg').textContent = 'Waiting for gh…'; const ok = await call('gh_login').catch(e => ($('ghMsg').textContent = e, false)); if (ok) acctDraw(); }
async function rmGh(u) { if (await sure(`Log @${u} out of gh?`, 'Log out', 'Cancel', undefined, true)) { await call('gh_logout', {user: u}); acctDraw(); } }

