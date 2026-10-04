// Isolated playback -> temporary PipeWire/PulseAudio monitor -> LEDs test.
const {spawn,execFileSync} = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const assert = require('node:assert/strict');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(),'synclight-audio-'));
const file = path.join(tmp,'tone.wav');
const rate=44100, frames=rate*8, wav=Buffer.alloc(44+frames*2);
wav.write('RIFF');wav.writeUInt32LE(wav.length-8,4);wav.write('WAVEfmt ',8);
wav.writeUInt32LE(16,16);wav.writeUInt16LE(1,20);wav.writeUInt16LE(1,22);
wav.writeUInt32LE(rate,24);wav.writeUInt32LE(rate*2,28);wav.writeUInt16LE(2,32);wav.writeUInt16LE(16,34);
wav.write('data',36);wav.writeUInt32LE(frames*2,40);
for(let i=0;i<frames;i++){
  const t=i/rate, phase=Math.floor(t/2);
  const envelope=Math.min(1,(t%2)*30,(2-t%2)*30);
  wav.writeInt16LE(phase%2===0?Math.round(Math.sin(2*Math.PI*440*t)*32767*.08*envelope):0,44+i*2);
}
fs.writeFileSync(file,wav);
(async()=>{
  let playback, moduleId;
  const sink=`synclight_test_${process.pid}`;
  try{
    moduleId=execFileSync('pactl',['load-module','module-null-sink',`sink_name=${sink}`,'sink_properties=device.description=SyncLightTest'],{encoding:'utf8'}).trim();
    const child=spawn(path.join(__dirname,'../run.sh'),['--audio-check',`--audio-source=${sink}.monitor`,...(process.env.SYNC_AUDIO_MODE?[`--audio-mode=${process.env.SYNC_AUDIO_MODE}`]:[]),...(process.env.SYNC_AUDIO_PALETTE?[`--audio-palette=${process.env.SYNC_AUDIO_PALETTE}`]:[])]);
    let output='', error='';
    child.stdout.on('data',b=>{output+=b;process.stdout.write(b);});
    child.stderr.on('data',b=>error+=b);
    const complete=new Promise((resolve,reject)=>{child.on('error',reject);child.on('exit',code=>code===0?resolve():reject(new Error(error||`exit ${code}`)));});
    complete.catch(()=>{}); // Keep early startup errors handled until the awaited check runs.
    await new Promise(r=>setTimeout(r,600));
    playback=spawn('paplay',[`--device=${sink}`,file]);
    const played=new Promise((resolve,reject)=>{playback.on('error',reject);playback.on('exit',code=>code===0?resolve():reject(new Error(`paplay exit ${code}`)));});
    await Promise.all([complete,played]);
    const samples=output.trim().split('\n').map(line=>JSON.parse(line)).filter(s=>s.audioSample).map(s=>s.audioSample);
    assert(samples.some(s=>s.frames>0&&s.level>.2),'tone was not captured');
    assert(samples.some(s=>s.frames>0&&s.level<.01),'silence was not captured; other playback may be active');
    const phases=samples.map(s=>s.level>.2?'tone':s.level<.01?'silence':'transition').filter(s=>s!=='transition').filter((s,i,a)=>i===0||s!==a[i-1]);
    assert(phases.join(',').includes('tone,silence,tone,silence'), `audio was delayed instead of following both bursts: ${phases}`);
    if(process.env.SYNC_AUDIO_PALETTE&&process.env.SYNC_AUDIO_PALETTE!=='selected')assert(samples.some(s=>s.level>.2&&s.distinct_colors>2),'the active audio frame did not contain multiple colors');
    console.log(`PASS: ${process.env.SYNC_AUDIO_MODE||'energy'} / ${process.env.SYNC_AUDIO_PALETTE||'selected'} — ordered playback bursts/silences and LED writes; physical matching needs visual confirmation.`);
  }finally{
    if(playback&&playback.exitCode===null)playback.kill();
    if(moduleId)execFileSync('pactl',['unload-module',moduleId]);
    fs.unlinkSync(file);fs.rmdirSync(tmp);
  }
})().catch(e=>{console.error(e);process.exitCode=1;});
