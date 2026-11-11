// ---- Services: what a project signs in with, takes payments through, ships to. Links, steps and its .env keys in one place ----
// ponytail: a static list, no AI. Add one = an entry here
const SERVICES = {
  google: {name: 'Google', note: 'Cloud project, APIs, billing, Sign in with Google', keys: ['GOOGLE_CLIENT_ID', 'GOOGLE_CLIENT_SECRET']},
  apple: {name: 'Sign in with Apple', note: 'Apple Developer account', keys: ['APPLE_CLIENT_ID', 'APPLE_TEAM_ID', 'APPLE_KEY_ID', 'APPLE_PRIVATE_KEY'],
    steps: ['Make an App ID with Sign in with Apple ticked', 'Make a Services ID with your web domain and return URL: that ID is the client ID', 'Make a key with Sign in with Apple and download the .p8 (only once)'],
    links: [['App IDs', 'https://developer.apple.com/account/resources/identifiers/list'], ['Services IDs', 'https://developer.apple.com/account/resources/identifiers/list/serviceId'], ['Keys', 'https://developer.apple.com/account/resources/authkeys/list']]},
  github: {name: 'Sign in with GitHub', note: 'OAuth app', keys: ['GITHUB_CLIENT_ID', 'GITHUB_CLIENT_SECRET'],
    steps: ['New OAuth App, with your callback URL', 'Generate a client secret'], links: [['OAuth apps', 'https://github.com/settings/developers']]},
  microsoft: {name: 'Sign in with Microsoft', note: 'Entra app registration', keys: ['MICROSOFT_CLIENT_ID', 'MICROSOFT_CLIENT_SECRET', 'MICROSOFT_TENANT_ID'],
    steps: ['New registration, with a Web redirect URI', 'Certificates & secrets: new client secret'], links: [['App registrations', 'https://entra.microsoft.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade']]},
  discord: {name: 'Sign in with Discord', note: 'Discord application', keys: ['DISCORD_CLIENT_ID', 'DISCORD_CLIENT_SECRET'],
    steps: ['New Application', 'OAuth2: add a redirect, copy the client ID and secret'], links: [['Applications', 'https://discord.com/developers/applications']]},
  auth0: {name: 'Auth0', note: 'Hosted sign-in', keys: ['AUTH0_DOMAIN', 'AUTH0_CLIENT_ID', 'AUTH0_CLIENT_SECRET'], links: [['Dashboard', 'https://manage.auth0.com/']]},
  clerk: {name: 'Clerk', note: 'Hosted sign-in', keys: ['CLERK_PUBLISHABLE_KEY', 'CLERK_SECRET_KEY'], links: [['Dashboard', 'https://dashboard.clerk.com/']]},
  supabase: {name: 'Supabase', note: 'Database, auth, storage', keys: ['SUPABASE_URL', 'SUPABASE_ANON_KEY', 'SUPABASE_SERVICE_ROLE_KEY'],
    steps: ['Make a project (pick the one to use by filling **SUPABASE_URL** if you have several)', '**Account** › **Access Tokens** › **Generate new token**', fillStep('SUPABASE_ACCESS_TOKEN', 'supabase'),
      autoStep('supabase', 'Clauracle reads the URL and keys with that token', 'Fill URL & keys', 'SUPABASE_SERVICE_ROLE_KEY', 'supabase')],
    links: [['Projects', 'https://supabase.com/dashboard/projects'], ['Access tokens', 'https://supabase.com/dashboard/account/tokens']]},
  firebase: {name: 'Firebase', note: 'Auth, Firestore, hosting', keys: ['FIREBASE_API_KEY', 'FIREBASE_AUTH_DOMAIN', 'FIREBASE_PROJECT_ID', 'FIREBASE_APP_ID'],
    steps: ['Pick the Google account and Cloud project under the **Google** service', autoStep('firebase', 'Clauracle adds Firebase and a web app to that project and fills its config', 'Fill config', 'FIREBASE_APP_ID', 'firebase')],
    links: [['Console', 'https://console.firebase.google.com/']]},
  stripe: {name: 'Stripe', note: 'Payments', keys: ['STRIPE_PUBLISHABLE_KEY', 'STRIPE_SECRET_KEY', 'STRIPE_WEBHOOK_SECRET'],
    steps: ['Copy the API keys (test mode first)', fillStep('STRIPE_SECRET_KEY', 'stripe'),
      autoStep('stripe', 'Clauracle adds a webhook endpoint at **your website address**/api/stripe/webhook and fills its signing secret', 'Make webhook', 'STRIPE_WEBHOOK_SECRET', 'stripe')], links: [['API keys', 'https://dashboard.stripe.com/apikeys'], ['Webhooks', 'https://dashboard.stripe.com/webhooks']]},
  resend: {name: 'Resend', note: 'Email', keys: ['RESEND_API_KEY'], steps: ['Verify your domain', 'Make an API key'], links: [['Domains', 'https://resend.com/domains'], ['API keys', 'https://resend.com/api-keys']]},
  sentry: {name: 'Sentry', note: 'Error tracking', keys: ['SENTRY_DSN'], links: [['Projects', 'https://sentry.io/organizations/']]},
  vercel: {name: 'Vercel', note: 'Hosting', keys: ['VERCEL_TOKEN'], links: [['Tokens', 'https://vercel.com/account/tokens'], ['Dashboard', 'https://vercel.com/dashboard']]},
  // steps: string | {t, a: () => action html}; **x** = UI name on the other site (bold); gh:'step' = Send button lives in a step
  macsign: {name: 'Mac signing', note: 'Developer ID: Mac builds open without the "unidentified developer" block', gh: 'step',
    keys: ['APPLE_CERTIFICATE', 'APPLE_CERTIFICATE_PASSWORD', 'APPLE_SIGNING_IDENTITY', 'APPLE_TEAM_ID', 'APPLE_ID', 'APPLE_PASSWORD'],
    get steps() { return [{t: 'Make the signing request: **devid.csr** lands in your **Downloads** and Apple\'s page opens', a: () => `<button class="btn sm primary" onclick="appleCsr()">Make signing request</button>`,
        u: s => s.csr ? `<div class="under dim mono" title="Copy" style="cursor:pointer" onclick="copy(this.textContent)">${esc(s.csr)}</div>` : ''},
      {t: 'At Apple pick **Developer ID Application**, then **G2 Sub-CA** › **Continue**, choose **devid.csr** › **Continue** › **Download**', a: () => `<button class="btn sm" data-url="${APPLE_CERTS}">Certificates</button>`},
      {t: 'Clauracle picks the downloaded **.cer** up from **Downloads** by itself and fills the first four keys', a: () => `<button class="btn sm" onclick="appleCer()">Load .cer…</button><small class="dim" id="cerWait"></small>`},
      {t: 'Make an **App-Specific Password** (under **Sign-In and Security**), then fill **APPLE_ID** (your Apple account email) and **APPLE_PASSWORD**',
        a: () => `<button class="btn sm" data-url="https://account.apple.com/account/manage">App-specific password</button><button class="btn sm" onclick="fillKeys('macsign', ['APPLE_ID', 'APPLE_PASSWORD'])">${icon('key')}Fill in</button>`},
      /* #267 CI off (default) → local/Network Mac builds sign from .env (relItems sign row): no secrets step */
      S.ci_builds ? {t: 'Send to GitHub: CI then signs and notarizes Mac builds', a: () => `<button class="btn sm" onclick="toSecrets('macsign', this)" title="Copies these keys from .env to the repo's Actions secrets">${icon('gh')}Send to GitHub secrets</button><small class="dim" id="sec-macsign"></small><p class="err" id="secErr-macsign" style="font-size:13px"></p>`}
        : 'Done: local Mac builds sign and notarize with these keys from **.env** (CI builds are off, so nothing goes to GitHub)',
      {t: 'Optional: register the **App ID** only if the app uses capabilities like iCloud or push', u: () => bundleRow(true) + `<div class="btnrow"><button class="btn sm" data-url="https://developer.apple.com/account/resources/identifiers/add/bundleId">Register App ID</button></div>`}]; }}, /* getter ∵ S.ci_builds read at draw time (#267) */
  appstore: {name: 'App Store Connect', note: 'iOS / macOS releases', keys: ['ASC_KEY_ID', 'ASC_ISSUER_ID', 'ASC_PRIVATE_KEY'],
    steps: ['Identifiers › + › App IDs › App: Explicit, paste the Bundle ID below, tick the capabilities the app uses', 'Certificates › + › Apple Distribution (iOS) or Mac App Distribution + Mac Installer Distribution (macOS)',
      'Profiles › + › App Store Connect: pick the App ID and certificate, download the profile', 'Make the app record with the same Bundle ID', 'Integrations: make an API key (for uploads from CI)'],
    links: [['Register App ID', 'https://developer.apple.com/account/resources/identifiers/add/bundleId'], ['Certificates', 'https://developer.apple.com/account/resources/certificates/add'], ['Profiles', 'https://developer.apple.com/account/resources/profiles/add'],
      ['Apps', 'https://appstoreconnect.apple.com/apps'], ['API keys', 'https://appstoreconnect.apple.com/access/integrations/api']]},
  play: {name: 'Google Play', note: 'Android releases', keys: ['PLAY_SERVICE_ACCOUNT_JSON'],
    steps: ['Make the app', 'Pick the Google account and Cloud project under the **Google** service',
      autoStep('play', 'Clauracle makes a service account in that project and fills its JSON key', 'Make service account', 'PLAY_SERVICE_ACCOUNT_JSON', 'play'),
      'Play Console › **Users and permissions** › **Invite new users**: paste the service account email (copied), give it release rights'],
    links: [['Play Console', 'https://play.google.com/console']]},
  // #40: keys/steps/links follow the picked host (HOSTS) → getters; host/domain/pages live in p.site (siteHtml)
  website: {name: 'Website', note: 'Landing page, privacy policy and terms on your domain',
    get keys() { return HOSTS[siteOf().host]?.keys || []; },
    get steps() { const h = HOSTS[siteOf().host]; return h && [...h.steps, `Point **${siteOf().domain || 'your subdomain'}** at it in your DNS: ${h.dns}. On Cloudflare keep it proxied (orange cloud) for free HTTPS`]; },
    get links() { const h = HOSTS[siteOf().host]; return h && [...h.links, ['Cloudflare DNS', 'https://dash.cloudflare.com/']]; }},
  steam: {name: 'Steam', note: 'Game releases', keys: ['STEAM_APP_ID', 'STEAM_WEB_API_KEY'],
    steps: ['Pay the app fee and make the app in Steamworks', 'Get a Web API key'], links: [['Steamworks', 'https://partner.steamgames.com/apps'], ['Web API key', 'https://steamcommunity.com/dev/apikey']]},
};
// #40 website hosts: fields (.env keys) + easiest setup path per kind. ponytail: static list, add one = an entry
// #166 KEYSTEP = lib site_auto sshkey (ssh-keygen → SITE_SSH_PRIVATE/PUBLIC_KEY, rerun = copy pubkey again); keyInstall = password once → authorized_keys
const SSHK = ['SITE_SSH_HOST', 'SITE_SSH_USER', 'SITE_SSH_PRIVATE_KEY', 'SITE_PATH'], A_REC = 'an **A** record to the server\'s IP (shown in the panel)';
const KEYSTEP = autoStep('sshkey', 'Clauracle makes a deploy key and copies its **public key** (click again to copy it again)', 'Make deploy key', 'SITE_SSH_PRIVATE_KEY');
const SSHFILL = what => ({t: `Fill host = your server name, user = ${what}, path = SITE_PATH`, k: 'SITE_SSH_HOST', a: () => `<button class="btn sm" onclick="fillKeys('website', ['SITE_SSH_HOST', 'SITE_SSH_USER', 'SITE_PATH'])">${icon('key')}Fill in</button>`});
function keyInstall() {
  dlg(`<h2>Install the deploy key${infoI('Logs in once with the server password to add the key. The password is not saved.')}</h2>
    ${field('Password', `<input id="kiPw" type="password" onkeydown="event.key=='Enter'&&keyInstallGo()">`)}<p id="kiMsg" class="err" style="font-size:13px"></p>
    <div class="btns"><button class="btn ghost" onclick="closeDlg()">Cancel</button><button id="kiBtn" class="btn primary" onclick="keyInstallGo()">Install</button></div>`);
  $('kiPw').focus();
}
async function keyInstallGo() {
  const b = $('kiBtn'); b.disabled = true; b.innerHTML = '<span class="spin"></span>Installing…';
  try { await call('site_key_install', {pid: cur, pw: $('kiPw').value}); closeDlg(); toast('Deploy key installed', 'ok');
    stepSet('website', SERVICES.website.steps.findIndex(y => y.h === 'keyinst')); }
  catch (e) { $('kiMsg').textContent = e; b.disabled = false; b.textContent = 'Install'; }
}
const HOSTS = {
  cpanel: {name: 'cPanel', how: 'SSH (rsync/scp) into the subdomain\'s folder', keys: SSHK, dns: A_REC, links: [],
    steps: ['In cPanel: **Domains** › **Create a New Domain** with your subdomain; its document root is **SITE_PATH** (like public_html/app)',
      KEYSTEP, '**SSH Access** › **Manage SSH Keys** › **Import Key**: paste the public key, then **Authorize** it', SSHFILL('cPanel username')]},
  directadmin: {name: 'DirectAdmin', how: 'SSH (rsync/scp) into the subdomain\'s folder', keys: SSHK, dns: A_REC, links: [],
    steps: ['**Subdomain Management**: add the subdomain; folder = **SITE_PATH** (like domains/example.com/public_html/app)',
      KEYSTEP, '**SSH Keys** › **Paste key**: the public key (no SSH menu? ask your host to enable SSH)', SSHFILL('DirectAdmin username')]},
  plesk: {name: 'Plesk', how: 'SSH (rsync/scp) into the subdomain\'s folder', keys: SSHK, dns: A_REC, links: [],
    steps: ['**Websites & Domains** › **Add Subdomain**; its document root is **SITE_PATH**', '**Web Hosting Access**: set SSH access to /bin/bash', KEYSTEP, '**SSH Keys** › **Add**: paste the public key', SSHFILL('the subscription system user')]},
  ssh: {name: 'SSH / SFTP server (VPS)', how: 'rsync over SSH into SITE_PATH (served by the web server there)', keys: SSHK, dns: A_REC, links: [],
    steps: ['Make a web root for the subdomain on the server (like /var/www/app) and a server block / vhost for it with HTTPS (Caddy does it by itself)', SSHFILL('your login on it'), KEYSTEP,
      {t: 'Clauracle installs the key with your server password (asked once, not saved)', h: 'keyinst', a: () => `<button class="btn sm primary" onclick="keyInstall()">${icon('key')}Install key</button>`}]},
  cfpages: {name: 'Cloudflare Pages', how: 'wrangler pages deploy', keys: ['CLOUDFLARE_API_TOKEN', 'CLOUDFLARE_ACCOUNT_ID'], dns: 'add it under the Pages project › **Custom domains** (Cloudflare makes the CNAME)',
    steps: ['**My Profile** › **API Tokens** › **Create Token** with **Cloudflare Pages: Edit**', fillStep('CLOUDFLARE_API_TOKEN'), autoStep('cfpages', 'Clauracle reads your **Account ID** with that token', 'Get Account ID', 'CLOUDFLARE_ACCOUNT_ID')],
    links: [['API tokens', 'https://dash.cloudflare.com/profile/api-tokens']]},
  netlify: {name: 'Netlify', how: 'netlify deploy --prod', keys: ['NETLIFY_AUTH_TOKEN', 'NETLIFY_SITE_ID'], dns: 'a **CNAME** to <site>.netlify.app (Domain management shows it)',
    steps: ['**User settings** › **Applications** › **New access token**', fillStep('NETLIFY_AUTH_TOKEN'), autoStep('netlify', 'Clauracle makes the site and fills its **Site ID**', 'Make site', 'NETLIFY_SITE_ID')],
    links: [['Sites', 'https://app.netlify.com/'], ['Tokens', 'https://app.netlify.com/user/applications#personal-access-tokens']]},
  vercel: {name: 'Vercel', how: 'vercel deploy --prod', keys: ['VERCEL_TOKEN'], dns: 'a **CNAME** to cname.vercel-dns.com (Project › Domains)',
    steps: ['**Account Settings** › **Tokens** › **Create**', 'Fill the key'], links: [['Tokens', 'https://vercel.com/account/tokens']]},
  ghpages: {name: 'GitHub Pages', how: 'a GitHub Actions Pages workflow with a CNAME file', keys: [], dns: 'a **CNAME** to <owner>.github.io',
    steps: [autoStep('ghpages', 'Turn on **Pages** with source **GitHub Actions** (repo must be on GitHub)', 'Turn on Pages'), 'Optional: tick **Enforce HTTPS** once the domain check passes'], links: [['Pages docs', 'https://docs.github.com/pages']]},
};
const SITEPG = {landing: ['Landing page', '/'], privacy: ['Privacy Policy', '/privacy'], terms: ['Terms of Service', '/terms']};
const siteOf = (p = P()) => ({pages: Object.keys(SITEPG), ...(p.site || {})});
const siteUrl = (pg, p) => `https://${siteOf(p).domain}${SITEPG[pg][1]}`;
async function siteSet(ch) {
  if ('host' in ch) stepReset('website'); // other host = other steps
  if ('domain' in ch) { ch.domain = ch.domain.trim().toLowerCase().replace(/^https?:\/\//, '').replace(/\/.*$/, ''); delete BUNDLE[cur]; delete RELC[cur]; }
  await setP({site: {...siteOf(), ...ch}}); await refresh(); drawServices();
}
const siteTog = (pg, on) => siteSet({pages: Object.keys(SITEPG).filter(k => k === pg ? on : siteOf().pages.includes(k))});
function siteHtml() {
  const s = siteOf(), h = HOSTS[s.host];
  return field('Host', `<select onchange="siteSet({host:this.value})">${options({'': 'Pick your host…', ...Object.fromEntries(Object.entries(HOSTS).map(([k, x]) => [k, x.name]))}, s.host || '')}</select>`)
    + field('Address', `<input class="mono" placeholder="app.example.com" value="${esc(s.domain || '')}" onchange="siteSet({domain:this.value})">`)
    + `<div class="box" style="margin:0">${Object.entries(SITEPG).map(([k, [n, path]]) => optRow(esc(n), `<span class="mono">${s.domain ? esc(s.domain) : ''}${path}</span>`,
      sw(s.pages.includes(k), `siteTog('${k}', this.checked)`))).join('')}</div>`
    + (s.domain ? bundleRow() + '<p class="hint">Bundle ID suggestion follows this address, reversed</p>' : '')
    + (h && s.domain && s.pages.length ? `<button class="btn primary block" onclick="siteMake()">${icon('sparkle')}Make &amp; deploy the website</button>` : '');
}
function sitePrompt() {
  const s = siteOf(), h = HOSTS[s.host], pg = s.pages.map(k => `${SITEPG[k][0]} (${SITEPG[k][1]})`).join(', ');
  if (!h || !s.domain) return 'Set up the project website: ask me via the ask tool which host and address to use.';
  return `Make the project website as a small static site in site/ (plain HTML/CSS, unless the project already has a web setup): ${pg}, served at https://${s.domain}. `
    + `Read the README and code first; plain voice of this app, no template filler. Privacy policy and terms must match what the app really collects and does `
    + `(what's required by Google Play / App Store); ask me via the ask tool for the owner name, contact email and country if you can't find them. `
    + `Deploy to ${h.name} via ${h.how}${h.keys.length ? `, with a deploy script that reads ${h.keys.join(', ')} from .env at run time (never print or commit them)` : ''}. `
    + `Tell me the deploy command so I can set it as the project's Deploy step.`;
}
async function siteMake() { const text = sitePrompt(); if (await sure(`Ask Clauracle?\n\n${text}\n${estTok()}`)) sendNow(cur, {text, atts: []}); }
const svcOf = p => [...new Set([...(p.gcp ? ['google'] : []), ...(p.services || [])])].filter(id => SERVICES[id]);
async function svcAdd(id) { await setP({services: [...svcOf(P()), ...(id === 'firebase' ? ['google'] : []), id]}); /* firebase needs the Google card's account+project */ await refresh(); drawRight(); }
async function svcRemove(id) { if (!await sure(`Remove ${SERVICES[id].name} from this project?\n\nIts keys in .env stay; you can add the service again.`, 'Remove', 'Cancel', undefined, true)) return; await setP({services: svcOf(P()).filter(x => x !== id)}); await refresh(); drawRight(); }
// #162 type › provider › service; an id may sit in several types; provider '' = no caption; unlisted ids → Other
const SVCTYPE = {'Sign-in': {Google: ['google', 'firebase'], Apple: ['apple'], GitHub: ['github'], Microsoft: ['microsoft'], Discord: ['discord'], Auth0: ['auth0'], Clerk: ['clerk'], Supabase: ['supabase']},
  Database: {Supabase: ['supabase'], Google: ['firebase']}, Payments: {Stripe: ['stripe']}, Email: {Resend: ['resend']},
  Hosting: {'': ['website'], Vercel: ['vercel'], Google: ['firebase']}, Monitoring: {Sentry: ['sentry']},
  'App stores': {Apple: ['appstore', 'macsign'], Google: ['play'], Valve: ['steam']}, Cloud: {Google: ['google']}};
// #247 services that are features inside the app (≠ hosting/stores/cloud) → svcWire: AI checks the code, files `needs-user` tasks for unused ones
const svcFeats = ids => ids.filter(id => ['Sign-in', 'Database', 'Payments', 'Email', 'Monitoring'].some(t => Object.values(SVCTYPE[t]).flat().includes(id)));
// #259 not integrated yet = in-app services not yet handed off (p.wired); ponytail: handed off ≠ verified in code, Claude's check decides per service
const svcUnwired = p => svcFeats(svcOf(p)).filter(id => !(p.wired || []).includes(id));
async function svcWire() {
  const p = P(), ids = svcUnwired(p), n = ids.map(id => SERVICES[id].name); if (!n.length) return;
  if (!await sure(`Integrate ${n.join(', ')} into the app?\n\nClaude checks the code for each one (a sign-in screen or button, a checkout, sent emails…) and adds a task to wire in each one the app doesn't use yet. They show on the GitHub tab.\n${estTok()}`)) return;
  await setP({wired: [...new Set([...(p.wired || []), ...ids])]}); rightTab('gh');
  sendNow(cur, {text: `These services were added to the project but may not be integrated into the app yet: ${n.join(', ')}. For each, check whether the app's code already uses it where users see it (e.g. Sign in with Apple → a login screen/button that calls it; Stripe → a checkout; Resend → emails sent; Sentry → SDK init). Don't change code now. For each one not wired in and no open issue covers: \`gh issue create --label clauracle --label feature\`, title "Integrate <service> into the app", body = what's missing + the plan (where it shows, the flow; keys are already in .env). Then reply with one line per service: used (where) or filed #N.`, atts: []});
}
const svcItems = () => [{icon: 'key', label: 'Secrets (.env)', run: () => jumpTo('[data-f=svc-secrets]')}, '-', ...tabItems('svc')];
function svcMenu(el) {
  const left = id => SERVICES[id] && !svcOf(P()).includes(id), it = id => ({label: SERVICES[id].name, hint: SERVICES[id].note, run: () => svcAdd(id)});
  const seen = new Set(Object.values(SVCTYPE).flatMap(t => Object.values(t).flat()));
  const types = {...SVCTYPE, Other: {'': Object.keys(SERVICES).filter(id => !seen.has(id))}};
  const menu = Object.entries(types).map(([t, pr]) => {
    // provider w/ ≥2 left → caption + indented rows; 1 → plain row
    const items = Object.entries(pr).flatMap(([n, ids]) => { ids = ids.filter(left);
      return n && ids.length > 1 ? [{title: n}, ...ids.map(id => ({...it(id), sub: 1}))] : ids.map(it); });
    return items.length && {label: t, items};
  }).filter(Boolean);
  menuAt(el, menu.length ? menu : [{title: 'All services added'}]);
}
let envKeys = {files: [], have: new Set()};
const BUNDLE = {}; // pid → bundle ID, cached per app run ∵ file reads; restart after editing the config
async function drawServices() {
  const box = $('rbody'), p = P(), ids = svcOf(p);
  const files = await call('env_list', {pid: cur}).catch(() => []);
  envKeys = {files, have: new Set(files.flatMap(f => f.keys.filter(([, v]) => v).map(([k]) => k)))};
  if ((ids.includes('macsign') || ids.includes('appstore') || ids.includes('website')) && !(cur in BUNDLE)) BUNDLE[cur] = await call('bundle_id', {pid: cur}).catch(() => null);
  if (tab !== 'svc') return;
  if (!ids.length) {
    box.innerHTML = sec('Your services' + infoI('Services this project signs in with, takes payments through or ships to.'), empty('No services yet', `<button class="ib" title="Add a service" onclick="svcMenu(this)">${icon('plus')}</button>`)) + secretsFold(files); // the header Add is the one action
    SVCDOT[cur] = false; tabDots();
    return envJump(files);
  }
  box.innerHTML = ids.map(id => { const x = SERVICES[id], miss = x.keys.filter(k => !envKeys.have.has(k));
    return sec(esc(x.name), (id === 'google' ? '<div id="gcloud" class="sc" style="padding:0;border:0;gap:10px">' + skel(60, 'b100') + '</div>' : '')
      + (id === 'website' ? siteHtml() : '')
      + (x.steps ? stepper(id, x.steps) : '')
      + (x.links ? `<div class="btnrow">${x.links.map(([l, u]) => `<button class="btn sm" data-url="${esc(u)}">${esc(l)}</button>`).join('')}</div>` : '')
      + (id === 'appstore' ? bundleRow() : '')
      + `<div class="keys">${x.keys.map(k => `<div><span>${k}</span>${envKeys.have.has(k) ? '<small class="ok">set</small>' : '<small class="faint">missing</small>'}</div>`).join('')}</div>`
      + (miss.length ? `<button class="btn primary block" onclick="fillKeys('${id}')">${icon('key')}Fill in ${miss.length} key${miss.length > 1 ? 's' : ''}</button>` : '')
      + (x.gh === true && S.ci_builds /* #267 local builds read .env */ ? `<button class="btn block" onclick="toSecrets('${id}', this)" title="Copies these keys from .env to the repo's Actions secrets">${icon('gh')}Send to GitHub secrets <small class="dim" id="sec-${id}"></small></button><p class="err" id="secErr-${id}" style="font-size:13px"></p>` : ''),
      (id === 'website' && !siteReady() ? '<span class="pill dim">set up</span>'
        : `<span class="pill${miss.length ? ' dim' : ''}">${miss.length ? `${x.keys.length - miss.length}/${x.keys.length}` : 'ready'}</span>`)
      + `<button class="ib" title="Remove from this project" onclick="svcRemove('${id}')">${icon('trash')}</button>`); }).join('')
    + secretsFold(files)
    // level up: every service set → Next step to Build & Release; pill floats at the bottom till the card is seen
    + (ids.every(svcDone) ? `<div class="nextstep" id="nextStep"><p><span class="nw">All services and secrets set up.</span><span class="nn">Next step:</span></p><button class="btn sm primary block" onclick="rightTab('build')"><span class="nw">Next step:&nbsp;</span>Build &amp; Release ${icon('go')}</button>
      ${(u => u.length ? `<button class="btn sm block" onclick="svcWire()" title="${esc(u.map(id => SERVICES[id].name).join(', '))}: Claude checks the code and adds a task on the GitHub tab to wire in each one the app doesn't use yet">${icon('back')}Go back: Integrate ${u.length} service${u.length > 1 ? 's' : ''}</button>` : '')(svcUnwired(p))}</div>
      <button class="newpill" id="newPill" onclick="$('nextStep').scrollIntoView({behavior: 'smooth', block: 'end'})">${icon('down')}New!</button>` : '');
  if ($('nextStep')) new IntersectionObserver(([e]) => { if (e.isIntersecting && $('newPill')) $('newPill').hidden = true; }).observe($('nextStep'));
  if (ids.includes('google')) drawCloud();
  ids.filter(id => SERVICES[id].gh).forEach(id => secCount(id));
  if (ids.includes('macsign') && !envKeys.have.has('APPLE_CERTIFICATE') && !cerT) cerWatch();
  SVCDOT[cur] = ids.some(id => !svcDone(id)); tabDots();
  envJump(files);
}
// opened from a .env file in the tree (previewFile): open Secrets, scroll to + mark that file
function envJump(files) {
  if (!envAt) return;
  const i = files.findIndex(f => f.file === envAt), el = $('rbody').querySelectorAll('.envfile')[i];
  if (el) { el.closest('details').open = true; el.scrollIntoView({block: 'start'}); el.style.borderColor = 'var(--accent)'; } envAt = null;
}
// step wizards: progress per project+service in localStorage `steps` {pid: {svc: {d: [done idx], csr}}} ∵ UI-only state, survives restarts
const STEPS = (() => { try { return JSON.parse(localStorage.steps || '{}'); } catch { return {}; } })();
const stepOf = id => (STEPS[cur] ||= {})[id] ||= {d: []};
function stepSet(id, i, on, extra) {
  const s = Object.assign(stepOf(id), extra); s.d = s.d.filter(x => x !== i);
  if (on ?? true) s.d.push(i);
  try { localStorage.steps = JSON.stringify(STEPS); } catch {}
  if (tab === 'svc') drawServices();
}
const stepTog = (id, i) => stepSet(id, i, !stepOf(id).d.includes(i));
function stepReset(id) { delete (STEPS[cur] || {})[id]; try { localStorage.steps = JSON.stringify(STEPS); } catch {} drawServices(); }
// auto-tick from checks: .env keys / repo secrets ⇒ earlier steps done. s.a = auto idx already applied ∴ a manual untick sticks
const MAC4 = ['APPLE_CERTIFICATE', 'APPLE_CERTIFICATE_PASSWORD', 'APPLE_SIGNING_IDENTITY', 'APPLE_TEAM_ID'];
function stepAuto(id) {
  const h = k => envKeys.have.has(k), all = SERVICES[id].keys.every(h), st = SERVICES[id].steps || [];
  if (id !== 'macsign') {
    // every key filled means the steps that produce them are done; the website's DNS step can't be read from keys
    if (all && id !== 'website') return stepReq(st);
    return st.flatMap((x, i) => x.k && h(x.k) ? [i] : []); // a step's own key set means that step is done
  }
  const sec = SECRETS[cur];
  if (sec && SERVICES.macsign.keys.every(k => sec.includes(k))) return [0, 1, 2, 3, 4];
  if (all && !S.ci_builds) return [0, 1, 2, 3, 4]; // CI off: step 5 is just "Done", nothing left to send
  return [...(MAC4.every(h) ? [0, 1, 2] : []), ...(h('APPLE_ID') && h('APPLE_PASSWORD') ? [3] : [])];
}
function stepSync(id) { // → true if new steps got ticked (persisted)
  const s = stepOf(id), n = stepAuto(id).filter(i => !(s.a || []).includes(i));
  if (!n.length) return false;
  s.a = [...(s.a || []), ...n]; s.d = [...new Set([...s.d, ...n])];
  try { localStorage.steps = JSON.stringify(STEPS); } catch {}
  return true;
}
// required = not "Optional…"; all ticked → service's steps count as done
const stepReq = steps => steps.map((x, i) => i).filter(i => !/^Optional/.test(steps[i].t || steps[i]));
const stepsDone = id => { const st = SERVICES[id].steps; return !st || (stepSync(id), stepReq(st).every(i => stepOf(id).d.includes(i))); };
const siteReady = () => { const s = siteOf(); return !!(HOSTS[s.host] && s.domain); };
const svcDone = id => (id !== 'website' || siteReady()) && stepsDone(id) && SERVICES[id].keys.every(k => envKeys.have.has(k));
function stepOpen(id) { const s = stepOf(id); s.open = !s.open; try { localStorage.steps = JSON.stringify(STEPS); } catch {} drawServices(); }
function stepper(id, steps) {
  stepSync(id); const s = stepOf(id), done = stepsDone(id);
  const head = done ? `<button class="stdone${s.open ? ' open' : ''}" onclick="stepOpen('${id}')" title="${s.open ? 'Fold the steps' : 'Show the steps'}">${icon('check')}<span>All ${stepReq(steps).length} steps done</span>${icon('down')}</button>` : '';
  if (done && !s.open) return head;
  return head + (s.d.length || s.csr ? `<div class="streset"><button class="btn sm ghost" onclick="stepReset('${id}')">Reset steps</button></div>` : '')
    + `<ol class="stepper">${steps.map((x, i) => { const on = s.d.includes(i);
      return `<li${on ? ' class="done"' : ''}><button class="sn" onclick="stepTog('${id}', ${i})" title="${on ? 'Mark not done' : 'Mark done'}">${on ? icon('check') : i + 1}</button>`
        + `<span>${esc(x.t || x).replace(/\*\*(.+?)\*\*/g, '<b>$1</b>')}</span>${x.a ? `<div class="btnrow">${x.a()}</div>` : ''}${x.u ? x.u(s) : ''}</li>`; }).join('')}</ol>`;
}
// Bundle ID row: current value (copy) + Change (edit dialog w/ live check, suggestion = com.<gh owner>.<app>)
function bundleRow(stack) {
  const b = BUNDLE[cur] || {};
  return `<div class="keys${stack ? ' st' : ''}"><div><span>Bundle ID</span><small>${b.id ? `<a href="#" title="Copy" onclick="copy(BUNDLE[cur].id);return false">${esc(b.id)}</a>` : '<span class="faint">not set</span>'}
    ${b.file || b.suggest ? ` <a href="#" onclick="bundleEdit();return false">Change</a>` : ''}</small></div></div>`;
}
const BID_OK = s => /^[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)+$/.test(s);
function bundleEdit() {
  const b = BUNDLE[cur] || {}, v = b.id || b.suggest || '';
  dlg(`<h2>Bundle ID${infoI("Your app's ID at Apple. Changing it after the first release makes it a different app.")}</h2>
    ${field('ID', `<input id="bidIn" class="mono" value="${esc(v)}" oninput="bidPrev()">`)}
    ${b.suggest && b.suggest !== v ? `<div class="btnrow"><button class="btn sm ghost" onclick="$('bidIn').value='${esc(b.suggest)}';bidPrev()">Use ${esc(b.suggest)}</button></div>` : ''}
    <p id="bidMsg" class="dim" style="font-size:13px"></p>
    <div class="btns"><button class="btn ghost" onclick="closeDlg()">Cancel</button><button class="btn primary" id="bidSave" onclick="bundleSave()">Save</button></div>`);
  bidPrev(); $('bidIn').focus();
}
function bidPrev() {
  const v = $('bidIn').value.trim(), ok = BID_OK(v), b = BUNDLE[cur] || {};
  $('bidMsg').className = ok ? 'dim' : 'err';
  $('bidMsg').innerHTML = ok ? `Apple App ID <b>${esc(v)}</b> · written to <span class="mono">${esc(b.file || '(no config found)')}</span>` : 'Use letters, digits and -, split by dots, like com.you.app';
  $('bidSave').disabled = !ok || !b.file;
}
async function bundleSave() {
  try { await call('bundle_id_set', {pid: cur, id: $('bidIn').value.trim()}); delete BUNDLE[cur]; closeDlg(); drawServices(); }
  catch (e) { $('bidMsg').className = 'err'; $('bidMsg').textContent = e; }
}
const APPLE_CERTS = 'https://developer.apple.com/account/resources/certificates/add';
let cerT = 0; // Downloads poll for Apple's .cer: 3s, stops on find / project switch / 20 min
function cerWatch() {
  clearInterval(cerT); const pid = cur, t0 = Date.now();
  cerT = setInterval(async () => {
    const stop = () => { clearInterval(cerT); cerT = 0; };
    if (cur !== pid || Date.now() - t0 > 20 * 60e3) return stop();
    const f = await call('apple_cer_find', {pid}).catch(() => null);
    if (f && cerT) { stop(); appleLoad(f); }
  }, 3000);
  const el = $('cerWait'); if (el) el.textContent = ' Watching Downloads…';
}
// repo secret names cached per project ∵ gh round trip; refreshed after a send
const SECRETS = {};
async function secCount(id, fresh) {
  const pid = cur; if (fresh || !SECRETS[pid]) SECRETS[pid] = await call('gh_secrets', {pid}).catch(() => null);
  const s = SECRETS[pid], el = $('sec-' + id), k = SERVICES[id].keys;
  if (el) el.textContent = s ? `${k.filter(x => s.includes(x)).length}/${k.length} on GitHub` : '';
  if (s && SERVICES[id].steps && stepSync(id) && tab === 'svc') drawServices();
}
async function toSecrets(id, btn) {
  btn.disabled = true; $('secErr-' + id).textContent = '';
  try { await call('gh_secrets_set', {pid: cur, keys: SERVICES[id].keys}); await secCount(id, true); if (SERVICES[id].gh === 'step') stepSet(id, SERVICES[id].steps.findIndex(s => /secrets/.test(s.t))); }
  catch (e) { $('secErr-' + id).textContent = e; }
  btn.disabled = false;
}
async function appleCsr() {
  try { const f = await call('apple_csr', {pid: cur}); T.opener.openUrl(APPLE_CERTS); stepSet('macsign', 0, true, {csr: f}); cerWatch(); }
  catch (e) { say(e); }
}
async function appleCer() {
  const cer = await call('apple_cer_find', {pid: cur}).catch(() => null)
    || await T.dialog.open({filters: [{name: 'Certificate', extensions: ['cer', 'pem', 'crt']}]});
  if (cer) appleLoad(cer);
}
async function appleLoad(cer) {
  try { const who = await call('apple_p12', {pid: cur, cer}); addEntry(cur, {kind: 'note', text: 'Mac signing: ' + who}); stepSet('macsign', 2); } catch (e) { say(e); }
}
// every missing key of a service (or just `only`) in one dialog, written into the project's .env (values never go through Claude)
function fillKeys(id, only) {
  const x = SERVICES[id], miss = (only || x.keys).filter(k => !envKeys.have.has(k)), files = envKeys.files.map(f => f.file);
  if (!miss.length) return toast('Already set');
  dlg(`<h2>${esc(x.name)} keys${infoI("Saved in the project's .env file, hidden from Clauracle. Leave a field empty to skip it.")}</h2>
    <div class="sc" style="border:0;padding:0;background:none">${miss.map(k => field(`<span class="mono">${k}</span>`, /KEY$|JSON/.test(k) && /PRIVATE|JSON/.test(k)
      ? `<textarea data-k="${k}" rows="3" class="mono"></textarea>` : `<input data-k="${k}" class="mono">`)).join('')}
    ${field('File', `<select id="fkFile">${options(Object.fromEntries((files.length ? files : ['.env']).map(f => [f, f])), files[0] || '.env')}</select>`)}</div>
    <p id="fkMsg" class="err" style="font-size:13px"></p>
    <div class="btns"><button class="btn ghost" onclick="closeDlg()">Cancel</button><button class="btn primary" onclick="saveKeys()">Save</button></div>`);
  $('dlg').querySelector('[data-k]')?.focus();
}
async function saveKeys() {
  const file = $('fkFile').value;
  try { for (const el of $('dlg').querySelectorAll('[data-k]')) if (el.value.trim()) await call('env_set', {pid: cur, file, key: el.dataset.k, value: el.value.trim()}); }
  catch (e) { $('fkMsg').textContent = e; return; }
  closeDlg(); drawRight();
}
async function drawCloud() {
  const box = $('gcloud'), p = P();
  if (!box) return;
  const g = await call('gcloud_info', {pid: cur}).catch(e => ({installed: true, error: String(e)}));
  if (!$('gcloud')) return;
  if (!g.installed) {
    box.innerHTML = `<p class="hint">Cloud projects, APIs and billing go through Google's own gcloud CLI.</p><button class="btn primary block" onclick="install('gcloud', this).then(ok => ok && drawCloud())">${icon('cloud')}Install gcloud</button>`;
    return;
  }
  const list = x => Array.isArray(x) ? x : [], err = g.error || [g.accounts, g.projects].find(x => x?.error)?.error;
  const accts = list(g.accounts).map(a => a.account), projs = list(g.projects);
  const on = new Set(list(g.apis).map(a => a.config?.name || a.name?.split('/').pop()));
  const id = p.name.toLowerCase().replace(/[^a-z0-9-]+/g, '-').replace(/^[^a-z]+/, '').slice(0, 23) + '-' + Math.random().toString(36).slice(2, 6);
  const con = path => `https://console.cloud.google.com/${path}?project=${encodeURIComponent(p.gcp)}`;
  box.innerHTML = field('Google account', `<div class="row"><select class="grow" onchange="setP({gaccount:this.value}).then(drawCloud)">${options({'': 'gcloud default', ...Object.fromEntries(accts.map(a => [a, a]))}, p.gaccount)}</select>
      <button class="ib" title="Add a Google account" onclick="gLogin(this)">${icon('plus')}</button></div>`)
    + field('Cloud project', `<select onchange="setP({gcp:this.value}).then(refresh).then(drawRight)">${options({'': 'None', ...Object.fromEntries(projs.map(x => [x.projectId, `${x.name} (${x.projectId})`]))}, p.gcp)}</select>`)
    + `<div class="row"><input id="gNew" class="grow mono" value="${esc(id)}" title="6-30 characters: lowercase letters, digits, hyphens"><button class="btn" onclick="gDo('create', $('gNew').value.trim())">Create</button></div>`
    + (p.gcp ? field('Billing', `<select onchange="gDo('billing', this.value)">${options({'': g.linked?.billingEnabled ? 'Linked' : 'Not linked',
      ...Object.fromEntries(list(g.billing).filter(b => b.open).map(b => [b.name.split('/').pop(), b.displayName]))}, (g.linked?.billingAccountName || '').split('/').pop())}</select>`)
    + `<div class="fl"><span>APIs</span><div class="box" style="margin:0">${Object.entries(APIS).map(([api, l]) => optRow(esc(l), `<span class="mono">${api}</span>`,
      sw(on.has(api), `gDo('api', '${api}', this.checked, this)`))).join('')}</div></div>
    <div class="fl"><span>Sign in with Google${infoI('Set up the consent screen and a client in Google, then fill in the keys below.')}</span></div>
    <div class="btnrow"><button class="btn sm" data-url="${con('auth/overview')}">Consent screen</button><button class="btn sm" data-url="${con('auth/clients')}">Clients</button><button class="btn sm" data-url="${con('home/dashboard')}">Console</button></div>` : '')
    + `<p id="gMsg" class="${err ? 'err' : 'hint'}" style="font-size:13px;margin:0">${esc(err || '')}</p>`;
}
async function gDo(action, arg, on, el) {
  if (!arg) return;
  $('gMsg').className = 'hint'; $('gMsg').textContent = 'Working…';
  try { await call('gcloud_do', {pid: cur, action, arg, on}); if (action === 'create') { await setP({gcp: arg}); await refresh(); return drawRight(); } drawCloud(); }
  catch (e) { $('gMsg').className = 'err'; $('gMsg').textContent = e; if (el) el.checked = !on; }
}
async function gLogin(btn) {
  btn.disabled = true; $('gMsg').textContent = 'Finish signing in in your browser…';
  try { await call('gcloud_login'); drawCloud(); } catch (e) { $('gMsg').textContent = e; btn.disabled = false; }
}

const PRESETS = {
  'Google OAuth': ['GOOGLE_CLIENT_ID', 'GOOGLE_CLIENT_SECRET', 'GOOGLE_REDIRECT_URI'],
  'Apple Sign in': ['APPLE_CLIENT_ID', 'APPLE_TEAM_ID', 'APPLE_KEY_ID', 'APPLE_PRIVATE_KEY'],
  'GitHub OAuth': ['GITHUB_CLIENT_ID', 'GITHUB_CLIENT_SECRET'],
  'Stripe': ['STRIPE_SECRET_KEY', 'STRIPE_PUBLISHABLE_KEY', 'STRIPE_WEBHOOK_SECRET'],
  'Database': ['DATABASE_URL'],
};
const shown = {};
// #31: .env lives in Services as the "Secrets" fold (closed by default); redraw = the whole Services tab
const drawEnv = () => drawServices();
const secretsFold = files => fold('svc-secrets', 'Secrets' + infoI('.env files. Values stay hidden until you show them.'), envHtml(files), 0,
  `<span class="pill dim">${files.length} file${files.length === 1 ? '' : 's'}</span>`);
function envHtml(files) {
  return (P().hide_env ? '<p class="hint">Clauracle can\'t read these files.</p>' : '')
    + files.map(f => `<div class="envfile"><div class="row"><b class="grow mono">${esc(f.file)}</b>
      ${f.ignored === false ? `<span class="pill err">not git-ignored</span><button class="btn sm ghost" onclick="call('env_ignore', {pid:cur, file:'${esc(f.file)}'}).then(drawEnv)">Ignore it</button>` : f.ignored ? '<small class="ok">git-ignored</small>' : ''}</div>
      ${f.keys.map(([k, v]) => { const id = f.file + ':' + k; return `<div class="kv"><span class="k sel">${esc(k)}</span>
        <span class="row" style="gap:0"><button class="ib" title="${shown[id] ? 'Hide' : 'Show'}" onclick="shown['${esc(id)}']=!shown['${esc(id)}']; drawEnv()">${icon('eye')}</button><button class="ib" title="Copy" onclick='navigator.clipboard.writeText(${esc(JSON.stringify(v))})'>${icon('copy')}</button><button class="ib" title="Edit" onclick="editKey('${esc(f.file)}', '${esc(k)}')">${icon('pen')}</button><button class="ib" title="Delete" onclick="keyDel('${esc(f.file)}', '${esc(k)}')">${icon('trash')}</button></span>
        <span class="v sel">${v ? (shown[id] ? esc(v) : '•'.repeat(Math.min(24, v.length))) : '<i>empty</i>'}</span></div>`; }).join('')}
      <div class="row" style="margin-top:8px"><input class="grow mono" placeholder="NEW_KEY" id="nk-${esc(f.file)}"><button class="ib" title="Add" onclick="editKey('${esc(f.file)}', $('nk-${esc(f.file)}').value)">${icon('plus')}</button>
      <select onchange="preset('${esc(f.file)}', this.value); this.value=''"><option value="">Preset…</option>${Object.keys(PRESETS).map(k => `<option>${k}</option>`).join('')}</select></div></div>`).join('')
    + `<div class="row"><input id="newEnv" class="mono" value="${files.length ? '.env.local' : '.env'}" style="width:130px"><button class="btn" onclick="setKey($('newEnv').value, 'EXAMPLE_KEY', '')">New file</button></div>`;
}
async function setKey(file, key, value) {
  try { await call('env_set', {pid: cur, file, key, value}); } catch (e) { say(e); }
  drawRight();
}
async function keyDel(file, key) { if (await sure(`Delete ${key} from ${file}?\n\nThe value is removed from the file and can't be restored.`, 'Delete', 'Cancel', undefined, true)) setKey(file, key, null); }
function editKey(file, key) {
  if (!key) return;
  dlg(`<h2 class="mono">${esc(key)} <small class="dim">${esc(file)}</small></h2><textarea id="ev" rows="4" style="width:100%" class="mono" placeholder="Value (paste multi-line keys as they are)"></textarea>
    <div class="btns"><button class="btn ghost" onclick="closeDlg()">Cancel</button><button class="btn primary" onclick="setKey('${esc(file)}', '${esc(key)}', $('ev').value); closeDlg()">Save</button></div>`);
}
async function preset(file, name) {
  if (!name) return;
  const have = (await call('env_list', {pid: cur})).find(f => f.file === file)?.keys.map(k => k[0]) || [];
  for (const k of PRESETS[name]) if (!have.includes(k)) await call('env_set', {pid: cur, file, key: k, value: ''});
  drawEnv();
}

