const assert = require('node:assert/strict');
const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require('playwright');

const root = path.resolve(__dirname, '../ui');
const server = http.createServer((req, res) => {
  const pathname = decodeURIComponent(new URL(req.url, 'http://localhost').pathname);
  const file = path.resolve(root, pathname === '/' ? 'index.html' : '.' + pathname);
  if (!file.startsWith(root + path.sep)) return res.writeHead(403).end();
  fs.readFile(file, (err, data) => {
    if (err) return res.writeHead(404).end();
    res.setHeader('Content-Type', {'.html':'text/html', '.js':'text/javascript', '.css':'text/css', '.ttf':'font/ttf'}[path.extname(file)] || 'application/octet-stream');
    res.end(data);
  });
});

async function instrument(page, native) {
  await page.addInitScript(({ native }) => {
    window.previewDraws = 0;
    const getContext = HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext = function (...args) {
      const context = getContext.apply(this, args);
      if (context && !context.instrumented) {
        const clear = context.clearRect.bind(context);
        context.clearRect = (...values) => { window.previewDraws++; return clear(...values); };
        context.instrumented = true;
      }
      return context;
    };
    if (!native) return;
    window.calls = [];
    window.mockMode = null;
    window.__TAURI__ = {
      event: { listen: async () => () => {} },
      core: { invoke: async (name, args = {}) => {
        window.calls.push({ name, args });
        if (name === 'get_controller_config') return JSON.parse(localStorage.getItem('mock-native-config') || 'null');
        if (name === 'save_controller_config') { localStorage.setItem('mock-native-config', JSON.stringify(args.config)); return; }
        if (name === 'device_status') return { open:true, found:true, totalLeds:54, sections:[12,30,12] };
        if (name === 'get_saved_state') return { sections:[12,30,12], brightness:119, r:0, g:0, b:0 };
        if (name === 'list_audio_sources') return [['test.monitor', 'Test playback']];
        if (name === 'list_screen_outputs') return [{ name:'DP-1', size:{width:1920,height:1080} }];
        if (name === 'effects_stop') window.mockMode = null;
        if (name === 'effects_start') window.mockMode = args.name;
        if (name === 'effects_status') return { running:!!window.mockMode, active:window.mockMode };
        if (name === 'ambilight_status') return { running:true, metrics:{ frames:32, output:'DP-1', achieved_fps:24.5, frame_ms:25, capture_ms:14, processing_ms:2, write_ms:9, capture_width:672, capture_height:378, capture_scale:.35 } };
        if (name === 'audio_status') return { running:true };
        return { ok:true };
      } }
    };
  }, { native });
}

(async () => {
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const browser = await chromium.launch({ headless:true, args:['--disable-gpu'] });
  const base = `http://127.0.0.1:${server.address().port}`;
  const errors = [];
  try {
    const page = await browser.newPage({ viewport:{width:1280,height:900} });
    page.on('pageerror', e => errors.push(e.message));
    await instrument(page, true);
    await page.goto(base);
    await page.waitForFunction(() => !document.getElementById('power').disabled);
    if (process.argv.includes('--measure')) {
      await page.locator('[data-view="lighting"]').click();
      await page.waitForTimeout(450);
      const before = await page.evaluate(() => ({draws:window.previewDraws,saves:window.calls.filter(c => c.name === 'save_controller_config').length}));
      await page.locator('#speed').evaluate(input => {
        for (let i=0;i<30;i++) { input.value=String(1+i%10); input.dispatchEvent(new Event('input',{bubbles:true})); }
      });
      await page.waitForTimeout(1000);
      const after = await page.evaluate(() => ({draws:window.previewDraws,saves:window.calls.filter(c => c.name === 'save_controller_config').length}));
      console.log(JSON.stringify({sliderEvents:30,nativeSaves:after.saves-before.saves,previewDraws:after.draws-before.draws,windowMs:1000}));
      return;
    }
    await page.locator('[data-view="presets"]').click();
    assert.equal(await page.locator('.preset-row').count(), 60, 'offer 60 built-in scenes');
    const ids = await page.locator('[data-preset]').evaluateAll(nodes => nodes.map(n => n.dataset.preset));
    assert.equal(new Set(ids).size, 60, 'every built-in scene has a unique ID');
    await page.locator('#presetSearch').fill('  OCEAN  ');
    assert.ok(await page.locator('.preset-row').count() >= 3, 'search ignores case and surrounding whitespace');
    await page.locator('#presetSearch').fill('nothing-matches-this-scene');
    assert.equal(await page.locator('.preset-row').count(), 0);
    await page.waitForFunction(() => document.activeElement?.id === 'presetSearch');
    assert.match(await page.locator('.empty-state').textContent(), /No scenes match/);
    await page.locator('#clearPresetSearch').click();
    await page.locator('[data-favorite="warm"]').click();
    await page.locator('#presetFilter').selectOption('favorites');
    assert.equal(await page.locator('.preset-row').count(), 1);
    await page.waitForTimeout(350);
    await page.reload();
    await page.locator('[data-view="presets"]').click();
    await page.locator('#presetFilter').selectOption('favorites');
    assert.equal(await page.locator('[data-favorite="warm"]').getAttribute('aria-pressed'), 'true');
    await page.locator('#presetFilter').selectOption('screen');
    assert.equal(await page.locator('.preset-row').count(), 8);
    const before = await page.evaluate(() => JSON.parse(localStorage.getItem('snzhy-controller')));
    await page.locator('[data-run-preset="screen-gaming"]').click();
    await page.waitForFunction(() => window.calls.some(c => c.name === 'ambilight_start' && c.args.fps === 30 && c.args.captureScale === .2));
    const after = await page.evaluate(() => JSON.parse(localStorage.getItem('snzhy-controller')));
    assert.deepEqual(after.sections, before.sections, 'built-ins preserve calibrated LED layout');
    assert.equal(after.source, before.source);
    assert.equal(after.output, before.output);
    await page.locator('[data-view="screen"]').click();
    await page.locator('#captureScale').fill('0.5');
    await page.locator('#captureScale').dispatchEvent('input');
    await page.waitForFunction(() => window.calls.some(c => c.name === 'ambilight_start' && c.args.captureScale === .5));
    await page.waitForFunction(() => document.getElementById('screenPerformance')?.textContent.includes('24.5'));
    assert.match(await page.locator('#screenPerformance').textContent(), /672.*378/);
    await page.locator('#stopMode').click();
    await page.locator('[data-view="lighting"]').click();
    assert.equal(await page.locator('[data-effect]').count(), 20, 'expose 20 lighting effects');
    await page.locator('[data-effect="aurora"]').click();
    await page.locator('#startMode').click();
    await page.waitForFunction(() => window.calls.some(c => c.name === 'effects_start' && c.args.name === 'aurora'));
    await page.locator('#stopMode').click();
    await page.waitForTimeout(450);
    const savesBefore = await page.evaluate(() => window.calls.filter(c => c.name === 'save_controller_config').length);
    await page.locator('#speed').evaluate(input => {
      for (let i = 0; i < 30; i++) { input.value = String(1 + i % 10); input.dispatchEvent(new Event('input', {bubbles:true})); }
    });
    await page.waitForFunction(() => JSON.parse(localStorage.getItem('mock-native-config')).speed === 10);
    const savesAfter = await page.evaluate(() => window.calls.filter(c => c.name === 'save_controller_config').length);
    assert.ok(savesAfter - savesBefore <= 2, `coalesce slider saves (saw ${savesAfter - savesBefore})`);
    await page.locator('#saveScene').click();
    await page.locator('#presetName').fill('My Aurora');
    await page.locator('#presetForm button[type="submit"]').click();
    await page.waitForFunction(() => JSON.parse(localStorage.getItem('mock-native-config')).sceneLibrary?.scenes.some(s => s.name === 'My Aurora'));
    await page.evaluate(() => { localStorage.removeItem('snzhy-scenes'); localStorage.removeItem('snzhy-favorites'); });
    await page.reload();
    await page.locator('[data-view="presets"]').click();
    await page.locator('#presetFilter').selectOption('personal');
    assert.equal(await page.getByText('My Aurora', {exact:true}).count(), 1, 'restore personal scenes from native persistence');
    await page.locator('#presetFilter').selectOption('favorites');
    assert.equal(await page.locator('[data-favorite="warm"]').getAttribute('aria-pressed'), 'true');
    await page.locator('#presetFilter').selectOption('all');
    const output = path.join(__dirname, '../.impeccable/review');
    fs.mkdirSync(output, {recursive:true});
    await page.screenshot({path:path.join(output, 'enhanced-presets-desktop.png'), fullPage:true});
    await page.setViewportSize({width:390,height:844});
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, 'scene library fits narrow screens');
    await page.screenshot({path:path.join(output, 'enhanced-presets-mobile.png'), fullPage:true});
    await page.locator('[data-view="screen"]').click();
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, 'capture controls fit narrow screens');

    const preview = await browser.newPage();
    preview.on('pageerror', e => errors.push(e.message));
    await instrument(preview, false);
    await preview.goto(base);
    await preview.waitForTimeout(200);
    const drawBefore = await preview.evaluate(() => window.previewDraws);
    await preview.waitForTimeout(400);
    assert.equal(await preview.evaluate(() => window.previewDraws), drawBefore, 'static preview stops repeated rendering');
    await preview.locator('[data-view="lighting"]').click();
    await preview.locator('[data-effect="aurora"]').click();
    const animatedBefore = await preview.evaluate(() => window.previewDraws);
    await preview.waitForTimeout(300);
    assert.ok(await preview.evaluate(() => window.previewDraws) > animatedBefore, 'animated effects continue rendering');
    await preview.emulateMedia({reducedMotion:'reduce'});
    await preview.waitForTimeout(150);
    const reducedBefore = await preview.evaluate(() => window.previewDraws);
    await preview.waitForTimeout(300);
    assert.equal(await preview.evaluate(() => window.previewDraws), reducedBefore, 'reduced motion settles the preview');
    assert.deepEqual(errors, []);
    console.log(`PASS: 60 scenes, 20 effects, search, favorites, native scene restore, scaled-capture wiring, metrics, coalesced saves (${savesAfter - savesBefore} for 30 events), and idle/reduced-motion rendering.`);
  } finally {
    await browser.close();
    server.close();
  }
})().catch(error => { console.error(error); server.close(); process.exitCode = 1; });
