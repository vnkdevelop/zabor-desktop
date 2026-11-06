const { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } = require('node:fs');
const { join, resolve } = require('node:path');
const { execFileSync } = require('node:child_process');
const { createHash } = require('node:crypto');

if (!process.versions.electron) {
  const temporary = mkdtempSync(join(process.env.LOCALAPPDATA, 'Temp/opencode/zabor-auth-reference-'));
  const env = { ...process.env, ZABOR_AUTH_CAPTURE_TEMP: temporary };
  delete env.ELECTRON_RUN_AS_NODE;
  try {
    const parts = [process.execPath, require.resolve('electron/cli.js'), __filename, ...process.argv.slice(2)];
    if (parts.some(value => /["%\r\n]/.test(value))) throw new Error('Unsupported capture argument');
    execFileSync('cmd.exe', ['/d', '/v:off', '/s', '/c', `"${parts.map(value => `"${value}"`).join(' ')}"`], { env, stdio: 'inherit', timeout: 90000, windowsVerbatimArguments: true });
  } finally {
    rmSync(temporary, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }
  return;
}

const { app, BrowserWindow, ipcMain, nativeImage } = require('electron');
const { buildSync } = require('esbuild');
const ts = require('typescript');
const postcss = require('postcss');
const tailwind = require('tailwindcss');
const args = process.argv.slice(2);
const value = (key, fallback) => args.includes(key) ? args[args.indexOf(key) + 1] : fallback;
const output = resolve(value('--output', 'migration-artifacts/native-auth/react'));
const scale = Number(value('--scale', '1'));
if (![1, 1.25, 1.5, 2].includes(scale)) throw new Error('Unsupported DPI');
const temporary = process.env.ZABOR_AUTH_CAPTURE_TEMP;
if (!temporary) throw new Error('Run through node scripts/native/capture-auth.cjs');
app.setPath('userData', temporary);
app.setPath('sessionData', temporary);
let window;
let fixture;
const watchdog = setTimeout(() => app.exit(1), 80000);
app.whenReady().then(async () => {
  const file = 'src/renderer/src/App.tsx';
  const source = ts.createSourceFile(file, readFileSync(file, 'utf8'), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const cards = [];
  const visit = node => {
    if (ts.isJsxElement(node) && node.openingElement.attributes.properties.some(attribute => ts.isJsxAttribute(attribute) && attribute.name.getText(source) === 'ref' && attribute.initializer?.getText(source) === '{authCardRef}')) cards.push(node.getText(source));
    ts.forEachChild(node, visit);
  };
  visit(source);
  if (cards.length !== 3) throw new Error('Auth reference structure changed');
  const entry = `import React,{useRef,useState} from 'react';import {createRoot} from 'react-dom/client';import i18n from 'i18next';import {I18nextProvider,useTranslation} from 'react-i18next';import {Eye,EyeSlash as EyeOff,Camera,ArrowsClockwise} from '@phosphor-icons/react';import {TitleBar} from './src/renderer/src/components/Layout/TitleBar';import ru from './src/renderer/src/locales/ru.json';import en from './src/renderer/src/locales/en.json';const fixture=await window.windowControls.getFixture();await i18n.init({lng:fixture.locale,fallbackLng:'ru',resources:{ru:{translation:ru},en:{translation:en}},interpolation:{escapeValue:false}});function Fixture(){const {t}=useTranslation();const [login,setLogin]=useState('');const [password,setPassword]=useState('');const [showPassword,setShowPassword]=useState(false);const [authStep,setAuthStep]=useState(fixture.state);const [displayName,setDisplayName]=useState('');const [captchaAnswer,setCaptchaAnswer]=useState('');const authCardRef=useRef(null),authErrorRef=useRef(null),loginInputRef=useRef(null),passwordInputRef=useRef(null);const isLoading=false,isCaptchaLoading=false,error='',avatarBase64=null,avatarColor='#C81E70',captchaData=null;const handleAuth=()=>{},fetchCaptcha=()=>{},onFileChange=()=>{};return <div className="fixed inset-0 flex flex-col bg-appBg text-textMain select-none"><TitleBar/><div className="flex-1 flex items-center justify-center p-4">{authStep==='login'?(${cards[0]}):authStep==='confirm'?(${cards[1]}):(${cards[2]})}</div></div>};createRoot(document.getElementById('root')).render(<I18nextProvider i18n={i18n}><Fixture/></I18nextProvider>);`;
  const wrapped = entry.replace('const fixture=await', 'async function boot(){const fixture=await') + '\n}boot();';
  const bundle = buildSync({ stdin: { contents: wrapped, resolveDir: process.cwd(), loader: 'tsx' }, jsx: 'automatic', bundle: true, format: 'iife', platform: 'browser', target: 'es2022', write: false }).outputFiles[0].text;
  const css = await postcss([tailwind(require(resolve('tailwind.config.js')))]).process(readFileSync('src/renderer/src/index.css', 'utf8'), { from: resolve('src/renderer/src/index.css') });
  writeFileSync(join(temporary, 'reference.js'), bundle);
  writeFileSync(join(temporary, 'reference.css'), css.css);
  writeFileSync(join(temporary, 'index.html'), '<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src \'self\' data:; script-src \'self\'; style-src \'self\' \'unsafe-inline\'; connect-src \'none\'"><link rel="stylesheet" href="reference.css"></head><body><div id="root"></div><script src="reference.js"></script></body></html>');
  writeFileSync(join(temporary, 'preload.cjs'), "const {contextBridge,ipcRenderer}=require('electron');contextBridge.exposeInMainWorld('windowControls',{getFixture:()=>ipcRenderer.invoke('fixture'),onMaximizeChange:()=>()=>{},minimize:()=>{},maximize:()=>{},close:()=>{}});");
  ipcMain.handle('fixture', () => fixture);
  window = new BrowserWindow({ width: 1100, height: 760, useContentSize: true, frame: false, show: false, webPreferences: { preload: join(temporary, 'preload.cjs'), nodeIntegration: false, contextIsolation: true, sandbox: true, offscreen: true, backgroundThrottling: false } });
  window.webContents.on('console-message', event => { if (event.level === 'error') console.error(event.message); });
  mkdirSync(output, { recursive: true });
  const captures = [];
  for (const locale of ['ru', 'en']) for (const state of ['login', 'confirm', 'setup']) {
    fixture = { locale, state };
    const target = join(output, `react-auth-${locale}-${state}-${scale}.png`);
    if (existsSync(target)) throw new Error('Capture already exists');
    await window.loadFile(join(temporary, 'index.html'), { query: { locale, state } });
    if (!window.webContents.debugger.isAttached()) window.webContents.debugger.attach('1.3');
    await window.webContents.debugger.sendCommand('Emulation.setDeviceMetricsOverride', { width: 1100, height: 760, deviceScaleFactor: scale, mobile: false });
    await window.webContents.executeJavaScript(`new Promise((resolve,reject)=>{const timeout=setTimeout(()=>reject(Error('Auth reference did not mount')),5000);const check=()=>{if(document.querySelector('.glass-modal')){clearTimeout(timeout);resolve();}else requestAnimationFrame(check);};check();})`);
    await window.webContents.executeJavaScript(`(async()=>{await document.fonts.ready;await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));})()`);
    const metrics = await window.webContents.executeJavaScript(`({width:innerWidth,height:innerHeight,scale:devicePixelRatio,elements:[...document.querySelectorAll('.glass-modal,.glass-modal *')].map(element=>{const r=element.getBoundingClientRect(),s=getComputedStyle(element);return {tag:element.tagName,text:element.children.length?null:element.textContent,x:r.x,y:r.y,width:r.width,height:r.height,fontSize:s.fontSize,color:s.color}})})`);
    if (metrics.width !== 1100 || metrics.height !== 760 || metrics.scale !== scale) throw new Error('Auth viewport mismatch');
    const capture = await window.webContents.debugger.sendCommand('Page.captureScreenshot', { format: 'png', fromSurface: true, captureBeyondViewport: false });
    const image = nativeImage.createFromBuffer(Buffer.from(capture.data, 'base64'));
    const size = image.getSize();
    if (size.width !== 1100 * scale || size.height !== 760 * scale) throw new Error('Auth capture dimensions mismatch');
    writeFileSync(target, image.toPNG());
    captures.push({ locale, state, file: target, metrics });
  }
  const sources = Object.fromEntries([file, 'src/renderer/src/index.css', 'tailwind.config.js', 'src/renderer/src/locales/ru.json', 'src/renderer/src/locales/en.json'].map(file => [file, createHash('sha256').update(readFileSync(file)).digest('hex')]));
  writeFileSync(join(output, `auth-reference-${scale}.json`), JSON.stringify({ sources, captures }, null, 2));
  window.webContents.debugger.detach();
  window.destroy();
  app.quit();
  console.log(JSON.stringify({ captures: captures.length, output }));
}).catch(error => { console.error(error.message); if (window && !window.isDestroyed()) window.destroy(); app.exit(1); });
app.on('quit', () => clearTimeout(watchdog));
