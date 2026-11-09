const reference = require('./reference-contract.cjs').openReference();
reference.group('capture');
reference.capture();
const { app, BrowserWindow, ipcMain, nativeImage } = reference.requirePackage('electron');
const { buildSync } = reference.requirePackage('esbuild');
const postcss = reference.requirePackage('postcss');
const tailwind = reference.requirePackage('tailwindcss');
const { createHash } = require('node:crypto');
const { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } = require('node:fs');
const { join, resolve } = require('node:path');
const { execFileSync } = require('node:child_process');

if (!process.versions.electron) {
  const base = join(process.env.LOCALAPPDATA, 'Temp/opencode');
  mkdirSync(base, { recursive: true });
  const temporary = mkdtempSync(join(base, 'zabor-native-reference-'));
  const env = { ...process.env, ZABOR_NATIVE_CAPTURE_TEMP: temporary };
  delete env.ELECTRON_RUN_AS_NODE;
  try {
    const arguments = [process.execPath, reference.packagePath('electron', 'cli.js'), __filename, ...process.argv.slice(2)];
    if (arguments.some(value => /["%\r\n]/.test(value))) throw new Error('Unsupported character in capture arguments');
    execFileSync('cmd.exe', ['/d', '/s', '/c', `"${arguments.map(value => `"${value}"`).join(' ')}"`], { stdio: 'inherit', env, timeout: 70000, windowsVerbatimArguments: true });
  } finally {
    rmSync(temporary, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }
  return;
}

const args = process.argv.slice(2);
const value = (key, fallback) => args.includes(key) ? args[args.indexOf(key) + 1] : fallback;
const scale = Number(value('--scale', '1'));
if (![1, 1.25, 1.5, 2].includes(scale)) throw new Error('Unsupported capture scale');
const output = resolve(value('--output', 'migration-artifacts/native-ui'));
const atMs = Number(value('--at-ms', '550'));
if (!Number.isFinite(atMs) || atMs < 0) throw new Error('Invalid animation time');
const reducedMotion = args.includes('--reduced-motion');
const suffix = reducedMotion ? '-reduced' : '';
const report = join(output, `react-reference-${scale}-${atMs}${suffix}.json`);
for (const locale of ['ru', 'en']) for (const state of ['downloading', 'installing', 'error']) {
  const path = join(output, `react-updater-${locale}-${state}-${scale}-${atMs}${suffix}.png`);
  if (existsSync(path)) throw new Error(`Existing capture preserved: ${path}`);
}
if (existsSync(report)) throw new Error('Existing reference report preserved');
mkdirSync(output, { recursive: true });
const temporary = process.env.ZABOR_NATIVE_CAPTURE_TEMP;
if (!temporary) throw new Error('Run this capture through node scripts/native/capture-react.cjs');
app.setName('zabor-react-reference');
app.setPath('userData', temporary);
app.setPath('sessionData', temporary);
app.commandLine.appendSwitch('force-device-scale-factor', String(scale));
if (reducedMotion) app.commandLine.appendSwitch('force-prefers-reduced-motion');
let snapshot;
let window;
const watchdog = setTimeout(() => { console.error('Reference capture timed out'); app.exit(1); }, 60000);

app.whenReady().then(async () => {
  const sourceFiles = ['src/renderer/src/components/UpdaterWindow.tsx', 'src/renderer/src/index.css', 'tailwind.config.js', 'src/renderer/src/locales/ru.json', 'src/renderer/src/locales/en.json'];
  const sources = Object.fromEntries(sourceFiles.map(file => [file, createHash('sha256').update(reference.text(file).replace(/\r\n/g, '\n')).digest('hex')]));
  const entry = `import React from 'react'; import { createRoot } from 'react-dom/client'; import i18n from 'i18next'; import { I18nextProvider } from 'react-i18next'; import UpdaterWindow from './src/renderer/src/components/UpdaterWindow'; import ru from './src/renderer/src/locales/ru.json'; import en from './src/renderer/src/locales/en.json'; i18n.init({lng:new URLSearchParams(location.hash.slice(1)).get('locale'),fallbackLng:'ru',resources:{ru:{translation:ru},en:{translation:en}},interpolation:{escapeValue:false}}).then(()=>createRoot(document.getElementById('root')).render(<I18nextProvider i18n={i18n}><UpdaterWindow/></I18nextProvider>));`;
  const bundle = buildSync({ stdin: { contents: entry, resolveDir: reference.root, loader: 'tsx' }, bundle: true, format: 'iife', platform: 'browser', target: 'es2022', write: false }).outputFiles[0].text;
  const css = await postcss([tailwind(require(reference.path('tailwind.config.js')))]).process(reference.text('src/renderer/src/index.css'), { from: reference.path('src/renderer/src/index.css') });
  writeFileSync(join(temporary, 'reference.js'), bundle);
  writeFileSync(join(temporary, 'reference.css'), css.css);
  writeFileSync(join(temporary, 'index.html'), '<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src \'self\' data:; script-src \'self\'; style-src \'self\' \'unsafe-inline\'; connect-src \'none\'"><link rel="stylesheet" href="reference.css"></head><body><div id="root"></div><script src="reference.js"></script></body></html>');
  writeFileSync(join(temporary, 'preload.cjs'), "const {contextBridge,ipcRenderer}=require('electron');const noop=()=>()=>{};contextBridge.exposeInMainWorld('windowControls',{getUpdateStatus:()=>ipcRenderer.invoke('fixture-state'),onUpdateProgress:noop,onUpdateDownloaded:noop,onUpdateInstalling:noop,onUpdateError:noop,installUpdate:async()=>({success:true}),cancelUpdateDownload:()=>ipcRenderer.invoke('fixture-action','cancel'),close:()=>ipcRenderer.invoke('fixture-action','close'),startUpdateDownload:async()=>({success:true})});");
  ipcMain.handle('fixture-state', () => snapshot);
  ipcMain.handle('fixture-action', (_event, action) => action);
  const captures = [];
  for (const locale of ['ru', 'en']) for (const state of ['downloading', 'installing', 'error']) {
    snapshot = { status: state, updateInfo: { version: '3.8.0-rust', downloadUrl: 'https://fixture.invalid/never-requested' }, progress: { percent: 42, transferred: 44_040_192, total: 104_857_600 }, error: state === 'error' ? 'Unverified beta update' : null };
    if (!window) {
      window = new BrowserWindow({ width: 420, height: 240, useContentSize: true, frame: false, show: false, backgroundColor: '#09090b', webPreferences: { preload: join(temporary, 'preload.cjs'), contextIsolation: true, nodeIntegration: false, sandbox: true, offscreen: true, backgroundThrottling: false } });
    }
    await window.loadFile(join(temporary, 'index.html'), { query: { capture: `${locale}-${state}` }, hash: `locale=${locale}` });
    if (!window.webContents.debugger.isAttached()) window.webContents.debugger.attach('1.3');
    await window.webContents.debugger.sendCommand('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: reducedMotion ? 'reduce' : 'no-preference' }] });
    await window.webContents.debugger.sendCommand('Emulation.setDeviceMetricsOverride', { width: 420, height: 240, deviceScaleFactor: scale, mobile: false });
    const dictionary = JSON.parse(reference.text(`src/renderer/src/locales/${locale}.json`)).updater;
    const expectedHeading = dictionary[state === 'error' ? 'errorTitle' : state];
    await window.webContents.executeJavaScript(`new Promise((resolve,reject)=>{const expected=${JSON.stringify(expectedHeading)};const ready=()=>[...document.querySelectorAll('span')].some(element=>element.textContent===expected);if(ready()){resolve();return;}const observer=new MutationObserver(()=>{if(ready()){clearTimeout(timeout);observer.disconnect();resolve();}});const timeout=setTimeout(()=>{observer.disconnect();reject(Error('Reference state was not rendered'));},5000);observer.observe(document.documentElement,{subtree:true,childList:true,characterData:true});})`);
    await window.webContents.executeJavaScript(`(async()=>{await document.fonts.ready;await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));const animations=document.getAnimations();for(const animation of animations)animation.pause();await Promise.all(animations.map(animation=>animation.ready));for(const animation of animations)animation.currentTime=${atMs};})()`);
    await window.webContents.executeJavaScript(`new Promise((resolve,reject)=>{let previous='',stable=0,frames=0;const check=()=>{const signature=JSON.stringify([...document.querySelectorAll('#root *')].map(element=>{const r=element.getBoundingClientRect(),s=getComputedStyle(element);return [r.x,r.y,r.width,r.height,s.fontSize,s.color]}));stable=signature===previous?stable+1:0;previous=signature;if(stable>=3){resolve();return;}if(++frames>=120){reject(Error('Reference layout did not settle'));return;}requestAnimationFrame(check);};requestAnimationFrame(check);})`);
    const metrics = await window.webContents.executeJavaScript(`({width:innerWidth,height:innerHeight,devicePixelRatio,reducedMotion:matchMedia('(prefers-reduced-motion: reduce)').matches,bodyFont:getComputedStyle(document.body).fontFamily,elements:[...document.querySelectorAll('#root *')].map((element,index)=>{const r=element.getBoundingClientRect(),s=getComputedStyle(element);return {index,tag:element.tagName,text:element.children.length?null:element.textContent,x:r.x,y:r.y,width:r.width,height:r.height,font:s.fontFamily,fontSize:s.fontSize,fontWeight:s.fontWeight,color:s.color,background:s.backgroundColor,border:s.borderColor,radius:s.borderRadius,letterSpacing:s.letterSpacing,transform:s.transform}})})`);
    if (metrics.width !== 420 || metrics.height !== 240 || metrics.devicePixelRatio !== scale || metrics.reducedMotion !== reducedMotion) throw new Error(`Reference viewport mismatch: ${JSON.stringify({ width: metrics.width, height: metrics.height, scale: metrics.devicePixelRatio, reducedMotion: metrics.reducedMotion })}`);
    const captured = await window.webContents.debugger.sendCommand('Page.captureScreenshot', { format: 'png', fromSurface: true, captureBeyondViewport: false });
    const image = nativeImage.createFromBuffer(Buffer.from(captured.data, 'base64'));
    const size = image.getSize();
    if (size.width !== 420 * scale || size.height !== 240 * scale) throw new Error(`Unexpected reference image dimensions: ${JSON.stringify(size)}`);
    const name = `react-updater-${locale}-${state}-${scale}-${atMs}${suffix}.png`;
    const path = join(output, name);
    if (existsSync(path)) throw new Error(`Existing capture preserved: ${path}`);
    writeFileSync(path, image.toPNG());
    captures.push({ file: name, locale, state, scale, atMs, reducedMotion, metrics });
  }
  writeFileSync(report, JSON.stringify({ sources, runtime: process.versions, captures }, null, 2));
  console.log(JSON.stringify({ captured: captures.length, output, report }));
  window.webContents.debugger.detach();
  window.destroy();
  app.quit();
}).catch(error => {
  console.error(error.stack || error.message);
  if (window && !window.isDestroyed()) window.destroy();
  app.exit(1);
});
app.on('quit', () => { clearTimeout(watchdog); });
