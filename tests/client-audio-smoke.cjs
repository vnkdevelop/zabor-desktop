const { app, BrowserWindow } = require('electron');
const { buildSync } = require('esbuild');
const { mkdtempSync, readFileSync, writeFileSync, rmSync, mkdirSync } = require('node:fs');
const { join } = require('node:path');
const { tmpdir } = require('node:os');

const base = join(process.env.LOCALAPPDATA || tmpdir(), 'Temp', 'opencode');
mkdirSync(base, { recursive: true });
const root = mkdtempSync(join(base, 'zabor-client-smoke-'));
app.setName('zabor-client-dsp-smoke');
app.setPath('userData', root);
app.setPath('sessionData', root);
app.disableHardwareAcceleration();
app.commandLine.appendSwitch('autoplay-policy', 'no-user-gesture-required');
let window;
app.whenReady().then(async () => {
  const code = buildSync({ stdin: { contents: "import { registerMicPipeline } from './src/renderer/src/services/mic-pipeline-core.ts'; registerMicPipeline('fixture', () => {let ready=false; return {label:'fixture',supportsAttenuationLimit:false,get isReady(){return ready},get stageName(){return ready?'ready':'loading'},get frame(){return ready?480:0},start:async read=>{if(!await read('pkg/df_bg.wasm'))throw Error('fixture-startup');ready=true},process:f=>f,setAttenuationLimit:()=>{},setPostFilterBeta:()=>{}}});", resolveDir: process.cwd(), loader: 'ts' }, bundle: true, format: 'iife', platform: 'browser', target: 'es2022', write: false }).outputFiles[0].text;
  writeFileSync(join(root, 'worklet.js'), code);
  writeFileSync(join(root, 'index.html'), '<!doctype html><meta charset="utf-8"><title>isolated dsp test</title>');
  window = new BrowserWindow({ show: false, webPreferences: { contextIsolation: true, nodeIntegration: false, sandbox: true } });
  await window.loadFile(join(root, 'index.html'));
  const bytes = readFileSync('src/renderer/public/rust-voicing/zabor_voicing.wasm').toString('base64');
  const result = await window.webContents.executeJavaScript(`(async () => {
    const bytes = Uint8Array.from(atob(${JSON.stringify(bytes)}), char => char.charCodeAt(0));
    const module = await WebAssembly.compile(bytes);
    const context = new AudioContext({sampleRate:48000});
    await context.audioWorklet.addModule('./worklet.js');
    const oldNode = new AudioWorkletNode(context,'fixture');
    const newNode = new AudioWorkletNode(context,'fixture',{processorOptions:{rustVoicingModule:module}});
    const oldMessages=[]; const newMessages=[];
    const requestedAt=performance.now();
    const prepare=(node,messages,requestId)=>new Promise((resolve,reject)=>{
      const timeout=setTimeout(()=>reject(Error('real-worklet-preparation-timeout')),6000);
      node.port.onmessage=event=>{
        const data=event.data; messages.push(data);
        if(data.type==='fetchRequest')setTimeout(()=>{
          const buffer=new ArrayBuffer(1); node.port.postMessage({type:'fetchResponse',url:data.url,buffer},[buffer]);
        },200);
        if(data.type==='transmissionReady'&&data.requestId===requestId){
          clearTimeout(timeout); resolve({...data,elapsedMs:performance.now()-requestedAt});
        }
      };
      node.port.postMessage({type:'prepareTransmission',requestId});
    });
    const preparation=Promise.all([prepare(oldNode,oldMessages,10),prepare(newNode,newMessages,20)]);
    const oscillator=context.createOscillator(); oscillator.frequency.value=170;
    const muted=context.createGain(); muted.gain.value=0;
    oscillator.connect(oldNode); oscillator.connect(newNode); oldNode.connect(muted); newNode.connect(muted); muted.connect(context.destination);
    oscillator.start(); await context.resume();
    const prepared=await preparation;
    if(prepared.some(result=>result.sampleRate!==48000||result.frameSize!==480||result.preparedFrames<32||result.inputQueued>=480||result.outputQueued>1920||!Number.isFinite(result.clockSpanMs)||Math.abs(result.clockSpanMs-result.clockAudioMs)>15+result.clockAudioMs*0.05))throw Error('real-worklet-clock-not-prepared');
    await new Promise(resolve=>setTimeout(resolve,1800));
    oldNode.port.postMessage({type:'getDspDiagnostics'}); newNode.port.postMessage({type:'getDspDiagnostics'});
    await new Promise(resolve=>setTimeout(resolve,200));
    const old=oldMessages.filter(message=>message.type==='dspDiagnostics').at(-1);
    const current=newMessages.filter(message=>message.type==='dspDiagnostics').at(-1);
    oldNode.port.postMessage({type:'dispose'}); newNode.port.postMessage({type:'dispose'}); oscillator.stop();
    await context.close();
    if(!old||!current||current.runtime!=='rust-voicing'||old.runtime!=='legacy'||old.frame===0||current.frame===0) throw Error('real-worklet-runtime-not-active');
    if(old.lag!==current.lag||Math.abs(old.score-current.score)>1e-12) throw Error('real-worklet-parity');
    return {old,current,prepared,realAudioWorklet:true};
  })()`);
  console.log(JSON.stringify(result));
  window.destroy();
  app.quit();
}).catch(error => { console.error(error.message); if (window && !window.isDestroyed()) window.destroy(); app.exit(1); });
app.on('quit', () => { try { rmSync(root, { recursive: true, force: true }); } catch {} });
