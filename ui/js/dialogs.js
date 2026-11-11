// ---- projects ----
async function addProject() {
  const path = await T.dialog.open({directory: true});
  if (!path) return;
  const p = await call('add_project', {path}); await refresh(); await openProject(p.id); askProject(P(p.id));
}

// ---- dialogs ----
// #408 one modal shell for every .small-dlg: first child h2 (or a row holding one) = .dh header, last .btns = footer, rest → .db scroller
function dlgShell(w) { if (!w?.classList.contains('small-dlg')) return; const k = [...w.children];
  const h = k[0]?.matches('h2, :has(> h2)') ? k.shift() : null, f = k.at(-1)?.matches('.btns') ? k.pop() : null;
  h?.classList.add('dh'); const b = document.createElement('div'); b.className = 'db'; b.append(...k); w.insertBefore(b, f); }
function dlg(html, cls = 'small-dlg') { const d = $('dlg'); d.className = 'glass'; if (cls !== 'viewer') d.style.cssText = ''; /* #239 viewer's pinned spot */ d.innerHTML = `<div class="${cls}">${html}</div>`; dlgShell(d.firstElementChild); if (!d.open) d.showModal();
  if (!document.activeElement?.matches('input, textarea, select, [autofocus]')) document.activeElement?.blur(); } // #248 showModal focused the first button → ring on a random one
const closeDlg = () => $('dlg').close();
const sw = (on, change, dis = false) => `<label class="sw"><input type="checkbox" ${on ? 'checked' : ''}${dis ? ' disabled' : ''} onchange="${change}"><i></i></label>`;
const optRow = (title, hint, control) => `<div class="opt-row"><div class="grow"><div>${title}</div>${hint ? `<small class="dim">${hint}</small>` : ''}</div>${control}</div>`;
// collapsible section: open state remembered per key (only when clicked); never clicked → open when it has something.
// count (number or short text) shows as a pill = collapsed summary; buttons in `actions` don't toggle it; open = default when never clicked (#257: filled → false)
const folds = (() => { try { return JSON.parse(localStorage.folds || '{}'); } catch { return {}; } })();
const HOTF = new Set(['w-wait', 'w-sug', 'changes']); // counts that wait on the user or are live: highlighted
const fold = (key, title, body, count = 0, actions = '', open = !!count) => `<details class="sec fold" data-f="${key}" ${(folds[key] ?? open) ? 'open' : ''}>
  <summary class="sh" onclick="setTimeout(() => { folds['${key}'] = this.parentNode.open; try { localStorage.folds = JSON.stringify(folds); } catch {} })">${icon('chev')}<span class="label">${title}</span>
  ${count ? `<span class="pill${HOTF.has(key) ? '' : ' dim'}">${count}</span>` : ''}<span onclick="event.preventDefault()">${actions}</span></summary><div class="sc">${body}</div></details>`;
// #105 skeleton: skel(60, 40) = bars at those % widths; 'b60' = button-height bar
const skel = (...ws) => ws.map(w => `<i class="sk${String(w)[0] === 'b' ? ' b' : ''}" style="width:${String(w).replace('b', '')}%"></i>`).join('');
const sec = (title, body, actions = '') => `<section class="sec"><div class="sh"><span class="label">${title}</span>${actions}</div><div class="sc">${body}</div></section>`;
// #404 ⓘ = explanation on hover (tap → toast) instead of a paragraph in the UI; inside a fold title the click must not toggle it
// hover = the first sentences as a summary, click = the whole text in a rich tip at the button (help is not a notification)
const infoSum = t => { let s = ''; for (const x of t.match(/[^.!?]+[.!?]*s*/g) || [t]) { if (s.length >= 25) return s.trim() + ' Click for more.'; s += x; } return s.trim(); };
const infoI = tip => `<button class="ib" data-tip="${esc(infoSum(tip))}" data-info="${esc(tip)}" onclick="event.preventDefault(); event.stopPropagation(); infoTip(this)">${icon('info')}</button>`;
const field = (label, control) => `<label class="fl"><span>${label}</span>${control}</label>`;
// empty state + its next step (CLAUDE.md: never dead-end); act = button html
const empty = (t, act = '') => `<p class="empty">${t}</p>${act ? `<div class="btnrow" style="justify-content:center">${act}</div>` : ''}`;
const startProj = `<button class="btn sm" onclick="menuAt(this, addItems())">${icon('plus')}Add project</button>`;
const options = (map, val) => Object.entries(map).map(([k, l]) => `<option value="${esc(k)}" ${k === val ? 'selected' : ''}>${esc(l)}</option>`).join('');

// Project manager: every project's own GitHub, connections, skills, build and token savers
const setM = changes => ('license' in changes && delete RELC[mp], call('update_project', {pid: mp, changes}).then(() => openSettings('projects'))); // #412 licence change → dep fits stale
function openManager(pid) {
  if (pid && pid !== mp) { mp = pid; $('dlg').querySelector('.body')?.scrollTo(0, 0); }
  return openSettings('projects');
}
async function pickRoot() {
  const dir = await T.dialog.open({directory: true, title: 'Projects folder'});
  if (dir) { await call('set_projects_dir', {dir}); openSettings('projects'); }
}
/* #257 setup 1-2-3 at the top of the Projects manager; gone once every step is done (the folds below show summaries) */
function projSteps(p) {
  const st = [
    [!!p.gh, 'Pick its GitHub account', '', 'p-acct'],
    [!!p.build.build, 'Set the build and run commands', '', 'p-build'],
    [onOf(p).some(id => !builtin(id)), 'Pick its connections and skills', '', 'p-cx']];
  if (st.every(s => s[0])) return '';
  return sec('Set up', `<ol class="stepper">${st.map(([ok, t, body, f], i) => `<li class="${ok ? 'done' : ''}"><button class="sn" onclick="jumpTo('[data-f=${f}]')">${ok ? icon('check') : i + 1}</button>
    <b>${t}</b>${ok ? '' : body || `<div class="btnrow"><button class="btn" onclick="jumpTo('[data-f=${f}]')">Set up</button></div>`}</li>`).join('')}</ol>`);
}
// the project's accounts: one per kind, picked from the cards set up on the Accounts page; any card can serve many projects
function projAccts(p) {
  if (GACC === null) gAccts().then(() => page === 'projects' && $('dlg').open && openSettings());
  const add = `<button class="ib" title="Set up accounts" onclick="openSettings('gh')">${icon('plus')}</button>`;
  const kind = (t, cards) => `<div class="fl"><span>${t}</span><div class="idcards">${cards.join('') || add}</div></div>`;
  const tog = (k, v) => `onclick="setM({${k}: ${esc(JSON.stringify(p[k] === v && k !== 'account' ? '' : v))}})"`;
  return kind('AI', S.accounts.map(a => acard('claude', 'Claude', a.nick, tog('account', a.id), a.id === p.account)))
    + kind('GitHub', [...new Set(SET.gh_users)].map(u => acard('gh', 'GitHub', '@' + u, tog('gh', u), u === p.gh)))
    + (p.gh && mp === cur ? `<div class="btnrow"><button class="btn ghost sm" onclick="collabGo()">${icon('user')}Collaborators…</button></div>` : '' /* #113: lives in the GitHub tab (open project only) */)
    + (GACC === false ? '' : kind('Google', (GACC || []).map(a => acard('google', 'Google', a, tog('gaccount', a), a === p.gaccount))));
}
function projectsPage() {
  if (!P(mp)) mp = cur || S.projects[0]?.id;
  const root = `<div class="grid2" style="grid-template-columns:120px 1fr;margin-top:0"><span>Projects folder</span><span class="row">
      <input class="grow mono" value="${esc(S.projects_dir)}" onchange="call('set_projects_dir', {dir:this.value.trim()}).then(refresh)"><button class="btn" onclick="pickRoot()">Choose…</button>
      <button class="btn" title="Sort folders into owner/repo and local/" onclick="menuAt(this, [{label: 'Organize the projects folder', run: () => organize('')}, {label: 'Scan another folder…', run: async () => { const d = await T.dialog.open({directory: true, title: 'Folder to organize'}); if (d) organize(d); }}])">${icon('brush')}Organize…</button></span>
      <span></span><small class="dim" style="margin-top:-6px">Clones go to <span class="mono">&lt;owner&gt;/&lt;repo&gt;</span> in here, new projects to <span class="mono">local/</span>.</small></div>`;
  if (!mp) return root + empty('No projects yet', startProj);
  const p = P(mp);
  const items = kind => cxList(kind, 'project:' + p.id, p);
  setTimeout(placeFill); setTimeout(availFill);
  return root + `<div class="split"><div class="plist">${stHead('Active projects', `<button class="ib" title="Add project" onclick="closeDlg(); menuAt($('projects'), addItems())">${icon('plus')}</button>`)}${recent().map(x => `<button class="${x.id === mp ? 'on' : ''}" onclick="openManager('${x.id}')">${icon(x.shared ? 'gh' : 'folder')}<span>${esc(x.name)}</span></button>`).join('')}<div id="availBox"></div></div>
    <div><div class="row"><input value="${esc(p.name)}" onchange="this.value.trim() ? setM({name:this.value.trim()}) : (this.value = this.defaultValue)" title="Display name in Clauracle only; the folder and repo keep their names" placeholder="Project name" style="font-size:17px;font-weight:600" class="grow"></div>
    <small class="dim sel mono">${esc(p.path)}</small><div id="placeBox" class="row" style="gap:6px;margin:6px 0 0;min-height:30px"></div>
    ${projSteps(p)}
    ${fold('p-acct', 'Accounts' + infoI('Click a card to assign it to this project, click it again to unassign it. The project keeps these whichever AI account runs it.'), projAccts(p),
      [p.gh ? '@' + p.gh : 'no GitHub', S.accounts.find(a => a.id === p.account)?.nick, p.gaccount].filter(Boolean).join(' · '),
      `<button class="ib" title="Set up accounts" onclick="event.preventDefault(); openSettings('gh')">${icon('plus')}</button>`, !p.gh)}
    ${fold('p-sess', 'Session', `<div class="grid2" style="margin-top:0">
      <span>Model</span><select onchange="setM({model:this.value})">${options(MODELS, p.model)}</select>
      <span>Permissions</span><select onchange="setM({mode:this.value})">${options(PERMS, p.mode)}</select>
      <span>Clear & continue</span><select onchange="setM({cont:this.value})">${options(Object.fromEntries(Object.entries(MODES).map(([k, v]) => [k, v[0]])), p.cont)}</select>
      <span>Hand over at</span><span class="row"><input type="number" min="0" max="95" value="${p.context_limit}" style="width:76px" onchange="setM({context_limit:+this.value})"><small class="dim">% of the context window (0 = never)</small></span>
    </div>`, [p.model && mName(p.model), PERMS[p.mode]].filter(Boolean).join(' · '), '', false)}
    ${fold('p-save', 'Options', `<div class="box">
      ${optRow('Ponytail', 'The simplest code that works: no bloat, no speculative abstractions', sw(p.ponytail, 'setM({ponytail:this.checked})'))}
      ${(p.allow || []).length ? /* 3 rows: title / tags / description (#53) ∵ side-by-side squeezed the text */ `<div class="opt-row" style="display:block"><div>Always allowed</div><span class="row wrap" style="gap:4px;margin:6px 0">${p.allow.map(t => `<span class="pill" style="cursor:pointer" title="Ask again for ${esc(t)}" onclick="setM({allow: P(mp).allow.filter(x => x !== '${esc(t)}')})">${esc(t.replace(/@\d+$/, ' (24 h)'))} ${icon('x')}</span>`).join('')}</span><small class="dim">Tools this project runs without asking</small></div>` : ''}
      ${optRow('Credit Claude in commits', 'Off: commits carry no Co-Authored-By or "Generated with" lines', sw(p.ai_credit, 'setM({ai_credit:this.checked})'))}
      ${optRow('Keep .env away from Clauracle', 'Clauracle can\'t read or edit .env files', sw(p.hide_env, 'setM({hide_env:this.checked})'))}
    </div>`, [p.ponytail, p.ai_credit, p.hide_env].filter(Boolean).length + ' on', '', false)}
    ${fold('p-build', 'Build', `<div class="grid2" style="margin-top:0">${['build', 'run', 'deploy'].map(k => `<span>${k[0].toUpperCase() + k.slice(1)}</span><input class="mono" value="${esc(p.build[k])}" placeholder="${{build: 'npm run build', run: 'npm run dev', deploy: 'npm run deploy'}[k]}" onchange="setBuild('${k}', this.value)">`).join('')}
      <span>Local build</span><input class="mono" value="${esc(p.build.local || '')}" placeholder="Auto: fast debug build (e.g. tauri build --debug --no-bundle)" title="Used by builds after a task and /build on this machine. Release / deploy keep the full Build command." onchange="setBuild('local', this.value)">
      <span></span><label class="row">${sw(p.build.release, 'setBuild(\'release\', this.checked)')}<small>Release builds after tasks (slower: the full Build command)</small></label>
      <span>After each task</span><span>${thenBtn(p, 'm')}</span>
      <span></span><label class="row">${sw(p.build.fix, 'setBuild(\'fix\', this.checked)')}<small>A failed step goes back to Clauracle to fix</small></label></div>`,
      ['build', 'run', 'deploy'].filter(k => p.build[k]).length + '/3', '', !p.build.build)}
    ${(g => fold('p-lic', 'License' + infoI('What others may do with this project\'s code. The release checklist writes the LICENSE file from this.'), `<div class="row" style="gap:8px">
      <select onchange="setM({license:this.value})">${options({...(p.license ? {} : {'': 'Not picked'}), ...LICS}, p.license)}</select>
      ${!p.license && g ? `<button class="btn primary" onclick="setM({license:'${g}'})">${icon('check')}Confirm ${esc(LICS[g] || g)} (from its LICENSE file)</button>` : ''}<button class="btn" onclick="licHelp()">${icon('info')}Help me choose</button></div>
      <div id="licw">${licInfo(p.license)}</div>${sec('Libraries it ships', `<div id="licd">${licDeps(RELC[mp]?.c, mp)}</div>`)}`,
      p.license ? LICS[p.license] || p.license : '', '', !p.license))(RELC[mp]?.c?.lic_id)}
    ${fold('p-disk', 'Disk' + infoI('Storage Janitor deletes build and cache folders that git ignores after 14 days idle.'), `      <div id="diskBox"><button class="btn" onclick="drawDisk()">Check disk use</button></div>`)}
    ${cxGroup('p-cx', 'Connections &amp; Skills', 'project:' + p.id, p)}${cxFold('p-plugin', 'Plugins & mods', 'plugin', 'project:' + p.id, p)}
    </div></div>`;
}
const setBuild = (k, v) => setM({build: {...P(mp).build, [k]: v}});
async function drawDisk() {
  const box = $('diskBox'); box.innerHTML = skel(90, 'b100');
  const l = await call('disk', {pid: mp}).catch(e => (box.textContent = e, null)); if (!l) return;
  box.innerHTML = l.length ? l.map(c => `<div class="row"><span class="mono grow">${esc(c.dir)}</span><span class="dim">${mb(c.size)}</span>
      <button class="btn" onclick="cleanDisk(['${esc(c.dir)}'])">Clean</button></div>`).join('')
    + (l.length > 1 ? `<div class="btnrow"><button class="btn" onclick="cleanDisk([])">Clean all · ${mb(l.reduce((a, c) => a + c.size, 0))}</button></div>` : '')
    : '<p class="hint">Nothing to clean.</p>';
}
async function cleanDisk(dirs) {
  const what = dirs.length ? dirs.join(', ') : 'every build and cache folder listed here';
  if (!await sure(`Delete ${what}?\n\nGit-ignored build and cache folders; they come back on the next build.`, 'Delete', 'Cancel', undefined, true)) return;
  $('diskBox').innerHTML = skel(90, 'b100');
  const n = await call('disk_clean', {pid: mp, dirs}).catch(e => ($('diskBox').textContent = e, null)); if (n == null) return;
  await drawDisk(); $('diskBox').insertAdjacentHTML('afterbegin', `<p class="hint">Freed ${mb(n)}.</p>`);
}
/* #35/#36 organize: every project under the projects folder as <owner>/<repo> (GitHub origin) or local/<name>.
   PLACE pid → where it belongs (place_of runs git → cached per app run); SCAN = scan_folder(root), 60s */
const PLACE = {}; let SCAN = null;
const scanGet = force => (!force && SCAN && Date.now() - SCAN.t < 60e3) ? Promise.resolve(SCAN.l) : call('scan_folder', {dir: ''}).catch(() => []).then(l => (SCAN = {l, t: Date.now()}).l);
const samePath = (a, b) => { const n = s => s.replace(/\\/g, '/').replace(/\/$/, '').toLowerCase(); return n(a) === n(b); };
async function placeFill() {
  const pid = mp, p = P(pid), box = $('placeBox'); if (!box || !p) return;
  const t = PLACE[pid] ??= await call('place_of', {path: p.path}).catch(() => '');
  if ($('placeBox') !== box) return;
  box.innerHTML = !t || samePath(t, p.path) ? `<small class="ok grow">${icon('check')} In place in the projects folder</small><button class="btn ghost sm" onclick="moveTo()">Move to…</button>`
    : `<small class="dim grow">Belongs in <span class="mono">${esc(t)}</span></small><button class="btn sm" onclick="moveProj(PLACE[mp])">Move there</button><button class="btn ghost sm" onclick="moveTo()">Move to…</button>`;
}
async function moveTo() {
  const dir = await T.dialog.open({directory: true, title: 'Move the project into this folder'}); if (!dir) return;
  moveProj(dir.replace(/[\\/]$/, '') + (dir.includes('\\') ? '\\' : '/') + P(mp).path.split(/[\\/]/).filter(Boolean).pop());
}
async function moveProj(to) {
  const p = P(mp), pid = mp;
  if (!await sure(`Move ${p.name}?\n\n${p.path}\n→ ${to}\n\nIts session closes and reopens (history kept).`, 'Move')) return openSettings('projects');
  try { await call('move_folder', {from: p.path, to}); } catch (e) { await say(e); return openSettings('projects'); }
  delete PLACE[pid]; SCAN = null; await refresh();
  if (pid === cur) { cur = null; await openProject(pid); }
  openSettings('projects');
}
// Organize: scan (root or a picked folder) → tick list of what isn't in place → moves one by one (projects keep working)
async function organize(dir) {
  dlg(`<div style="padding:20px">${skel(50, 'b100', 'b100', 'b100')}</div>`);
  const l = (await call('scan_folder', {dir}).catch(() => [])).filter(x => !x.placed);
  if (!l.length) { await say('Everything there is already in place.'); return openSettings('projects'); }
  dlg(`<h2>Organize${infoI('GitHub repos go to <owner>/<repo>, the rest to local/. Untick what stays.')}</h2>
    <div class="box" style="max-height:50vh;overflow:auto">${l.map((x, i) => `<label class="check"><input type="checkbox" value="${i}" checked><span class="grow mono" style="font-size:12px">${esc(x.path)}${x.project ? ' <span class="pill">active</span>' : ''}<small class="dim" style="display:block">→ ${esc(x.target)}</small></span></label>`).join('')}</div>
    <p id="orgMsg" class="err" style="font-size:13px"></p><div class="btns"><button class="btn ghost" onclick="openSettings('projects')">Cancel</button><button class="btn primary" id="orgGo">Move</button></div>`);
  $('orgGo').onclick = async () => {
    const pick = [...document.querySelectorAll('#dlg input[type=checkbox]:checked')].map(c => l[+c.value]), errs = [];
    $('orgGo').disabled = true;
    for (const [i, x] of pick.entries()) { $('orgGo').textContent = `Moving ${i + 1}/${pick.length}…`;
      try { await call('move_folder', {from: x.path, to: x.target}); if (x.project) delete PLACE[x.project]; } catch (e) { errs.push(`${x.name}: ${e}`); } }
    SCAN = null; await refresh();
    if (pick.some(x => x.project === cur)) { const c = cur; cur = null; await openProject(c); }
    if (errs.length) { $('orgMsg').textContent = errs.join('\n'); $('orgGo').textContent = 'Done with errors'; } else openSettings('projects');
  };
}
// Available projects (#36): folders in the projects folder not added yet; faded, folded; click adds it
async function availFill() {
  const l = (await scanGet()).filter(x => !x.project), box = $('availBox'); if (!box) return;
  box.innerHTML = l.length ? fold('p-avail', 'Available projects', l.map(x => `<button style="opacity:.55" title="${esc(x.path)}" onclick="availAdd('${esc(x.path.replace(/\\/g, '\\\\'))}')">${icon('folder')}<span>${esc(x.name)}</span></button>`).join(''), 0,
    `<span class="pill dim">${l.length}</span>`) : '';
}
async function availAdd(path) { const p = await call('add_project', {path}); SCAN = null; await refresh(); mp = p.id; openSettings('projects'); askProject(P(p.id)); }
async function removeProject() {
  if (!await sure(`Remove ${P(mp).name} from Clauracle? The folder stays on disk.`, 'Remove', 'Cancel', undefined, true)) return;
  await call('remove_project', {pid: mp}); closeDlg();
  if (mp === cur) goHome();
  mp = null; refresh();
}

