// ---- team tasks: open GitHub issues labelled `clauracle`; taken = assignee + `in progress` ----
// ponytail: polled every 60s for the open project (live push needs webhooks and a server)
async function pollTeam() {
  const pid = cur;
  if (!pid || !P(pid).gh) return;
  try { team[pid] = await call('team', {pid}); } catch { return; }
  if (pid !== cur) return;
  drawChrome();
  if ($('tasksBox')) $('tasksBox').innerHTML = tasksHtml();
}
setInterval(pollTeam, 60000);
/* #33 optimistic issues: Clauracle's `gh issue create … clauracle …` adds a "#…" row at once (title/labels from the command),
   its output URL gives the number; `gh issue close N` drops the row at once. Result → pollTeam 3s later replaces all.
   ponytail: regex over the command text; titles in shell variables ($t) show as the variable until the sync */
const ghArg = (s, re) => [...s.matchAll(re)].map(m => m[2] ?? m[3] ?? m[4]);
function issueGuess(pid, d) {
  const cmd = String(d.detail?.command || ''), t = team[pid];
  if (!/\bgh\s+issue\s+(create|close|edit|reopen)\b/.test(cmd) || !Array.isArray(t?.tasks)) return;
  ISSUE_TOOL[d.id] = pid; ISSUE_LINK[d.id] = cmd.split(/ghs+issues+create/).slice(1).map(seg => !/suggestion/.test(seg));
  for (const seg of cmd.split(/\bgh\s+issue\s+create\b/).slice(1)) {
    if (!/clauracle/.test(seg)) continue;
    const title = ghArg(seg, /(?:--title|-t)\s+("([^"]*)"|'([^']*)'|(\S+))/g)[0]; if (!title) continue;
    const labels = ghArg(seg, /(?:--label|-l)\s+("([^"]*)"|'([^']*)'|(\S+))/g).flatMap(l => l.split(',')).map(name => ({name: name.trim(), color: '888'}));
    t.tasks.push({number: 0, title, url: '', labels, assignees: [], pending: d.id});
  }
  for (const m of cmd.matchAll(/\bgh\s+issue\s+close\s+#?(\d+)/g)) { WDONE.add(+m[1]); t.tasks = t.tasks.filter(x => x.number !== +m[1]); (SCRUB[d.id] ||= []).push(m[1]); }
  for (const m of cmd.matchAll(/\bgh\s+issue\s+reopen\s+#?(\d+)/g)) WDONE.delete(+m[1]);
  issueRedraw(pid);
}
const ISSUE_TOOL = {}, ISSUE_LINK = {}, SCRUB = {}; // tool_use id → pid, so its result fills numbers + syncs; SCRUB: id → closed issue numbers
function issueResult(d) {
  const pid = ISSUE_TOOL[d.result_of]; if (!pid) return; delete ISSUE_TOOL[d.result_of];
  // #309: AI closed it → strip the quoted raw prompt (after the command ran ∵ its own edit could re-add it)
  for (const n of SCRUB[d.result_of] || []) call('task_do', {pid, action: 'scrub', arg: n}).catch(() => {}); delete SCRUB[d.result_of];
  const nums = [...String(d.text).matchAll(/\/issues\/(\d+)/g)].map(m => +m[1]);
  (team[pid]?.tasks || []).filter(x => x.pending === d.result_of).forEach((x, i) => { if (nums[i]) x.number = nums[i]; });
  linkAsk(pid, nums.filter((n, i) => ISSUE_LINK[d.result_of]?.[i] !== false)); delete ISSUE_LINK[d.result_of];
  issueRedraw(pid);
  setTimeout(() => pid === cur && pollTeam(), 3000);
}
// the session filed issues itself (Continue triage, a question that was a task after all, a split): tag the user message(s)
// it is answering. Several messages sent together and as many issues → one each, in order; otherwise all on the last one
function linkAsk(pid, nums) {
  const l = logs[pid] || [], isAsk = x => x.kind === 'user' && !x.cont && !/^(Continue: |Carry on with the next task)/.test(x.text);
  let i = l.findLastIndex(x => x.kind === 'user'); if (!nums.length || i < 0 || !isAsk(l[i])) return;
  const run = []; for (; i >= 0 && isAsk(l[i]); i--) run.unshift(l[i]);
  nums.forEach((n, k) => { const e = run.length === nums.length ? run[k] : run.at(-1); if (!(e.tasks ||= []).includes(n)) e.tasks.push(n); });
  run.forEach(e => reBub(e)); saveLog(pid);
}
const issueRedraw = pid => { if (pid !== cur) return; drawAtp(); if ($('tasksBox')) $('tasksBox').innerHTML = tasksHtml(); };
// #226 just-filed issues: gh issue list lags ~30s after create → shown from FRESH until listed (≤3 min); was: ask "not in the list"
const FRESH = {};
const teamTasks = pid => { if (!Array.isArray(team[pid]?.tasks)) return []; const t = team[pid].tasks;
  FRESH[pid] = (FRESH[pid] || []).filter(x => x.local || Date.now() - x.t < 180e3 && !t.some(y => y.number === x.number)); // local = until fileAsk ends
  return [...t, ...FRESH[pid]].filter(x => !WDONE.has(x.number)); }; // #80 closed here → gone now, gh list lags
const isDoing = x => x.labels.some(l => l.name === 'in progress') || x.assignees.length;
// what's next: HANDOVER.md's queue, else free team tasks
// "All tasks done" presets: [button, prompt sent]
// #346 Sweep = full code sweep; one prompt, the AI picks scope (helper agents for the reading)
const NEXT_IDEAS = [['Sweep', 'bug', 'Full code sweep, then fix what you find: 1) bugs: unhandled edge cases, null risks, race conditions, latent logic flaws; 2) dead/stale code: unused variables, orphaned files, deprecated functions, outdated comments/docs; 3) de-slop: split overly complex functions, merge duplicated logic, simplify tangled code; 4) hygiene: formatting, linter/compiler warnings, stale dependencies, small performance bottlenecks. Verify with the build and tests, then summarize what changed.'],
  ['Tests', 'shield', 'Find the riskiest untested code and add one small check for it.'],
  ['Suggest', 'sparkle', 'Suggest the three most useful next improvements for this project, briefly, and ask which to do.']];
const askNext = i => sendNow(cur, {text: NEXT_IDEAS[i][2], atts: []});
/* #37 done bar's first button = the next dev→release step, from cached state only (no calls): open services →
   Services; no build command → set it up; release checks open → Build & Release; else build */
function nextStep(p) {
  if (SVCDOT[cur]) return ['Finish services', 'layers', `rightTab('svc')`];
  if (!p.build?.build) return ['Set up build', 'hammer', `rightTab('build')`];
  const c = RELC[cur]?.c, left = c && !c.err ? relItems(c, p).filter(x => x.s <= Math.max(SK.indexOf(p.stage), RELTO[cur] ?? 0) && !x.ai && !x.ok && x.id !== 'build').length : 0;
  if (left) return [`Release checks (${left})`, 'rocket', `rightTab('build')`];
  return ['Build', 'hammer', `call('run_step', {pid:cur, step:'build'}); rightTab('build')`];
}
// issue mode (project on GitHub + gh account = claude::issue_mode): Queue = open `task` issues minus suggestions and
// others' (same rule as store::task_queue), in P.order (local; unlisted → after, by number). null → PLAN.md mode / not loaded
const issueOn = pid => !!(P(pid)?.owner && P(pid)?.gh);
const isIdea = x => x.labels.some(l => l.name === 'suggestion');
const WAIT_TIP = "Blocked on something only you can do (the issue says what). Do it, then click Done: back into Clauracle's queue."; // #209 (i) tip: hover title, tap → toast
const isWait = x => x.labels.some(l => l.name === 'needs-user'); // blocked on the user: "Waiting on you", not Clauracle queue
/* #41 issue a turn works on, per project; localStorage ∵ app swap mid-turn → resumed turn still finishes it. Set on
   first busy sight (= first open issue), dropped when idle + closed or on clear-continue → "Finishing up #N…" while busy+closed */
const WDONE = new Set(); // #71 issues Clauracle closed this run: hidden even if a gh poll lags (else the new turn re-picked them → "Finishing up #N" forever)
const WORK = (() => { try { return JSON.parse(localStorage.workOn || '{}'); } catch { return {}; } })();
function workOf(pid, drop) {
  const iq = issueQ(pid); if (!iq) return;
  const was = WORK[pid], open = iq.some(x => x.number === was);
  if (drop || !busy[pid] && was && !open) delete WORK[pid];
  else if (busy[pid] && P(pid).work && was !== P(pid).work && iq.some(x => x.number === P(pid).work)) WORK[pid] = P(pid).work; // pinned (urgent ask / ask into an empty queue); a non-urgent filed ask isn't started
  else if (busy[pid] && contOf(pid) && was !== contOf(pid)) WORK[pid] = contOf(pid); // #74: the #N the Continue named, not a guess
  else if (busy[pid] && !was && iq[0]?.number && !askOf(pid)) WORK[pid] = iq[0].number;
  if (was !== WORK[pid]) try { localStorage.workOn = JSON.stringify(WORK); } catch {}
  return WORK[pid];
}
/* direct ask = the turn runs on a user's own prompt, not a queued task → Issues list shows "Working on: <prompt>".
   ponytail: last user entry not like Carry on/continue/#N = direct ask; a Continue message worded otherwise misreads */
const lastUser = pid => (logs[pid] || []).findLast(e => e.kind === 'user')?.text?.trim() || '';
const askOf = pid => { const t = lastUser(pid);
  return t && !/^(carry on|continue)|#\d+/i.test(t) && !t.startsWith('New from the user') ? t : ''; };
// #74 first #N of a Continue message (button "…(GitHub issue): #N" or clear-continue "- #N" list) = the task it hands over
const contOf = pid => { const t = lastUser(pid); return /^(carry on|continue)/i.test(t) ? +(t.match(/#(\d+)/)?.[1] || 0) : 0; };
function issueQ(pid) {
  if (!issueOn(pid) || !team[pid]?.repo) return null;
  const me = P(pid).gh.toLowerCase(), ord = P(pid).order || [], w = P(pid).work, at = n => { const i = ord.indexOf(n); return !n ? 2e9 : n === w ? -1 : i < 0 ? 1e9 + n : i; }; // n 0 = optimistic "#…" row → end; work (in progress) first = store::task_queue
  return teamTasks(pid).filter(x => !isIdea(x) && !isWait(x) && !WDONE.has(x.number) &&(!x.assignees.length || x.assignees.some(a => a.login.toLowerCase() === me))).sort((a, b) => at(a.number) - at(b.number));
}
// Issues list (#15): row = #N link + title + type pills (GitHub colour; `clauracle` hidden ∵ every row has it)
// #187 labels = own line below title (.lbls), never clamped inline; title clamps alone in .tc
// #217 labels = coloured dots under the one-line title (hover = names); row click → editIssue (tc[data-n], delegated below)
// tk: the task is being worked on → title scrolls as a ticker (tickers() after render)
const marq = h => `<span class="tick"><span>${h}</span></span>`;
const issueHtml = (x, tk) => { const t = (x.number ? `<a class="inum" data-url="${esc(x.url)}">#${x.number}</a> ` : '<span class="faint">#…</span> ') + (tk ? marq(esc(x.title)) : esc(x.title)),
  l = x.labels.filter(l => l.name !== 'clauracle'), L = l.map(l => `<i style="--lc:#${esc(l.color || '888')}"></i>`).join('');
  return `<span class="tc"${x.number ? ` data-n="${x.number}"` : ''}>${t}</span>${L ? `<span class="ldots" title="${esc(l.map(l => l.name).join(', '))}">${L}</span>` : ''}`; };
document.addEventListener('click', e => { if (e.target.closest('input, a, button:not(.qpen), .qdrag, dialog')) return;
  const n = e.target.closest('.ti')?.querySelector('.tc[data-n]')?.dataset.n; if (n && $('right')?.contains(e.target)) editIssue(+n); });
// after the drag queue: suggestions (Accept/Dismiss), others' issues (who), new-issue field
// #216 issue-mode pieces (GitHub tab steps + the plain list in issuesRest)
const issueParts = () => { const iq = issueQ(cur);
  return {sug: teamTasks(cur).filter(isIdea).map((x, i) => tli(issueHtml(x), {cls: 'sug',
      end: `<span class="qpill"><span class="qbtns"><button class="ib qedit" title="Accept: queue it" onclick="planWait(${i}, true)">${icon('check')}</button><button class="ib qedit" title="Dismiss: close the issue" onclick="planWait(${i}, false)">${icon('x')}</button></span></span>`})).join(''),
    others: teamTasks(cur).filter(x => !isIdea(x) && !isWait(x) && !iq.includes(x)).map(x => tli(issueHtml(x), {cls: 'in_progress', end: `<span class="pill dim">${esc(x.assignees.map(a => a.login).join(', '))}</span>`})).join(''),
    wait: teamTasks(cur).filter(isWait).map(x => tli(issueHtml(x), {cls: 'sug', end: `<span class="qpill"><span class="qbtns"><button class="ib qedit" title="You did your part: back into Clauracle's queue" onclick="taskDo('unblock', ${x.number})">${icon('check')}</button><button class="ib qedit" title="Delete (close as not planned)" onclick="dismissIssue(${x.number})">${icon('trash')}</button></span></span>`})).join(''), // #313 hover-fade icon btns like the queue rows
    add: `<div class="row"><input id="newTask" class="grow" placeholder="New issue (task)" onkeydown="if(event.key==='Enter')taskDo('new', this.value)"><button class="ib" title="Add issue" onclick="taskDo('new', $('newTask').value)">${icon('plus')}</button></div>`}; };
/* #216 GitHub tab = steps (like service setup): 1 Tasks (scroll box, drag order, edge autoscroll) 2 Run the task list (= Clear &
   Continue p.cont, same setting as the status-bar loop menu; stop reason from Rust's "Paused:"/"Queue is empty" notes) 3 Then. Rest folded. */
function ghSteps(iq, qRows, t, open, fin, dn) {
  const p = P(), x = issueParts(), n = iq[0]?.number, L = a => `<div class="tlist">${a}</div>`;
  const now = [...open.map(y => tli(esc(y.status === 'in_progress' && y.activeForm || y.content), {cls: y.status})), ...fin.slice(-3).map(y => tli(esc(y.content), {done: 1, cls: 'completed', tip: y.content}))].join('')
    + dn.map(y => tli(esc(y), {done: 1, cls: 'completed', tip: y})).join('');
  const nw = teamTasks(cur).filter(isWait).length, ns = teamTasks(cur).filter(isIdea).length;
  return `<ol class="stepper ghsteps tsteps">
    <li><span class="sn">1</span>${stHead('Tasks', (iq.length ? `<span class="pill dim">${iq.length}</span>` : '') + (iq.length > 1 ? `<button class="ib" title="Clean up: Clauracle merges related tasks, drops stale ones, re-prioritises (you confirm first)\n${estTok()}" onclick="cleanPlan()">${icon('sparkle')}</button>` : ''))}
      ${iq.length || busy[cur] ? `<div class="qbox">${qRows}</div>` : `<div class="nextstep" style="margin:4px 0 8px"><p><span class="nw">No tasks! Set up Services?</span><span class="nn">Next step:</span></p><button class="btn sm primary block" onclick="rightTab('svc')"><span class="nw">Next step:&nbsp;</span>Services ${icon('go')}</button></div>`}${x.add}</li>
    ${iq.length || busy[cur] ? `<li><span class="sn">2</span>${stHead('Run the task list')}${runStep(p, n)}</li>
    <li><span class="sn">3</span>${thenRow(p)}</li>` : ''}</ol>`
    + (nw ? fold('w-wait', `Waiting on you${infoI(WAIT_TIP)}`, L(x.wait), nw) : '')
    + (ns ? fold('w-sug', 'Suggestions', L(x.sug), ns) : '')
    + (x.others ? fold('w-others', 'Taken by others', L(x.others)) : '')
    + (now ? fold('w-act', 'Activity', L(now)) : '');
}
function runStep(p, n) {
  const w = workOf(cur), q = p.questions.length;
  const last = (logs[cur] || []).findLast(e => e.kind === 'user' || e.kind === 'note' && /^(Paused:|Queue is empty)/.test(e.text));
  const why = busy[cur] ? (w ? `Working on #${w}` : 'Working…') + (p.cont.startsWith('auto') ? ' · keeps going until a limit, a question or an empty list' : p.cont === 'manual' ? ' · then waits for you' : '')
    : q ? `Paused: ${q} question${q > 1 ? 's' : ''} for you` : p.cont !== 'off' && last?.kind === 'note' ? esc(last.text)
    : !n ? 'Nothing queued' : p.cont === 'off' ? 'Tasks run only when you ask' : `Next: #${n}`;
  const act = q && !busy[cur] ? `<button class="btn sm" onclick="hiddenQs[cur] = false; qsOpen[cur] = true; drawLog()">Answer</button>`
    : '';
  return `<select class="qw" title="${esc(MODES[p.cont]?.[1] || '')}" onchange="setP({cont: this.value})">${Object.entries(MODES).map(([k, [l, h]]) => `<option value="${k}" title="${esc(h)}"${p.cont === k ? ' selected' : ''}>${l}</option>`).join('')}</select>
    <div class="row" style="gap:8px;margin-top:8px;align-items:center"><span class="grow faint" style="font-size:12.5px">${why}</span>${act}</div>`;
}
const issuesRest = (then = '') => { const x = issueParts();
  return `<div class="tlist">${x.sug}${x.others}</div>${then}${x.wait ? `${stHead(`Waiting on you${infoI(WAIT_TIP)}`)}<div class="tlist">${x.wait}</div>` : ''}${x.add}`; };
const nextUp = pid => { const iq = issueQ(pid); if (iq) return iq.map(x => `#${x.number} ${x.title}`); const q = S.queues?.[pid] || []; return q.length ? q : teamTasks(pid).filter(x => !isDoing(x)).map(x => `#${x.number} ${x.title}`); };
function tasksHtml() {
  const t = team[cur];
  if (!t?.repo) return '';
  const warns = (t.conflicts || []).map(c => `<div class="warn">@${esc(c.author)}'s PR <a data-url="${esc(c.url)}">#${c.number}</a> also changes ${c.files.map(f => `<code>${esc(f)}</code>`).join(', ')}</div>`).join('');
  if (issueOn(cur)) return warns; // issue mode: the Issues list (drawAtp) holds them all
  const me = (t.me || '').toLowerCase();
  // one row per task: title on the left, who / what you can do on the right
  const row = x => { const who = x.assignees.map(a => a.login), mine = who.some(l => l.toLowerCase() === me);
    return tli(`<a data-url="${esc(x.url)}">#${x.number}</a> ${esc(x.title)}`, {cls: isDoing(x) ? 'in_progress' : '', end:
      mine ? `<button class="btn ghost sm" onclick="taskDo('release', ${x.number})">Release</button><button class="btn sm" onclick="taskDo('done', ${x.number})">Done</button>`
        : isDoing(x) ? `<span class="pill">${esc(who.join(', ') || 'in progress')}</span>` : `<button class="btn sm" onclick="taskStart(${x.number})">Start</button>`}); };
  const ts = teamTasks(cur), doing = ts.filter(isDoing).length;
  return fold('tasks', 'Team tasks', warns
    +(ts.length ? `<div class="tlist">${ts.map(row).join('')}</div>` : empty('No open tasks'))
    + `<div class="row"><input id="newTask" class="grow" placeholder="New task for the team" onkeydown="if(event.key==='Enter')taskDo('new', this.value)"><button class="ib" title="Add task" onclick="taskDo('new', $('newTask').value)">${icon('plus')}</button></div>`,
    ts.length ? `${doing}/${ts.length}` : (t.conflicts || []).length);
}
// #112 edit modal: title/body/labels/assignees → task_do edit (gh issue edit, diff only). IMETA = repo labels + assignable
// people per pid, 10 min ∵ rarely change; issue itself re-fetched per open (edit must start from fresh text)
// #217 IVIEW = last fetched issue per pid+n in localStorage (≤150) → opens at once, fresh copy re-renders it if nothing was typed yet
const IMETA = {}, IVIEW = (() => { try { return JSON.parse(localStorage.iview || '{}'); } catch { return {}; } })();
const ivSet = (k, x) => { delete IVIEW[k]; if (x) IVIEW[k] = x; const ks = Object.keys(IVIEW); ks.slice(0, -150).forEach(y => delete IVIEW[y]); try { localStorage.iview = JSON.stringify(IVIEW); } catch {} };
async function editIssue(n) {
  const pid = cur, k = pid + '#' + n, c = IVIEW[k], m = IMETA[pid]?.t > Date.now() - 6e5 ? IMETA[pid] : null;
  const get = Promise.all([call('task_do', {pid, action: 'view', arg: String(n)}).then(JSON.parse), m || call('task_do', {pid, action: 'meta', arg: ''}).then(JSON.parse)]);
  if (c && m) { eiDraw(pid, n, c, m); get.then(([x, meta]) => { ivSet(k, x); IMETA[pid] = {...meta, t: m.t};
      if ($('eiGo') && !$('eiGo').disabled && $('dlg').dataset.ei === k && !$('dlg')._typed && JSON.stringify(x) !== JSON.stringify(c)) eiDraw(pid, n, x, IMETA[pid]); }).catch(() => {}); return; }
  dlg(`<h2>Issue #${n}</h2>${skel(60, 'b100')}`);
  let x, meta;
  try { [x, meta] = await get; }
  catch (e) { return dlg(`<h2>Issue #${n}</h2><div class="warn">${esc(e)}</div><div class="btns"><button class="btn ghost" onclick="closeDlg()">Close</button></div>`); }
  IMETA[pid] = {...meta, t: m?.t || Date.now()}; ivSet(k, x);
  if (!$('dlg').open) return; // closed while loading
  eiDraw(pid, n, x, IMETA[pid]);
}
function eiDraw(pid, n, x, meta) {
  const L = new Set(x.labels.map(l => l.name)), A = new Set(x.assignees.map(a => a.login)), labels = meta.labels || [], people = [...new Set([...(meta.people || []), ...A])];
  // #222 assignees = same .lbl shape as labels (accent) so both rows line up; Invite = dashed action, never dimmed
  const chip = (k, v, on, c) => `<span class="pill lbl${on ? ' on' : ''}" data-k="${k}" data-v="${esc(v)}" style="--lc:${c ? '#' + esc(c) : 'var(--accent)'}" onclick="this.classList.toggle('on')">${esc(v)}</span>`;
  dlg(`<h2>Issue #${n}</h2>
    <label class="fl">Title<input id="eiT" value="${esc(x.title)}"></label>
    <label class="fl" style="margin-top:8px">Description<textarea id="eiB" rows="8">${esc(x.body)}</textarea></label>
    <div class="fl" style="margin-top:8px">Labels<div class="chips">${labels.map(l => chip('L', l.name, L.has(l.name), l.color || '888')).join('') || '<span class="faint">No labels in this repo</span>'}</div></div>
    <div class="fl" style="margin-top:8px">Assignees<div class="chips">${people.map(p => chip('A', p, A.has(p))).join('') }<span class="pill lbl add" style="--lc:var(--accent)" title="Invite a collaborator (GitHub tab)" onclick="collabGo()">${icon('plus')}Invite…</span></div></div>
    <div class="btns"><button class="btn ghost" onclick="closeDlg()">Cancel</button><button class="btn primary" id="eiGo">Save</button></div>`);
  $('eiGo').onclick = async () => {
    const on = k => new Set([...$('dlg').querySelectorAll(`.chips .pill.on[data-k="${k}"]`)].map(e => e.dataset.v)), nl = on('L'), na = on('A');
    const diff = (was, now) => [[...now].filter(v => !was.has(v)), [...was].filter(v => !now.has(v))], [addL, rmL] = diff(L, nl), [addA, rmA] = diff(A, na);
    $('eiGo').disabled = true;
    try { await call('task_do', {pid, action: 'edit', arg: JSON.stringify({n, title: $('eiT').value.trim() || x.title, body: $('eiB').value, addL, rmL, addA, rmA})}); }
    catch (e) { $('eiGo').disabled = false; return $('eiGo').closest('.btns').insertAdjacentHTML('beforebegin', `<div class="warn">${esc(e)}</div>`); }
    ivSet(pid + '#' + n, null); closeDlg(); await pollTeam(); $('atp') && ($('atp')._h = null); drawAtp();
  };
  const d = $('dlg'); d.dataset.ei = pid + '#' + n; d._typed = false; d.querySelectorAll('input, textarea').forEach(i => i.oninput = () => d._typed = true);
  $('eiT').focus();
}
async function taskDo(action, arg) {
  if (!String(arg).trim()) return;
  try { await call('task_do', {pid: cur, action, arg: String(arg)}); if (/^(done|dismiss)$/.test(action)) WDONE.add(+arg); } catch (e) { $('tasksBox') ? $('tasksBox').insertAdjacentHTML('afterbegin', `<div class="warn">${esc(e)}</div>`) : addEntry(cur, {kind: 'err', text: String(e)}); return; }
  await pollTeam(); drawAtp();
}
// claim it on GitHub (others see it's taken), then hand it to Claude
/* pre-fill the composer: appended below what's typed (was replaced = typed text lost), saved at once */
function putPrompt(t) { const p = $('prompt'); p.value = p.value.trim() ? p.value.replace(/\s*$/, '') + '\n\n' + t : t; grow(); if (cur) { keepDraft(cur, true); drawChrome(); } p.focus(); }
// #191 ▶: this issue becomes the next one (front of the order, pinned as work); idle → Clauracle starts it now
function playTask(n) {
  const pid = cur, p = P(pid), order = [n, ...(p.order || []).filter(x => x !== n)];
  p.order = order; p.work = n; call('update_project', {pid, changes: {order, work: n}}).catch(() => {}); drawRight();
  if (busy[pid]) return toast(`#${n} is next, after the current task`, 'info');
  contGo(pid);
}
async function taskStart(n) {
  const pid = cur, x = teamTasks(cur).find(t => t.number === n);
  await taskDo('claim', n);
  if (pid !== cur) return; // switched project meanwhile
  putPrompt(`Work on issue #${n} "${x?.title}" (${x?.url}): read it with gh, do it on a new branch, and close the issue when it's done.`);
}

// local-only project → its own GitHub repo; defaults to the project's (or its AI account's) GitHub account
function publishForm(p, users) {
  if (!users.length) return `<p class="hint">Only on this computer. Add a GitHub account to publish it.</p><button class="ib" title="Add a GitHub account" onclick="addGh()">${icon('plus')}</button>`;
  const def = p.gh || S.accounts.find(a => a.id === p.account)?.gh || users[0];
  return `<p class="hint">Only on this computer so far. Publishing git-ignores .env files first.</p>
    ${field('Account', `<select id="pubUser">${users.map(u => `<option ${u === def ? 'selected' : ''}>${esc(u)}</option>`).join('')}</select>`)}
    ${field('Name', `<input id="pubName" value="${esc(p.name)}" placeholder="name or org/name">`)}
    <label class="chk"><input type="checkbox" id="pubPrivate" checked> Private repository</label>
    <button class="btn primary block" onclick="publish(this)">${icon('gh')}Publish to GitHub</button><p id="pubMsg" class="dim" style="margin:0;font-size:13px"></p>`;
}
async function publish(btn) {
  btn.disabled = true; $('pubMsg').textContent = 'Publishing…';
  try {
    await call('publish_repo', {pid: cur, user: $('pubUser').value, name: $('pubName').value.trim(), private: $('pubPrivate').checked});
    await refresh(); drawRight(true);
  } catch (e) { $('pubMsg').textContent = e; btn.disabled = false; }
}
function draft(kind, x) {
  putPrompt(kind === 'pr' ? `Review PR #${x.number} "${x.title}" (${x.url}): read the diff and comments with gh, then list risks and suggested changes.`
    : `Work on issue #${x.number} "${x.title}" (${x.url}): read it with gh, then fix it on a new branch.`);
}

// files: the tree lives in the left panel (Files tab); a picked file previews in the right panel's Code tab
const codeAt = {}, filesAt = {};
// path bar: back (history per project) + home, then the crumbs in a field that keeps its right end in view
const filesHist = {};
const filesGo = rel => { if (rel !== (filesAt[cur] ?? '')) (filesHist[cur] ||= []).push(filesAt[cur] ?? ''); drawFiles(rel); };
const filesBack = () => { const h = filesHist[cur]; if (h?.length) drawFiles(h.pop()); else if (!filesAt[cur]) setSide('projects'); }; // #401 Back at home → Projects tab
async function drawFiles(rel = filesAt[cur] ?? '') {
  const box = $('files');
  if (!cur) { box.innerHTML = empty('No project open', startProj); return; }
  filesAt[cur] = rel;
  const parts = rel ? rel.split('/') : [], items = fsort(await call('list_dir', {pid: cur, rel}).catch(() => []));
  box.innerHTML = `<div class="pathbar"><button class="ib" title="${filesHist[cur]?.length || rel ? 'Back' : 'Back to Projects'}" ${filesHist[cur]?.length || !rel ? '' : 'disabled'} onclick="filesBack()">${icon('back')}</button>
      <button class="ib" title="Project root" onclick="filesGo('')">${icon('home')}</button>
      <div class="crumbs mono"><a data-p="" onclick="filesGo(this.dataset.p)">${esc(P().name)}</a>${parts.map((x, i) =>
      `<span class="faint">/</span><a data-p="${esc(parts.slice(0, i + 1).join('/'))}" onclick="filesGo(this.dataset.p)">${esc(x)}</a>`).join('')}</div></div>
    <div class="tree${UI.fv === 'icons' ? ' icons' : ''}">${items.map(i => { const path = (rel ? rel + '/' : '') + i.name;
      return `<div class="${codeAt[cur]?.file === path ? 'on' : ''}" data-p="${esc(path)}" onclick="${i.dir ? 'filesGo' : 'previewFile'}(this.dataset.p, event.ctrlKey || event.metaKey)" title="${esc(fTip(i))}">${i.dir ? icon('folder') : ftIcon(i.name)}<span class="fn">${esc(i.name)}</span>${fDet(i)}</div>`; }).join('')
      || empty('Empty folder', `<button class="btn sm" onclick="setView('edit'); evNew()">${icon('plus')}New file</button>`)}</div>`;
  const c = box.querySelector('.pathbar .crumbs'); c.scrollLeft = c.scrollWidth;
  badgeFiles();
}
/* #377 sidebar '…' = menu (rule: '…' opens a menu, gear opens Settings). UI.fs sort name|type|date|size, UI.fv list|icons;
   dirs always first; Rust list_dir gives name order + t (mtime s) + size */
const FSORT = [['name', 'Name'], ['type', 'Type'], ['date', 'Date modified'], ['size', 'Size']];
const fsort = xs => { const k = UI.fs, ext = n => n.includes('.') ? n.split('.').pop().toLowerCase() : '';
  return k && k !== 'name' ? [...xs].sort((a, b) => b.dir - a.dir || (k === 'type' ? ext(a.name).localeCompare(ext(b.name)) : k === 'date' ? b.t - a.t : b.size - a.size)) : xs; }; // stable ∴ ties keep name order
// #396 row tooltip = size + mtime; faint right detail follows the sort (name → none)
const fDate = t => t ? new Date(t * 1000).toLocaleString([], {dateStyle: 'medium', timeStyle: 'short'}) : '';
const fTip = i => [i.name, i.dir ? '' : mb(i.size || 0), i.t ? 'Modified ' + fDate(i.t) : '', i.dir || view !== 'edit' ? '' : 'Ctrl+click: open in the Code tab on the side'].filter(Boolean).join('\n');
const fDet = i => { const k = UI.fs, n = i.name, d = k === 'type' ? (i.dir ? 'folder' : n.lastIndexOf('.') >= 0 ? n.slice(n.lastIndexOf('.')).toLowerCase() : 'file')
  : k === 'size' ? (i.dir ? '' : mb(i.size || 0)) : k === 'date' && i.t ? new Date(i.t * 1000).toLocaleDateString() : '';
  return d && UI.fv !== 'icons' ? `<span class="fd">${esc(d)}</span>` : ''; };
const fset = (k, v) => { UI[k] = v; layout(); if (UI.st === 'files') drawFiles(); };
const sideItems = () => [{icon: 'folder', label: 'Project manager…', run: () => openManager()}, '-', {title: 'Sort Files by'},
  ...FSORT.map(([k, l]) => ({label: l, check: (UI.fs || 'name') === k, run: () => fset('fs', k)})), '-', {title: 'View'},
  {label: 'List', check: UI.fv !== 'icons', run: () => fset('fv', 'list')}, {label: 'Icons', check: UI.fv === 'icons', run: () => fset('fv', 'icons')}];
const sideMenu = b => menuAt(b, sideItems()); // #404 also = Files empty-space right-click
// Files tree badges: files Claude edited (live, this app run). < 1 min: folders show how many files inside, files a dot;
// 1–2 min: a plain dot that fades out; then gone. EDITS[pid][rel path] = ms of the last edit
const EDITS = {}, EDIT_MS = 60000;
const relOf = (pid, f) => { const r = (P(pid)?.path || '').replace(/[\\/]+$/, '').replace(/\\/g, '/'); f = f.replace(/\\/g, '/');
  return r && f.toLowerCase().startsWith(r.toLowerCase() + '/') ? f.slice(r.length + 1) : f; };
function badgeFiles() {
  clearTimeout(badgeFiles.t);
  const ed = Object.entries(EDITS[cur] || {}), now = Date.now();
  let live = false;
  document.querySelectorAll('#files .tree > div[data-p]').forEach(r => {
    const p = r.dataset.p, ages = ed.filter(([f]) => f === p || f.startsWith(p + '/')).map(([, t]) => now - t).filter(a => a < 2 * EDIT_MS);
    const n = ages.filter(a => a < EDIT_MS).length, isDir = !ed.some(([f]) => f === p);
    let b = r.querySelector('.eb');
    if (!ages.length) return b?.remove();
    live = true; b ||= r.appendChild(document.createElement('span')); b.className = 'eb';
    b.textContent = n && isDir ? n : ''; b.style.opacity = n ? 1 : Math.max(0, 2 - Math.min(...ages) / EDIT_MS); // linear fade over min 2 b.title = n ? `${n} file${n > 1 ? 's' : ''} edited in the last minute` : 'Edited lately';
  });
  if (live) badgeFiles.t = setTimeout(badgeFiles, 5000);
}
let envAt = null;
function previewFile(rel, side) { // side: Ctrl+click in Edit view → Code tab as split view (#369)
  if (/\.pdf$/i.test(rel)) return openPdf(P().path.replace(/[\\/]+$/, '') + '/' + rel); // #247 File viewer
  if (view === 'edit') { if (!side) return evOpen(rel); } // #285/#376 Edit view: plain click = editor tab (.env too), Ctrl = Code tab
  else if (/^\.env(\.|$)/.test(rel.split('/').pop())) { envAt = rel; expandRight('svc'); return; } // outside Edit: secrets panel
  codeAt[cur] = {file: rel}; expandRight('code'); drawFiles();
}
/* #248 syntax colour, regex per line (no parser, no dep): block comments carry across lines; # comments only in hash langs.
   ponytail: one keyword set for all langs; > 2 MB → plain. 5k lines ≈ a few ms */
const HKW = new Set('if else for while do return function const let var class new this import export from as async await try catch finally throw switch case break continue default typeof instanceof in of fn pub mut impl struct enum use mod match self Self where trait loop def elif lambda None True False null undefined true false and or not with yield static void type interface extends implements package func defer crate super move ref dyn unsafe extern'.split(' '));
/* #338 Markdown: fences (whole block = string colour), headings, list/quote markers, inline `code`, **bold**, [links](url) */
function hiMd(lines) { let fence = false;
  const inl = l => esc(l).replace(/(`+)(.+?)\1|\*\*(.+?)\*\*|(\[[^\]]*\]\([^)]*\))/g, (m, b, c, bo, ln) => b ? `<i class="hs">${m}</i>` : bo ? `<i class="hb">${m}</i>` : `<i class="ht">${m}</i>`);
  return lines.map(l => { if (/^\s*(```|~~~)/.test(l)) { fence = !fence; return `<i class="hc">${esc(l)}</i>`; }
    if (fence) return `<i class="hs">${esc(l)}</i>`;
    if (/^#{1,6}\s/.test(l)) return `<i class="hk hb">${esc(l)}</i>`;
    const m = l.match(/^(\s*(?:[-*+]|\d+[.)]|>)\s)(.*)$/); return m ? `<i class="hn">${esc(m[1])}</i>${inl(m[2])}` : inl(l); }); }
function hiCode(text, ext) {
  const lines = text.split('\n');
  if (text.length > 2e6 || /^(txt|log|csv)$/.test(ext)) return lines.map(esc);
  if (/^(md|markdown|mdx)$/.test(ext)) return hiMd(lines);
  const hash = /^(py|sh|bash|zsh|ps1|toml|ya?ml|rb|r|conf|ini|env|gitignore|dockerfile|mk)$/.test(ext), ml = /^(html?|xml|svg|vue)$/.test(ext);
  const re = new RegExp(String.raw`(/\*[\s\S]*?(?:\*/|$)${ml ? '|<!--[^]*?(?:-->|$)' : ''})|(${hash ? '#' : '//'}.*)|("(?:\\.|[^"\\])*"?|'(?:\\.|[^'\\])*'?|\`(?:\\.|[^\`\\])*\`?)|(\b\d[\w.]*)|(</?[A-Za-z][\w-]*|/?>)|([A-Za-z_$][\w$]*)`, 'g');
  let blk = null; // open block comment end token
  return lines.map(l => { let o = '', i = 0;
    if (blk) { const k = l.indexOf(blk); if (k < 0) return `<i class="hc">${esc(l)}</i>`; o = `<i class="hc">${esc(l.slice(0, k + blk.length))}</i>`; i = k + blk.length; blk = null; }
    re.lastIndex = i; let m;
    while ((m = re.exec(l))) { o += esc(l.slice(i, m.index)); const t = m[0], c = m[1] || m[2] ? 'hc' : m[3] ? 'hs' : m[4] ? 'hn' : m[5] ? (ml ? 'ht' : '') : HKW.has(t) ? 'hk' : '';
      if (m[1] && !/(\*\/|-->)$/.test(t)) blk = t.startsWith('<!--') ? '-->' : '*/';
      o += c ? `<i class="${c}">${esc(t)}</i>` : esc(t); i = re.lastIndex; if (!t) re.lastIndex++; }
    return o + esc(l.slice(i)); });
}
// #338 colour toggle (left of Wrap): .code.plain = CSS-only, no re-render; remembered in localStorage.codeHl
const hlOn = () => localStorage.codeHl !== '0';
const hlBtn = (st = '', off = false) => `<button class="ib hlb${hlOn() ? ' on' : ''}" style="${st}"${off ? ' disabled' : ''} title="Syntax highlighting${off ? ' (not used in the formatted view)' : ''}" onclick="hlSet()">${icon('code')}</button>`;
function hlSet() { const p = hlOn(); try { localStorage.codeHl = p ? '0' : '1'; } catch {} // #392 one switch for Edit view, Code tab, File viewer
  document.querySelectorAll('#ebody .code, #rbody .code, .vtext .code').forEach(c => c.classList.toggle('plain', p)); document.querySelectorAll('.hlb').forEach(b => b.classList.toggle('on', !p)); }
// #371 .md in Code tab / File viewer: rendered (mdDoc) by default, toggle remembered in localStorage.mdRaw
// #392 formatted view per file type, shared by Edit view / Code tab / File viewer: localStorage mdRaw | jsonRaw = '1' → off
const isMd = f => /\.(md|markdown|mdx)$/i.test(f), fmtKind = f => isMd(f) ? 'md' : /\.json$/i.test(f) ? 'json' : '';
const fmtOn = f => { const k = fmtKind(f); return !!k && localStorage[k + 'Raw'] !== '1'; }, mdOn = () => fmtOn('.md');
const fmtFlip = f => { try { localStorage[fmtKind(f) + 'Raw'] = fmtOn(f) ? '1' : '0'; } catch {} };
const docHtml = (text, file, wrap) => { if (fmtOn(file)) { if (isMd(file)) return `<div class="mdoc" style="padding:2px 4px">${mdDoc(text)}</div>`; try { text = JSON.stringify(JSON.parse(text), null, 2); } catch {} }
  return codeHtml(text, file.split('.').pop().toLowerCase(), wrap); };
// #410 --lw = fixed CSS on .code: 4ch digits (≤9999 lines) + 14px pad/margin + 1px border + 15px lint icon ∴ gutter never jumps (lint, formatted toggle, file length)
const codeHtml = (text, ext, wrap) => { const h = hiCode(text, ext); return `<div class="code${wrap ? ' wrap' : ''}${hlOn() ? '' : ' plain'}"><pre class="sel">${h.map(l => `<div><span>${l}</span></div>`).join('')}</pre></div>`; };
// #248 recent files: newest first from this project's chat log (tool file_path) then git heat; ≤ 8, inside the project only
function recentFiles() {
  const out = [], add = f => { if (!f) return; const r = relOf(cur, f); if (r && !/^[a-z]:|^\//i.test(r) && !out.includes(r)) out.push(r); };
  for (let k = (logs[cur] || []).length - 1; k >= 0 && out.length < 8; k--) add(logs[cur][k].detail?.file_path);
  (HEAT[cur]?.d.files || []).forEach(f => out.length < 8 && add(f[0]));
  return out;
}
async function drawCode() {
  const file = codeAt[cur]?.file, box = $('rbody');
  if (!file) { const r = recentFiles();
    box.innerHTML = r.length ? `<p class="faint" style="margin:0 0 6px 8px;font-size:12px">Recently worked on</p><div class="tree">${r.map(f => `<div data-p="${esc(f)}" title="${esc(f)}" onclick="previewFile(this.dataset.p, true)">${ftIcon(f)}${esc(f.split('/').pop())}<small class="faint">${esc(f.split('/').slice(0, -1).join('/'))}</small></div>`).join('')}</div>`
      : view === 'edit' ? empty('Ctrl+click a file on the left to open it here, next to the editor.', `<button class="btn sm" onclick="evNew()">${icon('plus')}New file</button>`)
      : empty('No file open', `<button class="btn sm" onclick="setSide('files')">${icon('folder')}Show files</button>`);
    if (UI.lc || UI.st !== 'files') { UI.lc = false; setSide('files'); } return; }
  const wrap = !!UI.ew, ext = file.split('.').pop().toLowerCase(), img = /\.(png|jpe?g|gif|webp|bmp)$/i.test(file), fm = fmtOn(file), pv = fmtKind(file); // #392 same toggles + state as the Edit view bar (evSet redraws both)
  /* #443 light File viewer: plain name header (tab-label layout, no tab look) + .vctx context bar (= evBar layout: mode name, toggles, zoom right) + .cvs scroller */
  const head = `<div class="cview"><div class="cvh" title="${esc(file)}">${ftIcon(file)}<span class="vname">${esc(file.split('/').pop())}</span><button class="ib x" title="Close" onclick="codeClose()">${icon('x')}</button><span class="grow"></span>${img ? '' : `<button class="ib" title="Open in file viewer" onclick="codeView()">${icon('expand')}</button>`}</div>
    <div class="vctx">${pv ? `<button class="ib${fm ? ' on' : ''}" title="Formatted view" onclick="fmtFlip(codeAt[cur].file); evSet()">${icon('eye')}</button>` : ''}${hlBtn('', fm && isMd(file))}${LINTX.test(ext) ? `<button class="ib${UI.nl ? '' : ' on'}" title="Lint: errors/warnings in the gutter" onclick="evSet('nl', !UI.nl)">${icon('bug')}</button>` : ''}<button class="ib${wrap ? ' on' : ''}" title="Word wrap" onclick="evSet('ew', !UI.ew)">${icon('wrap')}</button>
    <span class="vur">${zoomCtl('Math.round((UI.cz || 1) * 100)', 'czp(p)', ', Ctrl+wheel')}</span></div>`;
  if (img) { // #446 image = read-only view: name row + image, no context/tool bar; Ctrl+wheel zoom (UI.cz)
    const b = await call('read_image', {path: P().path.replace(/[\/\\]+$/, '') + '/' + file}).catch(e => e); if (codeAt[cur]?.file !== file) return;
    if (typeof b === 'string') { box.innerHTML = head.replace(/<div class="vctx">[^]*$/, '') + `<p class="dim" style="padding:12px 14px">${esc(b)}</p></div>`; return; }
    URL.revokeObjectURL(drawCode.u || ''); drawCode.u = URL.createObjectURL(new Blob([b]));
    box.innerHTML = head.replace(/<div class="vctx">[^]*$/, '') + `<div class="cvs cimg" style="zoom:${UI.cz || 1}"><img src="${drawCode.u}" alt="${esc(file)}"></div></div>`;
    box.querySelector('.cvs').onwheel = e => { if (!e.ctrlKey) return; e.preventDefault(); e.stopPropagation(); czp(zStep(Math.round((UI.cz || 1) * 100), e.deltaY < 0 ? 1 : -1)); }; return; }
  let text;
  try { text = await call('read_file', {pid: cur, rel: file}); } catch (e) { box.innerHTML = head + `<p class="dim" style="padding:12px 14px">${esc(e)}</p></div>`; return; }
  codeAt[cur].text = text;
  box.innerHTML = head + `<div class="cvs" style="zoom:${UI.cz || 1}">${docHtml(text, file, wrap)}</div></div>`;
  box.querySelector('.cvs').onwheel = e => { if (!e.ctrlKey) return; e.preventDefault(); e.stopPropagation(); czp(zStep(Math.round((UI.cz || 1) * 100), e.deltaY < 0 ? 1 : -1)); };
  if (UI.nl || fm || !LINTX.test(ext)) return; const r = await lintOf(file, text), rows = box.querySelector('.code pre')?.children; // #392 lint marks, message = row tip
  if (codeAt[cur]?.file === file && rows) for (const x of r) { const d = rows[x.l - 1]; if (d) { d.classList.add(x.s === 'e' ? 'le' : 'lw'); d.dataset.tip = (d.dataset.tip ? d.dataset.tip + '\n' : '') + (x.s === 'e' ? 'Error: ' : 'Warning: ') + x.m; } }
}
// #443 Code tab zoom (own UI.cz, 50–300%): restyle only, no re-read
function czp(p) { UI.cz = Math.min(300, Math.max(50, p)) / 100; try { localStorage.ui = JSON.stringify(UI); } catch {} const s = $('rbody').querySelector('.cvs'), z = $('rbody').querySelector('.zctl .zp');
  if (s) s.style.zoom = UI.cz; if (z) z.textContent = Math.round(UI.cz * 100) + '%'; }
// #458 header X: close the Code tab file → recent list
function codeClose() { delete codeAt[cur]; drawCode(); drawFiles(); }
// #248 Code tab file → File viewer (same colouring; Save as… copies the file)
function codeView() { const c = codeAt[cur]; if (!c?.text) return;
  fileView({name: c.file.split('/').pop(), path: P().path.replace(/[\/]+$/, '') + '/' + c.file, text: c.text, html: docHtml(c.text, c.file, !!UI.ew)}); }
function setSide(t) { UI.st = t; layout('l'); drawSide(); }
function toggleLeft() { UI.lc = !UI.lc; layout('l'); }
// #421 icon bar click: active icon of an open panel → collapse, else open on it
function barL(s) { if (!LC() && s === UI.st) return toggleLeft(); UI.lc = false; setSide(s); }
function barR(t) { if (!RC() && t === tab) return toggleRight(); UI.tab = t; expandRight(t); }
function drawSide() {
  const t = ['files', 'notes'].includes(UI.st) ? UI.st : 'projects'; // old saved 'tasks'/'graph' tab (removed; #370 Hot → GitHub tab fold) → projects
  document.querySelectorAll('#stabs button').forEach(b => b.classList.toggle('on', !LC() && b.dataset.s === t));
  tabHead($('lhead'), document.querySelector(`#stabs [data-s="${t}"] span`).textContent, 'sideMenu(this)',
    {projects: ['Add a project', 'menuAt(this, addItems())'], files: cur && ['New file', "setView('edit'); evNew()"]}[t]);
  $('projects').hidden = t !== 'projects'; $('files').hidden = t !== 'files'; $('notes').hidden = t !== 'notes';
  if (t === 'files') drawFiles();
  if (t === 'notes') drawNotes();
}
/* #371 Notes tab: <project>/NOTES.md via lib notes (git-excluded on first save), edited as rich text (mdDoc in, htmlMd out).
   Autosave 0.8s per project. NOTETXT[pid] = text, read once per session (ponytail: outside edits show after restart) */
const NOTETXT = {}, NOTEST = {}, NRO = new ResizeObserver(() => nFit()); let NSEL = null;
const NHEAD = [['p', 'Normal text'], ['h1', 'Heading 1'], ['h2', 'Heading 2'], ['h3', 'Heading 3']];
const NFMT = [['fhead', 'Text style'], ['fbold', 'Bold', 'bold', 'Ctrl+B'], ['fital', 'Italic', 'italic', 'Ctrl+I'], ['funder', 'Underline', 'underline', 'Ctrl+U'],
  ['fstrike', 'Strikethrough', 'strikeThrough'], ['ful', 'Bulleted list', 'insertUnorderedList'], ['fol', 'Numbered list', 'insertOrderedList'],
  ['fquote', 'Quote', 'formatBlock', '', 'blockquote'], ['code', 'Code block', 'formatBlock', '', 'pre']];
async function drawNotes() {
  const box = $('notes'), pid = cur;
  if (!pid) { box.innerHTML = empty('No project open', startProj); return; }
  if (NOTETXT[pid] == null) { box.innerHTML = skel(70, 50, 60); NOTETXT[pid] = await call('notes', {pid}).catch(() => ''); if (cur !== pid) return; }
  document.execCommand('defaultParagraphSeparator', false, 'p');
  box.innerHTML = `<div class="nbar">${NFMT.map(([ic, l, c, k], i) => `<button class="ib"${c && c !== 'formatBlock' ? ` data-c="${c}"` : ''} title="${l}${k ? ' (' + k + ')' : ''}" onmousedown="event.preventDefault()" onclick="nFmt(${i}, this)">${icon(ic)}</button>`).join('')}<button class="ib nmore" title="More formatting" onmousedown="event.preventDefault()" onclick="nMore(this)">${icon('more')}</button></div>`
    + `<div class="mdoc ned" contenteditable="true" spellcheck="true" oninput="nSave(this)" onkeyup="nState()" onmouseup="nState()" onkeydown="if (event.key === 'Tab' && getSelection().anchorNode?.parentElement?.closest('li')) { event.preventDefault(); nCmd(event.shiftKey ? 'outdent' : 'indent'); }"
      onpaste="event.preventDefault(); document.execCommand('insertText', false, event.clipboardData.getData('text/plain'))" onchange="nSave(this)">${mdDoc(NOTETXT[pid])}</div>`;
  const ed = box.querySelector('.ned');
  ed.querySelectorAll('input[type=checkbox]').forEach(x => x.disabled = false);
  ed.classList.toggle('ph', !ed.innerText.trim());
  NRO.observe(box); nFit();
}
// toolbar buttons that don't fit go into the "…" menu on the right
function nFit() { const bar = $('notes').querySelector('.nbar'); if (!bar) return; const bs = [...bar.children], more = bs.pop();
  bs.forEach(b => b.hidden = false); more.hidden = true;
  for (let i = bs.length - 1; i > 0 && bar.scrollWidth > bar.clientWidth; i--) { more.hidden = false; bs[i].hidden = true; } }
const nKeep = () => { const s = getSelection(), ed = $('notes').querySelector('.ned'); NSEL = s.rangeCount && ed?.contains(s.anchorNode) ? s.getRangeAt(0).cloneRange() : null; };
const nHeads = () => NHEAD.map(([tag, label]) => ({label, run: () => nCmd('formatBlock', tag)}));
function nFmt(i, btn) { const [, , c, , v] = NFMT[i]; if (c) return nCmd(c, v); nKeep(); menuAt(btn, nHeads()); }
function nMore(btn) { nKeep(); const bs = [...$('notes').querySelectorAll('.nbar > button:not(.nmore)')];
  menuAt(btn, NFMT.map(([ic, l, c, k, v], i) => !bs[i].hidden ? null : c ? {icon: ic, label: l, hint: k, run: () => nCmd(c, v)} : {icon: ic, label: l, items: nHeads()}).filter(Boolean)); }
function nCmd(c, v) { const ed = $('notes').querySelector('.ned'); if (!ed) return; ed.focus();
  if (NSEL) { const s = getSelection(); s.removeAllRanges(); s.addRange(NSEL); NSEL = null; }
  document.execCommand(c, false, v); nSave(ed); }
function nSave(ed) { ed.classList.toggle('ph', !ed.innerText.trim()); notesPut(htmlMd(ed)); nState(); }
function nState() { $('notes').querySelectorAll('.nbar [data-c]').forEach(b => b.classList.toggle('on', document.queryCommandState(b.dataset.c))); }
/* rich text → Markdown, the subset mdDoc renders; underline stays <u>…</u> (Markdown has none).
   ponytail: typed * _ ` in plain text are not escaped, so they turn into formatting on the next load */
function htmlMd(root) {
  const inl = n => [...n.childNodes].map(c => { if (c.nodeType === 3) return c.data.replace(/ /g, ' '); if (c.nodeType !== 1) return '';
    const t = c.tagName, s = inl(c);
    if (t === 'BR') return '\n'; if (t === 'IMG') return `![${c.alt}](${c.getAttribute('src')})`; if (!s.trim()) return s;
    const w = (a, b = a) => { const m = s.match(/^(\s*)([\s\S]*?)(\s*)$/); return m[1] + a + m[2] + b + m[3]; }; // markers hug the text, not its spaces
    return /^(B|STRONG)$/.test(t) ? w('**') : /^(I|EM)$/.test(t) ? w('*') : t === 'U' ? w('<u>', '</u>') : /^(S|STRIKE|DEL)$/.test(t) ? w('~~')
      : t === 'CODE' ? w('`') : t === 'A' ? w('[', `](${c.dataset.url || c.href})`) : s; }).join('');
  const lst = (l, d) => [...l.children].flatMap((li, k) => { if (li.tagName !== 'LI') return /^[UO]L$/.test(li.tagName) ? lst(li, d + 1) : [];
    const ind = d + Math.round((parseInt(li.style.marginLeft) || 0) / 18), cb = li.querySelector(':scope > input[type=checkbox]'),
      sub = [...li.children].filter(x => /^[UO]L$/.test(x.tagName)), txt = inl({childNodes: [...li.childNodes].filter(x => !sub.includes(x))}).replace(/\n/g, ' ').trim();
    return ['  '.repeat(ind) + (l.tagName === 'OL' ? k + 1 + '.' : '-') + ' ' + (cb ? `[${cb.checked ? 'x' : ' '}] ` : '') + txt, ...sub.flatMap(x => lst(x, ind + 1))]; });
  const blk = (n, out = []) => { let run = ''; const flush = () => { if (run.trim()) out.push(run.trim()); run = ''; };
    for (const c of n.childNodes) { const t = c.nodeType === 1 ? c.tagName : '';
      if (!/^(H[1-6]|P|DIV|UL|OL|BLOCKQUOTE|PRE|TABLE|HR)$/.test(t)) { run += inl({childNodes: [c]}); continue; }
      flush();
      if (/^H\d$/.test(t)) out.push('#'.repeat(+t[1]) + ' ' + inl(c).replace(/\n/g, ' ').trim());
      else if (t === 'HR') out.push('---');
      else if (t === 'PRE' || c.classList.contains('code')) out.push('```' + (c.dataset.lang || '') + '\n' + c.innerText.replace(/\n+$/, '') + '\n```');
      else if (t === 'UL' || t === 'OL') out.push(lst(c, 0).join('\n'));
      else if (t === 'BLOCKQUOTE') out.push(blk(c).join('\n\n').replace(/^/gm, '> '));
      else if (t === 'TABLE') { const rs = [...c.rows].map(r => '| ' + [...r.cells].map(x => inl(x).replace(/\|/g, '\\|').trim()).join(' | ') + ' |');
        if (rs.length) rs.splice(1, 0, '|' + ' --- |'.repeat(c.rows[0].cells.length)); out.push(rs.join('\n')); }
      else if (c.querySelector('h1,h2,h3,h4,h5,h6,p,div,ul,ol,blockquote,pre,table,hr')) blk(c, out);
      else { const s = inl(c).trim(); if (s) out.push(s); } }
    flush(); return out; };
  const md = blk(root).join('\n\n'); return md ? md + '\n' : '';
}
function notesPut(v) { const pid = cur; NOTETXT[pid] = v; clearTimeout(NOTEST[pid]);
  NOTEST[pid] = setTimeout(() => call('notes', {pid, text: v}).catch(e => toast('Notes not saved: ' + e, 'err')), 800); }
// file_heat cache per project (search, @/slash, recent-files rail): {t, d: {files: [[path, bytes, churn, …]], commits}}
const HEAT = {};

// browser view: in-app browser (native child webview, own profile shared by all projects), or open in a real browser profile
const webUrl = {}, QRS = new Map(); // QR svg per url (cached)
let BROWSERS = null, LAN = null, webPhone = false, HOMEPG;
async function drawWeb() {
  BROWSERS ||= await call('browsers');
  // start page: dev server from the build log, else the first browser profile's homepage, else localhost:3000
  const b0 = BROWSERS[0];
  HOMEPG ??= b0 ? await call('homepage', {exe: b0.exe, profile: b0.profiles[0]?.dir || ''}).catch(() => '') : '';
  const url = webUrl[cur] || (builds[cur]?.out.match(/https?:\/\/(localhost|127\.0\.0\.1)(:\d+)?[^\s]*/g) || []).pop() || HOMEPG || 'http://localhost:3000';
  webUrl[cur] = url;
  const opts = BROWSERS.flatMap(b => (b.profiles.length ? b.profiles : [{dir: '', name: 'default'}]).map(p =>
    `<option value='${esc(JSON.stringify([b.exe, p.dir]))}'>${esc(b.name)} · ${esc(p.name)}${p.user ? ' (' + esc(p.user) + ')' : ''}</option>`)).join('');
  let phone = '';
  if (webPhone) { // the phone needs this PC's LAN address, not localhost (dev server must listen on 0.0.0.0, e.g. vite --host)
    LAN ??= await call('lan_ip');
    const purl = LAN ? url.replace(/\/\/(localhost|127\.0\.0\.1)/, '//' + LAN) : url, local = purl !== url;
    if (!QRS.has(purl)) QRS.set(purl, await call('qr', {text: purl}).catch(() => ''));
    // only reachable from the phone if the dev server listens on all interfaces → offer Claude the fix
    const pu = new URL(purl), off = local && !await call('reach', {host: pu.hostname, port: +pu.port || 80}).catch(() => true);
    const fix = off ? `<div class="row" style="margin-bottom:8px"><small class="dim grow">Not reachable from the phone: the dev server only listens on localhost.</small>
      <button class="btn" onclick="this.disabled=true; this.innerHTML='&lt;i class=&quot;spin&quot;&gt;&lt;/i&gt;Fixing…'; bgAI(cur, 'Phone access', 'Make the dev server listen on all interfaces (0.0.0.0, e.g. vite --host) so a phone on the same Wi-Fi can open ${esc(purl)}').then(() => drawRight())">Fix</button></div>` : '';
    // QR: 2 screen px per module; click → fills the panel width for scanning, click again → back
    phone = `<div class="qrbox"><div class="qr" title="Click to enlarge for scanning" onclick="this.classList.toggle('big')">${QRS.get(purl)}</div><div class="grow" style="min-width:0"><small class="dim">Scan on your phone${local ? ' (same Wi-Fi)' : ''}</small>
      <a class="mono" data-url="${esc(purl)}" style="display:block;overflow-wrap:anywhere;color:var(--accent);cursor:pointer">${esc(purl)}</a></div></div>${fix}`;
  }
  // reload ↔ go: typing a different address turns it into "go"
  $('rbody').innerHTML = `<div class="row" style="margin-bottom:8px"><button class="ib" id="wgo" title="Reload" onclick="webGo()">${icon('refresh')}</button>
    <input id="wurl" class="grow sel mono" style="min-width:0" value="${esc(url)}" onkeydown="if(event.key==='Enter')webGo()"
      oninput="const g = this.value !== webUrl[cur]; $('wgo').innerHTML = icon(g ? 'go' : 'refresh'); $('wgo').title = g ? 'Go' : 'Reload'">
    <span class="seg"><button class="${webPhone ? '' : 'on'}" title="Desktop" onclick="webPhone=false; drawWeb()">${icon('monitor')}</button><button class="${webPhone ? 'on' : ''}" title="Phone" onclick="webPhone=true; drawWeb()">${icon('phone')}</button></span></div>
    ${phone}${webPhone ? `<div class="phone"><div class="screen"><div class="sbar"><b id="ptime">${new Date().toTimeString().slice(0, 5)}</b><i class="cam"></i><span class="bat"></span></div>
      <div id="web"></div></div></div>` : `<div id="web"></div>`}`
    + `<div class="row" style="margin-top:8px"><small class="dim">Open in</small><select id="bsel" class="grow">${opts || '<option value="">no Chromium browser found</option>'}</select>
      <button class="btn" onclick="openIn()">Open</button></div>`;
  const ph = $('rbody').querySelector('.phone'); // fit the 414px-wide phone into the panel (zoom keeps its 390px viewport)
  if (ph) ph.style.zoom = Math.min(1, ($('rbody').clientWidth - 4) / 414, (innerHeight - 330) / 868);
  syncWeb();
}
function webGo() { const v = $('wurl').value.trim(); webUrl[cur] = /^\w+:\/\//.test(v) ? v : 'http://' + v; webNav = ''; drawWeb(); } // webNav '' → (re)load
// native webview follows #web's box; hidden when #web is gone or a dialog/menu is open (a native view covers any HTML)
// ponytail: 4×/s poll, catches resize/rail/tabs/modals in one place; ResizeObserver+events if lag shows
let webAt = '', webNav = '';
function syncWeb() {
  const el = $('web'), z = LOOK.scale / 100, r = el?.getBoundingClientRect();
  const cover = [...document.querySelectorAll('dialog[open], :popover-open')].some(e => e.id !== 'tip');
  const rect = r?.width && r.height && !cover ? [r.x, r.y, r.width, r.height].map(v => v * z) : null;
  const ph = el?.closest('.phone'), wz = ph ? z * (+ph.style.zoom || 1) : 1; // phone: keep its 390px viewport
  const url = webUrl[cur] || '', nav = !!rect && url !== webNav, key = JSON.stringify([rect, wz, url]);
  if (key === webAt && !nav) return; webAt = key; if (nav) webNav = url;
  call('web_view', {url, rect, zoom: wz, nav}).catch(e => { if (nav) toast(String(e), 'err'); });
}
setInterval(syncWeb, 250);
function openIn() {
  const v = $('bsel').value; if (!v) return;
  const [exe, profile] = JSON.parse(v);
  call('open_in_browser', {exe, profile, url: $('wurl').value}).catch(alert);
}

// .env: keys shown, values masked until revealed
// Google Cloud: the project's own Google account + Cloud project (env for Claude and build steps, like GH_TOKEN),
// APIs, billing. Sign-in-with-Google clients have no CLI/API, so those are console links + the .env preset.
const APIS = {'run.googleapis.com': 'Cloud Run', 'cloudbuild.googleapis.com': 'Cloud Build', 'secretmanager.googleapis.com': 'Secret Manager',
  'storage.googleapis.com': 'Cloud Storage', 'firestore.googleapis.com': 'Firestore', 'firebase.googleapis.com': 'Firebase',
  'maps-backend.googleapis.com': 'Maps JavaScript', 'places-backend.googleapis.com': 'Places', 'people.googleapis.com': 'People (profile)',
  'gmail.googleapis.com': 'Gmail', 'drive.googleapis.com': 'Drive', 'calendar-json.googleapis.com': 'Calendar'};
// #161 host steps code does: fillStep = key dialog, autoStep = store-side action (lib site_auto) → ticks the step; above SERVICES ∵ TDZ
// #164 same for services: svc = SERVICES id the step lives in (supabase/firebase → site_auto arms too)
const fillStep = (k, svc = 'website') => ({t: `Fill **${k}**`, k, a: () => `<button class="btn sm" onclick="fillKeys('${svc}', ['${k}'])">${icon('key')}Fill in</button>`});
const autoStep = (h, t, b, k, svc = 'website') => ({t, h, k, a: () => `<button class="btn sm primary" onclick="siteAuto('${h}', this, '${svc}')">${icon('sparkle')}${b}</button>`});
async function siteAuto(h, b, svc = 'website') {
  b.disabled = true; b.innerHTML = '<span class="spin"></span>' + b.textContent;
  try { const v = await call('site_auto', {pid: cur, host: h});
    if (h === 'sshkey') { copy(v); toast('Deploy key in .env, public key copied: paste it where the next step says', 'ok'); }
    else if (h === 'play') { copy(v); toast('Key in .env, ' + v + ' copied: invite it in Play Console', 'ok'); }
    else toast(h === 'ghpages' ? 'GitHub Pages on' : 'Filled: ' + v, 'ok');
    stepSet(svc, SERVICES[svc].steps.findIndex(y => y.h === h)); drawRight(); }
  catch (e) { toast(String(e), 'err'); drawServices(); }
}
