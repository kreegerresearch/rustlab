// Mockup of the proposed git diff view, painted over the live watch page so
// the theme, layout and chrome are the real ones. Nothing here is product
// code; it is DOM surgery for screenshots.
import puppeteer from 'puppeteer-core';

const CSS = `
main section.rl-block, main .prose > * { position: relative; }
.dx::before { content: ''; position: absolute; left: -22px; top: 3px; bottom: 3px;
  width: 4px; border-radius: 2px; }
.dx-add::before { background: #a6e3a1; }
.dx-mod::before { background: #89b4fa; }
.dx-del::before { background: #f38ba8; }
ins.w { background: rgba(166,227,161,.26); color: inherit; text-decoration: none;
  border-radius: 3px; padding: 0 2px; }
del.w { background: rgba(243,139,168,.28); color: inherit; text-decoration: line-through;
  text-decoration-color: #f38ba8; border-radius: 3px; padding: 0 2px; }
p.dx-add > ins.w { padding: 1px 2px; box-decoration-break: clone; -webkit-box-decoration-break: clone; }
details.dx-removed { margin: 14px 0; }
details.dx-removed > summary { cursor: pointer; list-style: none; color: #a6adc8;
  font: 13px/1.4 system-ui, sans-serif; padding: 4px 0; }
details.dx-removed > summary::before { content: '▸ '; color: #f38ba8; }
details.dx-removed[open] > summary::before { content: '▾ '; }
details.dx-removed > summary b { color: #f38ba8; font-weight: 600; }
details.dx-removed .gone { opacity: .85; }
details.dx-removed .gone h2, details.dx-removed .gone p {
  text-decoration: line-through; text-decoration-color: #f38ba8; }
details.dx-removed .gone h2 { background: rgba(243,139,168,.22); display: inline-block; padding: 0 4px; border-radius: 3px; }
details.dx-removed .gone p span { background: rgba(243,139,168,.22); border-radius: 3px; padding: 1px 2px;
  box-decoration-break: clone; -webkit-box-decoration-break: clone; }
pre.mk-diff { padding-left: 0 !important; }
pre.mk-diff code { display: block; }
pre.mk-diff .l { display: block; padding: 0 1.2em 0 0; white-space: pre; }
pre.mk-diff .g { display: inline-block; width: 2.2em; text-align: center; color: #6c7086; user-select: none; }
pre.mk-diff .l.add { background: rgba(166,227,161,.10); }
pre.mk-diff .l.del { background: rgba(243,139,168,.10); }
pre.mk-diff .l.add .g { color: #a6e3a1; }
pre.mk-diff .l.del .g { color: #f38ba8; }
pre.mk-diff .l.add b { background: rgba(166,227,161,.32); font-weight: inherit; border-radius: 3px; }
pre.mk-diff .l.del b { background: rgba(243,139,168,.34); font-weight: inherit; border-radius: 3px;
  text-decoration: line-through; text-decoration-color: #f38ba8; }
.mk-out-note { font: 12px/1.3 system-ui, sans-serif; color: #a6adc8; margin: 4px 0 0; }
#rl-diff-banner, .rl-diff-tag, .rl-diff-note { display: none !important; }
.mk-status { position: fixed; top: 52px; right: 16px; z-index: 30; font: 13px/1.3 system-ui, sans-serif;
  color: #cdd6f4; background: #181825; border: 1px solid #313244; border-radius: 8px; padding: 7px 10px; }
.mk-status .k { color: #a6adc8; }
.mk-status .n { color: #89b4fa; }
/* menu */
.mk-panel { font: 13px/1.35 system-ui, sans-serif; color: #cdd6f4; width: 330px; }
.mk-panel .hd { font-size: 11px; text-transform: uppercase; letter-spacing: .06em; color: #a6adc8; margin: 12px 0 6px; }
.mk-panel .opt { display: grid; grid-template-columns: 18px 1fr auto; gap: 2px 8px; align-items: start;
  padding: 7px 8px; border-radius: 6px; cursor: pointer; }
.mk-panel .opt:hover { background: #313244; }
.mk-panel .opt.sel { background: rgba(203,166,247,.14); outline: 1px solid rgba(203,166,247,.5); }
.mk-panel .opt.off { opacity: .45; }
.mk-panel .dot { width: 12px; height: 12px; border-radius: 50%; border: 1.5px solid #a6adc8; margin-top: 2px; }
.mk-panel .sel .dot { border-color: #cba6f7; box-shadow: inset 0 0 0 3px #181825; background: #cba6f7; }
.mk-panel .t { font-weight: 600; }
.mk-panel .d { grid-column: 2 / 4; color: #a6adc8; font-size: 12px; }
.mk-panel .c { color: #a6adc8; font-size: 12px; white-space: nowrap; }
.mk-panel .seg { display: inline-flex; border: 1px solid #45475a; border-radius: 6px; overflow: hidden; }
.mk-panel .seg span { padding: 4px 12px; }
.mk-panel .seg span.on { background: #cba6f7; color: #1e1e2e; font-weight: 600; }
.mk-panel ul.chg { list-style: none; margin: 0; padding: 0; }
.mk-panel ul.chg li { display: grid; grid-template-columns: 10px 64px 1fr; gap: 8px; align-items: center;
  padding: 4px 6px; border-radius: 5px; cursor: pointer; }
.mk-panel ul.chg li:hover, .mk-panel ul.chg li.cur { background: #313244; }
.mk-panel ul.chg i { width: 4px; height: 14px; border-radius: 2px; display: block; }
.mk-panel ul.chg .k { color: #a6adc8; font-size: 12px; }
.mk-panel .keys { color: #6c7086; font-size: 11px; margin-top: 6px; }
.mk-panel kbd { font: 11px ui-monospace, monospace; border: 1px solid #45475a; border-radius: 3px; padding: 0 4px; color: #a6adc8; }
.mk-panel .sw { display: flex; align-items: center; gap: 8px; padding: 6px 4px; }
.mk-panel .swi { width: 30px; height: 17px; border-radius: 9px; background: #cba6f7; position: relative; }
.mk-panel .swi::after { content: ''; position: absolute; top: 2px; left: 15px; width: 13px; height: 13px; border-radius: 50%; background: #1e1e2e; }
.mk-panel hr { border: 0; border-top: 1px solid #313244; margin: 10px 0; }
.mk-panel .pick { display: grid; grid-template-columns: 62px 1fr; gap: 6px 8px; align-items: center; margin: 4px 4px 2px 30px; }
.mk-panel .pick .lab { color: #a6adc8; font-size: 12px; }
.mk-panel .fld { border: 1px solid #45475a; border-radius: 6px; padding: 4px 8px; background: #11111b;
  display: flex; justify-content: space-between; }
.mk-panel .fld.focus { border-color: #cba6f7; }
.mk-panel .arrow { text-align: center; color: #a6adc8; grid-column: 1 / 3; font-size: 12px; }
.mk-panel .list { margin: 4px 4px 0 30px; border: 1px solid #45475a; border-radius: 6px; background: #181825; padding: 4px; }
.mk-panel .list .g { font-size: 11px; text-transform: uppercase; letter-spacing: .05em; color: #6c7086; padding: 6px 6px 2px; }
.mk-panel .list .r { padding: 3px 6px; border-radius: 4px; display: flex; justify-content: space-between; gap: 10px; }
.mk-panel .list .r.hl { background: #cba6f7; color: #1e1e2e; }
.mk-panel .list .r .m { color: #a6adc8; font-size: 12px; }
.mk-panel .list .r.hl .m { color: #313244; }
.mk-panel .note { color: #a6adc8; font-size: 12px; margin: 6px 4px 0 30px; }
/* source view */
.mk-src { font: 13.5px/1.6 ui-monospace, SFMono-Regular, Menlo, monospace; background: #11111b; border-radius: 8px;
  border: 1px solid #313244; overflow: hidden; }
.mk-src .fh { font: 13px system-ui, sans-serif; color: #a6adc8; padding: 8px 12px; border-bottom: 1px solid #313244; background: #181825; }
.mk-src .l { display: grid; grid-template-columns: 3em 3em 1.6em 1fr; white-space: pre-wrap; }
.mk-src .n { color: #585b70; text-align: right; padding-right: 8px; user-select: none; }
.mk-src .l.add { background: rgba(166,227,161,.10); } .mk-src .l.del { background: rgba(243,139,168,.10); }
.mk-src .l.add .s { color: #a6e3a1; } .mk-src .l.del .s { color: #f38ba8; }
.mk-src .s { text-align: center; color: #585b70; }
.mk-src .l.add b { background: rgba(166,227,161,.32); font-weight: inherit; border-radius: 3px; }
.mk-src .l.del b { background: rgba(243,139,168,.34); font-weight: inherit; border-radius: 3px; }
.mk-src .hunk { color: #74c7ec; background: #181825; padding: 2px 12px; font-size: 12.5px; }
`;

const CODE = `<pre class="source mk-diff"><code><span class="l del"><span class="g">−</span>h <span class="syn-op">=</span> <span class="syn-fn">fir_lowpass</span>(<b><span class="syn-num">31</span></b>, <span class="syn-num">2500</span>, <span class="syn-num">10000</span>, <b><span class="syn-str">"hann"</span></b>);</span><span class="l add"><span class="g">+</span>h <span class="syn-op">=</span> <span class="syn-fn">fir_lowpass</span>(<b><span class="syn-num">63</span></b>, <span class="syn-num">2500</span>, <span class="syn-num">10000</span>, <b><span class="syn-str">"hamming"</span></b>);</span><span class="l"><span class="g"></span><span class="syn-fn">disp</span>(<span class="syn-fn">sum</span>(h))</span></code></pre>`;

const REMOVED = `<details class="dx dx-del dx-removed" id="mk-removed"><summary><b>Removed</b> · “Old notes” heading and 1 paragraph</summary>
<div class="gone"><h2>Old notes</h2>
<p><span>This section described an earlier rectangular-window design that we no longer use.</span></p></div></details>`;

const PANEL = (mode) => `<div class="mk-panel">
<div class="sw"><span class="swi"></span>Comments</div>
<div class="sw"><span class="swi"></span>Show changes</div>
<div class="hd">Compare</div>
<div class="opt ${mode==='other'?'':'sel'}"><span class="dot"></span><span class="t">Uncommitted changes</span><span class="c">4 changes</span>
  <span class="d">Your edits since the last commit</span></div>
<div class="opt"><span class="dot"></span><span class="t">This branch vs main</span><span class="c">4 changes</span>
  <span class="d">Everything on <code>feature/longer-filter</code> since it left main, like a pull request</span></div>
<div class="opt off"><span class="dot"></span><span class="t">Last commit</span><span class="c">—</span>
  <span class="d">No earlier commit to compare with</span></div>
<div class="opt ${mode==='other'?'sel':''}"><span class="dot"></span><span class="t">Other…</span><span class="c"></span>
  <span class="d">Pick any two branches, tags or commits</span></div>
${mode==='other' ? `<div class="pick"><span class="lab">base</span><span class="fld focus">v0.3<span>▾</span></span>
<span class="arrow">↓ changes from base to compare ↓</span>
<span class="lab">compare</span><span class="fld">Working copy (your files)<span>▾</span></span></div>
<div class="list"><div class="g">Branches</div>
<div class="r"><span>main</span><span class="m">2 h ago</span></div>
<div class="r"><span>feature/longer-filter</span><span class="m">current</span></div>
<div class="g">Tags</div><div class="r hl"><span>v0.3</span><span class="m">3 days ago</span></div>
<div class="g">Recent commits</div>
<div class="r"><span>5f01f1f Initial notebooks</span><span class="m">2 h ago</span></div></div>
<p class="note">Typing filters the list. A sha or <code>HEAD~3</code> works too.</p>` : ''}
<div class="hd">View</div>
<span class="seg"><span class="${mode==='source'?'':'on'}">Rendered</span><span class="${mode==='source'?'on':''}">Source</span></span>
${mode==='other' ? '' : `<hr><div class="hd" style="margin-top:0">4 changes</div>
<ul class="chg">
<li class="cur"><i style="background:#89b4fa"></i><span class="k">modified</span><span>Code cell 1</span></li>
<li><i style="background:#89b4fa"></i><span class="k">modified</span><span>Frequency response ¶1</span></li>
<li><i style="background:#f38ba8"></i><span class="k">removed</span><span>Old notes</span></li>
<li><i style="background:#a6e3a1"></i><span class="k">added</span><span>Summary ¶1</span></li></ul>
<div class="keys"><kbd>[</kbd> <kbd>]</kbd> previous / next change</div>`}
</div>`;

const SOURCE = `<div class="mk-src"><div class="fh">filters.md · uncommitted changes · 4 changes</div>
<div class="hunk">@@ Lowpass filter design @@</div>
<div class="l"><span class="n">6</span><span class="n">6</span><span class="s"></span><span class="x">\`\`\`rustlab</span></div>
<div class="l del"><span class="n">7</span><span class="n"></span><span class="s">−</span><span class="x">h = fir_lowpass(<b>31</b>, 2500, 10000, <b>"hann"</b>);</span></div>
<div class="l add"><span class="n"></span><span class="n">7</span><span class="s">+</span><span class="x">h = fir_lowpass(<b>63</b>, 2500, 10000, <b>"hamming"</b>);</span></div>
<div class="l"><span class="n">8</span><span class="n">8</span><span class="s"></span><span class="x">disp(sum(h))</span></div>
<div class="l"><span class="n">9</span><span class="n">9</span><span class="s"></span><span class="x">\`\`\`</span></div>
<div class="hunk">@@ Frequency response @@</div>
<div class="l"><span class="n">13</span><span class="n">13</span><span class="s"></span><span class="x">The response falls off smoothly past the cutoff, with about</span></div>
<div class="l del"><span class="n">14</span><span class="n"></span><span class="s">−</span><span class="x"><b>40</b> dB of stopband attenuation for <b>31</b> taps.</span></div>
<div class="l add"><span class="n"></span><span class="n">14</span><span class="s">+</span><span class="x"><b>53</b> dB of stopband attenuation for <b>63</b> taps.</span></div>
<div class="l"><span class="n">15</span><span class="n">15</span><span class="s"></span><span class="x"></span></div>
<div class="l del"><span class="n">16</span><span class="n"></span><span class="s">−</span><span class="x">## Old notes</span></div>
<div class="l del"><span class="n">17</span><span class="n"></span><span class="s">−</span><span class="x"></span></div>
<div class="l del"><span class="n">18</span><span class="n"></span><span class="s">−</span><span class="x">This section described an earlier rectangular-window design that we no</span></div>
<div class="l del"><span class="n">19</span><span class="n"></span><span class="s">−</span><span class="x">longer use.</span></div>
<div class="l del"><span class="n">20</span><span class="n"></span><span class="s">−</span><span class="x"></span></div>
<div class="l"><span class="n">21</span><span class="n">16</span><span class="s"></span><span class="x">## Summary</span></div>
<div class="l"><span class="n">22</span><span class="n">17</span><span class="s"></span><span class="x"></span></div>
<div class="l add"><span class="n"></span><span class="n">18</span><span class="s">+</span><span class="x">Doubling the taps doubles the cost of each output sample.</span></div>
<div class="l add"><span class="n"></span><span class="n">19</span><span class="s">+</span><span class="x"></span></div>
<div class="l"><span class="n">23</span><span class="n">20</span><span class="s"></span><span class="x">Longer filters give a sharper transition band.</span></div>
</div>`;

async function build(p, {source = false} = {}) {
  await p.goto('http://127.0.0.1:8097/n/filters?diff=HEAD', {waitUntil: 'networkidle0'});
  await p.evaluate((css, code, removed, source, src) => {
    const st = document.createElement('style'); st.textContent = css; document.head.appendChild(st);
    const s = document.createElement('div'); s.className = 'mk-status';
    s.innerHTML = '<span class="k">Showing</span> uncommitted changes <span class="k">· vs last commit 5f01f1f ·</span> <span class="n">4 changes</span>';
    document.body.appendChild(s);
    const secs = document.querySelectorAll('main section.rl-block');
    // code cell: one source, the diff itself; no second copy
    const cell = secs[1];
    cell.classList.remove('rl-diff', 'rl-diff-chg');
    cell.classList.add('dx', 'dx-mod');
    cell.querySelector('.rl-diff-lines').remove();
    cell.querySelector('pre.source').outerHTML = code;
    const out = cell.querySelector('pre.output');
    const note = document.createElement('p'); note.className = 'mk-out-note';
    note.textContent = 'Output is from your current file.';
    out.after(note);
    // prose: per paragraph, word level
    const prose = secs[2];
    prose.classList.remove('rl-diff', 'rl-diff-chg');
    const ps = prose.querySelectorAll('.prose > *');
    ps[1].classList.add('dx', 'dx-mod');
    ps[1].innerHTML = 'The response falls off smoothly past the cutoff, with about <del class="w">40</del><ins class="w">53</ins> dB of stopband attenuation for <del class="w">31</del><ins class="w">63</ins> taps.';
    ps[1].insertAdjacentHTML('afterend', removed);
    ps[3].classList.add('dx', 'dx-add');
    ps[3].innerHTML = '<ins class="w">' + ps[3].textContent + '</ins>';
    if (source) {
      document.querySelector('main').innerHTML = '<div style="margin-top:72px">' + src + '</div>';
    }
  }, CSS, CODE, REMOVED, source, SOURCE);
  await new Promise(r => setTimeout(r, 300));
}

async function openMenu(p, mode) {
  await p.click('#rl-more-btn');
  await p.evaluate((html) => {
    const panel = document.getElementById('rl-more-panel');
    panel.innerHTML = html;
  }, PANEL(mode));
  await new Promise(r => setTimeout(r, 200));
}

const b = await puppeteer.launch({executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true});
const p = await b.newPage();
await p.setViewport({width: 1280, height: 900, deviceScaleFactor: 1.5});

await build(p);
await p.screenshot({path: 'after-page.png', fullPage: true});
await p.evaluate(() => { document.getElementById('mk-removed').open = true; });
const r = await p.evaluate(() => { const e = document.getElementById('mk-removed').closest('section'); const b = e.getBoundingClientRect(); return {x: b.x - 40, y: b.y + window.scrollY, w: b.width + 60, h: b.height}; });
await p.screenshot({path: 'after-removed-open.png', clip: {x: r.x, y: r.y, width: r.w, height: r.h}, captureBeyondViewport: true});

await build(p);
await openMenu(p, 'presets');
await p.screenshot({path: 'after-menu.png', clip: {x: 760, y: 0, width: 520, height: 720}});

await build(p);
await openMenu(p, 'other');
await p.screenshot({path: 'after-other.png', clip: {x: 760, y: 0, width: 520, height: 800}});

await build(p, {source: true});
await openMenu(p, 'source');
await p.evaluate(() => { document.getElementById('rl-more-btn').click(); });
await p.screenshot({path: 'after-source.png', fullPage: true});

await b.close();
