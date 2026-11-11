// ---- clone ----
async function cloneLogin(b) { b.disabled = true; b.textContent = 'Waiting for gh…'; if (await call('gh_login').catch(e => (toast(String(e), 'err'), false))) { SET = null; openClone(); } else b.disabled = false; }
async function openClone() {
  SET ||= await call('settings'); ghUsers = SET.gh_users;
  dlg(`<h2>Clone from GitHub</h2>${ghUsers.length ? `<div class="row"><select id="cloneUser">${ghUsers.map(u => `<option>${esc(u)}</option>`).join('')}</select>
    <input id="cloneFilter" class="grow" placeholder="Filter, or owner/repo"></div><small class="dim">Into <span class="mono">${esc(S.projects_dir)}/&lt;owner&gt;/&lt;repo&gt;</span></small><div id="repoList" class="dim" style="height:340px;overflow:auto;margin-top:8px">${skel(...Array(8).fill('b100'))}</div>`
    : `<p class="dim">No GitHub account yet.</p><button class="btn primary block" onclick="cloneLogin(this)">${icon('gh')}Log in to GitHub</button>`}<div class="btns"><button class="btn ghost" onclick="closeDlg()">Cancel</button></div>`);
  if (!ghUsers.length) return;
  let repos = [];
  const draw = () => { const f = $('cloneFilter').value.trim(), fl = f.toLowerCase();
    const list = repos.filter(r => r.nameWithOwner.toLowerCase().includes(fl)).slice(0, 100);
    if (f.includes('/') && !list.some(r => r.nameWithOwner.toLowerCase() === fl)) list.unshift({nameWithOwner: f});
    $('repoList').innerHTML = list.map(r => `<div class="item"><a onclick="pickRepo('${esc(r.nameWithOwner)}', '${esc(r.branch || '')}')">${esc(r.nameWithOwner)}</a><div class="dim" style="font-size:12px">${esc(r.description)}</div></div>`).join('') || 'No repos here. Type owner/repo to clone any repo.'; };
  const load = async () => { const u = $('cloneUser').value; $('repoList').innerHTML = skel(...Array(8).fill('b100')); repos = REPOS[u] ||= await call('gh_repos', {user: u}) || []; draw(); };
  $('cloneUser').onchange = load; $('cloneFilter').oninput = draw; cloneBack = draw; load();
}
const REPOS = {}; // gh user → repo list, per app run ∵ paginated gh api call
let cloneBack;
// step 2: branch to check out (default first) and an optional new branch made from it
async function pickRepo(repo, def) {
  const box = $('repoList'), user = $('cloneUser').value;
  box.innerHTML = `<div class="sc" style="padding:0;border:0;gap:10px"><b>${esc(repo)}</b>
    <label class="dim">Branch <select id="cloneBranch" class="block"><option>${esc(def || 'default')}</option></select></label>
    <label class="dim">New branch from it (optional) <input id="cloneNew" class="block" placeholder="e.g. my-feature"></label>
    <div class="btnrow"><button class="btn ghost" onclick="cloneBack()">Back</button><button class="btn primary" onclick="clone('${esc(repo)}')">${icon('gh')}Clone</button></div></div>`;
  const bs = await call('gh_branches', {user, repo}).catch(() => []);
  if ($('cloneBranch')) $('cloneBranch').innerHTML = (bs.length ? [def, ...bs.filter(b => b !== def)].filter(Boolean) : [def || 'default']).map(b => `<option>${esc(b)}</option>`).join('');
}
async function clone(repo) {
  const branch = $('cloneBranch')?.value, new_branch = $('cloneNew')?.value.trim() || null;
  $('repoList').textContent = `Cloning ${repo}…`;
  try { const p = await call('clone_repo', {user: $('cloneUser').value, repo, branch: branch === 'default' ? null : branch, newBranch: new_branch /* Tauri 2 args = camelCase; snake key silently → None */}); closeDlg(); await refresh(); await openProject(p.id); askProject(P(p.id)); }
  catch (e) { $('repoList').textContent = e; }
}

// ---- panels: drag to resize (no minimum); under SNAP px a panel collapses to a rail ----
const RC = () => $('right').classList.contains('rail'), LC = () => $('side').classList.contains('rail'); // #387 shown state (layout may rail a panel the user left open)
const RAIL = 52, SNAP = 140, MAINW = 480; // ponytail: fixed guess of the header's need; measure #head if it drifts
const rMin = () => 240 + RAIL, lMin = () => 180 + RAIL; // #421 widths include the icon bar
let UI = {lw: 252, rw: 380, lc: false, rc: true, st: 'projects'};
try { Object.assign(UI, JSON.parse(localStorage.ui || '{}')); } catch {}
UI.ew ??= true; // #386 Edit view word wrap on by default
if (UI.tab) tab = {tasks: 'gh', env: 'svc'}[UI.tab] || UI.tab; // old Plan tab → GitHub, old .env tab → Services
applyLook();
let KEEP = 'r'; // #387 panel the user opened last: the other one rails first
function layout(keep) {
  if (keep) KEEP = keep;
  try { localStorage.ui = JSON.stringify(UI); } catch {}
  // #387 main keeps MAINW (header: switcher + branch + Edit menu bar): shrink panels to their mins → rail the not-KEEP one → the other; display only ∴ UI.lw/rw/lc/rc untouched, widening restores
  let lc = UI.lc, rc = UI.rc, lw = lc ? RAIL : Math.max(UI.lw, lMin()), rw = rc ? RAIL : Math.max(UI.rw, rMin());
  const room = () => innerWidth - MAINW - lw - rw;
  if (room() < 0 && !rc) rw = Math.max(rMin(), rw + room());
  if (room() < 0 && !lc) lw = Math.max(lMin(), lw + room());
  for (const s of KEEP === 'r' ? 'lr' : 'rl') if (room() < 0) s === 'r' ? (rc || (rc = true, rw = RAIL)) : (lc || (lc = true, lw = RAIL));
  const fold = $('right').classList.contains('rail') !== rc;
  $('side').classList.toggle('rail', lc); $('side').style.width = lw + 'px';
  $('right').classList.toggle('rail', rc); $('right').style.width = rw + 'px';
  drawRails(); if (fold) drawRight();
}
addEventListener('resize', () => layout());
document.querySelectorAll('.grip').forEach(g => {
  const left = g.dataset.side === 'l';
  g.ondblclick = () => { left ? UI.lc = !UI.lc : UI.rc = !UI.rc; layout(); drawRight(); };
  g.onpointerdown = e => dragOn(e, e => {
    const w = left ? e.clientX : innerWidth - e.clientX, rail = w < SNAP;
    if (left) { UI.lc = rail; if (!rail) UI.lw = Math.max(lMin(), Math.min(w, innerWidth * .5)); }
    else { const was = UI.rc; UI.rc = rail; if (!rail) UI.rw = Math.max(rMin(), Math.min(w, innerWidth * .75)); if (was !== rail) drawRight(); }
    layout(left ? 'l' : 'r');
  });
});
const initials = n => esc(n.split(/[\s_.-]+/).filter(Boolean).slice(0, 2).map(w => w[0].toUpperCase()).join('') || '?');
const dotOf = p => busy[p.id] ? 'busy' : p.ready ? 'ready' : '';
// files Claude touched lately in this project, newest first
const touched = pid => [...new Set((logs[pid] || []).filter(e => e.kind === 'tool' && e.detail?.file_path).map(e => e.detail.file_path).reverse())].slice(0, 6);
// Claude Code's own task list (its latest TodoWrite)
// Claude's own task list: TaskCreate/TaskUpdate (newer Claude Code; ids = creation order from 1, per session) else the latest TodoWrite
// ponytail: ids inferred from order, not read from TaskCreate's result; read the result text if they ever skip numbers
const todos = pid => {
  const l = logs[pid] || [], tasks = [];
  for (const e of l) {
    if (e.kind === 'note' && e.text === 'Fresh session') tasks.length = 0;
    if (e.kind !== 'tool') continue;
    const n = e.text.split(' ')[0], d = e.detail || {};
    if (n === 'TaskCreate') tasks.push({id: String(tasks.length + 1), content: d.subject || d.description || '', activeForm: d.activeForm, status: 'pending'});
    if (n === 'TaskUpdate') { const t = tasks.find(x => x.id === String(d.taskId)); if (t) { if (d.status) t.status = d.status; if (d.subject) t.content = d.subject; if (d.activeForm) t.activeForm = d.activeForm; } }
  }
  if (tasks.length) return tasks.filter(t => t.status !== 'deleted');
  return l.filter(e => e.kind === 'tool' && /^TodoWrite\b/.test(e.text) && Array.isArray(e.detail?.todos)).pop()?.detail.todos || [];
};
// subagents Claude started (Task/Agent tool): running = no result yet while the turn runs; their own tool calls carry `parent`
// running only if started this turn (after the last user msg) ∵ an old agent w/o logged result (background, cut by restart) looked revived each prompt
// a background agent's first result is only "Async agent launched": it runs until its end notice (a 2nd result), for the whole session
const ASYNC = /^Async agent launched/;
const agentsOf = pid => { const l = logs[pid] || [], turn = l.findLastIndex(e => e.kind === 'user'), sess = l.findLastIndex(e => e.kind === 'note' && /^(Fresh|Resumed) session/.test(e.text));
  return l.filter(e => e.kind === 'tool' && /^(Task|Agent)\b/.test(e.text)).map(e => { const sub = l.filter(x => x.parent && x.parent === e.id), i = l.indexOf(e);
    const d = e.detail || {}, n = +((d.description || '').match(/#(\d+)/) || (d.prompt || '').match(/#(\d+)/) || [])[1] || 0; // the issue it was handed
    const rs = e.id ? l.filter(x => x.result_of === e.id) : [], bg = rs.length > 0 && rs.every(x => ASYNC.test(x.text || ''));
    return {type: d.subagent_type || 'general-purpose', desc: d.description || '', issue: n, running: !!e.id && (bg ? i > sess : !!busy[pid] && i > turn && !rs.length),
      err: !!e.id && l.some(x => x.result_of === e.id && x.kind === 'err'),
      steps: sub.length, last: sub.at(-1)?.text.replace(/^↳ /, '')}; }); };
// #atp = Plan block at the top of the GitHub tab, or the whole Agents tab (drawRight/drawGh create it; absent → no-op).
// Plan: Suggestions → Claude's live list (open + last 3 done) → Queue (issues / PLAN.md, drag order) → Done lately
// #248 no helpers started → the set-up agents, faded when off; top bar = switch (left) + settings (right)
// #473 role names: edit ROLES to rename an agent; unknown ones get a name from keywords in their name/description
const ROLES = {'general-purpose': 'The Generalist', claude: 'The All-Rounder', Explore: 'The Scout', Plan: 'The Architect', 'statusline-setup': 'The Decorator',
  'web-fetch': 'The Courier', 'claude-code-guide': 'The Guide', 'output-style-setup': 'The Stylist'};
const ROLEKW = [[/secur|vuln|audit/i, 'The Security Expert'], [/fix|debug|bug/i, 'The Fixer'], [/test|verif/i, 'The Tester'], [/review/i, 'The Reviewer'],
  [/build|deploy|\bci\b|pipeline|install/i, 'The Plumber'], [/research|search|explor/i, 'The Researcher'], [/plan|architect/i, 'The Architect'],
  [/design|\bui\b|css|style/i, 'The Designer'], [/\bdocs?\b|writ/i, 'The Writer'], [/data|sql/i, 'The Librarian'], [/fetch|web|url/i, 'The Courier']];
const roleOf = a => ROLES[a.name] || ROLEKW.find(([r]) => r.test(a.name + ' ' + (a.description || '')))?.[1] || 'The ' + a.name.replace(/[-_]+/g, ' ').replace(/^\w/, c => c.toUpperCase());
// face from the name's hash: mirrored 16x16 pixel creature (body, arms, feet, maybe antennas), eyes only; same name → same face
function face(name, px = 32) {
  let h = 2166136261; for (const c of name) h = Math.imul(h ^ c.charCodeAt(0), 16777619);
  const r = n => { h = Math.imul(h ^ h >>> 15, 2246822519) + 0x6D2B79F5 | 0; return ((h ^ h >>> 13) >>> 0) % n; };
  const hue = r(360), g = 1 + r(2), top = 3 + r(3), bot = 10 + r(3), base = 4 + r(3), shape = r(4), on = new Set();
  const put = (x, y) => { on.add(x + ',' + y); on.add(15 - x + ',' + y); }; // draw the left half, mirrored onto the right
  for (let y = top; y <= bot; y++) {
    const k = (y - top) / (bot - top); // 0 at the top row, 1 at the bottom row
    let w = base - [0, k < .2 ? 2 : k < .4 ? 1 : 0, Math.round((1 - k) * 2), y === bot ? -1 : 0][shape];
    if (y >= 5 && y <= 8) w = Math.max(w, g + 4); // eye rows and the rows above and below keep a pixel of body around the eyes while they glance
    for (let x = 8 - Math.min(w, 7); x < 8; x++) put(x, y);
  }
  const edge = y => { for (let x = 0; x < 8; x++) if (on.has(x + ',' + y)) return x; return 8; };
  const ay = bot - 2 - r(2), ax = edge(ay), arm = r(3); // arms leave the side 2-3 rows above the bottom: straight, down or up
  const torso = new Set(on); // arms go in their own path so working faces can wave them
  if (ax > 1) { put(ax - 1, ay); if (ax > 2) put(ax - 2, ay + [0, 1, -1][arm]); } // column 0 stays empty: a 1-pixel margin all round
  const arms = [...on].filter(p => !torso.has(p)); arms.forEach(p => on.delete(p));
  const lx = 8 - Math.max(1, Math.round((8 - edge(bot)) * (.4 + r(3) * .2))), lh = 1 + r(2); // feet hang under the bottom row
  for (let i = 1; i <= lh; i++) put(lx, bot + i);
  if (r(2)) put(lx - 1, bot + lh);
  if (r(2)) { const tx = 7 - r(Math.max(1, 7 - edge(top))); put(tx, top - 1); if (r(2)) put(tx - 1 + r(3), top - 2); } // antennas
  const path = ps => ps.map(p => { const [x, y] = p.split(','); return `M${x} ${y}h1v1h-1z`; }).join(''), body = path([...on]);
  const ex = 8 - g - 2, d = r(40) / 10; // eye gap 2 or 4 pixels; a random delay so faces don't blink in step
  const sz = Math.max(1, Math.round(px * devicePixelRatio / 16)) * 20 / devicePixelRatio; // whole screen pixels per face pixel, nearest to the asked creature size; 2 pixels of padding all round (round background in CSS: .pxf)
  return `<svg class="pxf" width="${sz}" height="${sz}" viewBox="-2 -2 20 20" shape-rendering="crispEdges" aria-hidden="true" style="--d:-${d}s">
    <g class="bod" fill="hsl(${hue} 65% 66%)"><path d="${body}"/><path class="arms" d="${path(arms)}"/>
    <g class="eyes" fill="#1d1d21"><rect x="${ex}" y="6" width="2" height="2"/><rect x="${16 - ex - 2}" y="6" width="2" height="2"/></g></g></svg>`;
}
// Agents tab: every enabled agent (own helpers + Claude Code's) with its state this session; ran-but-disabled ones too
// an agent's description cut to its first clause: provider texts can run to a whole paragraph
const agBrief = d => { const s = (d || '').split(/\.\s|:\s|\s\(/)[0]; return s.length > 80 ? s.slice(0, 79) + '…' : s; };
const AGST = {busy: 'Working', fail: 'Last run failed', 'ready pul': 'Done, ready again', idle: 'Idle'};
function agentIdle(ag = []) {
  if (!AGENTS || !SET) { agentIdle.p ||= Promise.all([call('agents'), SET || call('settings')]).then(([a, s]) => { AGENTS = a; SET = s; agentIdle.p = null; drawAtp(); }); return skel(40); }
  const on = S.agents_on, off = SET.off.filter(i => i.startsWith('agent:')).map(i => i.slice(6)), mine = new Set(AGENTS.map(a => a.name.trim()));
  const cc = (CCAG || []).filter(a => !mine.has(a.name)), ccOn = cc.filter(a => !off.includes(a.name)), xs = [...AGENTS, ...cc];
  const extra = ag.filter((r, i) => !xs.some(a => a.name === r.type) && ag.findIndex(x => x.type === r.type) === i).map(r => ({name: r.type}));
  const row = a => { const rs = ag.filter(r => r.type === a.name), run = rs.filter(r => r.running), last = rs.at(-1);
    const st = run.length ? 'busy' : last?.err ? 'fail' : last ? 'ready pul' : 'idle';
    const job = run.map(r => teamTasks(cur).find(x => x.number === r.issue)).find(Boolean); // on a task issue → "#N title" ticker in place of the description
    const sub = run.length ? run.map(r => esc(r.desc + (r.last ? ': ' + r.last : ''))).join(' · ') : last ? esc(last.desc) + (last.err ? ' · failed' : ' · done') : esc(agBrief(a.description));
    return `<div class="atrow"><span class="face">${face(a.face || a.name)}<span class="dot ${st === 'idle' ? '' : st}" data-tip="${AGST[st]}"></span></span><span class="grow" style="min-width:0"><b>${esc(roleOf(a))}</b>${run.length > 1 ? ` ×${run.length}` : ''} <small class="am">${esc(a.name)}${a.model ? ' · ' + esc(a.model) : ''}</small>
      ${job ? `<small class="atk"><b class="inum">#${job.number}</b> ${marq(esc(job.title))}</small>` : `<small data-tip="${sub}">${sub}</small>`}</span></div>`; };
  // a section's X/Y pill is highlighted (HOTF) while its switch is on; the switch's label stops the click so the fold doesn't toggle
  // count = running / enabled; highlighted only while one of them runs
  const runs = new Set(ag.filter(r => r.running).map(r => r.type)), nRun = xs => xs.filter(a => runs.has(a.name)).length;
  const sect = (key, title, isOn, tip, change, list, n, total, none = '', dis = false) => { HOTF[n ? 'add' : 'delete'](key);
    return fold(key, title, `<div class="agl">${list || none}</div>`, `${n}/${total}`, `<span data-tip="${tip}" onclick="event.stopPropagation()">${sw(isOn, change, dis)}</span>`, true); };
  // Clauracle agents = the fixed ones the AI provider brings (CCAG, never hard-coded); Custom agents = made by the user, always listed (empty state links to Settings)
  const own = AGENTS.filter(a => !on || a.on);
  return sect('ag-cc', 'Clauracle agents', ccOn.length > 0, ccOn.length ? 'Switch Clauracle agents off' : 'Switch Clauracle agents on', 'agentsCc(this.checked)',
      [...ccOn, ...extra].map(row).join(''), nRun([...ccOn, ...extra]), ccOn.length || cc.length,
      empty(cc.length ? 'Clauracle agents are off' : 'Listed once a project\'s session has started'))
    + sect('ag-own', 'Custom agents', on && AGENTS.length > 0, !AGENTS.length ? 'Add a custom agent first' : on ? 'Switch custom agents off' : 'Switch custom agents on', "call('set_agents', {agents: AGENTS, on: this.checked}).then(refresh)",
      own.map(a => on ? row(a) : `<div class="agoff" style="opacity:.5">${row(a)}</div>`).join(''), on ? nRun(own) : 0, own.length || AGENTS.length,
      empty('No custom agents yet', `<button class="btn sm" onclick="openSettings('cx').then(() => jumpTo('[data-f=g-own]'))">${icon('plus')}Add custom agents</button>`), !AGENTS.length);
}
// Clauracle agents switch: every provider agent on or off at once (global cfg.off "agent:<name>")
async function agentsCc(on) {
  const mine = new Set(AGENTS.map(a => a.name.trim()));
  for (const a of (CCAG || []).filter(a => !mine.has(a.name))) if (SET.off.includes('agent:' + a.name) === on) await setGlobalOff('agent:' + a.name, !on);
  drawAtp();
}
function drawAtp() {
  const el = $('atp'); if (!el || !cur) return;
  const iq = issueQ(cur); // issue mode: Queue/suggestions = GitHub issues; PLAN.md items left = "not filed yet"
  const ag = agentsOf(cur), t = todos(cur), run = ag.filter(a => a.running), dn = S.done?.[cur] || [];
  const q = iq ? iq.map(x => `#${x.number} ${x.title}`) : S.queues?.[cur] || [], pl = iq ? [{file: 'Issues', items: q}, ...(S.plans?.[cur] || []).filter(o => o.items.length)] : S.plans?.[cur] || [];
  const open = t.filter(x => x.status !== 'completed'), fin = t.filter(x => x.status === 'completed');
  const w = workOf(cur), closing = busy[cur] && w && !iq.some(x => x.number === w); // #41 closed but turn still verifying
  if (w && iq?.some(x => x.number === w)) WSEEN.add(w); // #56: seen open → its absence = closed; never seen = just filed, list not refreshed
  const L = a => `<div class="tlist">${a.join('')}</div>`;
  const qRows = L([...(closing ? [tli(`<span class="inum">#${w}</span> ${WSEEN.has(w) ? 'Finishing up' : 'Working on'}…`, {done: WSEEN.has(w), cls: 'working'})] : []),
          ...(busy[cur] && !w && askOf(cur) ? [tli(`<b>Your ask:</b> ${esc(askOf(cur).split(/\s+/).slice(0, 14).join(' '))}`, {cls: 'working', tip: askOf(cur)})] : []), ...q.map((x, i) => { const by = iq && run.find(a => a.issue === iq[i].number), wk = by || busy[cur] && (iq ? iq[i].number === w : !i);
            return tli(iq ? issueHtml(iq[i], wk) : esc(x), {on: `planCheck(${i}, this)`, cls: wk ? 'working' : '', lead: by && `<span class="aface" data-tip="${esc(roleOf({name: by.type}))} works on this">${face(by.type, 16)}</span>`,
            end: '<span class="qpill">' + (iq ? `<span class="qbtns"><button class="ib qedit qpen" title="Edit issue" tabindex="-1">${icon('pen')}</button><button class="ib qedit" title="Start this task now (or next, if Clauracle is busy)" onclick="event.stopPropagation(); playTask(${iq[i].number})">${icon('play')}</button></span>` : '') + `<button class="qdrag" title="Drag to reorder${iq ? '' : ' (writes PLAN.md)'} · right-click: Move to top/bottom"></button></span>`}); })]).replace('class="tlist"', 'class="tlist" id="qlist"');
  const wt = iq ? teamTasks(cur).filter(isIdea).map(x => `#${x.number} ${x.title}`) : S.waits?.[cur] || [];
  const tasks = ((wt.length && !iq ? `${stHead('Suggestions', '', `${iq ? 'GitHub issues labelled suggestion' : 'PLAN.md › Waiting on the user'}: Clauracle noted these; they need your yes`)}` + L(wt.map((x, i) => tli(hl(x), {cls: 'sug',
        end: `<span class="qpill"><span class="qbtns"><button class="ib qedit" title="Accept: move to the Queue" onclick="planWait(${i}, true)">${icon('check')}</button><button class="ib qedit" title="Dismiss: ${iq ? 'close the issue' : 'remove from PLAN.md'}" onclick="planWait(${i}, false)">${icon('x')}</button></span></span>`}))) : '')
    + (t.length ? stHead('Now') + L([...open.map(x => tli(esc(x.status === 'in_progress' && x.activeForm || x.content), {cls: x.status})),
        ...fin.slice(-3).map(x => tli(esc(x.content), {done: 1, cls: 'completed', tip: x.content}))]) : '')
    + (iq || pl.some(o => o.items.length) ? `<div class="sh"><span class="label">${iq ? 'Next' : 'Plan'}</span>
        <button class="ib" title="Where these tasks come from" onclick="menuAt(this, [{title: 'Read from'}, ...(issueQ(cur) ? [{label: 'GitHub issues (clauracle)', hint: issueQ(cur).length + ' open', run: () => expandRight('gh')}] : []), ...S.plans[cur].filter(o => !issueQ(cur) || o.items.length).map(o => ({label: o.file, hint: o.items.length + ' open', run: () => previewFile(o.file)}))])">${icon('info')}</button>
        ${!iq && (pl.length > 1 || pl[0].file !== 'PLAN.md') ? `<button class="ib" title="Merge every plan file into PLAN.md\n${estTok()}" onclick="mergePlans()">${icon('brush')}</button>` : ''}
        ${q.length > 1 ? `<button class="ib" title="Clean up: Clauracle merges related tasks, drops stale ones, re-prioritises (you confirm first)\n${estTok()}" onclick="cleanPlan()">${icon('sparkle')}</button>` : ''}
        ${iq && pl.length > 1 ? `<button class="btn sm" title="Turn ${pl.slice(1).map(o => o.file).join(', ')} into task issues (removed from the files after)\n${estTok()}" onclick="plansToIssues()">${icon('gh')}File as issues</button>`
          : !iq && P().gh && q.length ? `<button class="ib" title="File the queue as GitHub issues (Team tasks)" onclick="queueToTeam(this)">${icon('gh')}</button>` : ''}</div>`
        + qRows
        + (iq ? issuesRest(tab === 'gh' ? thenRow(P()) : '') : '')
        + pl.slice(1).map(o => stHead(`${esc(o.file)}${iq ? ' · not issues yet' : ''}`) + L(o.items.map(x => tli(esc(x))))).join('') : '')
    + (dn.length ? stHead('Done lately') + L(dn.map(x => tli(esc(x), {done: 1, cls: 'completed', tip: x}))) : ''))
    || empty(`Clauracle's task list and ${iq ? 'open task issues' : "PLAN.md's queue"} show here`, `<button class="btn sm" onclick="putPrompt('${iq ? 'Plan this as GitHub task issues: ' : 'Plan this in PLAN.md: '}')">${icon('list')}Plan with Clauracle</button>`);
  const agents = tab === 'agents' ? agentIdle(ag) : '';
  const h = (tab === 'agents' ? agents : iq && tab === 'gh' ? ghSteps(iq, qRows, t, open, fin, dn) : tasks);
  if (qDragging) return; // mid-drag: a rebuild would drop the row
  if (el._h === h) return; // drawChrome → drawRails → here on every keystroke: same HTML → keep DOM (open rows, animations)
  el.innerHTML = el._h = h;
  if ($('qlist')) qSortable($('qlist'));
  tickers(el);
}
// a ticker whose title overflows scrolls in a loop (text + copy, -50%), paused on hover by CSS; one that fits stays still
function tickers(root) {
  for (const t of root.querySelectorAll('.tick')) { const s = t.firstChild;
    if (s.offsetWidth <= t.clientWidth || t.classList.contains('run')) continue;
    s.innerHTML = (s.innerHTML + '<i> &gt;&gt; </i>').repeat(2); t.classList.add('run'); s.style.setProperty('--t', s.offsetWidth / 60 + 's'); } // 30 px/s
}
// sweep: Claude merges every plan file's open tasks into PLAN.md (confirmed first, showing what moves)
async function mergePlans() {
  const pl = S.plans?.[cur] || [], from = pl.filter(o => o.file !== 'PLAN.md' && o.items.length);
  if (!from.length && pl[0]?.file === 'PLAN.md') return;
  if (!await sure(`Merge into PLAN.md?\n\n${from.map(o => `${o.file}: ${o.items.length} task${o.items.length === 1 ? '' : 's'}`).join('\n') || 'Creates PLAN.md'}\n\nClaude moves them into PLAN.md's Queue (by priority, duplicates merged) and removes them from those files.\n${estTok()}`)) return;
  sendNow(cur, {text: `Merge the open tasks of ${from.map(o => o.file).join(', ') || 'HANDOVER.md'} into PLAN.md's "## Queue" (by priority, merge duplicates; things needing the user's yes under "## Waiting on the user"), then remove them from those files (in HANDOVER.md drop its Queue section). Don't start any task.`, atts: []});
}
// #58 issue mode: plan files' open items (PLAN/TASKS/TODO, HANDOVER open-work sections) → issues. AI ∵ HANDOVER items
// are prose (merge dups vs open issues, pick type/suggestion, trim the files); ponytail: deterministic path = queueToTeam
async function plansToIssues() {
  const from = (S.plans?.[cur] || []).filter(o => o.items.length); if (!from.length) return;
  if (!await sure(`File these as GitHub issues?\n\n${from.map(o => `${o.file}: ${o.items.length} item${o.items.length === 1 ? '' : 's'}`).join('\n')}\n\nClaude files each open item as a task issue (skipping ones already filed), then removes them from those files.\n${estTok()}`)) return;
  sendNow(cur, {text: `File the open items of ${from.map(o => o.file).join(', ')} as GitHub issues labelled \`clauracle\` (+ task|feature|fix; needs the user's decision → \`suggestion\`), in their order; skip or merge ones an open issue already covers. Then remove them from those files (HANDOVER.md keeps only state/how-to/gotchas; delete PLAN.md/TASKS.md/TODO.md if left empty). Don't start any task.`, atts: []});
}
// cleanup: Claude proposes a tidier Queue (merge/drop/reorder vs project state) → user confirms in chat before it writes
async function cleanPlan() {
  if (issueQ(cur)) { const n = issueQ(cur).length;
    if (!await sure(`Clean up the task issues?\n\nClaude reviews the ${n} open task issues against the project's current state: merges related ones, closes stale or already-done ones.\n\nIt shows the proposal first and only changes issues after you say yes.\n${estTok()}`)) return;
    return sendNow(cur, {text: 'Clean up the open GitHub issues labelled `clauracle`: check each against the code/git state; propose merges (one issue absorbs the other, the rest closed) and closing stale or already-done ones. Show the proposal with one line per change and why, then STOP: don\'t change any issue or start any task until I confirm.', atts: []}); }
  const n = (S.queues?.[cur] || []).length;
  if (!await sure(`Clean up the plan?\n\nClaude reviews the ${n} queued tasks against the project's current state: merges related ones, drops stale or already-done ones, re-orders by priority.\n\nIt shows the proposed Queue first and only writes PLAN.md after you say yes.\n${estTok()}`)) return;
  sendNow(cur, {text: `Clean up PLAN.md's "## Queue": check each task against the code/git state; merge related tasks, drop stale or already-done ones, re-order by priority (needs-the-user's-yes → "## Waiting on the user"). Show the proposed new Queue plus one line per merge/drop with why, then STOP: don't edit PLAN.md or start any task until I confirm.`, atts: []});
}
// Plan tab: Queue rows reorder by grip via SortableJS (ui/sortable.min.js, MIT 1.15.6); forceFallback ∵ Tauri's file-drop
// handler eats HTML5 DnD. Drop = plan_move rewrites PLAN.md's Queue (blocks whole, renumbered); optimistic S.queues splice
var qDragging = false; // var, not let: drawAtp may run before this line (TDZ)
function qSortable(list) {
  if (!window.Sortable) return; // ponytail: vendored file missing → no reorder, list still renders
  Sortable.create(list, {handle: '.qdrag', draggable: '.ti', forceFallback: true, fallbackOnBody: true, /* #193 clone in body: in-list clone mis-placed (relative .ti/scroll) + stayed as ghost */ animation: 150, ghostClass: 'qghost', scroll: true, scrollSensitivity: 50, scrollSpeed: 12, bubbleScroll: true, /* #216 drag to the box edge -> scrolls */
    onStart: () => { qDragging = true; document.body.classList.add('dragging'); },
    onEnd: ({oldIndex: from, newIndex: to}) => { qDragging = false; document.body.classList.remove('dragging'); qMove(from, to); }});
  list.oncontextmenu = e => { const r = e.target.closest('.ti'), d = r?.querySelector('.qdrag'); if (!d) return; e.preventDefault(); e.stopPropagation(); /* #281 own menu, not the global one */
    const all = [...list.querySelectorAll('.qdrag')], i = all.indexOf(d), n = all.length - 1, num = +(r.closest('[data-n]') || r.querySelector('[data-n]'))?.dataset.n || 0;
    menuAt({getBoundingClientRect: () => ({left: e.clientX, right: e.clientX, top: e.clientY, bottom: e.clientY, height: 0})}, [num && {icon: 'pen', label: 'Edit task', run: () => editIssue(num)}, i && {icon: 'totop', label: 'Move to top', run: () => qMove(i, 0)}, i < n && {icon: 'tobottom', label: 'Move to bottom', run: () => qMove(i, n)}].filter(Boolean)); };
}
// queue row from → to: drag drop + #281 right-click Move to top/bottom; issue mode = P.order, else PLAN.md via plan_move
async function qMove(from, to) {
      if (to === from) return;
      const iq = issueQ(cur); if (iq) { // issue mode: order is local (P.order), no file write
        iq.splice(to, 0, iq.splice(from, 1)[0]); const order = iq.map(x => x.number); P().order = order; $('atp')._h = null; drawAtp();
        return call('update_project', {pid: cur, changes: {order}}).catch(er => addEntry(cur, {kind: 'err', text: 'Reorder failed: ' + er})); }
      const q = S.queues?.[cur]; if (!q) return;
      const text = q[from]; q.splice(to, 0, q.splice(from, 1)[0]); $('atp')._h = null; drawAtp();
      try { await call('plan_move', {pid: cur, from, to, text}); } catch (er) { addEntry(cur, {kind: 'err', text: 'Reorder failed: ' + er}); } refresh();
}
// task text → HTML with the concrete bits in accent: `code`, paths/files (a/b, x.ext), identifiers (camelCase, snake_case, fn())
// ponytail: regex heuristic; misses plain-word names, fine for a confirm
const hl = s => esc(s).replace(/`([^`]+)`|(\b\w+\([^()]{0,30}\)|(?:[\w.~-]*\/)+[\w.*-]*|\b[\w-]{2,}\.[a-z]{1,5}\b|\b[a-z]+[A-Z]\w*\b|\b\w+_\w+\b)/g,
  (m, c, x) => `<code class="hl">${c ?? x}</code>`);
// Plan tab tick on Queue task i → confirm with the task highlighted → plan_move to:null drops it from PLAN.md
async function planCheck(i, box) {
  box.checked = false; const iq = issueQ(cur), t = iq ? iq[i] && `#${iq[i].number} ${iq[i].title}` : S.queues?.[cur]?.[i]; if (t == null) return;
  if (!await sure('Mark this task done?', iq ? 'Done, close the issue' : 'Done, remove it', 'Cancel', `These highlighted items are planned to be worked on:\n\n<span style="color:var(--primary)">${hl(t)}</span>`)) return;
  if (iq) return taskDo('done', iq[i].number);
  S.queues[cur].splice(i, 1); drawAtp();
  try { await call('plan_move', {pid: cur, from: i, to: null, text: t}); } catch (er) { addEntry(cur, {kind: 'err', text: 'Remove failed: ' + er}); } refresh();
}
// Plan tab suggestion (PLAN.md Waiting on the user) → accept = to the Queue's end, dismiss = removed; plain file edit, no AI
// F54 closing a GitHub issue (suggestion / waiting row) asks first
async function dismissIssue(n) { if (await sure(`Close #${n} as not planned?\n\nThe issue is closed on GitHub.`, 'Close issue', 'Cancel', undefined, true)) taskDo('dismiss', n); }
async function planWait(i, accept) {
  if (issueQ(cur)) { const x = teamTasks(cur).filter(isIdea)[i]; return x && (accept ? taskDo('accept', x.number) : dismissIssue(x.number)); }
  const w = S.waits?.[cur]; if (!w?.[i]) return;
  const [t] = w.splice(i, 1); if (accept) (S.queues[cur] ||= []).push(t); drawAtp();
  try { await call('plan_wait', {pid: cur, i, accept}); } catch (er) { addEntry(cur, {kind: 'err', text: (accept ? 'Accept' : 'Dismiss') + ' failed: ' + er}); } refresh();
}
async function queueToTeam(btn) {
  const have = new Set(teamTasks(cur).map(x => x.title.trim().toLowerCase()));
  // full text → task_do splits title/body (store::split_task); dedup: an issue title = the task or its lead
  const todo = (S.queues?.[cur] || []).filter(t => ![...have].some(h => t.toLowerCase().startsWith(h.replace(/…$/, ''))));
  const mv = issueOn(cur); // issue mode: the issues ARE the queue → PLAN.md's items go once all are filed
  if (!todo.length && !mv) return addEntry(cur, {kind: 'note', text: 'Every queued task is already a team task'});
  if (!await sure(todo.length ? `File ${todo.length} task${todo.length === 1 ? '' : 's'} as GitHub issues${mv ? '' : ' (Team tasks)'}, in this order?\n\n${todo.map((t, i) => `${i + 1}. ${t}`).join('\n')}${mv ? '\n\nThen PLAN.md\'s Queue is emptied.' : ''}` : 'Every PLAN.md task is already an issue.\n\nEmpty PLAN.md\'s Queue?')) return;
  btn.disabled = true;
  let n = 0;
  for (const title of todo) { // one by one, so issue numbers follow the queue's order
    try { await call('task_do', {pid: cur, action: 'new', arg: title}); n++; } catch (e) { addEntry(cur, {kind: 'err', text: String(e)}); break; }
  }
  if (mv && n === todo.length) while (S.queues?.[cur]?.length) { const t = S.queues[cur][0];
    try { await call('plan_move', {pid: cur, from: 0, to: null, text: t}); S.queues[cur].shift(); } catch (e) { addEntry(cur, {kind: 'err', text: 'PLAN.md cleanup: ' + e}); break; } }
  addEntry(cur, {kind: 'note', text: `${n} team task${n === 1 ? '' : 's'} filed from the queue`}); await pollTeam(); refresh(); btn.disabled = false;
}
function drawRails() {
  drawAtp();
  const st = ['files', 'notes'].includes(UI.st) ? UI.st : 'projects'; document.querySelectorAll('#stabs button').forEach(b => b.classList.toggle('on', !LC() && b.dataset.s === st)); // #421 every layout() (grip drag/dblclick too)
  if (LC()) {
    const p = P(), pid = cur;
    if (p && !HEAT[pid]) { HEAT[pid] = {t: 0, d: {files: []}}; call('file_heat', {pid}).then(d => { HEAT[pid] = {t: Date.now(), d}; pid === cur && drawRails(); }, () => {}); } // git heat fills the rail past the chat log; t:0 → Graph still rescans
    $('lrail').innerHTML = `<i class="rsep"></i><button class="ib" title="Add a project" onclick="menuAt(this, addItems())">${icon('plus')}</button>`
      + (p ? `<div class="tile on" title="${esc(p.name)} (open)">${initials(p.name)}<span class="dot ${dotOf(p)}"></span></div>`
        + recentFiles().slice(0, 5).map(f => `<button class="ib" title="${esc(f)}" data-p="${esc(f)}" onclick="previewFile(this.dataset.p)">${ftIcon(f)}</button>`).join('') + '<i class="rsep"></i>' : '') // #291 = Code tab's "Recently worked on"; redrawn per tool entry
      + recent().filter(x => x.id !== cur).map(x => `<div class="tile" title="${esc(x.name)}" onclick="openProject('${x.id}')">${initials(x.name)}<span class="dot ${dotOf(x)}"></span>
          ${x.questions.length ? '<span class="pill" style="position:absolute;top:-6px;right:-8px;padding:0 4px;font-size:10px">' + x.questions.length + '</span>' : ''}</div>`).join('')
      + `<span class="grow"></span><button class="ib racct" title="Clauracle menu: search, tabs, settings" onclick="startMenu($('acctBtn'))">${(() => { const a = S.accounts.find(x => x.id === (p?.account || S.default_account)) || S.accounts[0]; return p?.gh ? avGh(p.gh) : a ? avAI(a) : icon('user'); })()}<span class="sdot">${icon('search')}</span></button>`; /* #305 profile pic (project's GitHub, else AI account = drawAcct's pick) + search dot */
    railHide();
  }
}
function railHide() { // #456 rail items under the sticky .racct (incl. its search dot + bar's bottom padding) → hidden + inert; visibility:hidden blocks click/hover/drag
  const r = $('lrail'), a = r?.querySelector('.racct'); if (!a) return; const top = a.getBoundingClientRect().top - 8;
  for (const el of r.children) if (el !== a) el.classList.toggle('under', el.getBoundingClientRect().bottom > top);
}
$('lbar').addEventListener('scroll', railHide, {passive: true}); addEventListener('resize', railHide);
function expandRight(t) { tab = t; UI.rc = false; layout('r'); drawRight(); }
function openFile(abs) {
  const base = P().path.replace(/\\/g, '/').replace(/\/$/, ''), f = abs.replace(/\\/g, '/');
  if (!f.toLowerCase().startsWith(base.toLowerCase() + '/')) return;
  const rel = f.slice(base.length + 1); // #427 files → always the File viewer (was Code tab)
  call('read_file', {pid: cur, rel}).then(text => fileView({name: rel.split('/').pop(), path: f, text, html: docHtml(text, rel, !!UI.ew)}), e => toast(String(e), 'err'));
}
layout();
drawSide();
applyView();
refresh().then(() => { S.active && openProject(S.active); let seen = 1; try { seen = localStorage.onb; } catch {} if (!seen && !S.projects.length) onbOpen(); });
// First run: Clauracle's recommended skills install themselves in the background, once each (removing one later sticks).
// ponytail: the "tried" list lives in localStorage; a failed install (no Node, offline) is tried again on the next start.
const appSkillsInit = async () => { let done; try { done = JSON.parse(localStorage.appSkills || '[]'); } catch { done = []; }
  const todo = APPSKILLS.filter(a => a.auto && !done.includes(a.name)); if (!todo.length) return 'nothing to do';
  SET ||= await call('settings'); SET.catalog = await call('refresh_catalog'); // a stale list could still name a copy that is gone
  for (const a of todo) { if (!appItem(a)) { for (const b of [a.b, ...(a.more || [])]) await call('ext_add', {kind: a.kind, a: a.a, b}); SET.catalog = await call('refresh_catalog'); }
    done.push(a.name); try { localStorage.appSkills = JSON.stringify(done); } catch {} }
  return 'installed: ' + todo.map(a => a.name).join(', '); };
// Installed skills follow their source: one background update a day at most (the skills CLI re-fetches what changed).
const appSkillsUpdate = async () => { if (Date.now() - (+localStorage.skillsUp || 0) < 864e5) return 'checked within a day'; await call('ext_update'); try { localStorage.skillsUp = Date.now(); } catch {} return 'updated'; };
setTimeout(() => appSkillsInit().catch(() => {}).then(() => appSkillsUpdate()).catch(() => {}), 20000); // well after startup: npx may download
/* processes a crashed earlier run left behind: ask, never kill unasked */
call('orphans', {kill: false}).then(o => o?.length && setTimeout(() => sure(`A previous Clauracle run left ${o.length} process${o.length === 1 ? '' : 'es'} running:\n\n${o.map(x => `${x.name} (pid ${x.pid})`).join('\n')}\n\nEnd ${o.length === 1 ? 'it' : 'them'}?`, 'End').then(y => y && call('orphans', {kill: true}).then(() => toast('Ended', 'ok'))), 1500));
// dock grows (input lines, pills, queue) → the log shrinks; if it was at the bottom, glide up by the same amount
let dockH = 0;
new ResizeObserver(() => {
  const h = $('dock').offsetHeight, d = h - dockH, l = $('log'); dockH = h;
  if (d > 0 && l.scrollHeight - l.scrollTop - l.clientHeight - d < 50) l.scrollBy({top: d, behavior: 'smooth'});
}).observe($('dock'));
// #183: due = S.update_every hours (0 = never) since S.update_at (Rust stamps it per check); re-evaluated hourly
const updateDue = () => S.update_every && Date.now() / 1e3 - (S.update_at || 0) >= S.update_every * 3600 && checkUpdate().then(refresh);
setTimeout(updateDue, 5000); setInterval(updateDue, 3600e3);
