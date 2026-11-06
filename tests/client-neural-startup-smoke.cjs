const { app, BrowserWindow, ipcMain } = require('electron');
const { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } = require('node:fs');
const { join, resolve } = require('node:path');

const base = join(process.env.LOCALAPPDATA, 'Temp', 'opencode');
mkdirSync(base, { recursive: true });
const root = mkdtempSync(join(base, 'zabor-neural-startup-'));
app.setName('zabor-neural-startup-fixture');
app.setPath('userData', root);
app.setPath('sessionData', root);
app.disableHardwareAcceleration();
app.commandLine.appendSwitch('autoplay-policy', 'no-user-gesture-required');
const assetRoot = resolve('src/renderer/public/deepfilternet3');
const allowed = new Set(['pkg/df_bg.wasm', 'models/DeepFilterNet3_onnx.tar.gz']);
let window;

app.whenReady().then(async () => {
  const outputs = resolve('out/renderer/assets');
  if (!existsSync(outputs)) throw new Error('Build the production renderer before neural startup smoke');
  const files = readdirSync(outputs);
  for (const model of ['rnnoise', 'deepfilter']) {
    const name = files.find(name => name.startsWith(`mic-pipeline-${model}-`) && name.endsWith('.js'));
    if (!name) throw new Error(`Production ${model} worklet missing`);
    copyFileSync(join(outputs, name), join(root, `${model}.js`));
  }
  writeFileSync(join(root, 'index.html'), '<!doctype html><meta charset="utf-8"><title>isolated neural startup</title>');
  writeFileSync(join(root, 'preload.cjs'), "const { contextBridge, ipcRenderer } = require('electron'); contextBridge.exposeInMainWorld('neuralStartupFixture', { load: asset => ipcRenderer.invoke('neural-startup-asset', asset) });");
  ipcMain.handle('neural-startup-asset', (_event, asset) => {
    if (!allowed.has(asset)) throw new Error('Fixture asset refused');
    return new Uint8Array(readFileSync(join(assetRoot, asset)));
  });
  window = new BrowserWindow({ show: false, webPreferences: { preload: join(root, 'preload.cjs'), contextIsolation: true, nodeIntegration: false, sandbox: true } });
  await window.loadFile(join(root, 'index.html'));
  const wasm = readFileSync('src/renderer/public/rust-voicing/zabor_voicing.wasm').toString('base64');
  const result = await window.webContents.executeJavaScript(`(async () => {
    const bytes=Uint8Array.from(atob(${JSON.stringify(wasm)}),character=>character.charCodeAt(0));
    const module=await WebAssembly.compile(bytes);
    const results=[];
    for(const model of ['rnnoise','deepfilter']){
      const context=new AudioContext({sampleRate:48000});
      let node;
      let oscillator;
      try{
        await context.audioWorklet.addModule('./'+model+'.js');
        node=new AudioWorkletNode(context,'mic-pipeline-'+model,{processorOptions:{rustVoicingModule:module}});
        const messages=[];
        const startedAt=performance.now();
        const requestId=model==='rnnoise'?1:2;
        let resolveReady;
        let rejectReady;
        const ready=new Promise((resolve,reject)=>{resolveReady=resolve;rejectReady=reject});
        const timeout=setTimeout(()=>rejectReady(Error(model+'-startup-timeout')),20000);
        node.onprocessorerror=()=>rejectReady(Error(model+'-processor-failed'));
        node.port.onmessage=event=>{
          const data=event.data;messages.push(data);
          if(data.type==='fetchRequest'){
            const prefix='zabor-local://deepfilternet3/';
            if(!data.url.startsWith(prefix)){rejectReady(Error('fixture-asset-prefix'));return}
            window.neuralStartupFixture.load(data.url.slice(prefix.length)).then(bytes=>{
              const buffer=bytes.byteOffset===0&&bytes.byteLength===bytes.buffer.byteLength?bytes.buffer:bytes.slice().buffer;
              node.port.postMessage({type:'fetchResponse',url:data.url,buffer},[buffer]);
            }).catch(rejectReady);
          }
          if(data.type==='transmissionReady'&&data.requestId===requestId){clearTimeout(timeout);resolveReady({...data,elapsedMs:performance.now()-startedAt})}
        };
        node.port.postMessage({type:'setConfig',sileroVadEnabled:false,isMuted:false});
        node.port.postMessage({type:'loadWasm',assetBase:'zabor-local://deepfilternet3'});
        node.port.postMessage({type:'prepareTransmission',requestId});
        oscillator=context.createOscillator();oscillator.frequency.value=170;
        const destination=context.createMediaStreamDestination();
        const destinationTrack=destination.stream.getAudioTracks()[0];destinationTrack.enabled=false;
        const silent=context.createGain();silent.gain.value=0;
        oscillator.connect(node);node.connect(silent);silent.connect(destination);
        oscillator.start();await context.resume();
        const prepared=await ready;
        if(prepared.sampleRate!==48000||prepared.frameSize!==480||prepared.preparedFrames<32||prepared.inputQueued>=480||prepared.outputQueued>1920||!Number.isFinite(prepared.clockSpanMs)||Math.abs(prepared.clockSpanMs-prepared.clockAudioMs)>15+prepared.clockAudioMs*0.05)throw Error(model+'-clock-not-ready');
        node.port.postMessage({type:'getDspDiagnostics'});
        await new Promise(resolve=>setTimeout(resolve,50));
        const diagnostic=messages.filter(message=>message.type==='dspDiagnostics').at(-1);
        if(!diagnostic||diagnostic.runtime!=='rust-voicing'||diagnostic.frame===0)throw Error(model+'-runtime-not-active');
        if(!messages.some(message=>message.type==='ready'))throw Error(model+'-model-not-ready');
        results.push({model,prepared,diagnostic,destinationMuted:!destinationTrack.enabled,realProductionWorklet:true});
      }finally{
        if(node)node.port.postMessage({type:'dispose'});
        if(oscillator)oscillator.stop();
        await context.close();
      }
    }
    return results;
  })()`);
  console.log(JSON.stringify(result));
  window.destroy();
  app.quit();
}).catch(error => {
  console.error(error.stack || error.message);
  if (window && !window.isDestroyed()) window.destroy();
  app.exit(1);
});
app.on('quit', () => { ipcMain.removeHandler('neural-startup-asset'); try { rmSync(root, { recursive: true, force: true }); } catch {} });
