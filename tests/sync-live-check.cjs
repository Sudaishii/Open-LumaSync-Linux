// Real compositor -> grim -> LEDs test. Shows a fullscreen color pattern.
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const { spawn } = require('node:child_process');
const path = require('node:path');
const assert = require('node:assert/strict');
(async () => {
  const browser = await chromium.launch({headless:false});
  try {
    const page = await browser.newPage({viewport:null});
    await page.setContent('<style>html,body{margin:0;width:100%;height:100%;background:rgb(240,0,0)}</style>');
    const cdp = await page.context().newCDPSession(page);
    const {windowId} = await cdp.send('Browser.getWindowForTarget');
    await cdp.send('Browser.setWindowBounds',{windowId,bounds:{windowState:'fullscreen'}});
    await page.waitForTimeout(1000);
    const child = spawn(path.join(__dirname,'../run.sh'), ['--screen-check',...(process.env.SYNC_SCREEN_FPS?[`--screen-fps=${process.env.SYNC_SCREEN_FPS}`]:[])]);
    let output='', error='';
    child.stdout.on('data', b=>{output+=b;process.stdout.write(b);});
    child.stderr.on('data', b=>{error+=b;});
    const completed = new Promise((resolve,reject)=>{
      child.on('error',reject);
      child.on('exit',code=>code===0?resolve():reject(new Error(error || `exit ${code}`)));
    });
    const changes = (async()=> {
      await page.waitForTimeout(3500);
      await page.evaluate(()=>{document.body.style.background='rgb(0,0,240)';document.documentElement.style.background='rgb(0,0,240)';});
      await page.waitForTimeout(3500);
      await page.evaluate(()=>{document.body.style.background='rgb(0,240,0)';document.documentElement.style.background='rgb(0,240,0)';});
      await page.waitForTimeout(1500);
      await page.evaluate(()=>{document.body.style.background='black';document.documentElement.style.background='black';document.body.innerHTML='<div style="position:fixed;left:0;top:0;bottom:0;width:160px;background:rgb(240,0,0)"></div><div style="position:fixed;right:0;top:0;bottom:0;width:160px;background:rgb(0,0,240)"></div><div style="position:fixed;left:0;right:0;top:0;height:160px;background:rgb(0,240,0)"></div>';});
    })();
    await completed;
    await changes;
    const samples=output.trim().split('\n').map(line=>JSON.parse(line)).filter(s=>s.screenSample).map(s=>s.screenSample);
    assert(samples.some(s=>s.average_rgb[0]>150&&s.average_rgb[1]<60&&s.average_rgb[2]<60),'red was not captured');
    assert(samples.some(s=>s.average_rgb[2]>150&&s.average_rgb[0]<60&&s.average_rgb[1]<60),'blue was not captured');
    assert(samples.some(s=>s.average_rgb[1]>150&&s.average_rgb[0]<60&&s.average_rgb[2]<60),'green was not captured');
    assert(samples.some(s=>s.section_rgb&&s.section_rgb[0][0]>170&&s.section_rgb[0][1]<80&&s.section_rgb[1][1]>170&&s.section_rgb[2][2]>170&&s.section_rgb[2][1]<80),'different left/top/right colors were not mapped to their LED sections');
    const last=samples.at(-1);assert(last.achieved_fps>0&&Number.isFinite(last.frame_ms),'measured frame performance missing');
    console.log(`PASS: fullscreen RGB and separate red/green/blue edges captured and written; ${last.achieved_fps.toFixed(1)} fps measured at ${process.env.SYNC_SCREEN_FPS||8} fps target. Physical matching needs visual confirmation.`);
  } finally {await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
