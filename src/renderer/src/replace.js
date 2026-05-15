const fs = require('fs');
let code = fs.readFileSync('App.tsx', 'utf8');

// Replacements for Mic
code = code.replace(
    /<button\s+onClick=\{toggleMute\}\s+className=\{`w-14 h-14 rounded-full flex items-center justify-center relative transition-colors \$\{\(store\.currentUser\?\.isMuted \|\| store\.currentUser\?\.isServerMuted \|\| store\.currentUser\?\.isServerDeafened\)\s*\?\s*'bg-\\[#2B2D31\\] text-white'\s*:\s*'bg-surface hover:bg-surfaceHover text-white'\s*\}`\}\s*>\s*<Mic weight="bold" size=\{24\} \/>\s*\{\(store\.currentUser\?\.isMuted \|\| store\.currentUser\?\.isServerMuted \|\| store\.currentUser\?\.isServerDeafened\) && \(\s*<div className="absolute w-\[30px\] h-\[3px\] bg-danger rotate-45 rounded-full" \/>\s*\)\}\s*<\/button>/g,
    `<button
                  onClick={toggleMute}
                  className={\`group w-14 h-14 rounded-full flex items-center justify-center relative transition-colors \${(store.currentUser?.isMuted || store.currentUser?.isServerMuted || store.currentUser?.isServerDeafened)
                    ? 'bg-[#2B2D31] text-white'
                    : 'bg-surface hover:bg-surfaceHover text-white'
                    }\`}
                >
                  <div className="flex items-center justify-center transition-transform duration-200 group-active:scale-95 group-hover:scale-110">
                    <Mic weight="bold" size={24} />
                    <div className={\`absolute w-[30px] h-[3px] bg-danger rounded-full transition-transform duration-300 origin-center \${(store.currentUser?.isMuted || store.currentUser?.isServerMuted || store.currentUser?.isServerDeafened) ? 'scale-x-100 opacity-100 rotate-45' : 'scale-x-0 opacity-0 rotate-45'}\`} />
                  </div>
                </button>`
);

// Replacements for Headphones
code = code.replace(
    /<button\s+onClick=\{toggleDeafen\}\s+className=\{`w-14 h-14 rounded-full flex items-center justify-center relative transition-colors \$\{\(store\.currentUser\?\.isDeafened \|\| store\.currentUser\?\.isServerDeafened\)\s*\?\s*'bg-\\[#2B2D31\\] text-white'\s*:\s*'bg-surface hover:bg-surfaceHover text-white'\s*\}`\}\s*>\s*<Headphones weight="bold" size=\{24\} \/>\s*\{\(store\.currentUser\?\.isDeafened \|\| store\.currentUser\?\.isServerDeafened\) && \(\s*<div className="absolute w-\[30px\] h-\[3px\] bg-danger rotate-45 rounded-full" \/>\s*\)\}\s*<\/button>/g,
    `<button
                  onClick={toggleDeafen}
                  className={\`group w-14 h-14 rounded-full flex items-center justify-center relative transition-colors \${(store.currentUser?.isDeafened || store.currentUser?.isServerDeafened)
                    ? 'bg-[#2B2D31] text-white'
                    : 'bg-surface hover:bg-surfaceHover text-white'
                    }\`}
                >
                  <div className="flex items-center justify-center transition-transform duration-200 group-active:scale-95 group-hover:scale-110">
                    <Headphones weight="bold" size={24} />
                    <div className={\`absolute w-[30px] h-[3px] bg-danger rounded-full transition-transform duration-300 origin-center \${(store.currentUser?.isDeafened || store.currentUser?.isServerDeafened) ? 'scale-x-100 opacity-100 rotate-45' : 'scale-x-0 opacity-0 rotate-45'}\`} />
                  </div>
                </button>`
);

// Replacements for End Call
code = code.replace(
    /<button onClick=\{handleEndCall\} className="bg-danger hover:bg-red-600 text-white font-bold py-3\.5 px-8 rounded-full flex items-center gap-3 transition-colors text-\[15px\]">\s*<PhoneOff weight="bold" size=\{20\} \/> \{t\('main\.voice\.endCall'\)\}\s*<\/button>/g,
    `<button onClick={handleEndCall} className="group bg-danger hover:bg-red-600 text-white font-bold py-3.5 px-8 rounded-full flex items-center gap-3 transition-colors text-[15px]">
                  <div className="transition-transform duration-300 group-hover:-rotate-12 group-hover:scale-110">
                    <PhoneOff weight="bold" size={20} />
                  </div>
                  {t('main.voice.endCall')}
                </button>`
);

// Replacements for Leave Channel
code = code.replace(
    /<button onClick=\{\(\) => signalRService\.leaveChannel\(\)\} className="bg-danger hover:bg-red-600 text-white font-bold py-3\.5 px-8 rounded-full flex items-center gap-3 transition-colors text-\[15px\]">\s*<Phone weight="bold" size=\{20\} \/> \{t\('main\.voice\.endCall'\)\}\s*<\/button>/g,
    `<button onClick={() => signalRService.leaveChannel()} className="group bg-danger hover:bg-red-600 text-white font-bold py-3.5 px-8 rounded-full flex items-center gap-3 transition-colors text-[15px]">
                  <div className="transition-transform duration-300 group-hover:-rotate-12 group-hover:scale-110">
                    <Phone weight="bold" size={20} />
                  </div>
                  {t('main.voice.endCall')}
                </button>`
);

fs.writeFileSync('App.tsx', code);
console.log('done');
