const mod = require('./index.js')

const report = {
  available: mod.available(),
  loadError: mod.loadError,
  probe: mod.probe()
}

console.log(JSON.stringify(report, null, 2))

const p = report.probe
if (p && p.ok) {
  const c = p.capture || {}
  const r = p.render || {}
  console.log('')
  console.log(`capture: default ${Number(c.defaultPeriodMs || 0).toFixed(2)}ms -> min ${Number(c.minPeriodMs || 0).toFixed(2)}ms @ ${c.sampleRate || '?'}Hz`)
  console.log(`render:  default ${Number(r.defaultPeriodMs || 0).toFixed(2)}ms -> min ${Number(r.minPeriodMs || 0).toFixed(2)}ms @ ${r.sampleRate || '?'}Hz`)
}
