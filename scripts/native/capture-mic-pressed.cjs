const { createHash } = require('node:crypto');
const { readFileSync, writeFileSync } = require('node:fs');
const { join } = require('node:path');
const { nativeImage } = require('electron');

module.exports = async ({ window, output, scale, load }) => {
  const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex');
  const sources = Object.fromEntries([__filename, 'scripts/native/capture-mic-test.cjs', 'src/renderer/src/components/Settings/MicTestPanel.tsx', 'src/renderer/src/index.css', 'src/renderer/src/lib/anime.ts', 'tailwind.config.js', 'src/renderer/src/locales/ru.json', 'src/renderer/src/locales/en.json'].map(file => [file, digest(file)]));
  const js = script => window.webContents.executeJavaScript(script);
  const command = (name, parameters) => window.webContents.debugger.sendCommand(name, parameters);
  const settle = () => js('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))');
  const space = async (down, repeat = false) => command('Input.dispatchKeyEvent', { type: down ? 'keyDown' : 'keyUp', key: ' ', code: 'Space', windowsVirtualKeyCode: 32, autoRepeat: repeat });
  const tab = async (reverse = false) => {
    const event = { key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9, modifiers: reverse ? 8 : 0 };
    await command('Input.dispatchKeyEvent', { type: 'keyDown', ...event });
    await command('Input.dispatchKeyEvent', { type: 'keyUp', ...event });
  };
  let target;
  const point = async inside => inside ? js(`(()=>{const r=window.__pressedTarget.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2};})()`) : { x: 1000, y: 8 };
  const pointer = async (type, inside, down) => command('Input.dispatchMouseEvent', { type, button: type === 'mouseMoved' ? 'none' : 'left', buttons: down ? 1 : 0, clickCount: type === 'mouseMoved' ? 0 : 1, ...await point(inside) });
  const scenarios = {
    space: ['rest', 'unarmed-up', 'space-held', 'repeat-down', 'space-up', 'repeat-up'],
    transfer: ['rest', 'space-held', 'transfer-away', 'transfer-return', 'space-up'],
    blur: ['rest', 'space-held', 'blur', 'refocus-down', 'refocus-cancel', 'space-up'],
    disable: ['rest', 'space-held', 'disable', 'space-up'],
    pointer: ['pointer-rest', 'pointer-down', 'pointer-outside', 'pointer-return', 'pointer-cancel'],
    'pointer-commit': ['pointer-rest', 'pointer-down', 'pointer-outside', 'pointer-return', 'pointer-up'],
    'keyboard-pointer': ['rest', 'pointer-down', 'pointer-cancel'],
    rapid: ['rest', 'rapid-transfer', 'rapid-click'],
  };
  const captures = [];
  for (const locale of ['ru', 'en']) for (const state of ['idle', 'failed', 'paused', 'silent']) for (const [scenario, actions] of Object.entries(scenarios)) {
    await load(locale, state);
    if (!window.webContents.debugger.isAttached()) window.webContents.debugger.attach('1.3');
    await command('Emulation.setDeviceMetricsOverride', { width: 1100, height: 160, deviceScaleFactor: scale, mobile: false });
    await command('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: 'reduce' }] });
    await command('Emulation.setFocusEmulationEnabled', { enabled: true });
    await js(`(async()=>{await document.fonts.ready;await new Promise((resolve,reject)=>{const deadline=setTimeout(()=>reject(Error('Pressed panel did not mount')),5000);const check=()=>{if(document.querySelector('#root button')){clearTimeout(deadline);resolve();}else requestAnimationFrame(check);};check();});await new Promise(resolve=>setTimeout(resolve,700));})()`);
    if (state !== 'idle') {
      await js("document.querySelector('#root button').click()");
      await settle();
    }
    if (state === 'paused') await js("document.querySelector('#root button').click()");
    target = state === 'paused' || state === 'silent' ? 'retake' : 'primary';
    await js(`(()=>{const panel=document.querySelector('#root>div>div'),ready=!!panel.querySelector('[role=slider]');for(const[index,element]of[...panel.querySelectorAll('button,[role=slider]')].entries())element.dataset.focusKind=ready?['playback','seek','retake'][index]:'primary';window.__pressedTarget=[...panel.querySelectorAll('button')].find(element=>element.dataset.focusKind===${JSON.stringify(target)});window.__recordCalls=0;window.__fixture.state='arming';window.__pressedInput=[];for(const type of ['pointerdown','pointermove','pointerup','mousedown','mousemove','mouseup'])document.addEventListener(type,event=>window.__pressedInput.push({type,buttons:event.buttons,button:event.button,trusted:event.isTrusted,target:event.target.dataset.focusKind||'',x:event.clientX,y:event.clientY}));const next=document.createElement('button');next.textContent='fixture-only';next.style.cssText='position:absolute;left:1000px;top:8px;width:1px;height:1px;opacity:0';document.body.appendChild(next);})()`);
    let captureOrigin = await js("(()=>{const r=document.querySelector('#root>div>div').getBoundingClientRect();return{x:r.x,y:r.y,height:r.height};})()");
    if (!scenario.startsWith('pointer')) for (let index = 0; index < (target === 'primary' ? 1 : 3); index++) await tab();
    for (const action of actions) {
      switch (action) {
        case 'rest': case 'pointer-rest': break;
        case 'unarmed-up': case 'space-up': case 'repeat-up': await space(false); break;
        case 'space-held': await space(true); break;
        case 'repeat-down': await space(true, true); break;
        case 'transfer-away': await tab(); break;
        case 'transfer-return': await tab(true); break;
        case 'blur': await js('window.__pressedTarget.blur()'); break;
        case 'refocus-down': case 'pointer-down': await pointer('mousePressed', true, true); break;
        case 'pointer-outside': await pointer('mouseMoved', false, true); break;
        case 'pointer-return': await pointer('mouseMoved', true, true); break;
        case 'refocus-cancel': case 'pointer-cancel': await pointer('mouseReleased', false, false); break;
        case 'pointer-up': await pointer('mouseReleased', true, false); break;
        case 'disable': await js('window.__pressedTarget.click()'); break;
        case 'rapid-transfer': await space(true); await tab(); await tab(true); await space(false); break;
        case 'rapid-click': await space(true); await space(false); break;
        default: throw new Error('Unsupported pressed reference action');
      }
      await settle();
      const metrics = await js(`(()=>{const panel=document.querySelector('#root>div>div'),r=panel.getBoundingClientRect(),element=window.__pressedTarget,s=getComputedStyle(element),m=new DOMMatrixReadOnly(s.transform),box=element.getBoundingClientRect();return{panelX:r.x,panelY:r.y,panelWidth:r.width,panelHeight:r.height,scale:devicePixelRatio,target:${JSON.stringify(target)},connected:element.isConnected,disabled:element.disabled,active:element.matches(':active'),hover:element.matches(':hover'),input:window.__pressedInput,focused:document.activeElement===element,focusVisible:element.matches(':focus-visible'),focusedControl:document.activeElement.dataset.focusKind||'',transform:s.transform,scaleX:m.a,scaleY:m.d,outlineWidth:s.outlineWidth,outlineOffset:s.outlineOffset,borderRadius:s.borderRadius,box:{x:box.x-r.x,y:box.y-r.y,width:box.width,height:box.height},recordCalls:window.__recordCalls,primaryDisabled:!!panel.querySelector('button:disabled')};})()`);
      if (metrics.connected) captureOrigin = { x: metrics.panelX, y: metrics.panelY, height: Math.max(metrics.panelHeight, captureOrigin.height) };
      const screenshot = await command('Page.captureScreenshot', { format: 'png', fromSurface: true, captureBeyondViewport: false });
      const image = nativeImage.createFromBuffer(Buffer.from(screenshot.data, 'base64'));
      const crop = { x: Math.round((captureOrigin.x - 6) * scale), y: Math.round((captureOrigin.y - 6) * scale), width: Math.round(168 * scale), height: Math.ceil((captureOrigin.height + 12) * scale) };
      metrics.captureCrop = crop;
      const file = `react-pressed-${state}-${scenario}-${action}-${locale}-${scale}.png`;
      writeFileSync(join(output, file), image.crop(crop).toPNG(), { flag: 'wx' });
      captures.push({ locale, state, scenario, action, file, sha256: digest(join(output, file)), metrics });
      writeFileSync(join(output, `mic-pressed-diagnostic-${scale}.json`), JSON.stringify({ sources, captures, pressedStateAccepted: false }, null, 2));
    }
  }
  for (const [file, hash] of Object.entries(sources)) if (digest(file) !== hash) throw new Error('Pressed reference sources changed');
  writeFileSync(join(output, `mic-pressed-reference-${scale}.json`), JSON.stringify({ sources, captures, pressedStateAccepted: false, nativeClientReady: false, actualAudioServiceBundled: false, scope: 'unchanged React DOM :active/transform/focus and real CDP pointer/keyboard with repeated/transfer/blur/disable/rapid sequences; isolated reduced-motion memory-only fixture, record counts and blocked network; snapshots are settled observations, not physical window/full-modal/continuous-input acceptance' }, null, 2), { flag: 'wx' });
  console.log(JSON.stringify({ pressedReferenceCaptures: captures.length, scale, output }));
};
