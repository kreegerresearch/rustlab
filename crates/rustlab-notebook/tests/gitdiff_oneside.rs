//! Browser recovery for a notebook that exists on only one side.
//!
//! Spawns the real `watch --git-diff` server and drives headless Chrome:
//! missing on the compared ref (an old `?diff=` link), then a base that
//! has the file, the presets, Show changes off/on, an edit, a reload with
//! `sessionStorage`, two refs, the per-preset counts and change list, and
//! the reverse (missing in the working copy). Skips when git or Chrome is
//! absent, or when the binary cannot open a debug port, so `cargo test`
//! still passes on a machine whose Chrome cannot be driven.
//! The diff computation itself is pinned in `gitdiff::tests`.

use std::fs;
use std::io::Read;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn git(dir: &std::path::Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?}\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn chrome() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("CHROME") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    [
        "/usr/bin/google-chrome-stable",
        "/usr/local/bin/google-chrome",
        "/usr/bin/google-chrome",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.is_file())
}

struct Kill(std::process::Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn oneside_watch_recovers_in_the_browser() {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("SKIP oneside browser: git not found");
        return;
    }
    let Some(chrome) = chrome() else {
        eprintln!("SKIP oneside browser: chrome not found");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    fs::write(repo.join("readme.md"), "# Readme\n\nOnly this.\n").unwrap();
    git(&repo, &["add", "readme.md"]);
    git(&repo, &["commit", "-m", "readme"]);
    git(&repo, &["branch", "ancient"]);
    let note = "# Note\n\nShared.\n\n```rustlab\nx = 1\ny = 2\n```\n";
    fs::write(repo.join("note.md"), note).unwrap();
    git(&repo, &["add", "note.md"]);
    git(&repo, &["commit", "-m", "add note"]);
    git(&repo, &["checkout", "-b", "dropped"]);
    git(&repo, &["rm", "note.md"]);
    git(&repo, &["commit", "-m", "drop note"]);
    git(&repo, &["checkout", "main"]);
    fs::write(
        repo.join("note.md"),
        "# Note\n\nShared.\n\n```rustlab\nx = 1\ny = 3\n```\n",
    )
    .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut child = Command::new(env!("CARGO_BIN_EXE_rustlab-notebook"))
        .args([
            "watch",
            repo.to_str().unwrap(),
            "--git-diff",
            "--no-browser",
            "--port",
            &port.to_string(),
            "-t",
            "mocha",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("watch");
    let mut err = child.stderr.take().unwrap();
    let _guard = Kill(child);
    let base = format!("http://127.0.0.1:{port}/n/note");
    let start = Instant::now();
    let mut up = false;
    while start.elapsed() < Duration::from_secs(30) {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            up = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(up, "watch did not listen on {port}");

    let script = tmp.path().join("drive.mjs");
    fs::write(&script, DRIVER).unwrap();
    let out = Command::new("timeout")
        .args(["90", "node"])
        .arg(&script)
        .env("ONESIDE_URL", &base)
        .env("ONESIDE_CHROME", &chrome)
        .env("ONESIDE_REPO", &repo)
        .env("ONESIDE_PROFILE", tmp.path().join("chrome-profile"))
        .output()
        .expect("node");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    // Drop the server before reading its pipe. Otherwise a failed node
    // blocks forever on stderr, because the child still holds the write end.
    drop(_guard);
    let mut logged = String::new();
    let _ = err.read_to_string(&mut logged);
    // GitHub's preinstalled Chrome is a real binary, but on that image it
    // often never opens the debug port. The diff itself is pinned in the
    // library test; this browser pass runs only where Chrome can be driven.
    if stdout.contains("ONESIDE_SKIP") {
        eprintln!("SKIP oneside browser: chrome debug port did not open\n{stderr}");
        return;
    }
    if std::env::var_os("ONESIDE_VERBOSE").is_some() {
        eprintln!("{stdout}");
    }
    if !out.status.success() || !stdout.contains("ONESIDE_OK") {
        panic!("browser flow failed\nstdout:\n{stdout}\nstderr:\n{stderr}\nserver:\n{logged}");
    }
}

const DRIVER: &str = r#"
import { spawn } from 'node:child_process';
import { setTimeout as sleep } from 'node:timers/promises';
import { readFileSync, writeFileSync } from 'node:fs';

const base = process.env.ONESIDE_URL;
const chromeBin = process.env.ONESIDE_CHROME;
const repo = process.env.ONESIDE_REPO;
const profile = process.env.ONESIDE_PROFILE;
const port = 9333 + Math.floor(Math.random() * 1000);
const chrome = spawn(chromeBin, [
  '--headless=new', '--disable-gpu', '--no-sandbox', '--disable-dev-shm-usage',
  '--remote-allow-origins=*', '--remote-debugging-address=127.0.0.1',
  '--remote-debugging-port=' + port,
  '--user-data-dir=' + profile,
  '--window-size=1280,900', 'about:blank',
], { stdio: ['ignore', 'ignore', 'pipe'] });
let chromeErr = '';
chrome.stderr.on('data', (d) => {
  chromeErr += d.toString();
  if (chromeErr.length > 4000) chromeErr = chromeErr.slice(-4000);
});
function done(code) { try { chrome.kill(); } catch {} process.exit(code); }
async function json(url) { return (await fetch(url)).json(); }
let ws;
for (let i = 0; i < 150 && chrome.exitCode === null; i++) {
  let debugPort = String(port);
  try {
    const active = readFileSync(profile + '/DevToolsActivePort', 'utf8').split('\n')[0].trim();
    if (active) debugPort = active;
  } catch {}
  try {
    const list = await json('http://127.0.0.1:' + debugPort + '/json');
    const page = list.find(t => t.type === 'page' && t.webSocketDebuggerUrl);
    if (page) { ws = new WebSocket(page.webSocketDebuggerUrl); break; }
  } catch {}
  await sleep(100);
}
if (!ws) {
  console.log('ONESIDE_SKIP');
  console.error(chromeErr || 'chrome did not open a debug port');
  done(0);
}
await new Promise(res => ws.addEventListener('open', res));
let seq = 0;
const pending = new Map();
ws.addEventListener('message', ev => {
  const msg = JSON.parse(ev.data);
  if (msg.id && pending.has(msg.id)) { pending.get(msg.id)(msg); pending.delete(msg.id); }
});
function send(method, params = {}) {
  const id = ++seq;
  return new Promise((res, rej) => {
    pending.set(id, res);
    ws.send(JSON.stringify({ id, method, params }));
    setTimeout(() => { if (pending.has(id)) { pending.delete(id); rej(new Error('timeout ' + method)); } }, 10000);
  });
}
async function evalJs(expression) {
  const msg = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
  // A navigation destroys the execution context. Chrome then answers
  // without `result` (an error object, or an empty reply). Callers that
  // are waiting for the next page retry; a missing reply is not a script error.
  if (!msg.result || !msg.result.result) {
    throw new Error('no-result ' + JSON.stringify(msg.error || msg));
  }
  if (msg.result.exceptionDetails) {
    throw new Error(JSON.stringify(msg.result.exceptionDetails));
  }
  return msg.result.result.value;
}
async function goto(url) {
  await send('Page.navigate', { url });
  for (let i = 0; i < 40; i++) {
    try {
      const state = await evalJs('document.readyState');
      const banner = await evalJs('!!document.querySelector("main")');
      if (state === 'complete' && banner) break;
    } catch (e) {}
    await sleep(100);
  }
  await sleep(250);
}
function state() {
  return evalJs(`(() => {
    const banner = document.getElementById('rl-diff-banner');
    const notice = document.querySelector('main .rl-diff-notice');
    return {
      href: location.href,
      banner: banner && !banner.hidden ? banner.textContent : '',
      marks: [...document.querySelectorAll('main [data-rl-dx]')].map(e => e.getAttribute('data-rl-dx')),
      notice: notice ? notice.textContent : '',
      errors: window.__errs || [],
      picker: !!document.getElementById('rl-dx-presets'),
      body: document.body ? document.body.className : '',
      text: (document.querySelector('main') && document.querySelector('main').innerText || '').slice(0, 400),
      pref: sessionStorage.getItem('rl-diff-pref'),
      words: document.querySelectorAll('main ins.rl-w, main del.rl-w').length,
      source: !!document.querySelector('main .rl-dx-srcview'),
    };
  })()`);
}
function check(name, cond, snap) {
  if (!cond) {
    console.error('FAIL ' + name + ' ' + JSON.stringify(snap));
    done(1);
  }
  console.log('ok ' + name);
}
await send('Page.enable');
await send('Runtime.enable');
await send('Page.addScriptToEvaluateOnNewDocument', {
  source: 'window.__errs=[]; addEventListener("error", e => window.__errs.push(String(e.message)));'
});

// An old 0.3.12 link: `diff` is the before side.
await goto(base + '?diff=ancient');
let s = await state();
check('missing-on-ancient', s.banner.includes('ancient → working copy') && s.banner.includes('new file, not on ancient') && s.notice.includes('not on ancient') && !s.marks.length && s.errors.length === 0 && s.picker, s);

async function openMenu() {
  return evalJs(`(() => {
    var panel = document.getElementById('rl-more-panel');
    if (panel && panel.hidden) document.getElementById('rl-more-btn').click();
    return !document.getElementById('rl-more-panel').hidden;
  })()`);
}
async function pick(which, typed) {
  const id = which === 'compare' ? 'rl-diff-compare' : 'rl-diff-base';
  const list = '#' + id + '-list li';
  await openMenu();
  return evalJs(`(async () => {
    var other = document.querySelector('.rl-dx-opt[data-preset="custom"]');
    if (document.getElementById('rl-dx-other').hidden) other.click();
    var input = document.getElementById(${JSON.stringify(id)});
    input.focus();
    var hit = null;
    for (var i = 0; i < 60; i++) {
      input.value = ${JSON.stringify(typed)};
      input.dispatchEvent(new Event('input', { bubbles: true }));
      var lis = [...document.querySelectorAll(${JSON.stringify(list)})];
      hit = lis.find(li => li.getAttribute('role') === 'option' && (li.textContent || '').toLowerCase().indexOf(${JSON.stringify(typed)}) !== -1);
      if (hit) break;
      await new Promise(r => setTimeout(r, 50));
    }
    if (!hit) return 'no-row';
    hit.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true }));
    return 'clicked';
  })()`);
}
async function settle(pred) {
  let s = null;
  for (let i = 0; i < 60; i++) {
    try {
      const ready = await evalJs('document.readyState');
      s = await state();
      if (ready === 'complete' && s && s.picker && pred(s)) return s;
    } catch (e) {}
    await sleep(100);
  }
  console.error('FAIL settle ' + JSON.stringify(s));
  done(1);
  return s;
}
async function clickPick(which, typed) {
  let result = 'navigated';
  try { result = await pick(which, typed); } catch (e) {}
  if (result === 'no-row') {
    console.error('FAIL no-row ' + which + ' ' + typed);
    done(1);
  }
}
async function clickJs(js) {
  try { await evalJs(js); } catch (e) {}
}
async function clickToggle() {
  await clickJs(`document.getElementById('rl-diff-toggle').click()`);
}
async function clickPreset(key) {
  await openMenu();
  await clickJs(`document.querySelector('.rl-dx-opt[data-preset="${key}"]').click()`);
}
const qs = (s) => new URL(s.href).searchParams;

await clickPick('base', 'main');
s = await settle(s => qs(s).get('changes') === 'custom' && qs(s).get('base') === 'main' && s.banner.includes('1 change'));
check('base-main', !qs(s).get('diff') && s.banner.includes('main → working copy') && s.marks.includes('modified') && s.text.includes('y = 3') && s.errors.length === 0, s);
check('pref-follows-main', s.pref && s.pref.includes('"base":"main"') && s.pref.includes('"changes":"custom"'), s);

await clickPreset('uncommitted');
s = await settle(s => qs(s).get('changes') === 'uncommitted');
check('preset-uncommitted', s.banner.includes('Showing uncommitted changes') && s.banner.includes('vs last commit') && !qs(s).get('base') && s.errors.length === 0, s);

await clickToggle();
s = await settle(s => qs(s).get('changes') === 'off' && !s.body.includes('rl-diff-on'));
check('toggle-off', !s.marks.length && !s.banner && s.errors.length === 0, s);
await clickToggle();
s = await settle(s => qs(s).get('changes') === 'uncommitted' && s.banner.includes('Showing uncommitted changes'));
check('toggle-on-uncommitted', !s.banner.includes('new file') && s.errors.length === 0, s);

// A 0.3.12 saved choice still restores.
await evalJs(`sessionStorage.setItem('rl-diff-pref', JSON.stringify({on:true, diff:'ancient', base:''}))`);
await goto(base);
s = await settle(s => qs(s).get('base') === 'ancient' && s.banner.includes('new file'));
check('pref-restores-ancient-as-notice', qs(s).get('changes') === 'custom' && s.notice.includes('not on ancient') && !s.marks.length, s);
await goto(base + '?changes=custom&base=main');
s = await settle(s => qs(s).get('base') === 'main' && s.banner.includes('main → working copy'));
check('query-beats-pref', !s.banner.includes('new file') && (s.pref || '').includes('"base":"main"'), s);

writeFileSync(repo + '/note.md', '# Note\n\nShared.\n\n```rustlab\nx = 1\ny = 9\n```\n');
let saw = false;
for (let i = 0; i < 40; i++) {
  s = await state();
  if (s.text.includes('y = 9') && s.banner.includes('main → working copy')) { saw = true; break; }
  await sleep(150);
}
if (!saw) {
  await goto(base + '?changes=custom&base=main');
  s = await state();
}
check('edit-then-reload', s.text.includes('y = 9') && s.banner.includes('1 change') && !s.banner.includes('new file'), s);
check('word-marks', s.words >= 2, s);

// Rendered | Source.
await openMenu();
await clickJs(`document.querySelector('.rl-dx-seg [data-view="source"]').click()`);
s = await settle(s => qs(s).get('view') === 'source' && s.source);
check('source-view', qs(s).get('base') === 'main' && s.text.includes('y = 9') && s.text.includes('y = 2') && s.marks.includes('modified') && s.words >= 2 && s.errors.length === 0, s);
await openMenu();
await clickJs(`document.querySelector('.rl-dx-seg [data-view="rendered"]').click()`);
s = await settle(s => !qs(s).get('view') && !s.source);
check('rendered-view', qs(s).get('base') === 'main' && s.marks.includes('modified'), s);

// Counts and the change list in the menu.
await openMenu();
let menu = null;
for (let i = 0; i < 40; i++) {
  menu = await evalJs(`(() => ({
    unc: document.querySelector('.rl-dx-opt[data-preset="uncommitted"] .rl-dx-c').textContent,
    last: document.querySelector('.rl-dx-opt[data-preset="last"] .rl-dx-c').textContent,
    lastOff: document.querySelector('.rl-dx-opt[data-preset="last"]').disabled,
    branchHidden: document.querySelector('.rl-dx-opt[data-preset="branch"]').hidden,
    rows: [...document.querySelectorAll('#rl-dx-list li')].map(li => li.textContent),
  }))()`);
  if (menu.unc) break;
  await sleep(100);
}
check('menu-counts', menu.unc === '1 change' && menu.last === 'new file' && !menu.lastOff && menu.branchHidden, menu);
check('change-list', menu.rows.length === 1 && menu.rows[0].includes('modified') && menu.rows[0].includes('Code cell 1'), menu);

await goto(base + '?changes=custom&base=main&compare=dropped');
s = await settle(s => qs(s).get('compare') === 'dropped' && s.banner.includes('not on dropped'));
check('two-ref-deleted-keeps-chrome', s.banner.includes('main → dropped') && s.banner.includes('code was not run') && s.picker && s.body.includes('rl-diff-on') && s.errors.length === 0 && s.marks.includes('removed') && s.text.length > 0, s);
await clickPick('compare', 'working');
s = await settle(s => qs(s).get('base') === 'main' && !qs(s).get('compare') && s.banner.includes('main → working copy'));
check('compare-back-to-working-copy', s.banner.includes('1 change') && s.errors.length === 0 && s.picker, s);

const fs = await import('node:fs');
fs.renameSync(repo + '/note.md', repo + '/note.md.bak');
await goto(base + '?changes=custom&base=main');
s = await settle(s => s.banner.includes('deleted'));
check('deleted-worktree', s.banner.includes('deleted from the working copy') && s.notice.includes('still on main') && s.marks.includes('removed') && s.picker && s.errors.length === 0, s);
fs.renameSync(repo + '/note.md.bak', repo + '/note.md');
await goto(base + '?changes=custom&base=main');
s = await settle(s => !s.banner.includes('deleted') && s.banner.includes('1 change'));
check('restored-worktree', s.errors.length === 0 && s.picker, s);

console.log('ONESIDE_OK');
done(0);
"#;
