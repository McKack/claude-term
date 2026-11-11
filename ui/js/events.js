// ---- events from Rust ----
T.event.listen('clauracle', ({payload: {event, data: d}}) => ({
  shell: () => { const s = SH[d.pid]; if (!s) return; if (d.exit) { s.dead = true; s.term.write('\r\n\x1b[2m[shell ended: press a key for a new one]\x1b[0m\r\n'); } else s.term.write(d.d); },
  line: () => { if (d.kind === 'claude') live[d.pid] = '';
    if (d.task != null) { const l = logs[d.pid] || []; // a background agent's end notice: find its launch by agentId, drop repeats
      d.result_of ||= l.find(x => x.result_of && d.task && (x.text || '').includes('agentId: ' + d.task))?.result_of;
      if (!d.result_of || l.some(x => x.result_of === d.result_of && !ASYNC.test(x.text))) return; }
    if (d.kind === 'tool') issueGuess(d.pid, d); else if (d.result_of) { issueResult(d); missCheck(d.pid, d.text); }
    if (d.kind === 'tool' && d.id) (PEND[d.pid] ||= new Map()).set(d.id, Date.now()); else if (d.result_of) PEND[d.pid]?.delete(d.result_of);
    if (d.kind === 'tool' && !d.parent) setDoing(d.pid, 'Running: ' + d.text.replace(/\s+/g, ' ').slice(0, 60), d.id);
    else if (d.result_of && d.result_of === doing[d.pid]?.id) setDoing(d.pid, 'Thinking');
    if (reading[d.pid] && /^(claude|tool|err)$/.test(d.kind)) readNote(d.pid, false); // notes/system lines of the reconnect ≠ the reply
    addEntry(d.pid, {kind: d.kind, text: d.text, detail: d.detail, id: d.id, parent: d.parent, result_of: d.result_of, cmd: d.cmd && LASTCMD[d.pid] || undefined}); if (d.result_of && d.pid === cur) drawAtp(); },
  weburl: () => { if (d === 'about:blank') return; webUrl[cur] = webNav = d; const u = $('wurl'); if (u && document.activeElement !== u) u.value = d; }, // in-app browser navigated
  delta: () => { if (hand[d.pid] === true) return; live[d.pid] = (live[d.pid] || '') + d.text;
    if (d.pid === cur) { const el = $('live'), stick = atEnd(); if (el) el.innerHTML = md(live[d.pid]); else { const box = $('log').querySelector('.col'), n = box.appendChild(liveEl(d.pid)); hatch(box, n); } toEnd(stick); } },
  status: () => { if ('busy' in d) { if (d.busy) readAll(d.pid); if (busy[d.pid] && !d.busy) lastBusy[d.pid] = Date.now(); if (d.busy && !busy[d.pid]) { setDoing(d.pid, 'Thinking'); TURN0[d.pid] = Date.now(); if (lastBusy[d.pid]) delete WORK[d.pid]; /* #226 new turn (seen idle this run): recompute, never carry an old turn's #N */ } const flip = !busy[d.pid] !== !d.busy; busy[d.pid] = d.busy; workOf(d.pid); if (!cur) drawHome(); else if (flip && d.pid === cur) { drawAtp(); tabDots(); if (!$('ready').hidden) drawChrome(); /* Continue ↔ Queue label */ } /* Plan row 0 Working look on/off */ }
    if (d.busy === false) { repDone(d.pid); if (reading[d.pid] && !d.connected) readNote(d.pid, false); /* connected = new session up, Continue msg not sent yet → keep the pill */ live[d.pid] = ''; delete PEND[d.pid]; const tri = triaging[d.pid] || contQ[d.pid], cq = contQ[d.pid]; delete triaging[d.pid]; delete contQ[d.pid];
      Promise.all([refresh(), d.pid === cur && pollTeam()]).then(() => { /* pollTeam: issue mode's Queue may have changed */ if (d.pid === cur) drawChrome(); if (tri && !queue[d.pid]?.length) { const r = cq && P(d.pid).ready, t = nextUp(d.pid)[0]; contGo(d.pid); if (r) contNote(d.pid, t); } else { if (cq) contQ[d.pid] = true; /* Continue clicked + msgs queued: msgs first, Continue after (was dropped) */ flushQueue(d.pid); } }); } /* after refresh: ready/idle bar + new Next known; sent during triage → triaged again first */ // refresh: HANDOVER/CHANGES may have changed (Tasks tab)
    if ('context' in d) { ctx[d.pid] = d.context; const t = tk(d.pid); if (d.tokens != null) t.now = d.tokens; t.saved += Math.min(t.drop, Math.max(0, (d.max || 2e5) - t.now)); saveCtx(); }
    if ('busy' in d) hand[d.pid] = d.busy && !!d.handover;
    drawProjects(); if (d.pid === cur) drawChrome(); },
  restarting: () => { RESTARTING = true; setTimeout(() => { if (!RESTARTING) return; RESTARTING = false; toast("Restart didn't happen: queued messages are sending again.", "warn"); Object.keys(queue).forEach(flushQueue); }, 20000); }, // relaunch spawn failed → app lives on; RESTARTING stuck = queue frozen forever
  cleared: () => { workOf(d.pid, true); if ((logs[d.pid] || []).some(x => !x.old && x.kind !== 'note' && !x.cont)) freshLog(d.pid); /* else cont_ did it already */ if (reading[d.pid]) readNote(d.pid, true); ctx[d.pid] = 0; { const t = tk(d.pid); t.drop += t.now; t.now = 0; } saveCtx(); /* fresh session ≈ 0%, keeps the UI steady until the first reply reports it */ live[d.pid] = ''; refresh().then(() => d.pid === cur && drawLog()); },
  pushed: () => { ghiStale(d.pid); if (d.pid === cur && tab === 'gh') drawRight(true); },
  ready: () => { qsOpen[d.pid] = false; refresh().then(() => d.pid === cur && drawLog()); }, // unanswered → wait for the next Continue
  project: () => { if (!d) return; const o = P(d.id); S.projects = S.projects.map(p => p.id === d.id ? d : p); drawProjects(); if (d.id === cur) { if (d.keep) $('suggest').hidden = true; drawChrome(); drawLog(); if (o?.build?.after !== d.build?.after) drawRight(); /* #227 Then picker follows any change (answer applied by AI, Settings) at once */ } },
  queued: () => refresh(),
  mac_setup: () => macLine(d.line),
  ctxmenu: () => CTXP ? (clearTimeout(CTXP.t), CTXP.show(d.sp)) : d.sp.length && call('ctx_pick', {id: -1}), // #390 late → release deferral
  switched: () => toast(d.text, 'warn'), // #398 auto relay switch
  paused: () => { const t = new Date(d.until * 1000), at = `${t.toLocaleTimeString([], {hour: '2-digit', minute: '2-digit'})}, ${t.toLocaleDateString([], {weekday: 'short', day: 'numeric', month: 'short'})}`; // Rust has no local time: the page writes it
    const msg = `${d.relay ? `All usage limits of the ${d.relay} relay are hit` : 'The usage limits are hit'}: work is paused until ${at}.${P(d.pid)?.cont?.startsWith('auto') ? ' It carries on by itself then.' : ''}`;
    addEntry(d.pid, {kind: 'note', text: msg}); toast(P(d.pid) ? `${P(d.pid).name}: ${msg}` : msg, 'warn', 0, true, P(d.pid) && (() => openProject(d.pid))); },
  janitor: () => toast(`Storage Janitor freed ${mb(d.freed)} (old caches, Claude Code versions, temp files)`, 'ok'),
  questions: () => refresh().then(() => { hiddenQs[d.pid] = false; if (d.pid === cur) drawLog(); }),
  ask: () => addEntry(d.pid, {kind: 'ask', ...d}),
  usage: () => { S.usage[d.account] = d.usage; if (P()) drawChrome(); },
  commands: () => { if (Array.isArray(d.agents)) { CCAG = d.agents; try { localStorage.ccag = JSON.stringify(d.agents); } catch {} }
    if (Array.isArray(d.models) && d.models.length) { CMODELS = d.models; try { localStorage.cmodels = JSON.stringify(d.models); } catch {} }
    if (Array.isArray(d.commands)) cmds[d.pid] = [{name: 'ask', argumentHint: '<question>', description: 'Clauracle: ask a question; answered, never filed as a task'}, {name: 'build', argumentHint: '[windows|linux|macos|ios|android|all]', description: 'Clauracle: build a local copy and run it; or build one target / all'}, {name: 'janitor', description: 'Clauracle: run the Storage Janitor now (old build caches, Claude Code versions and temp files)'}, ...d.commands.map(c => typeof c === 'string' ? {name: c} : c)]; },
  build: () => { const b = builds[d.pid] ||= {out: ''}; if (d.line != null) b.out += d.line + '\n';
    if (d.step === 'run' && b.live !== (d.done == null)) { b.live = d.done == null; if (d.pid === cur && tab === 'build') drawRight(); }
    if (d.step === 'build' && b.busy && d.line && d.pid === cur && tab === 'build' && /\.(exe|msi|msix|dmg|pkg|appimage|deb|rpm|ipa|apk|aab)\b/i.test(d.line) && /Finished \d+ bundle/.test(b.out.slice(b.from || 0))) drawRight(); /* #174 an artifact landed → tick it */
    if (d.step === 'build' && 'local' in d) b.local = d.local; /* #361 every line: buildNow sets busy before the 1st event */
    if (d.step === 'build' && !!b.busy !== (d.done == null)) { if (!b.busy) { b.from = b.out.length - (d.line != null ? d.line.length + 1 : 0); b.t0 = Date.now() / 1e3; } b.busy = d.done == null; if (b.busy && !b.manual && !/^Smart build/.test(logs[d.pid]?.at(-1)?.text || '')) addEntry(d.pid, {kind: 'note', text: 'Building…'}); /* #363 smart note already says it */ /* after a task / resumed (#170/#175); Build button = manual */ if (d.pid === cur) tabDots(); if (d.pid === cur && tab === 'build' && b.busy) drawRight(); } /* #120: done → redraw below */
    if (d.step === 'build' && d.done) { BUILT[d.pid] = 1; if (d.pid === cur) tabDots(); } /* green dot till Build & Release is opened */
    if (d.step === 'build' && d.done != null) { b.ok = d.done; b.at = Date.now(); b.own = !!d.own; /* statusline right zone + card */
      if (!b.manual) addEntry(d.pid, d.done ? {kind: 'note', text: 'Built!', act: d.own ? 'restart' : P(d.pid)?.build.run ? 'run' : ''} : {kind: 'note', text: 'Build failed', act: 'out'});
      b.manual = false; drawGbar(); }
    if (d.step === 'build' && d.done != null) { ghiStale(d.pid); delete RELC[d.pid]; const rb = RELB[d.pid]; delete RELB[d.pid]; (d.done && rb ? call('git_do', {pid: d.pid, action: 'commit', arg: 'Release build'}).catch(() => {}) : Promise.resolve()).then(() => d.done && relStage(d.pid)).then(() => d.pid === cur && tab === 'build' && drawRight()); } /* new build files → release check refetch; wizard build → commit (nothing to commit = Err, ignored) */
    if (d.pid === cur && tab === 'build') { const o = $('buildOut'); if (o) { o.textContent = b.out; o.scrollTop = o.scrollHeight; } } },
  login_url: () => { const m = $('loginMsg'); if (m) m.innerHTML = `Finish signing in in your browser. Didn't open? <a href="#" data-url="${esc(d.url)}">Open the sign-in page</a>`; },
  gh_code: () => { const m = $('ghMsg'); if (m) m.innerHTML = `Enter <b style="font-size:18px" class="sel mono">${esc(d.code)}</b> at <a href="#" data-url="${esc(d.url)}">github.com/login/device</a>`; T.opener.openUrl(d.url); },
}[event] || (() => {}))());

// ---- question card: numbered, choices (recommended pre-picked), Other with its own text, a note under each ----
const notOther = o => !/^\s*other\b/i.test(o.label || ''); // #220: AI's own "Other — I'll say…" = our built-in Other → shown once
function questionCard(pid) {
  const qs = P(pid).questions, d = document.createElement('div');
  d.className = 'card qs';
  d.innerHTML = `<h4>Clauracle has ${qs.length} question${qs.length > 1 ? 's' : ''}</h4><p class="dim" style="margin:0 0 4px;font-size:13px">New messages wait in the queue until these are answered.</p>` + qs.map((q, i) => {
    const type = q.multi ? 'checkbox' : 'radio', name = 'q' + i;
    return `<div class="q" data-id="${esc(q.id)}"><div><span class="num">${i + 1}</span><b>${esc(q.question.replace(/^\s*\d+[.)]\s*/, ''))}</b>${q.blocking ? ' <span class="pill">needs you</span>' : ''}</div>
      ${q.options.filter(notOther).map(o => `<label class="choice"><input type="${type}" name="${name}" value="${esc(o.label)}" ${o.recommended ? 'checked' : ''}>
        <span>${esc(o.label)}${o.recommended ? ' <span class="pill">recommended</span>' : ''}${o.description ? `<small class="dim">${esc(o.description)}</small>` : ''}</span></label>`).join('')}
      <label class="choice"><input type="${type}" name="${name}" value="__other"><span>Other</span></label>
      <input class="note" placeholder="Note (optional)"></div>`;
  }).join('') + `<div class="btns"><button class="btn ghost" onclick="hiddenQs['${pid}']=true; drawLog()">Not now</button><button class="btn primary" onclick="confirmQs('${pid}', this.closest('.card'))">Confirm</button></div>`;
  // one text field per question: "Note" → "Your own answer" while Other is picked (user 2026-11-05: two fields = same thing)
  d.onchange = e => { const q = e.target.closest('.q'); if (q && e.target.type !== 'text') q.querySelector('.note').placeholder =
    q.querySelector('[value=__other]').checked ? 'Your own answer' : 'Note (optional)'; };
  // #310: one question → Enter in its text field = Confirm (several: Enter does nothing, user may still be on others)
  d.onkeydown = e => { if (e.key === 'Enter' && !e.shiftKey && !e.isComposing && qs.length === 1 && e.target.classList.contains('note')) { e.preventDefault(); confirmQs(pid, d); } };
  return d;
}
// #65: drawLog runs on every event while Clauracle works → reuse the card element (picks + typed notes survive);
// new questions → fresh card, but already-shown .q blocks (same id) move over with their state
const QC = {};
function qsKeep(pid) {
  const key = P(pid).questions.map(q => q.id).join('\n'), old = QC[pid];
  if (old?.key === key) return old.el;
  const el = questionCard(pid);
  if (old) el.querySelectorAll('.q').forEach(n => { const o = [...old.el.querySelectorAll('.q')].find(x => x.dataset.id === n.dataset.id); if (o) n.replaceWith(o); });
  return (QC[pid] = {key, el}).el;
}
function confirmQs(pid, card) {
  const answers = [...card.querySelectorAll('.q')].map(q => { const t = q.querySelector('.note').value, other = q.querySelector('[value=__other]').checked;
    return {id: q.dataset.id, choice: [...q.querySelectorAll('.choice input:checked')].map(i => i.value === '__other' ? 'Other: ' + t : i.value),
      note: other ? '' : t}; });
  const qs = P(pid).questions; // #9: card stays in the chat, read-only
  addEntry(pid, {kind: 'answered', qa: answers.map(a => [qs.find(q => q.id === a.id)?.question || '', a.choice.join(', ').replace(/^Other: /, ''), a.note])});
  call('answer_questions', {pid, answers});
}
// answered question card (#9): filled answers as plain text, empty ones dropped; qa = [[question, answer, note]]
function answeredCard(e) {
  const d = document.createElement('div'); d.className = 'card qs done';
  d.innerHTML = `<h4>You answered</h4>` + e.qa.map(([q, a, n], i) => `<div class="q"><div><span class="num">${i + 1}</span><b>${esc(q.replace(/^\s*\d+[.)]\s*/, ''))}</b></div>
    ${a.trim() ? `<div class="ans sel">${esc(a)}</div>` : ''}${n?.trim() ? `<div class="ans sel dim">${esc(n)}</div>` : ''}</div>`).join('');
  return d;
}
function showQuestions(pid) { hiddenQs[pid] = false; qsOpen[pid] = true; drawLog(); }

// ---- permission / interactive question (inline, so other projects keep going) ----
function askCard(e) {
  const d = document.createElement('div'), i = e.input;
  d.className = 'card';
  // answered permission → note: "<shield> Tool: Allowed for 24 h" + answer time on top like other notes (#54)
  if (e.done) { d.className = 'msg note'; d.innerHTML = `${icon('shield')} ${esc(e.tool)}: ${esc(e.done)}`; if (e.doneAt || e.ts) d.dataset.ts = tm(e.doneAt || e.ts); return d; }
  if (e.tool === 'AskUserQuestion') {
    d.classList.add('qs');
    d.innerHTML = `<h4>Clauracle asks</h4>` + (i.questions || []).map((q, n) => `<div class="q"><div><span class="num">${n + 1}</span><b>${esc(q.question)}</b></div>
      ${(q.options || []).filter(notOther).map(o => `<label class="choice"><input type="${q.multiSelect ? 'checkbox' : 'radio'}" name="a${n}" value="${esc(o.label)}"><span>${esc(o.label)}<small class="dim">${esc(o.description || '')}</small></span></label>`).join('')}
      <label class="choice"><input type="${q.multiSelect ? 'checkbox' : 'radio'}" name="a${n}" value="__other"><span>Other</span></label>
      <input class="other" placeholder="Your own answer" oninput="this.previousElementSibling.querySelector('input').checked = !!this.value"></div>`).join('')
      + `<div class="btns"><button class="btn ghost" data-a="0">Dismiss</button><button class="btn primary" data-a="1">Answer</button></div>`;
  } else {
    const body = i.command || (i.file_path ? i.file_path + '\n\n' + (i.new_string || i.content || '') : JSON.stringify(i, null, 2));
    // depth ("What?"): Bash grep -c → Bash | Bash grep | Bash grep -c; rules "Bash(grep:*)" (CLI syntax), medium default
    // compound cmd (cd x; git y | grep z) → one rule per part, all needed (claude::allowed splits the same way)
    // shell syntax (loops/ifs/subshells/functions) → no prefix rules, only plain "Bash"; 2nd word only if plain
    // (subcommand/flag/path: git commit, grep -n, cd …/x), never a quoted arg or glob
    const segs = e.tool === 'Bash' ? splitCmd(i.command || '') : [];
    const parts = segs.some(s => /^(for|while|until|if|then|else|elif|do|done|fi|case|esac|function|select)\b|^[({]|^\w+\s*\(\)/.test(s)) ? []
      : segs.map(s => { const w = s.split(/\s+/); return w[1] && /^[^'"`$*?{}()<>&|;]{1,60}$/.test(w[1]) ? w.slice(0, 2) : w.slice(0, 1); });
    // label only: long paths → …/last folder (rule keeps the full path); > 4 commands → "+N more"
    const short = s => s.length > 24 && /[\\/]/.test(s) ? '…/' + s.replace(/["']/g, '').split(/[\\/]/).filter(Boolean).pop() : s;
    const lvl = n => { const ws = [...new Map(parts.map(w => [w.slice(0, n).join(' '), w.slice(0, n)])).values()], ls = ws.map(w => w.map(short).join(' '));
      return [ws.map(w => `${e.tool}(${w.join(' ')}:*)`), `${e.tool} ${ls.slice(0, 4).join(' + ')}${ls.length > 4 ? ` +${ls.length - 4} more` : ''}`]; };
    rules = [[[e.tool], e.tool === 'Bash' ? 'Bash (any command)' : e.tool], ...(parts.length ? [lvl(1)] : []), ...(parts.some(w => w.length > 1) ? [lvl(2)] : [])];
    rule = rules[Math.min(1, rules.length - 1)][0];
    d.classList.add('perm');
    d.innerHTML = `<h4><span class="edot"></span>Allow ${e.tool === 'mcp__clauracle__ui' ? 'Clauracle to act in this window' : esc(e.tool)}?</h4><pre class="sel">${esc(body)}</pre><input class="deny" placeholder="Optional: why not, or what to do instead" style="width:100%">
      <div class="btns"><button class="btn ghost" data-a="0">Deny</button><button class="selbtn" data-m>Allow…</button><button class="btn primary" data-a="1">Allow now</button></div>`;
    // Allow… = dropdown: duration → (Bash only) what to allow, as a hover submenu
    const pick = (a, r) => () => { rule = r; answer(a); };
    d.querySelector('[data-m]').onclick = ev => menuAt(ev.currentTarget, [['3', 'This session', 'Until this session restarts or is cleared'],
      ['5', 'For 24 hours', 'In this project'], ['2', 'Always', 'In this project; undo in the project manager']]
      .map(([a, label, hint]) => rules.length > 1 || e.tool === 'Bash' ? {label, hint, items: [{title: 'What to allow'}, ...rules.map(([r, l]) => ({label: l, run: pick(a, r)}))]} : {label, hint, run: pick(a, rule)}));
  }
  var rules, rule;
  d.querySelectorAll('[data-a]').forEach(b => b.onclick = () => answer(b.dataset.a));
  function answer(a) {
    const allow = a !== '0', answers = {};
    // 2 = always, 5 = 24 h ("rule@unix secs", claude::rule_hit): this project's allow list
    if (a === '2' || a === '5') { const p = P(e.pid) || P(), r = rule.map(x => a === '5' ? `${x}@${Math.floor(Date.now() / 1000) + 86400}` : x);
      call('update_project', {pid: p.id, changes: {allow: [...new Set([...(p.allow || []), ...r])]}}).then(refresh); }
    (i.questions || []).forEach((q, n) => answers[q.question] = [...d.querySelectorAll(`[name=a${n}]:checked`)]
      .map(x => x.value === '__other' ? x.closest('.q').querySelector('.other').value : x.value).join(', '));
    call('answer_ask', {id: e.id, answer: {allow, answers, session: a === '3', rule, note: d.querySelector('.deny')?.value || ''}});
    if (i.questions && allow) { e.kind = 'answered'; e.qa = i.questions.map(q => [q.question, answers[q.question], '']); d.replaceWith(answeredCard(e)); return; } // #9
    e.done = {0: 'Denied', 1: 'Allowed once', 3: 'Allowed for this session', 5: 'Allowed for 24 h', 2: 'Always allowed'}[a] || 'Allowed';
    e.doneAt = new Date().toISOString();
    d.replaceWith(askCard(e));
  }
  return d;
}

