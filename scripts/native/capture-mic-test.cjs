const { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } = require('node:fs');
const { join, resolve } = require('node:path');
const { execFileSync } = require('node:child_process');
const { createHash } = require('node:crypto');

if (!process.versions.electron) {
  const temporary = mkdtempSync(join(process.env.LOCALAPPDATA, 'Temp/opencode/zabor-mic-reference-'));
  const env = { ...process.env, ZABOR_MIC_CAPTURE_TEMP: temporary };
  delete env.ELECTRON_RUN_AS_NODE;
  try {
    const parts = [process.execPath, require.resolve('electron/cli.js'), __filename, ...process.argv.slice(2)];
    if (parts.some(value => /["%\r\n]/.test(value))) throw new Error('Unsupported capture argument');
    execFileSync('cmd.exe', ['/d', '/v:off', '/s', '/c', `"${parts.map(value => `"${value}"`).join(' ')}"`], { env, stdio: 'inherit', timeout: 120000, windowsVerbatimArguments: true });
  } finally {
    rmSync(temporary, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }
  return;
}

const { app, BrowserWindow, ipcMain, nativeImage } = require('electron');
const { build } = require('esbuild');
const postcss = require('postcss');
const tailwind = require('tailwindcss');
const args = process.argv.slice(2);
const value = key => args[args.indexOf(key) + 1];
if (args.length !== 4 || !args.includes('--output') || !args.includes('--scale')) throw new Error('Usage: node scripts/native/capture-mic-test.cjs --output DIRECTORY --scale 1|1.25|1.5|2');
const output = resolve(value('--output'));
const scale = Number(value('--scale'));
if (![1, 1.25, 1.5, 2].includes(scale)) throw new Error('Unsupported DPI');
const temporary = process.env.ZABOR_MIC_CAPTURE_TEMP;
if (!temporary) throw new Error('Run through node scripts/native/capture-mic-test.cjs');
app.setPath('userData', temporary);
app.setPath('sessionData', temporary);
let window;
let fixture;
const watchdog = setTimeout(() => app.exit(1), 110000);
app.whenReady().then(async () => {
  const mock = `export const MIC_TEST_SILENCE_DBFS=-70;const audio=()=>window.__audio;export const webrtc={prepareMicTest:async()=>{},releaseMicTestPreparation:()=>{},onMicTestEnded:()=>{},disposeMicTest:()=>{audio().playing=false;},pauseMicTest:()=>{audio().playing=false;},isMicTestRecording:()=>false,getMicTestPosition:()=>audio().position,getMicTestDuration:()=>5,isMicTestPlaying:()=>audio().playing,seekMicTest:seconds=>{audio().position=Math.max(0,Math.min(4.95,seconds));},playMicTest:()=>{audio().playing=true;return true;},recordMicTest:callback=>{const state=window.__fixture.state;if(state==='arming')return new Promise(()=>{});if(state==='failed')return Promise.reject(Error('synthetic failure'));callback();if(state==='recording')return new Promise(()=>{});return Promise.resolve({durationSeconds:5,peakDb:state==='silent'?-80:-20});}};`;
  const entry = `import React from 'react';import {createRoot} from 'react-dom/client';import i18n from 'i18next';import {I18nextProvider} from 'react-i18next';import {MicTestPanel} from './src/renderer/src/components/Settings/MicTestPanel';import ru from './src/renderer/src/locales/ru.json';import en from './src/renderer/src/locales/en.json';async function boot(){window.__fixture=await window.fixture.get();window.__audio={playing:false,position:1.5};await i18n.init({lng:window.__fixture.locale,fallbackLng:'ru',resources:{ru:{translation:ru},en:{translation:en}},interpolation:{escapeValue:false}});createRoot(document.getElementById('root')).render(<I18nextProvider i18n={i18n}><div style={{position:'absolute',width:500,height:0,left:300,top:16}}><MicTestPanel isEnabled={true} isActive={true}/></div></I18nextProvider>);}boot();`;
  const built = await build({ stdin: { contents: entry, resolveDir: process.cwd(), loader: 'tsx' }, jsx: 'automatic', bundle: true, format: 'iife', platform: 'browser', target: 'es2022', write: false, metafile: true, plugins: [{ name: 'local-only-audio-fixture', setup(builder) { builder.onResolve({ filter: /services\/webrtc$/ }, () => ({ path: 'audio-fixture', namespace: 'fixture' })); builder.onLoad({ filter: /.*/, namespace: 'fixture' }, () => ({ contents: mock, loader: 'js' })); } }] });
  if (Object.keys(built.metafile.inputs).some(file => /services[\\/]webrtc\.ts$/.test(file))) throw new Error('Real audio service must not enter the reference fixture');
  const bundle = built.outputFiles[0].text;
  const css = await postcss([tailwind(require(resolve('tailwind.config.js')))]).process(readFileSync('src/renderer/src/index.css', 'utf8'), { from: resolve('src/renderer/src/index.css') });
  writeFileSync(join(temporary, 'reference.js'), bundle);
  writeFileSync(join(temporary, 'reference.css'), css.css);
  writeFileSync(join(temporary, 'index.html'), '<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src \'self\' data:; script-src \'self\'; style-src \'self\' \'unsafe-inline\'; connect-src \'none\'"><link rel="stylesheet" href="reference.css"></head><body style="background:#060606"><div id="root"></div><script src="reference.js"></script></body></html>');
  writeFileSync(join(temporary, 'preload.cjs'), "const {contextBridge,ipcRenderer}=require('electron');contextBridge.exposeInMainWorld('fixture',{get:()=>ipcRenderer.invoke('fixture')});");
  ipcMain.handle('fixture', () => fixture);
  window = new BrowserWindow({ width: 1100, height: 160, useContentSize: true, frame: false, show: false, webPreferences: { preload: join(temporary, 'preload.cjs'), nodeIntegration: false, contextIsolation: true, sandbox: true, offscreen: true, backgroundThrottling: false } });
  window.webContents.session.webRequest.onBeforeRequest({ urls: ['http://*/*', 'https://*/*', 'ws://*/*', 'wss://*/*'] }, (_, callback) => callback({ cancel: true }));
  mkdirSync(output, { recursive: true });
  const captures = [];
  for (const locale of ['ru', 'en']) for (const state of ['idle', 'arming', 'recording', 'playing', 'paused', 'silent', 'failed']) {
    fixture = { locale, state };
    const target = join(output, `react-mic-${state}-${locale}-${scale}.png`);
    if (existsSync(target)) throw new Error('Capture already exists');
    await window.loadFile(join(temporary, 'index.html'));
    if (!window.webContents.debugger.isAttached()) window.webContents.debugger.attach('1.3');
    await window.webContents.debugger.sendCommand('Emulation.setDeviceMetricsOverride', { width: 1100, height: 160, deviceScaleFactor: scale, mobile: false });
    await window.webContents.debugger.sendCommand('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: 'reduce' }] });
    await window.webContents.executeJavaScript(`new Promise((resolve,reject)=>{const deadline=setTimeout(()=>reject(Error('Mic reference did not mount')),5000);const check=()=>{if(document.querySelector('#root button')){clearTimeout(deadline);resolve();}else requestAnimationFrame(check);};check();})`);
    if (state !== 'idle') await window.webContents.executeJavaScript(`document.querySelector('#root button').click()`);
    await window.webContents.executeJavaScript(`(async()=>{await document.fonts.ready;await new Promise(resolve=>setTimeout(resolve,700));})()`);
    if (state === 'paused') {
      await window.webContents.executeJavaScript(`document.querySelector('#root button').click()`);
      await window.webContents.executeJavaScript(`new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))`);
    }
    const metrics = await window.webContents.executeJavaScript(`(()=>{const panel=document.querySelector('#root>div>div');const rect=panel.getBoundingClientRect();return {width:rect.width,height:rect.height,x:rect.x,y:rect.y,scale:devicePixelRatio,buttons:[...panel.querySelectorAll('button')].map(button=>{const r=button.getBoundingClientRect(),s=getComputedStyle(button);return {label:button.getAttribute('aria-label')||button.textContent,disabled:button.disabled,x:r.x-rect.x,y:r.y-rect.y,width:r.width,height:r.height,fontFamily:s.fontFamily,fontWeight:s.fontWeight};}),slider:panel.querySelector('[role=slider]')?.getAttribute('aria-valuenow'),text:panel.textContent};})()`);
    await window.webContents.debugger.sendCommand('DOM.enable');
    await window.webContents.debugger.sendCommand('CSS.enable');
    const document = await window.webContents.debugger.sendCommand('DOM.getDocument', { depth: 0 });
    const node = await window.webContents.debugger.sendCommand('DOM.querySelector', { nodeId: document.root.nodeId, selector: state === 'failed' || state === 'silent' ? '#root p' : '#root>div>div>button:last-of-type' });
    metrics.platformFonts = node.nodeId ? (await window.webContents.debugger.sendCommand('CSS.getPlatformFontsForNode', { nodeId: node.nodeId })).fonts : [];
    if (metrics.width !== 156 || metrics.scale !== scale || metrics.y !== 28) throw new Error('Reference panel geometry mismatch');
    const capture = await window.webContents.debugger.sendCommand('Page.captureScreenshot', { format: 'png', fromSurface: true, captureBeyondViewport: false });
    const image = nativeImage.createFromBuffer(Buffer.from(capture.data, 'base64'));
    const rect = { x: Math.round(metrics.x * scale), y: Math.round(metrics.y * scale), width: Math.round(metrics.width * scale), height: Math.ceil(metrics.height * scale) };
    writeFileSync(target, image.crop(rect).toPNG());
    captures.push({ locale, state, file: target, sha256: createHash('sha256').update(readFileSync(target)).digest('hex'), metrics });
  }
  const sources = Object.fromEntries([__filename, 'src/renderer/src/components/Settings/MicTestPanel.tsx', 'src/renderer/src/lib/anime.ts', 'src/renderer/src/index.css', 'tailwind.config.js', 'src/renderer/src/locales/ru.json', 'src/renderer/src/locales/en.json'].map(file => [file, createHash('sha256').update(readFileSync(file)).digest('hex')]));
  writeFileSync(join(output, `mic-reference-${scale}.json`), JSON.stringify({ sources, captures, actualAudioServiceBundled: false, scope: 'unchanged React panel, mocked memory-only audio, blocked network and isolated ephemeral profile; static reduced-motion state evidence, not physical audio or motion acceptance' }, null, 2), { flag: 'wx' });
  window.webContents.debugger.detach();
  window.destroy();
  app.quit();
  console.log(JSON.stringify({ captures: captures.length, output }));
}).catch(error => { console.error(error.stack); if (window && !window.isDestroyed()) window.destroy(); app.exit(1); });
app.on('quit', () => clearTimeout(watchdog));
