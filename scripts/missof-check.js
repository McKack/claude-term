// #167 check: node scripts/missof-check.js (regexes from ui/index.html missOf)
const s=require('fs').readFileSync(require('path').join(__dirname, '../ui/index.html'),'utf8');const a=s.indexOf('const TOOLPKG'),b=s.indexOf('const MISSED');
global.navigator={userAgent:'Windows'};eval(s.slice(a,b).replace('const TOOLPKG','globalThis.TOOLPKG')+';globalThis.missOf=missOf');const A=require('assert');
A.deepEqual(missOf("error[E0599]: no method named `into_json` found for struct `ureq::Response` in the current scope\n = note: the item is gated behind the `json` feature"),{k:'cargo',n:'ureq',f:'json',label:'Enable the json feature of ureq'});
A.equal(missOf("Error: Cannot find module '@tauri-apps/api/core'").n,'@tauri-apps/api');
A.equal(missOf("Cannot find module './x'"),null);
A.equal(missOf("ModuleNotFoundError: No module named 'requests'").k,'pip');
A.equal(missOf("/usr/bin/bash: line 1: jq: command not found").pkg,'jqlang.jq');
A.equal(missOf("'node' is not recognized as an internal or external command").n,'node');
A.equal(missOf("The term 'gh.exe' is not recognized as the name").n,'gh');
A.equal(missOf("foo: command not found"),null);
console.log('missOf ok');
