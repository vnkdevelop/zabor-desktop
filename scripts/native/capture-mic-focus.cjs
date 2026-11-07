const { createHash } = require('node:crypto');
const { readFileSync, writeFileSync } = require('node:fs');
const { join } = require('node:path');
const { nativeImage } = require('electron');

module.exports = async ({ window, output, scale, load }) => {
  const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex');
  const sourceFiles = [__filename, 'scripts/native/capture-mic-test.cjs', 'src/renderer/src/components/Settings/MicTestPanel.tsx', 'src/renderer/src/index.css', 'src/renderer/src/lib/anime.ts', 'tailwind.config.js', 'src/renderer/src/locales/ru.json', 'src/renderer/src/locales/en.json'];
  const sources = Object.fromEntries(sourceFiles.map(file => [file, digest(file)]));
  const js = script => window.webContents.executeJavaScript(script);
  const command = (name, parameters) => window.webContents.debugger.sendCommand(name, parameters);
  const settle = () => js('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))');
  const key = async (name, code, virtual, modifiers = 0, downOnly = false) => {
    const event = { key: name, code, windowsVirtualKeyCode: virtual, modifiers };
    await command('Input.dispatchKeyEvent', { type: 'keyDown', ...event });
    if (!downOnly) await command('Input.dispatchKeyEvent', { type: 'keyUp', ...event });
    await settle();
  };
  const pointer = async (control, down) => {
    const point = await js(`(()=>{const panel=document.querySelector('#root>div>div');const elements=[...panel.querySelectorAll('button,[role=slider]')];const element=elements.find(element=>element.dataset.focusKind===${JSON.stringify(control)});const r=element?.getBoundingClientRect();return r?{x:r.x+r.width/2,y:r.y+r.height/2}:{x:800,y:140};})()`);
    await command('Input.dispatchMouseEvent', { type: down ? 'mousePressed' : 'mouseReleased', button: 'left', buttons: down ? 1 : 0, clickCount: 1, ...point });
    await settle();
  };
  const captures = [];
  const diagnostic = join(output, `mic-focus-diagnostic-${scale}.json`);
  writeFileSync(diagnostic, JSON.stringify({ sources, captures, focusRasterAccepted: false }), { flag: 'wx' });
  const prepare = async (locale, state) => {
    await load(locale, state);
    if (!window.webContents.debugger.isAttached()) window.webContents.debugger.attach('1.3');
    await command('Emulation.setDeviceMetricsOverride', { width: 1100, height: 160, deviceScaleFactor: scale, mobile: false });
    await command('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: 'reduce' }] });
    await command('Emulation.setFocusEmulationEnabled', { enabled: true });
    await js(`(async()=>{await document.fonts.ready;await new Promise((resolve,reject)=>{const deadline=setTimeout(()=>reject(Error('Focus panel did not mount')),5000);const check=()=>{if(document.querySelector('#root button')){clearTimeout(deadline);resolve();}else requestAnimationFrame(check);};check();});await new Promise(resolve=>setTimeout(resolve,700));})()`);
    if (state !== 'idle') {
      await js("document.querySelector('#root button').click()");
      await js('new Promise(resolve=>setTimeout(resolve,150))');
    }
    if (state === 'paused') await js("document.querySelector('#root button').click()");
    await settle();
    await js(`(()=>{const panel=document.querySelector('#root>div>div');const ready=!!panel.querySelector('[role=slider]');for(const [index,element]of [...panel.querySelectorAll('button,[role=slider]')].entries())element.dataset.focusKind=ready?['playback','seek','retake'][index]:'primary';})()`);
  };
  for (const locale of ['ru', 'en']) for (const state of ['idle', 'arming', 'recording', 'paused', 'silent', 'failed']) {
    await prepare(locale, state);
    let captureOrigin;
    const capture = async (action, expectedControl, expectedVisible) => {
      const metrics = await js(`(()=>{const panel=document.querySelector('#root>div>div'),origin=panel.getBoundingClientRect();return {panelX:origin.x,panelY:origin.y,panelWidth:origin.width,panelHeight:origin.height,scale:devicePixelRatio,hidden:panel.getAttribute('aria-hidden')==='true',position:window.__audio.position,playing:window.__audio.playing,focusedControl:document.activeElement.dataset.focusKind||'',focusVisible:document.activeElement.matches(':focus-visible'),controls:[...panel.querySelectorAll('button,[role=slider]')].map(element=>{const r=element.getBoundingClientRect(),s=getComputedStyle(element),p=element.parentElement,c=getComputedStyle(p),clip=p.getBoundingClientRect();return {kind:element.dataset.focusKind,focused:document.activeElement===element,focusVisible:element.matches(':focus-visible'),disabled:!!element.disabled,tabIndex:element.tabIndex,x:r.x-origin.x,y:r.y-origin.y,width:r.width,height:r.height,outlineStyle:s.outlineStyle,outlineWidth:s.outlineWidth,outlineOffset:s.outlineOffset,outlineColor:s.outlineColor,borderRadius:s.borderRadius,parentOverflow:c.overflow,parentRadius:c.borderRadius,parentClip:{x:clip.x-origin.x,y:clip.y-origin.y,width:clip.width,height:clip.height}};})};})()`);
      const file = `react-focus-${state}-${action}-${locale}-${scale}.png`;
      const screenshot = await command('Page.captureScreenshot', { format: 'png', fromSurface: true, captureBeyondViewport: false });
      const image = nativeImage.createFromBuffer(Buffer.from(screenshot.data, 'base64'));
      if (!metrics.hidden) captureOrigin = metrics;
      const crop = { x: Math.round((captureOrigin.panelX - 6) * scale), y: Math.round((captureOrigin.panelY - 6) * scale), width: Math.round(168 * scale), height: Math.ceil((captureOrigin.panelHeight + 12) * scale) };
      metrics.captureCrop = crop;
      writeFileSync(join(output, file), image.crop(crop).toPNG(), { flag: 'wx' });
      captures.push({ locale, state, action, file, sha256: digest(join(output, file)), metrics });
      writeFileSync(diagnostic, JSON.stringify({ sources, captures, focusRasterAccepted: false }, null, 2));
      if (expectedControl !== undefined && (metrics.focusedControl !== expectedControl || (!!expectedControl && expectedVisible !== undefined && metrics.focusVisible !== expectedVisible))) throw new Error(`Reference focus mismatch ${locale}/${state}/${action}: ${metrics.focusedControl}/${metrics.focusVisible}`);
      return metrics;
    };
    await capture('initial', '', false);
    await key('Tab', 'Tab', 9);
    if (state === 'arming' || state === 'recording') {
      await capture('disabled-tab', '', false);
      continue;
    }
    const ready = state === 'paused' || state === 'silent';
    await capture('tab-first', ready ? 'playback' : 'primary', true);
    if (ready) {
      await key('Tab', 'Tab', 9);
      await capture('tab-seek', 'seek', true);
      await key('Tab', 'Tab', 9);
      await capture('tab-retake', 'retake', true);
      await key('Tab', 'Tab', 9, 8);
      await capture('reverse-seek', 'seek', true);
      await pointer('seek', true);
      await capture('pointer-seek-down', 'seek');
      await pointer('seek', false);
      await capture('pointer-seek-up', 'seek');
      await key('ArrowRight', 'ArrowRight', 39);
      await capture('pointer-to-keyboard', 'seek', true);
      await key('Shift', 'ShiftLeft', 16, 8);
      await capture('keyboard-modifier', 'seek', true);
      await pointer('playback', true);
      await capture('pointer-playback-down', 'playback');
      await pointer('playback', false);
      await capture('pointer-playback-up', 'playback');
      await pointer('retake', true);
      await capture('pointer-retake-down', 'retake');
      await pointer('', false);
      await capture('pointer-retake-cancel', 'retake');
      await key('Shift', 'ShiftLeft', 16, 8);
      await capture('pointer-modifier', 'retake');
      await key('Tab', 'Tab', 9, 8);
      await capture('reverse-from-pointer', 'seek', true);
      await key('Tab', 'Tab', 9, 8);
      await capture('reverse-playback', 'playback', true);
      await command('Input.dispatchKeyEvent', { type: 'keyUp', key: ' ', code: 'Space', windowsVirtualKeyCode: 32 });
      await settle();
      await capture('space-unarmed-up', 'playback', true);
      await key(' ', 'Space', 32, 0, true);
      await capture('space-down', 'playback', true);
      await command('Input.dispatchKeyEvent', { type: 'keyUp', key: ' ', code: 'Space', windowsVirtualKeyCode: 32 });
      await settle();
      await capture('space-up', 'playback', true);
      await command('Input.dispatchKeyEvent', { type: 'keyUp', key: ' ', code: 'Space', windowsVirtualKeyCode: 32 });
      await settle();
      await capture('space-repeat-up', 'playback', true);
      await key(' ', 'Space', 32, 0, true);
      await key('Tab', 'Tab', 9);
      await capture('space-arm-transfer', 'seek', true);
      await command('Input.dispatchKeyEvent', { type: 'keyUp', key: ' ', code: 'Space', windowsVirtualKeyCode: 32 });
      await settle();
      await capture('space-transfer-up', 'seek', true);
      await key('Tab', 'Tab', 9, 8);
      await capture('space-return-playback', 'playback', true);
      await command('Input.dispatchKeyEvent', { type: 'keyUp', key: ' ', code: 'Space', windowsVirtualKeyCode: 32 });
      await settle();
      await capture('space-after-transfer-up', 'playback', true);
      await js('window.__setMicActive(false)');
      await settle();
      await capture('hidden');
      await key('Tab', 'Tab', 9);
      const hidden = await capture('hidden-tab');
      if (!hidden.hidden || hidden.controls.some(control => control.tabIndex !== -1)) throw new Error('Hidden panel remained in tab order');
    } else {
      await pointer('primary', true);
      await capture('pointer-primary-down', 'primary');
      await pointer('', false);
      await capture('pointer-primary-cancel', 'primary');
      await key('Shift', 'ShiftLeft', 16, 8);
      await capture('pointer-modifier', 'primary');
      await key('ArrowRight', 'ArrowRight', 39);
      await capture('pointer-to-keyboard', 'primary', true);
    }
    await prepare(locale, state);
    await capture('fresh-initial', '', false);
    const first = ready ? 'playback' : 'primary';
    await pointer(first, true);
    await capture('fresh-pointer-down', first, false);
    await pointer('', false);
    await capture('fresh-pointer-cancel', first, false);
    await key('Control', 'ControlLeft', 17, 2);
    await capture('fresh-pointer-control', first, false);
    await key('Alt', 'AltLeft', 18, 1);
    await capture('fresh-pointer-alt', first, false);
    await key('Shift', 'ShiftLeft', 16, 8);
    await capture('fresh-pointer-shift', first, true);
    await pointer(first, true);
    await capture('fresh-keyboard-pointer-down', first, true);
    await pointer('', false);
    await capture('fresh-keyboard-pointer-cancel', first, true);
  }
  for (const [file, hash] of Object.entries(sources)) if (digest(file) !== hash) throw new Error('Reference source changed during focus checks');
  writeFileSync(join(output, `mic-focus-reference-${scale}.json`), JSON.stringify({ sources, captures, nativeClientReady: false, micTestUiReady: false, focusRasterAccepted: false, actualAudioServiceBundled: false, scope: 'unchanged React focus-visible, actual CDP keyboard/pointer, RU/EN six states; 6px-margin reduced-motion screenshots and CSS clipping metrics; isolated memory-only audio and blocked network; broader modal focus and exact native raster acceptance remain separate' }, null, 2), { flag: 'wx' });
  console.log(JSON.stringify({ referenceFocusCaptures: captures.length, output, scale }));
};
