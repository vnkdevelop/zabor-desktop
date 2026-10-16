import { useLayoutEffect, useRef } from 'react'
import { animate, utils, springs, EASE, EASE_OUT, EASE_IN, prefersReducedMotion } from '../../lib/anime'

const TRACK_FILL_ON = 'rgba(200, 30, 112, 0.9)'
const TRACK_FILL_OFF = 'rgba(200, 30, 112, 0)'
const TRACK_BORDER_ON = 'rgb(200, 30, 112)'
const TRACK_BORDER_OFF = 'rgb(121, 116, 126)'
const KNOB_FILL_ON = 'rgb(255, 255, 255)'
const KNOB_FILL_OFF = 'rgb(121, 116, 126)'

const KNOB_ON_X = 20
const KNOB_OFF_X = 0
const KNOB_ON_SCALE = 1
const KNOB_OFF_SCALE = 0.66
const TICK_LENGTH = 24

const TRACK_TWEEN_MS = 200
const KNOB_COLOR_MS = 200
const TICK_DRAW_MS = 180
const TICK_HIDE_MS = 100
const PRESS_DOWN_MS = 130

function snapTo(track: HTMLElement, knob: HTMLElement, tick: SVGPathElement, on: boolean): void {
  utils.set(track, {
    backgroundColor: on ? TRACK_FILL_ON : TRACK_FILL_OFF,
    borderColor: on ? TRACK_BORDER_ON : TRACK_BORDER_OFF,
    scale: 1,
  })
  utils.set(knob, {
    translateX: on ? KNOB_ON_X : KNOB_OFF_X,
    scale: on ? KNOB_ON_SCALE : KNOB_OFF_SCALE,
    backgroundColor: on ? KNOB_FILL_ON : KNOB_FILL_OFF,
  })
  utils.set(tick, {
    strokeDashoffset: on ? 0 : TICK_LENGTH,
    opacity: on ? 1 : 0,
    scale: on ? 1 : 0,
  })
}

export function Md3Switch({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void; }) {
  const trackRef = useRef<HTMLButtonElement>(null)
  const knobRef = useRef<HTMLSpanElement>(null)
  const tickRef = useRef<SVGPathElement>(null)
  const prevRef = useRef<boolean | null>(null)
  const pressedRef = useRef(false)

  useLayoutEffect(() => {
    const track = trackRef.current
    const knob = knobRef.current
    const tick = tickRef.current
    if (!track || !knob || !tick) return

    const prev = prevRef.current
    prevRef.current = checked
    if (prev === null || prev === checked) {
      snapTo(track, knob, tick, checked)
      return
    }

    if (prefersReducedMotion()) {
      snapTo(track, knob, tick, checked)
      return
    }

    if (checked) {
      animate(track, {
        backgroundColor: TRACK_FILL_ON,
        borderColor: TRACK_BORDER_ON,
        duration: TRACK_TWEEN_MS,
        ease: EASE,
      })
      animate(knob, {
        backgroundColor: KNOB_FILL_ON,
        duration: KNOB_COLOR_MS,
        ease: EASE,
      })
      animate(knob, {
        translateX: KNOB_ON_X,
        scale: KNOB_ON_SCALE,
        ease: springs.knob,
      })
      animate(tick, {
        strokeDashoffset: [TICK_LENGTH, 0],
        opacity: [0, 1],
        scale: [0.6, 1],
        duration: TICK_DRAW_MS,
        delay: 50,
        ease: EASE_OUT,
      })
    } else {
      animate(track, {
        backgroundColor: TRACK_FILL_OFF,
        borderColor: TRACK_BORDER_OFF,
        duration: TRACK_TWEEN_MS,
        ease: EASE,
      })
      animate(tick, {
        opacity: 0,
        scale: 0,
        duration: TICK_HIDE_MS,
        ease: EASE_IN,
      })
      animate(knob, {
        backgroundColor: KNOB_FILL_OFF,
        duration: KNOB_COLOR_MS,
        ease: EASE,
      })
      animate(knob, {
        translateX: KNOB_OFF_X,
        scale: KNOB_OFF_SCALE,
        ease: springs.knob,
      })
    }
  }, [checked])

  const pressDown = () => {
    pressedRef.current = true
    const track = trackRef.current
    if (!track || prefersReducedMotion()) return
    animate(track, { scale: 0.94, duration: PRESS_DOWN_MS, ease: EASE_OUT })
  }

  const pressUp = () => {
    if (!pressedRef.current) return
    pressedRef.current = false
    const track = trackRef.current
    if (!track) return
    if (prefersReducedMotion()) {
      utils.set(track, { scale: 1 })
      return
    }
    animate(track, { scale: 1, ease: springs.press })
  }

  return (
    <button
      ref={trackRef}
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      onPointerDown={pressDown}
      onPointerUp={pressUp}
      onPointerLeave={pressUp}
      onPointerCancel={pressUp}
      style={{ willChange: 'transform' }}
      className="relative w-[52px] h-[32px] rounded-full shrink-0 border-2"
    >
      <span
        ref={knobRef}
        style={{ willChange: 'transform' }}
        className="absolute top-[2px] left-[2px] w-6 h-6 rounded-full flex items-center justify-center"
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none">
          <path
            ref={tickRef}
            d="M20 6 L9 17 L4 12"
            stroke="#C81E70"
            strokeWidth={3.5}
            strokeLinecap="round"
            strokeLinejoin="round"
            style={{ strokeDasharray: `${TICK_LENGTH}`, transformBox: 'fill-box', transformOrigin: 'center' }}
          />
        </svg>
      </span>
    </button>
  );
}
