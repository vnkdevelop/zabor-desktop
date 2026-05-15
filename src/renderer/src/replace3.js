const fs = require('fs');
let code = fs.readFileSync('App.tsx', 'utf8');

code = code.replace(
    /className="w-full text-left px-4 py-2 text-danger hover:bg-surfaceHover flex items-center gap-3 font-medium mt-1">\s*<LeaveIcon weight="bold" size=\{16\} \/> Выйти из канала/g,
    `className="group w-full text-left px-4 py-2 text-danger hover:bg-surfaceHover flex items-center gap-3 font-medium mt-1">
                <div className="transition-transform duration-200 group-hover:translate-x-1">
                  <LeaveIcon weight="bold" size={16} />
                </div>
                Выйти из канала`
);

fs.writeFileSync('App.tsx', code);
