// Screenshots of the shipped phase A view (not a mockup). Run against
// `rustlab-notebook watch <demo>/nb --git-diff --port 8097 -t <theme>`:
//   node phase-a-shots.mjs <theme> <out-dir>
import puppeteer from 'puppeteer-core';
const theme = process.argv[2];
const out = process.argv[3];
const b = await puppeteer.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless:true, args:['--no-first-run']});
const p = await b.newPage();
const errs = [];
p.on('pageerror', e => errs.push(String(e)));
await p.setViewport({width:1280, height:900, deviceScaleFactor:1.5});
const base = 'http://127.0.0.1:8097/n/filters';
await p.goto(base + '?changes=uncommitted', {waitUntil:'networkidle0'});
await new Promise(r => setTimeout(r, 400));
await p.screenshot({path: `${out}/${theme}-page.png`, fullPage: true});
await p.click('#rl-diff-banner');
await new Promise(r => setTimeout(r, 800));
await p.screenshot({path: `${out}/${theme}-menu.png`, clip: {x: 700, y: 0, width: 580, height: 900}});
await p.click('.rl-dx-opt[data-preset="custom"]');
await new Promise(r => setTimeout(r, 800));
await p.screenshot({path: `${out}/${theme}-other.png`, clip: {x: 700, y: 0, width: 580, height: 900}});
await p.keyboard.press('Escape');
await p.keyboard.press('Escape');
await p.goto(base + '?changes=uncommitted', {waitUntil:'networkidle0'});
await p.evaluate(() => { const d = document.querySelector('details.rl-dx-removed'); if (d) d.open = true; });
await new Promise(r => setTimeout(r, 300));
const r = await p.evaluate(() => { const d = document.querySelector('details.rl-dx-removed'); if (!d) return null; const e = d.closest('section'); const b = e.getBoundingClientRect(); return {x: Math.max(0, b.x - 40), y: b.y + window.scrollY, w: b.width + 60, h: b.height}; });
if (r) await p.screenshot({path: `${out}/${theme}-removed-open.png`, clip: {x: r.x, y: r.y, width: r.w, height: r.h}, captureBeyondViewport: true});
await p.goto(base + '?changes=uncommitted&view=source', {waitUntil:'networkidle0'});
await new Promise(r => setTimeout(r, 300));
await p.screenshot({path: `${out}/${theme}-source.png`, fullPage: true});
console.log(theme, 'errors:', JSON.stringify(errs));
await b.close();
