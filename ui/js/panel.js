// ---- right panel: GitHub · Build · .env · Code · Browser ----
function toggleRight() { UI.rc = !UI.rc; layout('r'); drawRight(); }
/* #30 tab dots (tabs + collapsed rail, by data-t): gh = a turn is working (issue/task in progress); build = a build
   finished OK and Build & Release not opened since (BUILT); svc = a service step/key still open (SVCDOT, from env_list once) */
const BUILT = {}, SVCDOT = {};
function tabDots() {
  const p = cur && P(); if (!p) return;
  if (tab === 'build' && !$('right').hidden && !RC()) delete BUILT[cur];
  if (SVCDOT[cur] === undefined && svcOf(p).length) { SVCDOT[cur] = false; const pid = cur;
    call('env_list', {pid}).then(files => { if (pid !== cur) return delete SVCDOT[pid];
      envKeys = {files, have: new Set(files.flatMap(f => f.keys.filter(([, v]) => v).map(([k]) => k)))}; SVCDOT[pid] = svcOf(p).some(id => !svcDone(id)); tabDots(); }).catch(() => {}); }
  const dot = {gh: busy[cur] && 'var(--accent)', build: builds[cur]?.busy ? 'var(--accent)' : BUILT[cur] && 'var(--ok)', svc: SVCDOT[cur] && 'var(--err)'};
  document.querySelectorAll('#rtabs button[data-t]').forEach(b => { const c = dot[b.dataset.t];
    if (c) { b.dataset.dot = b.dataset.t === 'build' && builds[cur]?.busy ? 'pulse' : ''; b.style.setProperty('--dotc', c); } else delete b.dataset.dot; });
}
function rightTab(t) { tab = UI.tab = t; try { localStorage.ui = JSON.stringify(UI); } catch {} drawRight(); } // restored at start (UI.tab)
/* #114 tab right-click: Refresh = drop that tab's caches + redraw (opens the tab), Settings… = related page if any */
const TABREF = {gh: () => { ghiStale(cur); delete COLLAB[cur]; delete IMETA[cur]; }, build: () => { delete RELC[cur]; delete SECRETS[cur]; ghiStale(cur); },
  svc: () => { delete SECRETS[cur]; delete BUNDLE[cur]; }, web: () => { BROWSERS = null; HOMEPG = undefined; }, code: () => {}, agents: () => { AGENTS = null; }};
const LICS = {MIT: 'MIT', 'Apache-2.0': 'Apache 2.0', 'GPL-3.0': 'GPL 3.0', 'LGPL-3.0': 'LGPL 3.0', 'AGPL-3.0': 'AGPL 3.0', 'MPL-2.0': 'Mozilla 2.0', 'BSD-3-Clause': 'BSD 3-Clause', ISC: 'ISC', Unlicense: 'Unlicense (public domain)', Proprietary: 'All rights reserved'};
// #348 license info under the picker: [openness level, pros/cons + use case]
const LICINFO = {MIT: ['Permissive: very open', 'Anyone may use, change and sell it, even in closed products; they only keep your copyright notice. Simplest and most common. Con: others can close-source their copies. Good for most libraries, tools and apps.'],
  'Apache-2.0': ['Permissive: very open', 'Like MIT, plus an explicit patent grant and a notice of changes. Con: longer text. Good for company-backed or patent-sensitive projects.'],
  'BSD-3-Clause': ['Permissive: very open', 'Like MIT, plus others may not use your name to promote their products. Common in academia and older projects.'],
  ISC: ['Permissive: very open', 'Functionally MIT in even shorter words. Common in npm packages.'],
  'MPL-2.0': ['Weak copyleft: open per file', 'Changes to your files must stay open; they may be combined with closed code in a bigger product. Middle ground between MIT and GPL.'],
  'LGPL-3.0': ['Weak copyleft: open library', 'Apps may use your library and stay closed; changes to the library itself must stay open. Good for libraries you want kept open but widely used.'],
  'GPL-3.0': ['Strong copyleft: stays open', 'Anyone who distributes it or software built on it must share the full source under GPL. Con: many companies avoid it. Good when you want every derivative to stay open.'],
  'AGPL-3.0': ['Strongest copyleft: open even as a service', 'Like GPL, and also when people use it over a network (SaaS) they must get the source. Con: most companies avoid it. Good for server software you want kept open.'],
  Unlicense: ['Public domain: no conditions', 'You give up all rights: anyone may do anything, not even credit needed. Con: no warranty disclaimer in some countries, and it is final.'],
  Proprietary: ['Closed: all rights reserved', 'Nobody may copy, change or share it without your permission (even if the repo is public). Pick for commercial or private code.']};
// #412 dependency licence check: rows built from release_check `deps` (cargo tree / package-lock data, Rust lic_fit), never AI
const LICS_T = {bad: 'Not allowed under this project’s license', unk: 'License unknown: check by hand', note: 'Weak copyleft: ship it as a separate, unchanged library'};
function licDeps(c, pid) {
  if (!c?.deps) return `<button class="btn" onclick="licScan('${pid}', this)">${icon('loop')}Check library licenses</button>`;
  const d = c.deps, by = {}, row = x => `<span style="display:block">${esc(x.n)} ${esc(x.v)} <span class="dim">${esc(x.l || 'no license stated')} · ${x.e}${x.d ? '' : ', indirect'}</span></span>`;
  d.forEach(x => by[x.l || '?'] = (by[x.l || '?'] || 0) + 1);
  const grp = ['bad', 'unk', 'note'].map(s => [s, d.filter(x => x.s === s)]).filter(g => g[1].length);
  return `<p class="dim" style="margin:4px 0;font-size:13px">${d.length ? `${d.length} libraries (Rust + npm), ${d.length - grp.reduce((n, g) => n + (g[0] === 'note' ? 0 : g[1].length), 0)} fine` : 'No Rust or npm libraries found (other ecosystems aren’t scanned yet)'}
    <button class="ib" title="Check again" onclick="licScan('${pid}', this)">${icon('loop')}</button></p>
    ${grp.map(([s, l]) => `<div style="margin:6px 0;font-size:13px"><b class="${s === 'bad' ? 'err' : ''}">${LICS_T[s]} (${l.length})</b>${l.slice(0, 30).map(row).join('')}${l.length > 30 ? `<span class="dim">+${l.length - 30} more</span>` : ''}</div>`).join('')}
    ${c.credits_miss?.length ? `<div style="margin:6px 0;font-size:13px"><b>Not credited in CREDITS.md (${c.credits_miss.length})</b><span style="display:block" class="dim">${esc(c.credits_miss.join(', '))}</span>
      <button class="btn sm" style="margin-top:4px" onclick="licCredits('${pid}', this)">${icon('plus')}Add them to CREDITS.md</button></div>` : d.length ? '<p class="dim" style="margin:4px 0;font-size:13px">Every direct library is credited in CREDITS.md.</p>' : ''}
    <div class="row" style="gap:4px;flex-wrap:wrap;margin-top:6px">${Object.entries(by).sort((a, b) => b[1] - a[1]).map(([l, n]) => `<span class="pill dim">${esc(l)} ${n}</span>`).join('')}</div>`;
}
const licOkDeps = c => !c.deps || !c.deps.some(x => x.s === 'bad' || x.s === 'unk') && !c.credits_miss?.length;
async function licScan(pid, b) { if (b) b.disabled = true;
  const c = await call('release_check', {pid}).catch(e => ({err: String(e)}));
  if (c.err) return toast(c.err, 'err');
  c.staged = RELC[pid]?.c?.staged || []; RELC[pid] = {c, t: Date.now()};
  if ($('licd') && pid === mp) $('licd').innerHTML = licDeps(c, pid); if (pid === cur && tab === 'build') drawRight(); }
async function licCredits(pid, b) { b.disabled = true;
  const n = await call('lic_credits', {pid}).catch(e => (toast(String(e), 'err'), -1));
  if (n > 0) toast(`Added ${n} ${n === 1 ? 'library' : 'libraries'} to CREDITS.md`, 'ok'); licScan(pid); }
const licInfo = id => LICINFO[id] ? `<p class="dim" style="margin:8px 0 0;font-size:13px"><b>${LICINFO[id][0]}.</b> ${LICINFO[id][1]}</p>` : '';
// #348 Help me choose: yes/no tree, node = [question, yes → node|license id, no → …]; pure code, no AI
const LICQ = {open: ['Should others be allowed to use, copy or change this code?', 'pd', 'Proprietary'],
  pd: ['Give it away with no conditions at all (not even credit)?', 'Unlicense', 'share'],
  share: ['Must people who change and share it keep their changes open too?', 'lib', 'pat'],
  pat: ['Do patents matter (company project, or you want an explicit patent grant)?', 'Apache-2.0', 'MIT'],
  lib: ['Is it mainly a library other programs build on?', 'file', 'net'],
  file: ['Should only changes to your own files have to stay open (closed code may sit next to them)?', 'MPL-2.0', 'LGPL-3.0'],
  net: ['Will it mostly run as a server or web service people use over the network?', 'AGPL-3.0', 'GPL-3.0']};
function licHelp(k = 'open') {
  const box = $('licw'), q = LICQ[k], b = (l, n) => `<button class="btn" onclick="licHelp('${n}')">${l}</button>`;
  box.innerHTML = q ? `<div class="sec" style="margin:8px 0 0;padding:10px"><p style="margin:0 0 8px">${q[0]}</p><div class="row" style="gap:8px">${b('Yes', q[1])}${b('No', q[2])}${k !== 'open' ? b('Start over', 'open') : ''}</div></div>`
    : `<div class="sec" style="margin:8px 0 0;padding:10px"><p style="margin:0 0 4px">Suggested: <b>${esc(LICS[k])}</b></p>${licInfo(k)}<div class="row" style="gap:8px;margin-top:8px"><button class="btn primary" onclick="setM({license:'${k}'})">${icon('check')}Use ${esc(LICS[k])}</button>${b('Start over', 'open')}</div></div>`;
}
const licPick = () => openManager(cur).then(() => jumpTo('[data-f=p-lic]')); // #264
const buildSet = () => openManager(cur).then(() => jumpTo('[data-f=p-build]'));
const TABSET = {gh: () => openSettings('gh'), agents: () => openSettings('agents'), build: buildSet}; // svc: no settings of its own
const tabItems = t => [{icon: 'refresh', label: 'Refresh', run: () => { TABREF[t](); tab === t ? drawRight(true) : rightTab(t); }}, ...(TABSET[t] ? [{icon: 'gear', label: 'Settings…', run: TABSET[t]}] : [])]; // #404 tab btn + empty tab body
$('rtabs').oncontextmenu = e => { const b = e.target.closest('button[data-t]'); if (!b || !cur) return; e.preventDefault(); menuAt(b, tabItems(b.dataset.t)); };
// Build auto-setup: commands guessed from project files (Rust build_guess, no tokens) → confirm; cached per project
const GUESS = {};
const MAC = {}; // pid → ships for macOS (ships_mac), once per app run
async function guessHtml() {
  const g = GUESS[cur] ??= await call('build_guess', {pid: cur}).catch(() => ({}));
  const ks = ['build', 'run', 'deploy'].filter(k => g[k]), set = `<button class="btn${ks.length ? '' : ' primary'} block" onclick="openManager(cur)">${icon('hammer')}Set build and run commands</button>`;
  return ks.length ? `<p class="dim" style="margin:0 0 6px">Found in this project:</p><div class="grid2" style="margin-top:0">${ks.map(k => `<span>${k[0].toUpperCase() + k.slice(1)}</span><span class="mono">${esc(g[k])}</span>`).join('')}</div>
    <div class="btnrow"><button class="btn primary" onclick="useGuess()">${icon('hammer')}Use these</button><button class="btn" onclick="openManager(cur)">Edit</button></div>` : set;
}
const useGuess = () => call('update_project', {pid: cur, changes: {build: {...P().build, ...Object.fromEntries(Object.entries(GUESS[cur]).filter(([k, v]) => v && k !== 'after' && k !== 'fix'))}}}).then(refresh).then(drawRight);
/* Release wizard: stage per project (p.stage) alpha → beta → rc → published. Checklist = every item at or below the
   target stage (stricter going up). Facts from release_check (plain code, cached 60s/project); Claude only writes/judges
   via a Fix prompt. Same stage = light update (tag + publish in GitHub tab); Move up = target's full list first. */
const STAGES = {alpha: 'Alpha', beta: 'Beta', rc: 'Release candidate', published: 'Published'}, SK = Object.keys(STAGES);
const RELC = {}, RELTO = {}, RELBUSY = {}, RELFIX = {}; // RELFIX pid → Set of row ids whose background Fix/Run (#360 rel_fix) is out // pid → {c, t} check cache · pid → target stage while moving up
const RELAI = { // fix prompts: Claude writes/judges, never template
  // #357 one row = README (≥10 lines, touched since the last tag) + LICENSE (picked, present, id matches) + CREDITS.md
  get readme() { const c = RELC[cur]?.c || {}; return [ // #413 LICENSE.md + CREDITS = code (relDocs), README prose = AI
    c.readme < 10 ? 'Write README.md by hand-feel: plain voice of this project, what it does and for whom, how to install and run it. No template headings, badge walls or emoji; short beats complete. Read the code first.'
      : c.readme_old ? `README.md wasn't touched since ${c.tag}: bring it up to date with what changed since (git log ${c.tag}..HEAD), keep its voice, change only what's out of date.` : '',
  ].filter(Boolean).join(' '); },
  version: 'Give the project a version (0.1.0) in its main config (package.json / Cargo.toml / tauri.conf.json), consistent across files.',
  changelog: 'Write CHANGELOG.md from git history since the last tag: user-facing changes only, newest first, grouped by version.',
  bump: 'Bump the version for the next release (patch for fixes, minor for features, judge from git log since the last tag) in every config file that carries it.',
  ci: 'Add or fix a GitHub Actions workflow that builds and runs the tests on push, then push so it runs.',
  get ci_targets() { const c = RELC[cur]?.c || {}, m = tgtSplit(P(), c, []).miss.map(tgtName).join(', ');
    return `Extend the GitHub Actions release workflow so a v* tag push also builds ${m} and attaches the files to that tag's GitHub release (existing jobs untouched). Push when done.`; },
  icon: 'Make an app icon for this project (SVG source + the sizes its platform needs) and wire it into the config.',
  review: 'Release review: read the project and list release blockers for a public release (crashes, missing error handling, secrets in repo, debug leftovers, broken first-run). Plan only, don\'t change files.',
  get site_legal() { return sitePrompt(); },
  store: 'Store/compliance check for publishing: privacy policy, data collection disclosure, licences of bundled deps, store listing text/screenshots, age rating. List what\'s missing, plan only.',
};
const licOk = (c, l) => !!l && c.license && (!c.lic_id || c.lic_id === l) && (!c.lic_file || c.lic_file === 'LICENSE.md'); // #412 file must be named LICENSE.md
function relItems(c, p) {
  const v = (c.version || '').replace(/^v/, ''), tv = (c.tag || '').replace(/^v/, ''), ci = c.ci;
  // #129: CI off → local build signs from .env (c.sign = APPLE_*/AZURE_* keys there, passed to build steps)
  const sign = S.ci_builds ? SECRETS[cur]?.includes('APPLE_CERTIFICATE') : (c.sign || []).includes('APPLE_SIGNING_IDENTITY'), files = relFilesFor(cur, v), {ci: cit, miss, lack} = tgtSplit(p, c, files);
  const stale = files.filter(f => f.mtime && f.mtime < (c.code_t || 0)); // #92: built before the newest code commit → rebuild before release
  // array order = display order (filter keeps it) → Build stays last whatever the stage
  return [
    {s: 0, id: 'readme', t: 'Markdown files written', ok: c.readme >= 10 && !c.readme_old && c.credits && licOk(c, p.license),
      h: [c.readme < 10 ? (c.readme ? `README.md: only ${c.readme} lines` : 'README.md: missing') : c.readme_old ? `README.md: not updated since ${c.tag}` : 'README.md',
        // #264 licence picked in Settings › Projects (p-lic); not picked → Choose / Confirm (file exists) → licPick; picked but file missing/other → Fix prompt
        (c.lic_file || 'LICENSE.md') + (p.license ? `: ${LICS[p.license] || p.license}` + (!c.license ? ', no file yet' : c.lic_id && c.lic_id !== p.license ? `, file says ${c.lic_id}` : c.lic_file !== 'LICENSE.md' ? ', rename to LICENSE.md' : '') : c.license ? `: not picked, file looks like ${c.lic_id || 'an unknown license'}` : ': not picked'),
        c.credits ? 'CREDITS.md' : 'CREDITS.md: missing'].map(t => `<span style="display:block">${esc(t)}</span>`).join(''),
      ...(p.license ? {go: 'relDocs(this)'} : {go: 'licPick()', gl: c.license ? 'Confirm license' : 'Choose license'})},
    ...(c.deps ? [{s: 0, id: 'libs', t: 'Library licenses compatible', ok: licOkDeps(c), d: (b => b.length ? `${b.length} not allowed or unknown: ${b.slice(0, 5).map(x => x.n + ' (' + (x.l || '?') + ')').join(', ')}${b.length > 5 ? '…' : ''}` : '')(c.deps.filter(x => x.s === 'bad' || x.s === 'unk'))
      + (c.credits_miss?.length ? ` ${c.credits_miss.length} not credited in CREDITS.md` : '') || `${c.deps.length} libraries checked`, go: c.credits_miss?.length ? `licCredits(cur, this)` : `licPick()`, gl: c.credits_miss?.length ? 'Add credits' : 'Details'}] : []),
    {s: 0, id: 'version', t: 'Version set', ok: !!v, d: v ? `${v} · ${c.vfile}` + (tv ? ` · last released ${c.tag}` : ' · first release') : 'not found'},
    // #121: semver ≤ last tag → auto-bumped in relWizard; row stays only when that can't happen (non-semver) → AI fix
    ...(tv && !(v && vcmp(v, tv) > 0) ? [{s: 0, id: 'bump', t: 'Version newer than the last release', ok: false, d: `last released ${c.tag}`}] : []),
    {s: 0, id: 'clean', t: 'All changes saved to Git', ok: c.clean, go: `relCommit(this)`, gl: 'Commit'},
    {s: 1, id: 'changelog', t: 'Changelog', ok: c.changelog},
    ...(S.ci_builds ? [{s: 1, id: 'ci', t: 'Online build passed', ok: ci?.conclusion === 'success', d: !c.remote ? 'not on GitHub' : ci ? `${ci.workflowName}: ${ci.conclusion || ci.status}${ci.why ? ' · ' + ci.why : ''}` : 'no online builds yet',
      go: !c.remote ? `rightTab('gh')` : '', url: ci?.bill || ci?.url}] : []),
    // #237 required file: icon source SVG or PNG ≥1024px (stores: 512 Play / 1024 App Store; `tauri icon` wants 1024)
    {s: 1, id: 'icon', t: 'App icon (SVG or PNG ≥ 1024 px)', ok: !!c.icon && (c.icon_px == null || c.icon_px >= 1024), d: !c.icon ? 'none found' : c.icon_px ? `${c.icon} · ${c.icon_px} px` : c.icon},
    ...(macOn(p, cur) ? [{s: 2, id: 'sign', t: S.ci_builds ? 'Mac signing keys on GitHub' : 'Mac signing keys saved', d: 'so Macs trust and open the app', ok: sign, go: `svcAdd('macsign').then(() => rightTab('svc'))`}] : []),
    // #40 stores (Play/App Store) want privacy + terms URLs: RC up, mobile targets only; c.site = HTTP statuses (relWizard)
    ...((p.targets || []).some(x => x === 'ios' || x === 'android') ? [{s: 2, id: 'site_legal', t: 'Privacy policy and terms online',
      ok: !!c.site && c.site.every(x => x >= 200 && x < 400), d: siteOf(p).domain ? ['privacy', 'terms'].map((k, i) => `${SITEPG[k][1]} ${c.site?.[i] || 'unreachable'}`).join(' · ') : 'no website yet',
      ...(siteReady() ? {} : {go: `svcAdd('website').then(() => rightTab('svc'))`, gl: 'Set up'})}] : []),
    {s: 2, id: 'review', t: 'Release review by AI', ai: true},
    {s: 3, id: 'store', t: 'Store and compliance check', ai: true},
    ...((un => un.length ? [{s: 0, id: 'targets', t: 'Every picked platform can be built', ok: false, d: un.map(x => `${tgtName(x)}: ${tgtWhy(p, x)[0]}`).join(' · '), go: tgtWhy(p, un[0])[1]}] : [])((p.targets || []).filter(x => !(S.ci_builds && miss.includes(x)) && !tgtReady(p, x)))),
    ...(miss.length && S.ci_builds ? [{s: 0, id: 'ci_targets', t: 'GitHub builds the other platforms', ok: false, d: `GitHub can't build: ${miss.map(tgtName).join(', ')}`}] : []),
    {s: 0, id: 'build', t: 'Build new version', ok: files.length > 0 && !lack.length && !stale.length, ...relBuildD(c, files, stale, lack, cit, v),
      go: p.build.build ? 'relBuild(this)' : 'openManager(cur)', gl: p.build.build ? 'Build' : 'Set up', dh: builds[cur]?.busy ? bfiles(p, files) : ''},
  ];
}
/* #27 per-target builds: this OS builds its own desktop target + Android (macOS also iOS); the rest come from CI (tag push).
   ponytail: file type = platform, no arch check */
const HOST = /Windows/.test(navigator.userAgent) ? 'windows' : /Mac/.test(navigator.userAgent) ? 'macos' : 'linux';
const TGTEXT = {windows: /\.(exe|msi|msix)$/i, macos: /\.(dmg|pkg)$/i, linux: /\.(appimage|deb|rpm)$/i, ios: /\.ipa$/i, android: /\.(apk|aab)$/i};
const tgtName = id => TGT.flatMap(g => g[2]).find(x => x[0] === id)?.[2] || id;
const tgtSplit = (p, c, files) => { const t = p.targets || [], here = tgtLocal; // #123: all picked ∵ unready must block Release, not drop out silently
  const ci = t.filter(x => !here(x));
  return {ci, miss: ci.filter(x => !(c.ci_os || []).includes(x)), lack: t.filter(here).filter(x => !files.some(f => TGTEXT[x].test(f.name)))}; };
// release files for version v: newest build per file type named with v (versionless could be stale); builds via gh_info (GHI)
/* #174 while building: expected artifacts (last build's names, else target name) shimmer → check once Tauri prints them under "Finished N bundle(s) at:" (b.from = out offset at build start) */
const bdone = b => (b?.out || '').slice(b.from || 0).split(/Finished \d+ bundles? at:/).slice(1).join('\n').match(/[^\s\\/]+\.(exe|msi|msix|dmg|pkg|appimage|deb|rpm|ipa|apk|aab)\b/gi) || [];
/* #349 expected names from the start: this version's files, else known names (local builds + last release assets) with the
   version swapped in; one per extension. ✓ = a finished bundle with that extension (Network Mac output names may differ) */
const bext = n => n.toLowerCase().split('.').pop();
const bfiles = (p, files, pid = cur) => { if (builds[pid]?.local) return `<span class="bfile${builds[pid].busy ? ' shim' : ''}">Local build${builds[pid].busy || !builds[pid].ok ? '' : icon('check')}</span>`; /* #361 smart/after-task this-machine build: no release bundles */ const done = bdone(builds[pid]), loc = (p.targets || []).filter(tgtLocal), v = (RELC[pid]?.c?.version || '').replace(/^v/, '');
  const known = [...files.map(f => f.name), ...(GHI[pid]?.g?.builds || []).map(f => f.name), ...(GHI[pid]?.g?.release?.assets || []).map(a => a.name)];
  const exp = loc.flatMap(x => { const seen = new Set(), f = known.filter(n => TGTEXT[x].test(n)).map(n => v && fver(n) ? n.replace(fver(n), v) : n).filter(n => !seen.has(bext(n)) && seen.add(bext(n)));
    return f.length ? f.map(n => [n, done.some(d => bext(d) === bext(n))]) : [[tgtName(x), done.some(n => TGTEXT[x].test(n))]]; });
  return exp.map(([n, ok]) => `<span class="bfile${ok ? '' : ' shim'}">${esc(n)}${ok ? icon('check') : ''}</span>`).join(''); };
const relFilesFor = (pid, v) => v ? relPick(GHI[pid]?.g?.builds || [], v).filter(b => fver(b.name) === v) : [];
/* #237 build row: "Update build(s) from <last release>:" + one file (or missing target) per line, Win/Linux/mac ─ Android/iOS */
const BORD = ['windows', 'linux', 'macos', 'android', 'ios'], bplat = n => BORD.find(x => TGTEXT[x].test(n)) || n;
function relBuildD(c, files, stale, lack, cit, v) {
  const todo = stale.length || (lack.length && files.length), rows = todo ? [...stale.map(f => [bplat(f.name), f.name]), ...lack.map(x => [x, tgtName(x) + ' (not built yet)'])] : files.map(f => [bplat(f.name), f.name]);
  if (!rows.length) return {d: v ? `version ${v} isn't built yet` : ''};
  const mob = x => x === 'android' || x === 'ios', rel = GHI[cur]?.g?.release, tag = rel?.tagName || c.tag;
  rows.sort((a, b) => BORD.indexOf(a[0]) - BORD.indexOf(b[0]));
  const head = todo ? `Update build${rows.length > 1 ? 's' : ''}${tag ? ` from ${rel?.url ? `<a data-url="${esc(rel.url)}" style="cursor:pointer;color:var(--accent)">${esc(tag)}</a>` : esc(tag)}` : ''}:` : '';
  return {h: (head ? `<span style="display:block">${head}</span>` : '') + rows.map(([pl, n], i) => (i && mob(pl) && !mob(rows[i - 1][0]) ? '<hr class="bsep">' : '') + `<span style="display:block">${esc(n)}</span>`).join('')
    + (cit.length && !todo ? `<span style="display:block">built on GitHub: ${esc(cit.map(tgtName).join(', '))}</span>` : '')};
}
const stHead = (t, a = '', tip = '') => `<div class="sh"${tip ? ` title="${esc(tip)}"` : ''}><span class="label">${t}</span>${a}</div>`; // #232 right-panel step/list title row = same markup as sec()/fold() headers (title left, actions right)
// one header for every side-panel tab: name left, "+" where the tab can add something, "…" (the tab menu) always at the far right
function tabHead(el, name, more, add) {
  const h = `<span class="label grow">${esc(name)}</span>${add ? `<button class="ib" title="${add[0]}" onclick="${add[1]}">${icon('plus')}</button>` : ''}<button class="ib" title="More" onclick="${more}">${icon('more')}</button>`;
  if (el._h !== h) el.innerHTML = el._h = h; // redrawn often: no DOM churn
}
const TABADD = {gh: ['New issue', "$('newTask')?.scrollIntoView({block: 'center'}); $('newTask')?.focus()"], svc: ['Add a service', 'svcMenu(this)'],
  agents: ['Add custom agents', "openSettings('cx').then(() => jumpTo('[data-f=g-own]'))"]};
async function relWizard(p, force, cached) { // cached → draw RELC as is (caller ensures it exists)
  const pid = cur, k = p.stage ? SK.indexOf(p.stage) : -1, to = RELTO[pid] ?? Math.max(k, 0);
  if (!cached && (force || !RELC[pid] || Date.now() - RELC[pid].t > 60e3)) {
    const c = await call('release_check', {pid}).catch(e => ({err: String(e)}));
    c.staged = await call('stage', {pid, action: 'list'}).catch(() => []); // #266
    if (siteOf(p).domain && (p.targets || []).some(x => x === 'ios' || x === 'android')) c.site = await call('site_check', {urls: ['privacy', 'terms'].map(k => siteUrl(k, p))}).catch(() => null);
    const was = RELC[pid]?.c?.ci, now = c.ci; RELC[pid] = {c, t: Date.now()};
    if (was && now?.status === 'completed' && (was.status !== 'completed' || was.databaseId !== now.databaseId)) { // #123 CI run ended (or a new one already did)
      toast(`CI ${now.workflowName}: ${now.conclusion}${now.why ? ' · ' + now.why : ''}`, now.conclusion === 'success' ? 'ok' : 'err', 0, true, () => openProject(pid).then(() => rightTab('build'))); ghiStale(pid); }
    if (now && now.status !== 'completed') ciPoll(pid, 30e3);
    if (macOn(p, pid) && c.remote && !SECRETS[pid]) SECRETS[pid] = await call('gh_secrets', {pid}).catch(() => null);
  }
  const c = RELC[pid].c, re = `<button class="ib" title="Refresh" onclick="relRefresh()">${icon('refresh')}</button>`;
  const ask = `<button class="btn${k < 0 ? ' primary' : ''}" ${RELFIX[pid]?.has('summary') ? 'disabled><i class="spin"></i>' : `onclick="relAsk('summary')">${icon('sparkle')}`}What's missing for ${relNext(p)}?</button>`;
  if (c.err) return stHead('Release', re) + (`<p class="err">${esc(c.err)}</p>`);
  if (k < 0 && RELTO[pid] == null) return stHead('Release', re) + (`<p class="hint">Walk this project to a release, one checklist per stage.</p>
    <div class="btnrow col">${ask}<button class="btn" onclick="RELTO[cur]=0; drawRight()">Start at Alpha</button></div>`);
  const tv = (c.tag || '').replace(/^v/, ''), cv = (c.version || '').replace(/^v/, '');
  const top = (c.staged || []).reduce((a, x) => !a || vcmp(x.v, a) > 0 ? x.v : a, tv); // #266 staged versions count as taken
  if (top && SEMV.test(cv) && vcmp(cv, top) <= 0 && !c.bumping) { // #121 auto-bump past the latest tag / staged build (patch); once per check
    c.bumping = 1; const nv = vnext(cv, top);
    try { const f = await call('version_set', {pid, v: nv}); addEntry(pid, {kind: 'note', text: `Version ${nv} set in ${f.join(', ')} (last ${top === tv ? 'tag ' + c.tag : 'staged v' + top})`}); c.version = nv; c.clean = false; }
    catch (e) { addEntry(pid, {kind: 'note', text: `Auto-bump failed: ${e}`}); }
  }
  const items = relItems(c, p).filter(x => x.s <= to), left = items.filter(x => !x.ai && !x.ok).length;
  const row = x => `<div class="row" style="gap:8px;align-items:flex-start;padding:3px 0"><span style="width:16px;display:inline-flex;padding-top:2px;color:var(${x.ai ? '--dim' : x.ok ? '--ok' : '--err'})">${icon(x.ai ? 'sparkle' : x.ok ? 'check' : 'x')}</span>
    <span class="grow"><span${x.ok ? ' style="color:var(--text)"' : ''}>${esc(x.t)}</span>${x.dh ? '' : x.h ? `<small class="dim" style="display:block">${x.h}</small>` : x.d ? `<small class="dim" style="display:block">${x.url ? `<a data-url="${esc(x.url)}" style="cursor:pointer">${esc(x.d)}</a>` : esc(x.d)}</small>` : ''}</span>
    ${x.id === 'build' && builds[cur]?.busy ? buildingBtn(1) : x.ok ? '' : x.go ? `<button class="btn sm" onclick="${x.go}">${x.gl || 'Fix'}</button>` : RELAI[x.id] ? `<button class="btn sm" ${RELFIX[cur]?.has(x.id) ? 'disabled><i class="spin"></i>' : `onclick="relAsk('${x.id}')">`}${x.ai ? 'Run' : 'Fix'}</button>` : ''}</div>${x.dh ? `<small class="dim" style="display:block;padding-left:24px">${x.dh}</small>` : ''}`; /* #182 dh below the row: full width, not squeezed by the Building… button */
  const seg = `<span class="seg">${SK.map((s, i) => `<button class="${i === to ? 'on' : ''}" ${i < Math.max(k, 0) ? 'disabled' : ''} title="${i > k ? 'Move up: shows this stage\'s full list' : 'Current stage'}" onclick="RELTO[cur]=${i}; drawRight()">${STAGES[s].replace('Release candidate', 'RC')}</button>`).join('')}</span>`;
  const gi = GHI[pid]?.g, v = (c.version || '').replace(/^v/, '');
  // #265 build in flight → say so (was an enabled-looking button under "Ready to build!")
  // #266 staged builds (lowest first) = what Release ships; #354 checklist = working copy (newer version) ∴ never gates a staged one (own commit + copies)
  const stg = (c.staged || []).filter(x => !tv || vcmp(x.v, tv) > 0).sort((a, b) => vcmp(a.v, b.v));
  const why = !gi?.remote ? 'Not on GitHub yet (GitHub tab › publish)' : stg.length ? '' : left ? `${left} check${left > 1 ? 's' : ''} open` : bBusy(pid) ? 'Building… Release unlocks when it finishes' : '';
  const sv = (stg.find(x => x.v === RELSV[pid]) || stg[0])?.v, rv = sv || v; // #351 picked staged version (▾ menu), default lowest
  // Release = publish v<version> (that version's build files) on GitHub + set the stage; grayed out till every check is green
  const rel = RELBUSY[pid] ? `<button class="btn sm primary block" disabled><i class="spin"></i>Publishing…</button>` : `<button class="btn sm primary block" ${why ? `disabled title="${esc(why)}"` : ''} onclick="relRelease(${to}, this${sv ? `, '${esc(sv)}'` : ''})">${icon('rocket')}Release v${esc(rv)} as ${STAGES[SK[to]]}</button>`; /* #122 RELBUSY survives redraws */
  const act = stg.length ? `<div class="relsplit">${rel}<button class="btn sm primary relarr${stg.length > 1 ? ' pulse' : ''}" title="${stg.length} staged release${stg.length > 1 ? 's' : ''}: pick one" onclick="relMenu(this)">${icon('down')}</button></div>` : rel;
  return stHead('Release', (left ? `<span class="pill dim">${items.length - left}/${items.filter(x => !x.ai).length}</span>` : '<span class="pill">ready</span>') + re) + `<div class="row" style="gap:8px;align-items:center;margin-bottom:8px"><span class="dim">Stage:</span>${seg}</div>
    <div>${items.map(row).join('')}</div>
    <div class="nextstep"><p>${why ? esc(why) : 'Ready to release!'}</p>${act}</div><div class="btnrow col" style="margin-top:8px">${ask}</div>`; /* #237 header = Stage + switcher only; ask under the Release box */ // #232 Release button in the next-step box
}
/* #123 recheck CI while a run is in flight (one timer per project; only while Build & Release shows it) */
const CIPOLL = {};
const ciPoll = (pid, ms) => { if (CIPOLL[pid]) return; CIPOLL[pid] = setTimeout(() => { delete CIPOLL[pid]; if (pid === cur && tab === 'build') { if (RELC[pid]) RELC[pid].t = 0; drawRight(); } }, ms); };
// #211 next stage after the current one: 'Beta' at Alpha…; none yet → first release; published → next release
const relNext = p => { const k = p.stage ? SK.indexOf(p.stage) : -1; return k < 0 ? 'a first release' : k + 1 < SK.length ? STAGES[SK[k + 1]] : 'the next release'; };
const relRefresh = () => { TABREF.build(); drawRight(); };
// #360 one-click AI fixes = headless claude -p (lib rel_fix), never a chat prompt; reply → log note + Claude row, toast
async function bgAI(pid, label, text) {
  try { const r = await call('rel_fix', {pid, prompt: text}); addEntry(pid, {kind: 'note', text: `${label}: done in the background`, ts: Date.now()}); addEntry(pid, {kind: 'claude', text: r, ts: Date.now()}); toast(`${label}: done`, 'ok', 0, true, () => openProject(pid)); }
  catch (e) { toast(`${label}: ${e}`, 'err'); }
}
// #413 Markdown row Fix: LICENSE.md (GitHub licence text) + CREDITS.md (scan data) by code, no tokens; README prose → AI only if needed
async function relDocs(b) { b.disabled = true; const pid = cur, c = RELC[pid]?.c || {};
  try { if (!licOk(c, P(pid).license)) addEntry(pid, {kind: 'note', text: 'License written: ' + (await call('lic_write', {pid})).join(', ')});
    if (!c.credits || c.credits_miss?.length) await call('lic_credits', {pid}); } catch (e) { toast(String(e), 'err'); }
  if (c.readme < 10 || c.readme_old) return relAsk('readme');
  if (RELC[pid]) RELC[pid].t = 0; drawRight(); }
async function relAsk(id) {
  const c = RELC[cur]?.c || {}, open = relItems(c, P()).filter(x => !x.ai && !x.ok).map(x => x.t).join(', ');
  const text = id === 'summary' ? `Release prep: in a few lines, what this project is and does, then what's missing for ${relNext(P())} (that stage's checklist; open checks: ${open || 'none'}). Plan only, don't change files.` : RELAI[id];
  if (!await sure(`Run in the background?\n\n${text}\n${estTok()}`, 'Run')) return;
  // #360 headless claude -p (lib rel_fix), no chat turn: row spins till it returns → its reply lands in the log, checklist rechecks
  const pid = cur, row = relItems(c, P()).find(x => x.id === id)?.t || `What's missing for ${relNext(P())}`;
  (RELFIX[pid] ||= new Set()).add(id); drawRight();
  await bgAI(pid, row, text);
  RELFIX[pid].delete(id); if (RELC[pid]) RELC[pid].t = 0; if (pid === cur && tab === 'build') drawRight();
}
async function relRelease(to, btn, sv) { // sv = staged version (#266): its copies + commit
  const sg = sv && RELC[cur]?.c?.staged?.find(x => x.v === sv), v = sg ? sv : (RELC[cur]?.c?.version || '').replace(/^v/, ''), files = sg ? sg.files.map(f => ({path: f, name: f.split(/[\\/]/).pop()})) : relFilesFor(cur, v), st = STAGES[SK[to]];
  const main = to >= SK.indexOf('rc'); // #210 RC/Published: the default branch moves to this release (fast-forward only)
  if (!await sure(`Publish v${v} on GitHub as ${st}?\n\n${files.map(f => f.name).join('\n')}\n\n${main ? 'Main moves to this release too (fast-forward, never forced).\n' : ''}Releases are public on public repos.`, 'Release')) return;
  const pid = cur; RELBUSY[pid] = 1; btn.disabled = true; btn.innerHTML = '<i class="spin"></i>Publishing…'; // #122: push + upload can take minutes; RELBUSY keeps the spinner across redraws
  try {
    const url = await call('release', {pid: pid, tag: 'v' + v, files: files.map(f => f.path), main, sha: sg?.sha || null});
    if (sg) call('stage', {pid, action: 'done', v}).catch(() => {}); /* uploaded → copies go */ // gh release create prints the release URL
    addEntry(pid, {kind: 'note', text: 'Released: ' + url});
    const u = (String(url).match(/https?:\S+/) || [])[0];
    dlg(`<h2>${icon('rocket')}Released v${esc(v)}</h2><p class="hint">Published on GitHub as ${st}.</p><div class="btns"><button class="btn ghost" onclick="closeDlg()">Close</button>${u ? `<button class="btn primary" data-url="${esc(u)}" onclick="closeDlg()">Open release page</button>` : ''}</div>`);
    if (P(pid).stage !== SK[to]) await call('update_project', {pid: pid, changes: {stage: SK[to]}});
    if (P(pid).build.deploy) call('run_step', {pid: pid, step: 'deploy'}); // #34: deploy = part of Release (guessed by build_guess or set in the project)
    delete RELBUSY[pid]; delete RELTO[pid]; delete RELC[pid]; ghiStale(pid); await refresh(); pid === cur && drawRight(true); ciPoll(pid, 15e3); /* #123 tag → CI run */
  } catch (e) { addEntry(pid, {kind: 'note', text: `Release failed: ${e}`}); delete RELBUSY[pid]; if (pid === cur && tab === 'build') drawRight(); }
}
/* #266 full build done → staged release: copies + commit kept (later commits/builds can't stale it), version bumps past it.
   Only once the project walks the release flow (stage set / started). Not current version, missing local target or stale (built before the newest code commit) → nothing staged */
async function relStage(pid) {
  const p = P(pid); if (!p || (!p.stage && RELTO[pid] == null)) return;
  const c = await call('release_check', {pid}).catch(() => null), g = await call('gh_info', {pid}).catch(() => null); if (!c || c.err || !g) return;
  ghiSet(pid, g); c.staged = await call('stage', {pid, action: 'list'}).catch(() => []); RELC[pid] = {c, t: Date.now()};
  // stale = left over from an older build: compare with this build's start, since commits made while it ran (or the post-build commit) are not in it
  const v = (c.version || '').replace(/^v/, ''), files = relFilesFor(pid, v), t0 = (builds[pid]?.t0 || c.code_t || 0) - 5;
  if (!SEMV.test(v) || !files.length || tgtSplit(p, c, files).lack.length || files.some(f => f.mtime && f.mtime < t0) || c.staged.some(x => x.v === v)) return;
  try { c.staged.push(await call('stage', {pid, action: 'add', v, files: files.map(f => f.path)})); }
  catch (e) { return addEntry(pid, {kind: 'note', text: `Couldn't stage v${v}: ${e}`}); }
  const nv = vnext(v, v); // #363 one short line (file list = Release ▾ menu)
  try { await call('version_set', {pid, v: nv}); c.version = nv; c.clean = false; addEntry(pid, {kind: 'note', text: `Staged v${v} - next build set to v${nv}.`}); }
  catch (e) { addEntry(pid, {kind: 'note', text: `Staged v${v}; couldn't set v${nv}: ${e}`}); }
}
// #351 staged releases = ▾ menu on the Release btn (was a list): pick = RELSV, Skip = act w/ confirm
const RELSV = {};
function relMenu(btn) {
  const pid = cur, stg = (RELC[pid]?.c?.staged || []).slice().sort((a, b) => vcmp(a.v, b.v)), sel = (stg.find(x => x.v === RELSV[pid]) || stg[0])?.v;
  menuAt(btn, [{title: 'Staged releases'}, ...stg.map(x => ({check: x.v === sel, label: 'v' + x.v, hint: `${x.files.length} file${x.files.length > 1 ? 's' : ''} · built ${gago(x.t * 1e3)}${x.sha ? ' · ' + x.sha.slice(0, 7) : ''}`,
    run: () => { RELSV[pid] = x.v; drawRight(); }, acts: [{icon: 'trash', tip: 'Skip', items: [{label: `Skip v${x.v} (deletes its built files)`, run: () => relSkip(x.v)}]}]}))]);
}
async function relSkip(v) {
  const pid = cur;
  try { await call('stage', {pid, action: 'skip', v}); } catch (e) { toast(`Skip failed: ${e}`, 'err'); }
  const c = RELC[pid]?.c; if (c?.staged) c.staged = c.staged.filter(x => x.v !== v);
  if (RELSV[pid] === v) delete RELSV[pid]; ghiStale(pid); pid === cur && tab === 'build' && drawRight();
}
// Build from the wizard: on success the build event commits + pushes what it changed (Cargo.lock…) ∵ else "Everything committed" flips red again
const RELB = {};
// #120: busy at once (event confirms) → spinner + bar; no 2nd click → 2nd cargo blocked on the build-dir lock
const buildNow = () => { const b = builds[cur] ||= {out: ''}; b.from = b.out.length; b.t0 = Date.now() / 1e3; b.busy = true; b.manual = true; drawRight(); tabDots(); call('run_step', {pid: cur, step: 'build'}).catch(() => { builds[cur].busy = false; drawRight(); tabDots(); }); };
/* /build (or "build and run a new local copy"): THIS machine's build in plain code (no WSL / Network Mac passes), then run it;
   Clauracle's own repo restarts into it. Never filed, never queued behind tasks. /build all = every target, /build <os> (#262) = that target only (Rust build:<os>); neither runs. */
const BUILD_RE = /^\s*(please\s+|can you\s+|now\s+)*((do|make) (me )?a\s+)?(new\s+)?(local\s+)?(re)?build(\s+(it|clauracle|a new (local )?copy))?,?\s+(and|\+|&|then)\s+(run|restart|launch|start)(\s+(it|into it|the new (one|build|copy)|a new (local )?copy( of (it|yourself|clauracle))?))?[.!]*\s*$/i;
async function buildLocal(pid, tg = '') { // tg: '' = this OS + run | 'all' | windows|linux|macos|ios|android
  if (tg === HOST) tg = ''; const all = tg === 'all', nm = TGT.flatMap(g => g[2]).find(x => x[0] === tg)?.[2];
  const b = builds[pid] ||= {out: ''}; b.from = b.out.length; b.t0 = Date.now() / 1e3; b.busy = true; b.manual = true; if (pid === cur) drawRight(); tabDots();
  addEntry(pid, {kind: 'note', text: all ? 'Building every target…' : nm ? `Building for ${nm}…` : 'Building a local copy…', pill: 'hammer', ts: new Date().toISOString()});
  const ok = await call('run_step', {pid, step: all ? 'build' : tg ? 'build:' + tg : 'local'}).catch(() => false);
  if (!ok) return addEntry(pid, {kind: 'note', text: 'Build failed', act: 'out', ts: new Date().toISOString()});
  if (tg) return addEntry(pid, {kind: 'note', text: all ? 'Built every target' : `Built for ${nm}`, ts: new Date().toISOString()});
  addEntry(pid, {kind: 'note', text: 'Built: starting it', ts: new Date().toISOString()});
  // own repo → swap into it, else Run; own-repo swap error shown ∵ the old Run fallback started `tauri dev`, which the single-instance guard swallowed → "nothing happened"
  call('restart_into', {pid}).catch(e => /own repo/.test(e) ? call('run_step', {pid, step: 'run'}) : addEntry(pid, {kind: 'note', text: `Couldn't swap into the new build: ${e}`, ts: new Date().toISOString()}));
}
const relBuild = () => { RELB[cur] = 1; buildNow(); };
const ciBusy = pid => { const r = RELC[pid]?.c?.ci; return !!r && r.status !== 'completed'; }; // #123 CI run in flight (release_check, polled)
const bBusy = pid => builds[pid]?.busy || ciBusy(pid);
const buildingBtn = sm => `<button class="btn${sm ? ' sm' : ''}" disabled><i class="spin"></i>Building…</button>`;
// Target platforms (#25): p.targets; [] = not picked → Mac follows ships_mac (MAC). *signing/notarizing optional
const TGT = [['monitor', 'Desktop', [['windows', 'win', 'Windows', 'setup .exe (signed*)'], ['macos', 'apple', 'macOS', '.dmg with .app (notarized*)'], ['linux', 'term', 'Linux', '.AppImage']]],
  ['phone', 'Mobile', [['ios', 'apple', 'iOS', 'App Store / TestFlight'], ['android', 'android', 'Android', '.apk / .aab']]]];
const macOn = (p, pid) => p.targets?.length ? p.targets.includes('macos') : !!MAC[pid];
/* #34 readiness: a target is ready when this computer has its tools (local builds; DEPSC) and, from RC up, its signing/store
   service is set up (TGTSVC). Not ready → faded; click = its fix (Dependencies / add the service), not a pick; picked but
   not ready → click unpicks. tgtSplit only counts ready targets → release is limited to them. CI-built ones: no local deps. */
const TGTSVC = {macos: 'macsign', ios: 'appstore', android: 'play'};
// #131 Linux on Windows = local via WSL (run_step 2nd pass) when WSL is there or CI is off (→ "Needs WSL" fix); else CI. ponytail: no Docker/mac path
const tgtLocal = x => x === HOST || x === 'android' || (x === 'macos' && !!S.mac?.host) /* #149 Network Mac */ || (x === 'ios' && HOST === 'macos') || (x === 'linux' && HOST === 'windows' && (!S.ci_builds || !!DEPSC?.d.wsl));
const strictStage = p => Math.max(SK.indexOf(p.stage), RELTO[cur] ?? -1) >= SK.indexOf('rc');
function tgtWhy(p, id) { // '' = ready, else [why, fix js]
  const miss = tgtLocal(id) && DEPSC ? Object.keys(DEPS).filter(k => DEPSC.d[k] === false && DEPS[k][1].includes(id)) : [];
  if (miss.length) return [`Needs ${miss.map(k => DEPS[k][0]).join(', ')} on this computer`, `openSettings('build').then(() => jumpTo('[data-dep=${miss[0]}]'))`];
  const c = RELC[cur]?.c; // #123 CI-built targets: facts from release_check (none yet → no verdict)
  if (!tgtLocal(id) && !S.ci_builds) return [`Not buildable on this computer · CI builds are off (Settings › Build & Release)`, `openSettings('build').then(() => jumpTo('#set-ci'))`]; // #126
  if (!tgtLocal(id) && c && !c.err) {
    if (!c.remote) return ['Built by CI only: put this project on GitHub first', `rightTab('gh')`];
    if (!(c.ci_os || []).includes(id)) return ['Not built on this computer and no CI job builds it', `tgtFlip('${id}').then(() => relAsk('ci_targets'))`];
    if (c.ci?.why) return [`CI can't run: ${c.ci.why}${c.ci.bill ? ' · click to fix billing' : ''}`, `T.opener.openUrl('${c.ci.bill || c.ci.url}')`]; // #133
  }
  const s = TGTSVC[id];
  if (s && strictStage(p) && !(svcOf(p).includes(s) && (svcDone(s) || (s === 'macsign' && SECRETS[cur]?.includes('APPLE_CERTIFICATE')))))
    return [`Release candidate and up: set up ${SERVICES[s].name} first`, `svcJump('${s}')`];
  return '';
}
// #153 jump to a setting: wait ≤4s (pages fill async), open enclosing folds, scroll to center, flash. sel = CSS | () => el
async function jumpTo(sel) {
  for (let i = 0; i < 40; i++, await new Promise(r => setTimeout(r, 100))) {
    const el = typeof sel === 'function' ? sel() : document.querySelector(sel); if (!el) continue;
    for (let d = el.parentElement?.closest('details'); d; d = d.parentElement?.closest('details')) d.open = true;
    if (el.tagName === 'DETAILS') el.open = true;
    el.scrollIntoView({behavior: 'smooth', block: 'center'}); el.classList.remove('jumphl'); void el.offsetWidth; el.classList.add('jumphl'); return;
  }
}
// service card in the Services tab = n-th section (drawServices renders svcOf order); `.keys` inside ∵ old tab's sections linger till its async render
const svcJump = s => (svcOf(P()).includes(s) ? Promise.resolve() : svcAdd(s)).then(() => { rightTab('svc');
  jumpTo(() => { const el = $('rbody').querySelectorAll(':scope > section.sec')[svcOf(P()).indexOf(s)]; return el?.querySelector('.keys') && el; }); });
const tgtReady = (p, id) => !tgtWhy(p, id);
const tgtHtml = p => { const t = p.targets || [];
  return stHead('Target platforms', t.length ? `<span class="pill dim">${t.filter(id => tgtReady(p, id)).length}/${t.length}</span>` : '') + ((t.length ? '' : '<p class="hint">Not picked yet: tap what this project ships to.</p>')
    + TGT.map(([gi, g, xs]) => `<div class="row tgtrow" style="gap:8px;margin-bottom:6px"><span class="dim" data-tip="${g}">${icon(gi)}</span><span class="seg">${xs.map(([id, ic, n, d]) => { const w = tgtWhy(p, id), on = t.includes(id);
      return `<button class="${on ? 'on' : ''}" ${w ? 'style="opacity:.4"' : ''} data-tip="${esc(w ? w[0] + (on ? ' · click to unpick' : ' · click to fix') : d)}" onclick="${w && !on ? esc(w[1]) : `tgtFlip('${id}')`}">${icon(ic)}${n}</button>`; }).join('')}</span></div>`).join('')
    + (t.some(id => !tgtReady(p, id)) ? `<p style="margin:4px 0 6px;font-size:13px">${icon('info')} Not ready, Release is blocked till fixed or unpicked: ${t.filter(id => !tgtReady(p, id)).map(tgtName).join(', ')} (hover for why)</p>` : '')
    + '<small class="dim">* optional for Alpha and Beta; Release candidate and up need it (Services)</small>');
};
/* #128 Actions minutes this month (owner's billing report, 10-min cache). No `user` scope → one-click re-login adding it.
   Limit reached = last CI run's annotation (#123) says billing/spending limit → red + budgets link. */
const ACTU = {}, ACTINC = {free: 2000, pro: 3000, team: 3000, enterprise: 50000};
async function actLoad(pid, force) {
  if (!force && ACTU[pid] && Date.now() - ACTU[pid].t < 600e3) return;
  const d = new Date(); ACTU[pid] = {t: Date.now(), v: await call('actions_usage', {pid, year: d.getFullYear(), month: d.getMonth() + 1}).catch(e => ({err: String(e)}))};
}
async function actScope() { const m = $('ghMsg'); if (m) m.textContent = 'Waiting for gh…';
  const ok = await call('gh_login', {scopes: 'user'}).catch(e => (m && (m.textContent = e), false));
  if (ok) { delete ACTU[cur]; drawRight(); } }
const actHtml = () => { const a = ACTU[cur]?.v, c = RELC[cur]?.c; if (!S.ci_builds || !a || (c && !c.remote)) return '';
  const bill = a.owner ? (a.org ? `https://github.com/organizations/${a.owner}/settings/billing` : 'https://github.com/settings/billing') : '';
  const lim = /spending limit|billing|payment|budget/i.test(c?.ci?.why || ''), link = (t, u) => `<a data-url="${esc(u)}" style="cursor:pointer">${t}</a>`;
  const body = a.err ? `<small class="dim">${esc(a.err)}</small>`
    : a.scope ? `<p class="hint">Seeing ${a.org ? `${esc(a.owner)}'s` : 'your'} Actions minutes needs ${a.org ? 'org admin rights and ' : ''}the GitHub <b>user</b> permission. Sign in again as <b>${esc(P().gh)}</b> to add it.</p>
        <button class="btn" onclick="actScope()">${icon('gh')}Grant usage access</button> <span id="ghMsg" class="dim" style="font-size:13px"></span>`
    : (u => { const inc = ACTINC[a.plan]; return `<p style="margin:0 0 4px;font-size:13px"><b>${Math.round(u.min).toLocaleString()}</b> min this month${inc ? ` · ${inc.toLocaleString()} included (${esc(a.plan)})` : ''}${u.net > 0 ? ` · <b>billed $${u.net.toFixed(2)}</b>` : ''}</p>`
        + (Object.keys(u.sku).length ? `<small class="dim">${Object.entries(u.sku).map(([k, m]) => `${esc(k)} ${Math.round(m)}`).join(' · ')} · macOS counts 10×, Windows 2×</small>` : ''); })(a.u);
  return fold('p-actions', 'GitHub Actions', (lim ? `<p class="err" style="margin:0 0 6px;font-size:13px">${icon('info')} Limit reached: ${esc(c.ci.why)}${c.ci.bill ? ' · ' + link('Fix billing', c.ci.bill) : ''}</p>` : '') + body
    + (bill ? `<div style="margin-top:6px;font-size:13px">${link('Usage', bill + '/usage')} · ${link('Spending limit', bill + '/budgets')}</div>` : ''), lim ? 1 : 0,
    lim ? '<span class="pill err">limit</span>' : '');
};
async function tgtFlip(id) {
  const p = P(), t = p.targets || [];
  p.targets = t.includes(id) ? t.filter(x => x !== id) : [...t, id];
  await call('update_project', {pid: cur, changes: {targets: p.targets}}).catch(e => addEntry(cur, {kind: 'note', text: `Targets not saved: ${e}`}));
  drawRight();
}
// Build tab body. live=false → cache only (no git/gh calls), live=true → refetch what's stale (force → all)
async function buildHtml(p, has, live, force) {
  const pid0 = cur;
  if (live && MAC[pid0] === undefined) MAC[pid0] = await call('ships_mac', {pid: pid0}).catch(() => false);
  if (live) await depsGet(); // tgtWhy: local tools per target (60s cache)
  if (live && S.ci_builds) await actLoad(pid0, force === true); // #128
  if (live && strictStage(p) && svcOf(p).length) { const files = await call('env_list', {pid: pid0}).catch(() => []); // tgtWhy → svcDone reads envKeys
    envKeys = {files, have: new Set(files.flatMap(f => f.keys.filter(([, v]) => v).map(([k]) => k)))}; }
  const mac = macOn(p, pid0);
  let c = GHI[pid0];
  if (live && (force === true || !c || Date.now() - c.t > 30000)) { const g = await call('gh_info', {pid: pid0}).catch(() => null); if (g) { ghiSet(pid0, g); c = GHI[pid0]; } }
  // #232 stepper ① targets ② manual build ③ release (+ next-step box w/ Release); #34: no Deploy button ∵ Release runs the deploy command after publishing
  return `<ol class="stepper ghsteps"><li><span class="sn">1</span>${tgtHtml(p)}</li>
    <li><span class="sn">2</span>${stHead('Manual build', `<button class="ib" title="Edit commands (tasks build by themselves: see Then on the GitHub tab)" onclick="buildSet()">${icon('pen')}</button>`)}${has ? `<div class="btnrow">${[['build', 'hammer'], ['run', 'play']].map(([s, ic]) => s === 'run' && builds[cur]?.live
          ? `<button class="btn" onclick="builds[cur].live=false; call('stop_run', {pid:cur}).then(drawRight)">${icon('stop')}Stop</button>` /* Run ⇄ Stop while the app runs */
          : s === 'build' && builds[cur]?.busy ? buildingBtn()
          : `<button class="btn" ${p.build[s] ? '' : 'disabled'} onclick="${s === 'build' ? 'buildNow()' : `call('run_step', {pid:cur, step:'${s}'})`}">${icon(ic)}${s[0].toUpperCase() + s.slice(1)}</button>`).join('')}</div><div class="bbar${bBusy(cur) ? ' on' : ''}" title="${builds[cur]?.busy ? 'Building…' : ciBusy(cur) ? 'CI building…' : 'No build running'}"></div>${ciBusy(cur) ? `<small class="dim"><i class="spin"></i> CI: ${esc(RELC[cur].c.ci.workflowName)} ${esc(RELC[cur].c.ci.status)} · <a data-url="${esc(RELC[cur].c.ci.url)}" style="cursor:pointer">view run</a></small>` : ''}`
        : live || GUESS[pid0] ? await guessHtml() : skel('b45')}</li>
    <li><span class="sn">3</span>${live || RELC[pid0] ? await relWizard(p, live && force === true, !live) : stHead('Release') + skel(70, 45, 'b100')}</li></ol>`
      + actHtml()
      + (!svcOf(p).includes('macsign') && mac
        ? sec('Mac builds', `<p class="hint">Unsigned, testers get an "unidentified developer" block. An Apple Developer account fixes that.</p>
          <button class="btn block" onclick="svcAdd('macsign').then(() => rightTab('svc'))">Set up Mac signing</button>`) : '')
      + (has || builds[cur]?.out ? sec('Output', `<div class="out" id="buildOut">${esc(builds[cur]?.out || '')}</div>`) : '');
}
/* #182 p.build.after = '<build|smart>[_run][_deploy]', '' = nothing; 'smart_build' = smart, nothing after; legacy 'smart' = smart_run (Rust same) */
const aft = a => { a = a === 'smart' ? 'smart_run' : a || ''; return {b: a.startsWith('smart') ? 'smart' : a ? 'build' : '', t: ['run', 'deploy'].filter(x => a.includes(x)).join('_')}; };
const THEN = {run: 'Run', run_deploy: 'Run + Deploy', deploy: 'Deploy'};
const thenLbl = a => { const {b, t} = aft(a); return b ? (b === 'smart' ? 'Smart build' : 'Build') + (t ? ' + ' + THEN[t] : '') : 'Nothing'; };
const thenBtn = (p, w) => `<button class="selbtn" onclick="thenMenu(this, '${w}')">${esc(thenLbl(p.build.after))}</button>`;
function thenMenu(el, w) { // w: 'm' = Settings project (mp), else the open project; click the picked one again = off
  const p = w === 'm' ? P(mp) : P(), {b, t} = aft(p.build.after);
  const put = (b, t) => { const v = !b ? '' : b === 'smart' && !t ? 'smart_build' : b + (t ? '_' + t : '');
    w === 'm' ? setBuild('after', v) : setP({build: {...P().build, after: v}}).then(() => drawRight()); };
  menuAt(el, [{label: 'Do nothing', hint: 'after each task', check: !b, run: () => put('', '')}, '-', {title: '1 · Build'}, ...[['build', 'Build', 'every task'], ['smart', 'Smart build', 'only on big changes / fixes']].map(([k, l, h]) => ({label: l, hint: h, check: b === k, run: () => put(b === k ? '' : k, t)})),
    {and: 'and'}, ...Object.entries(THEN).map(([k, l]) => ({label: l, check: t === k, run: () => put(b || 'build', t === k ? '' : k)}))]);
}
const thenRow = p => `${stHead('Then', '', 'Runs in plain code when Clauracle finishes a task' + (p.path && /claude-term|clauracle/i.test(p.path) ? '. Clauracle itself: a good build restarts into it' : ''))}${thenBtn(p).replace('"selbtn"', '"selbtn qw"')}`; // #192 under the NEXT list, styled as its label
async function drawRight(force) {
  if ($('right').hidden || !cur) return;
  document.querySelectorAll('#rtabs button').forEach(b => b.classList.toggle('on', !RC() && b.dataset.t === tab)); // #421 collapsed → no active icon
  if (RC()) return tabDots();
  tabDots();
  tabHead($('rhead'), document.querySelector(`#rtabs [data-t="${tab}"] span`).textContent, `menuAt(this, ${tab === 'svc' ? 'svcItems()' : 'tabItems(tab)'})`, TABADD[tab]);
  const box = $('rbody'), p = P();
  if (box.dataset.t !== tab + cur) { box.dataset.t = tab + cur; if (['code', 'web', 'svc'].includes(tab)) box.innerHTML = sec('', skel(60, 'b100', 90, 75, 'b100')); } // #105: async tabs paint a skeleton on switch
  if (tab === 'agents') { box.innerHTML = '<div id="atp"></div>'; return drawAtp(); }
  if (tab === 'code') return drawCode();
  if (tab === 'web') return drawWeb();
  if (tab === 'svc') return drawServices();
  if (tab === 'build') {
    /* 2 passes like GHI: cached-only render at once (stale ok, missing → "Checking…"), then fetch what's stale
       (build_guess/ships_mac/release_check 60s/gh_info 30s) + re-render, keeping inputs the user changed meanwhile */
    const has = p.build.build || p.build.run || p.build.deploy, pid0 = cur, ok = () => cur === pid0 && tab === 'build' && !$('right').hidden && !RC();
    const old = (o, ms) => !o || Date.now() - o.t > ms;
    const stale = force === true || (!has && !GUESS[pid0]) || MAC[pid0] === undefined || old(RELC[pid0], 60e3) || old(GHI[pid0], 30e3);
    box.innerHTML = await buildHtml(p, has, false);
    if (!stale) return;
    const before = formVals(box), h = await buildHtml(p, has, true, force);
    if (!ok()) return; // awaits above (release_check: git+gh) → user may have switched tab/project
    const mine = formVals(box), focus = document.activeElement?.id;
    box.innerHTML = h;
    for (const [k, v] of Object.entries(mine)) if (v !== before[k]) { const el = formEl(box, k); if (el) el.type === 'checkbox' ? el.checked = v : el.value = v; }
    if (focus && $(focus)) $(focus).focus();
    return;
  }
  /* GHI cache: last gh_info per project → drawn at once, refetch only if >30s old or forced (Refresh/actions).
     While fetching: refresh icon pulses; inputs the user changed meanwhile are re-applied onto the fresh render (same → drop). */
  const pid = cur, c = GHI[pid], stale = force === true || !c || Date.now() - c.t > 30000;
  if (c) drawGh(c.g, stale); else { box.innerHTML = sec('Repository', skel(55)) + '<div id="atp"></div>' + skel('b100', 'b100'); drawAtp(); } // Plan needs no gh: show it at once
  if (!stale) return;
  const before = c && formVals(box), g = await call('gh_info', {pid}).catch(e => c?.g || {err: String(e)});
  if (!g.err) ghiSet(pid, g);
  if (pid !== cur || tab !== 'gh' || $('right').hidden || RC() || qDragging) return; // mid-drag re-render would drop the row; GHI already fresh
  const mine = before && formVals(box), focus = document.activeElement?.id;
  drawGh(g, false);
  if (mine) for (const [k, v] of Object.entries(mine)) if (v !== before[k]) { const el = formEl(box, k); if (el) el.type === 'checkbox' ? el.checked = v : el.value = v; }
  if (focus && $(focus)) $(focus).focus();
}
/* #104 GHI persisted (localStorage) ∴ GitHub tab draws at once after restart too; restored/invalidated = t:0 → shown + refetched (stale-while-revalidate) */
const GHI = (() => { try { const o = JSON.parse(localStorage.ghi || "{}"); for (const k in o) o[k].t = 0; return o; } catch { return {}; } })();
const ghiSet = (pid, g) => { GHI[pid] = {g, t: Date.now()}; try { localStorage.ghi = JSON.stringify(GHI); } catch {} }, ghiStale = pid => GHI[pid] && (GHI[pid].t = 0);
// form state in the panel: id → value, unnamed checkboxes keyed by their value
const formVals = box => Object.fromEntries([...box.querySelectorAll('input, select, textarea')].map(e => [e.id || 'v:' + e.value, e.type === 'checkbox' ? e.checked : e.value]));
const formEl = (box, k) => k.startsWith('v:') ? [...box.querySelectorAll('input[type=checkbox]')].find(e => !e.id && e.value === k.slice(2)) : box.querySelector('#' + CSS.escape(k));
function drawGh(g, busy) {
  if (qDragging) return; // #193 a rebuild mid-drag orphans Sortable's row/clone
  $('rhead').lastChild?.classList.toggle('upd', !!busy); // the tab menu (Refresh) pulses while fetching
  const box = $('rbody'), p = P(), item = (x, kind) => `<div class="item"><a data-url="${esc(x.url)}">#${x.number} ${esc(x.title)}</a>
    <div class="row dim" style="font-size:12px"><span class="grow">@${esc(x.author?.login)}</span><button class="btn ghost sm" onclick='draft(${JSON.stringify(kind)}, ${esc(JSON.stringify(x))})'>Ask Clauracle</button></div></div>`;
  const list = (xs, kind) => xs?.length ? `<div class="list">${xs.map(x => item(x, kind)).join('')}</div>` : empty('None open');
  box.innerHTML = sec('Repository', g.repo ? `<div class="row"><a class="grow" data-url="${esc(g.repo.url)}" style="color:var(--accent);cursor:pointer">${esc(g.repo.nameWithOwner)}</a><span class="pill dim">@${esc(p.gh)}</span></div>`
      : g.remote ? (p.gh ? `<p class="hint">Can't read the GitHub repo as @${esc(p.gh)}.</p><div class="btnrow"><button class="btn" onclick="openManager(cur)">${icon('gh')}Pick another account</button><button class="btn" onclick="TABREF.gh(); drawRight(true)">${icon('refresh')}Try again</button></div>` : `<p class="hint">This repo has no GitHub account picked yet.</p><button class="btn primary block" onclick="openManager(cur)">${icon('gh')}Pick a GitHub account</button>`)
      : g.err ? `<p class="err">${esc(g.err)}</p>` : publishForm(p, g.users || []))
    + '<div id="atp"></div>' // Plan (issue queue / PLAN.md, drag order, Suggestions, Cleanup): drawAtp fills it
    + `<div id="tasksBox">${tasksHtml()}</div>`
    + (g.repo ? fold('collab', 'Collaborators', `<div id="collabBox">${collabHtml(cur)}</div>`, COLLAB[cur]?.people?.length || 0) : '')
    + (g.remote ? changesSec(g.git || {}) : '')
    + fold('prs', 'Pull requests', list(g.prs, 'pr'), g.prs?.length || 0) + (g.issues?.length ? fold('issues', 'Other issues', list(g.issues, 'issue'), g.issues.length) : ''); // non-clauracle only (gh_info)
  drawAtp();
  if (g.repo) collabLoad();
}
/* #113 collaborators: task_do collab (people+role, pending invites), cached 10 min per pid ∵ rarely change; invite = PUT
   (GitHub sends the invite mail). IMETA (assignees) dropped on invite ∵ invitee becomes assignable once accepted */
const COLLAB = {};
const collabHtml = pid => { const c = COLLAB[pid]; if (!c) return skel(60, 40);
  return `<div class="tlist">${(c.people || []).map(x => tli(`@${esc(x.login)}`, {end: `<span class="pill dim">${esc(x.role || '')}</span>`})).join('')
    + (c.invites || []).map(l => tli(`@${esc(l)}`, {cls: 'in_progress', end: '<span class="pill dim">invited</span>'})).join('')}</div>`
    + (c.err ? `<div class="warn">${esc(c.err)}</div>` : '')
    + `<div class="row"><input id="collabU" class="grow" placeholder="Invite by GitHub username" onkeydown="if(event.key==='Enter')collabInvite()"><button class="ib" title="Invite" onclick="collabInvite()">${icon('plus')}</button></div>`; };
async function collabLoad(force) {
  const pid = cur;
  if (!force && COLLAB[pid]?.t > Date.now() - 6e5) return;
  try { COLLAB[pid] = {...JSON.parse(await call('task_do', {pid, action: 'collab', arg: ''})), t: Date.now()}; } catch (e) { COLLAB[pid] = {err: String(e), t: 0}; }
  if (pid === cur && $('collabBox')) $('collabBox').innerHTML = collabHtml(pid);
}
async function collabInvite() {
  const u = $('collabU')?.value.trim(), pid = cur;
  if (!u) return;
  try { await call('task_do', {pid, action: 'invite', arg: u}); delete IMETA[pid]; await collabLoad(true); toast(`Invited @${u.replace(/^@/, '')}`, 'ok'); }
  catch (e) { toast(String(e), 'err'); }
}
// assignee picker → GitHub tab, Collaborators open + field focused
function collabGo() { closeDlg(); folds.collab = true; expandRight('gh'); setTimeout(() => $('collabU')?.focus(), 300); }
// info only (user: no chores): uncommitted files listed; Claude commits, claude::after_turn pushes (store::auto_push)
function changesSec(s) {
  const f = s.files || [];
  if (!f.length && !s.ahead) return fold('changes', 'Changes', empty('Everything is committed and pushed'));
  return fold('changes', 'Changes', (f.length ? `<div class="keys">${f.map(x => `<div><span>${esc(x)}</span></div>`).join('')}</div>` : '')
    + (s.ahead ? `<p class="faint" style="margin:0;font-size:12.5px">${s.ahead} commit${s.ahead > 1 ? 's' : ''} not on GitHub yet: pushed when the turn ends.</p>` : ''),
    f.length || (s.ahead ? `${s.ahead}↑` : 0));
}
const mb = n => n > 1e9 ? (n / 1e9).toFixed(1) + ' GB' : n > 1e6 ? (n / 1e6).toFixed(1) + ' MB' : Math.ceil(n / 1e3) + ' kB';
// version in a file name (Clauracle_0.1.0_x64-setup.exe → 0.1.0); pre-release kept (0.2.0-beta.1) ∵ else a beta never matches its files
/* #119 semver: vcmp(a,b) <0|0|>0 on x.y.z (prerelease < release); vnext = patch after max(a,b) ∴ never ≤ latest tag */
const vcmp = (a, b) => { const p = s => (s || '').replace(/^v/, '').split('-'), [x, xp] = p(a), [y, yp] = p(b), n = s => s.split('.').map(Number);
  for (let i = 0, A = n(x), B = n(y); i < 3; i++) if ((A[i] || 0) !== (B[i] || 0)) return (A[i] || 0) - (B[i] || 0);
  return xp === yp ? 0 : !xp ? 1 : !yp ? -1 : xp < yp ? -1 : 1; };
const vnext = (a, b) => (vcmp(a, b) >= 0 ? a : b).replace(/^v/, '').replace(/^(\d+\.\d+\.)(\d+).*$/, (_, x, n) => x + (+n + 1));
const fver = n => n.match(/\d+\.\d+\.\d+(?:-[0-9A-Za-z]+(?:\.\d+)*)?/)?.[0] || '', SEMV = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?$/;
// newest build per file type whose version = v (or carries none); builds come newest first
const relPick = (builds, v) => { const seen = new Set(); return builds.filter(b => { const x = b.name.replace(/^[^.]*/, ''), bv = fver(b.name); if (seen.has(x) || (bv && bv !== v)) return false; seen.add(x); return true; }); };
// "Everything committed" Fix: commit all + push (git_do commit) ∵ GitHub tab has no manual commit/push anymore
async function relCommit(btn) {
  btn.disabled = true;
  try { await call('git_do', {pid: cur, action: 'commit', arg: 'Prepare release'}); addEntry(cur, {kind: 'note', text: 'Committed and pushed'}); delete RELC[cur]; drawRight(); }
  catch (e) { addEntry(cur, {kind: 'note', text: `Commit failed: ${e}`}); btn.disabled = false; }
}
