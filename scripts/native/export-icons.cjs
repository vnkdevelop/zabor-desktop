const React = require('react');
const { renderToStaticMarkup } = require('react-dom/server');
const { existsSync, mkdirSync, readFileSync, writeFileSync } = require('node:fs');
const { resolve, join } = require('node:path');
const directory = resolve(__dirname, '../../rust/crates/zabor-native/assets/icons');
mkdirSync(directory, { recursive: true });
import('@phosphor-icons/react').then(icons => {
for (const [name, component] of Object.entries({ eye: icons.Eye, 'eye-off': icons.EyeSlash, camera: icons.Camera, refresh: icons.ArrowsClockwise, gear: icons.Gear, plus: icons.Plus, users: icons.Users, bell: icons.Bell, mail: icons.Envelope, microphone: icons.Microphone, headphones: icons.Headphones, desktop: icons.Desktop, 'phone-off': icons.PhoneDisconnect, 'user-plus': icons.UserPlus })) {
  const svg = renderToStaticMarkup(React.createElement(component, { weight: 'bold', color: '#ffffff', size: 256 })).replace(/currentColor/g, '#ffffff') + '\n';
  const path = join(directory, `${name}.svg`);
  if (existsSync(path) && readFileSync(path, 'utf8') !== svg) throw new Error(`Existing icon differs: ${name}`);
  if (!existsSync(path)) writeFileSync(path, svg);
}
console.log('Native SVG icons match the existing Phosphor React components');
}).catch(error => { console.error(error.message); process.exitCode = 1; });
