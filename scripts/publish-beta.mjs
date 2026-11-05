import { runRelease } from './release.mjs';

runRelease('beta', process.argv.slice(2)).catch(error => { console.error(error.message); process.exitCode = 1; });
