import {
  animate as animeAnimate,
  createTimeline as animeCreateTimeline,
  spring,
  stagger,
  utils as animeUtils,
  cubicBezier
} from 'animejs'

const createSpring = spring

function resolveTargets(targets: unknown): any {
  if (targets == null) return targets
  if (Array.isArray(targets)) {
    return targets.map(resolveTargets)
  }
  if (typeof targets === 'object' && 'current' in (targets as object)) {
    return (targets as { current: unknown }).current
  }
  return targets
}

function hasValidTargets(targets: unknown): boolean {
  if (targets == null) return false
  if (typeof targets === 'string') {
    try {
      return typeof document !== 'undefined' && document.querySelectorAll(targets).length > 0
    } catch {
      return false
    }
  }
  if (Array.isArray(targets)) {
    return targets.length > 0 && targets.some(hasValidTargets)
  }
  if (typeof NodeList !== 'undefined' && targets instanceof NodeList) {
    return targets.length > 0
  }
  if (typeof HTMLCollection !== 'undefined' && targets instanceof HTMLCollection) {
    return targets.length > 0
  }
  if (typeof targets === 'object' && 'current' in (targets as object)) {
    return hasValidTargets((targets as any).current)
  }
  return typeof targets === 'object'
}

function sanitizeTargets(targets: unknown): any {
  const resolved = resolveTargets(targets)
  if (Array.isArray(resolved)) {
    const valid = resolved.filter(hasValidTargets)
    return valid.length === 1 ? valid[0] : valid
  }
  return resolved
}

function createCompletedAnimation(parameters?: any): any {
  if (parameters && typeof parameters.onComplete === 'function') {
    try { parameters.onComplete() } catch {}
  }
  const promise = Promise.resolve()
  const dummy: any = {
    pause: () => dummy,
    play: () => dummy,
    restart: () => dummy,
    revert: () => dummy,
    resume: () => dummy,
    cancel: () => dummy,
    seek: () => dummy,
    complete: () => dummy,
    alternate: () => dummy,
    reset: () => dummy,
    stretch: () => dummy,
    refresh: () => dummy,
    progress: 1,
    currentTime: 0,
    iterationProgress: 1,
    currentIteration: 0,
    speed: 1,
    duration: 0,
    iterationDuration: 0,
    began: true,
    paused: true,
    completed: true,
    cancelled: false,
    targets: [],
    then: (onfulfilled?: any, onrejected?: any) => promise.then(onfulfilled, onrejected),
    catch: (onrejected?: any) => promise.catch(onrejected),
    finally: (onfinally?: any) => promise.finally(onfinally)
  }
  return dummy
}

const animate: typeof animeAnimate = (targets: any, parameters?: any) => {
  const sanitized = sanitizeTargets(targets)
  if (!hasValidTargets(sanitized)) {
    return createCompletedAnimation(parameters)
  }
  return animeAnimate(sanitized, parameters)
}

const createTimeline: typeof animeCreateTimeline = (parameters?: any) => {
  const tl = animeCreateTimeline(parameters)
  const origAdd = tl.add.bind(tl)
  tl.add = (a1: any, a2?: any, a3?: any) => {
    if (a2 && typeof a2 === 'object') {
      const sanitized = sanitizeTargets(a1)
      if (!hasValidTargets(sanitized)) {
        return tl
      }
      return origAdd(sanitized, a2, a3)
    }
    return origAdd(a1, a2, a3)
  }
  return tl
}

const utils = {
  ...animeUtils,
  set: (targets: any, parameters: any) => {
    const sanitized = sanitizeTargets(targets)
    if (!hasValidTargets(sanitized)) return
    return animeUtils.set(sanitized, parameters)
  },
  remove: (targets: any) => {
    const sanitized = sanitizeTargets(targets)
    if (!hasValidTargets(sanitized)) return
    return animeUtils.remove(sanitized)
  },
  cleanInlineStyles: (targets: any) => {
    const sanitized = sanitizeTargets(targets)
    if (!hasValidTargets(sanitized)) return
    return animeUtils.cleanInlineStyles(sanitized)
  }
}

const set = utils.set
const remove = utils.remove

export { animate, createTimeline, spring, createSpring, stagger, utils, cubicBezier, set, remove }

export const EASE = cubicBezier(0.16, 1, 0.3, 1)
export const EASE_OUT = 'outExpo'
export const EASE_IN = 'inQuad'
export const DUR = { micro: 150, base: 240, slow: 320, enter: 440 }

export const springs = {
  tab: spring({ stiffness: 220, damping: 18, mass: 1 }),
  squash: spring({ stiffness: 150, damping: 15, mass: 1 }),
  knob: spring({ stiffness: 90, damping: 12, mass: 1 }),
  press: spring({ stiffness: 300, damping: 14, mass: 1 }),
  pop: spring({ stiffness: 120, damping: 9, mass: 1 }),
  slider: spring({ stiffness: 70, damping: 9, mass: 1 }),
  pip: spring({ stiffness: 260, damping: 22, mass: 1 }),
  card: spring({ stiffness: 80, damping: 11, mass: 1 }),
  dock: spring({ stiffness: 90, damping: 12, mass: 1 }),
}

export function prefersReducedMotion(): boolean {
  return typeof window !== 'undefined'
    && typeof window.matchMedia === 'function'
    && window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

type Props = Record<string, any>

export function play(target: unknown, params: Props): Promise<void> {
  return new Promise((resolve) => {
    const sanitized = sanitizeTargets(target)
    if (!hasValidTargets(sanitized)) { resolve(); return }
    let settled = false
    const finish = () => { if (!settled) { settled = true; resolve() } }
    if (prefersReducedMotion()) {
      const final: Props = {}
      for (const k in params) {
        const v = (params as Record<string, any>)[k]
        if (Array.isArray(v)) final[k] = v[v.length - 1]
        else if (k !== 'duration' && k !== 'ease' && k !== 'delay') final[k] = v
      }
      utils.set(sanitized as object, final)
      finish()
      return
    }
    animate(sanitized as object, { ...params, onComplete: finish })
    const duration = typeof params.duration === 'number' ? params.duration : DUR.base
    const delay = typeof params.delay === 'number' ? params.delay : 0
    setTimeout(finish, duration + delay + 140)
  })
}

export function enter(target: unknown, opts: { y?: number; scale?: number; duration?: number; ease?: unknown; delay?: number } = {}): Promise<void> {
  const { y = 10, scale = 0.98, duration = DUR.slow, ease = EASE, delay = 0 } = opts
  return play(target, { opacity: [0, 1], translateY: [y, 0], scale: [scale, 1], duration, ease, delay })
}

export function exit(target: unknown, opts: { y?: number; scale?: number; duration?: number } = {}): Promise<void> {
  const { y = 8, scale = 0.97, duration = DUR.base } = opts
  return play(target, { opacity: [1, 0], translateY: [0, y], scale: [1, scale], duration, ease: EASE_IN })
}
