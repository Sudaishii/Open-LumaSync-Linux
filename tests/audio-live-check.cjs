// Isolated playback -> temporary PipeWire/PulseAudio monitor -> LEDs test.
const {spawn,execFileSync} = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const assert = require('node:assert/strict');
const {performance} = require('node:perf_hooks');
const frequency=Number(process.env.SYNC_AUDIO_FREQUENCY||440);
assert(Number.isFinite(frequency)&&frequency>=20&&frequency<=20000,'test frequency must be 20–20000 Hz');
const continuous=process.env.SYNC_AUDIO_CONTINUOUS==='1';
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
  const signal=continuous
    ? Math.sin(2*Math.PI*frequency*t)*.10+(phase%2===0?Math.sin(2*Math.PI*1000*t)*.30*envelope:0)
    : (phase%2===0?Math.sin(2*Math.PI*frequency*t)*.08*envelope:0);
  wav.writeInt16LE(Math.round(signal*32767),44+i*2);
}
fs.writeFileSync(file,wav);
(async()=>{
  let playback, moduleId;
  const sink=`synclight_test_${process.pid}`;
  try{
    moduleId=execFileSync('pactl',['load-module','module-null-sink',`sink_name=${sink}`,'sink_properties=device.description=SyncLightTest'],{encoding:'utf8'}).trim();
    const child=spawn(path.join(__dirname,'../run.sh'),['--audio-check',`--audio-source=${sink}.monitor`,...(process.env.SYNC_AUDIO_MODE?[`--audio-mode=${process.env.SYNC_AUDIO_MODE}`]:[]),...(process.env.SYNC_AUDIO_PALETTE?[`--audio-palette=${process.env.SYNC_AUDIO_PALETTE}`]:[]),...(process.env.SYNC_AUDIO_SENSITIVITY?[`--audio-sensitivity=${process.env.SYNC_AUDIO_SENSITIVITY}`]:[])]);
    let output='', error='', pending='';
    const timedSamples=[];
    child.stdout.on('data',b=>{
      output+=b;pending+=b;process.stdout.write(b);
      const lines=pending.split('\n');pending=lines.pop();
      for(const line of lines){if(!line.startsWith('{'))continue;const parsed=JSON.parse(line);if(parsed.audioSample)timedSamples.push({sample:parsed.audioSample,at:performance.now()});}
    });
    child.stderr.on('data',b=>error+=b);
    const complete=new Promise((resolve,reject)=>{child.on('error',reject);child.on('exit',code=>code===0?resolve():reject(new Error(error||`exit ${code}`)));});
    complete.catch(()=>{}); // Keep early startup errors handled until the awaited check runs.
    await new Promise(r=>setTimeout(r,600));
    const playbackStarted=performance.now();
    playback=spawn('paplay',[`--device=${sink}`,file]);
    const played=new Promise((resolve,reject)=>{playback.on('error',reject);playback.on('exit',code=>code===0?resolve():reject(new Error(`paplay exit ${code}`)));});
    await Promise.all([complete,played]);
    const samples=output.trim().split('\n').map(line=>JSON.parse(line)).filter(s=>s.audioSample).map(s=>s.audioSample);
    assert(samples.some(s=>s.frames>0&&s.level>.2),'tone was not captured');
    if(continuous){
      const loud=[],background=[];
      for(const {sample,at} of timedSamples){
        const t=(at-playbackStarted)/1000, within=t%2;
        if(t<0||t>=8||within<.4||within>1.7)continue;
        (Math.floor(t/2)%2===0?loud:background).push(sample);
      }
      assert(loud.length>=2&&background.length>=2,'not enough steady-state continuous playback samples');
      assert(background.every(s=>s.level>.1),'background audio incorrectly went silent between accents');
      const median=values=>{const sorted=[...values].sort((a,b)=>a-b),mid=Math.floor(sorted.length/2);return sorted.length%2?sorted[mid]:(sorted[mid-1]+sorted[mid])/2;};
      const quiet=median(background.map(s=>s.output_brightness)),accent=median(loud.map(s=>s.output_brightness));
      assert(accent-quiet>=15,`continuous BGM flattened the light response: background=${quiet}, accents=${accent}`);
      console.log(`PASS: continuous BGM stays audible and changes LED brightness from ${quiet} to ${accent} for louder accents.`);
    }else{
      assert(samples.some(s=>s.frames>0&&s.level<.01),'silence was not captured; other playback may be active');
      const phases=samples.map(s=>s.level>.2?'tone':s.level<.01?'silence':'transition').filter(s=>s!=='transition').filter((s,i,a)=>i===0||s!==a[i-1]);
      assert(phases.join(',').includes('tone,silence,tone,silence'), `audio was delayed instead of following both bursts: ${phases}`);
    }
    if(process.env.SYNC_AUDIO_PALETTE&&process.env.SYNC_AUDIO_PALETTE!=='selected')assert(samples.some(s=>s.level>.2&&s.distinct_colors>2),'the active audio frame did not contain multiple colors');
    assert(samples.some(s=>s.level>.2&&s.output_brightness>=40),'quiet audio was captured but the LED output stayed too dim');
    if(process.env.SYNC_AUDIO_MODE==='spectrum'&&!continuous){
      const edges=[20,60,250,500,2000,4000,6000,12000,20000];
      // The finite FFT window blends frequencies within one ~10.8 Hz bin of
      // a boundary; either adjacent band is valid there, including 20 kHz.
      const tolerance=44100/4096;
      const expected=edges.slice(0,8).map((edge,i)=>frequency>=edge-tolerance&&frequency<=edges[i+1]+tolerance?i:-1).filter(i=>i>=0);
      assert(samples.some(s=>s.level>.2&&s.bands?.length===8&&expected.includes(s.bands.indexOf(Math.max(...s.bands)))),'the tone was mapped to the wrong frequency band');
    }
    console.log(`PASS: ${process.env.SYNC_AUDIO_MODE||'energy'} / ${process.env.SYNC_AUDIO_PALETTE||'selected'} / ${frequency} Hz — ${continuous?'continuous volume response':'ordered playback bursts/silences'} and visible LED output; physical matching needs visual confirmation.`);
  }finally{
    if(playback&&playback.exitCode===null)playback.kill();
    if(moduleId)execFileSync('pactl',['unload-module',moduleId]);
    fs.unlinkSync(file);fs.rmdirSync(tmp);
  }
})().catch(e=>{console.error(e);process.exitCode=1;});
