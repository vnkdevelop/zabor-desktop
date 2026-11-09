const fs = require('node:fs');
const path = require('node:path');
const root = process.argv[2];
if (process.argv.length !== 3 || !root || !path.isAbsolute(root) || fs.realpathSync.native(root) !== root) throw new Error('Explicit isolated reference root required');
const postcss = require(path.join(root, 'node_modules/postcss'));
const tailwind = require(path.join(root, 'node_modules/tailwindcss'));
const esbuild = require(path.join(root, 'node_modules/esbuild'));
const entry = "import React from 'react'; import { createRoot } from 'react-dom/client'; import i18n from 'i18next'; import { I18nextProvider } from 'react-i18next'; import UpdaterWindow from './src/renderer/src/components/UpdaterWindow'; import ru from './src/renderer/src/locales/ru.json'; import en from './src/renderer/src/locales/en.json'; i18n.init({lng:new URLSearchParams(location.hash.slice(1)).get('locale'),fallbackLng:'ru',resources:{ru:{translation:ru},en:{translation:en}},interpolation:{escapeValue:false}}).then(()=>createRoot(document.getElementById('root')).render(<I18nextProvider i18n={i18n}><UpdaterWindow/></I18nextProvider>));";
process.chdir(root);
const bundle = esbuild.buildSync({ stdin: { contents: entry, resolveDir: root, loader: 'tsx' }, bundle: true, format: 'iife', platform: 'browser', target: 'es2022', write: false }).outputFiles[0].text;
postcss([tailwind(require(path.join(root, 'tailwind.config.js')))]).process(fs.readFileSync(path.join(root, 'src/renderer/src/index.css'), 'utf8'), { from: path.join(root, 'src/renderer/src/index.css') }).then(css => {
  fs.writeFileSync(path.join(root, 'reference.js'), bundle, { flag: 'wx' });
  fs.writeFileSync(path.join(root, 'reference.css'), css.css, { flag: 'wx' });
  process.stdout.write(JSON.stringify({ javascriptBytes: Buffer.byteLength(bundle), cssBytes: Buffer.byteLength(css.css), node: process.version }));
}).catch(error => { console.error(error.message); process.exitCode = 1; });
