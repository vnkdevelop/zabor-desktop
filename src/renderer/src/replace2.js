const fs = require('fs');
let code = fs.readFileSync('App.tsx', 'utf8');

// Replace inner content of Mic
code = code.replace(
    /<Mic weight="bold" size=\{24\} \/>\s*\{\(store\.currentUser\?\.isMuted \|\| store\.currentUser\?\.isServerMuted \|\| store\.currentUser\?\.isServerDeafened\) && \(\s*<div className="absolute w-\[30px\] h-\[3px\] bg-danger rotate-45 rounded-full" \/>\s*\)\}/g,
    `<div className="flex items-center justify-center transition-transform duration-200 group-active:scale-95 group-hover:scale-110">
                    <Mic weight="bold" size={24} />
                    <div className={\`absolute w-[30px] h-[3px] bg-danger rounded-full transition-all duration-300 origin-center \${ (store.currentUser?.isMuted || store.currentUser?.isServerMuted || store.currentUser?.isServerDeafened) ? 'scale-100 opacity-100 rotate-45' : 'scale-0 opacity-0 rotate-45' }\`} />
                  </div>`
);

// Replace inner content of Headphones
code = code.replace(
    /<Headphones weight="bold" size=\{24\} \/>\s*\{\(store\.currentUser\?\.isDeafened \|\| store\.currentUser\?\.isServerDeafened\) && \(\s*<div className="absolute w-\[30px\] h-\[3px\] bg-danger rotate-45 rounded-full" \/>\s*\)\}/g,
    `<div className="flex items-center justify-center transition-transform duration-200 group-active:scale-95 group-hover:scale-110">
                    <Headphones weight="bold" size={24} />
                    <div className={\`absolute w-[30px] h-[3px] bg-danger rounded-full transition-all duration-300 origin-center \${ (store.currentUser?.isDeafened || store.currentUser?.isServerDeafened) ? 'scale-100 opacity-100 rotate-45' : 'scale-0 opacity-0 rotate-45' }\`} />
                  </div>`
);

// Add 'group' class to the muting/deafening buttons so group-hover works
code = code.replace(
    /className=\{\`w-14 h-14 rounded-full flex items-center justify-center relative transition-colors/g,
    `className={\`group w-14 h-14 rounded-full flex items-center justify-center relative transition-colors`
);

fs.writeFileSync('App.tsx', code);
