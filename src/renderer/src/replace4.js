const fs = require('fs');
let code = fs.readFileSync('App.tsx', 'utf8');

code = code.replace(
    /className="w-full bg-danger hover:bg-red-600 text-white py-3 rounded-xl font-bold flex items-center justify-center gap-2 transition-colors">\s*<LogOut weight="bold" size=\{18\} \/>\s*\{t\('settings\.privacy\.logout'\)\}\s*<\/button>/g,
    `className="group w-full bg-danger hover:bg-red-600 text-white py-3 rounded-xl font-bold flex items-center justify-center gap-2 transition-colors">
                  <div className="transition-transform duration-200 group-hover:-translate-x-1">
                    <LogOut weight="bold" size={18} />
                  </div>
                  {t('settings.privacy.logout')}
                </button>`
);

fs.writeFileSync('App.tsx', code);
