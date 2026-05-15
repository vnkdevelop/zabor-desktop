const fs = require('fs');
let code = fs.readFileSync('App.tsx', 'utf8');

const icons = ['Settings', 'Mic', 'MicOff', 'Headphones', 'Phone', 'Eye', 'EyeOff', 'UserMinus', 'Camera', 'Check', 'X', 'LogOut', 'UserPlus', 'Mail', 'Edit2', 'Volume2', 'PhoneOff', 'Wifi', 'WifiOff', 'Users', 'LeaveIcon', 'Crown', 'UserX', 'Globe', 'Trophy'];

const regex = new RegExp('<(' + icons.join('|') + ')(?=\\s|>)', 'g');
code = code.replace(regex, '<$1 weight="bold"');

fs.writeFileSync('App.tsx', code);
