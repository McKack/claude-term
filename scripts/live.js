// Check a change in the real app: starts the debug build, runs JS expressions in its page, prints each result, closes it.
// Usage: cargo build --manifest-path src-tauri/Cargo.toml, then  node scripts/live.js "<expr>" ["<expr>" …]
// No expression = smoke test (the page script loaded). Never touches the mouse, keyboard or an installed Clauracle.
// Careful: this is the user's real config. send() or task_do "new"/"ask" with a task files a real GitHub issue.
const {spawn} = require('child_process'), path = require('path');
const exprs = process.argv.slice(2), PORT = 9333;
if (!exprs.length) exprs.push("typeof send + ' ' + typeof fileAsk + ' ' + S.projects.length + ' projects'");
// Shells started by Claude Code carry CLAUDE* and NO_COLOR; the app must not inherit them
const env = Object.fromEntries(Object.entries(process.env).filter(([k]) => !/^(CLAUDE|NO_COLOR$)/.test(k)));
env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=' + PORT; // ponytail: Windows (WebView2) only
const app = spawn(path.join(__dirname, '../src-tauri/target/debug/clauracle.exe'), {env, stdio: 'ignore'});
const quit = code => { try { spawn('taskkill', ['/F', '/T', '/PID', String(app.pid)], {stdio: 'ignore'}); } catch {} process.exitCode = code; };
const sleep = ms => new Promise(r => setTimeout(r, ms));

(async () => {
  let page;
  for (let i = 0; i < 60 && !page; i++) { // up to 30 s for the window
    await sleep(500);
    try { page = (await (await fetch(`http://127.0.0.1:${PORT}/json`)).json()).find(t => t.type === 'page' && !/^devtools/.test(t.url)); } catch {}
  }
  if (!page) throw new Error('the app did not open a page in 30 s');
  const ws = new WebSocket(page.webSocketDebuggerUrl), wait = {};
  await new Promise((ok, no) => { ws.onopen = ok; ws.onerror = no; });
  ws.onmessage = m => { const d = JSON.parse(m.data); wait[d.id]?.(d.result); };
  const run = (id, expression) => new Promise(ok => { wait[id] = ok;
    ws.send(JSON.stringify({id, method: 'Runtime.evaluate', params: {expression, awaitPromise: true, returnByValue: true}})); });
  // the page script ends with refresh(): wait until it has filled the state
  for (let i = 0; i < 40; i++) { if ((await run(1000 + i, "typeof S === 'object' && !!S.projects")).result?.value) break; await sleep(500); }
  let bad = 0;
  for (const [i, e] of exprs.entries()) {
    const r = await run(i + 1, e);
    if (r.exceptionDetails) { bad = 1; console.log('ERROR', r.exceptionDetails.exception?.description || r.exceptionDetails.text); }
    else console.log(typeof r.result.value === 'string' ? r.result.value : JSON.stringify(r.result.value));
  }
  ws.close(); quit(bad);
})().catch(e => { console.error('live check failed:', e.message); quit(1); });
