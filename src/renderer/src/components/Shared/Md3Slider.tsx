import React, { useState, useEffect, useLayoutEffect, useRef } from 'react';
import { SpeakerHigh, SpeakerSlash } from '@phosphor-icons/react';
import { animate, utils, springs, EASE_OUT, DUR, prefersReducedMotion } from '../../lib/anime';

export function Md3Slider({
  min,
  max,
  value,
  step = 1,
  onChange,
  onChangeEnd,
  label,
  showPercentage = false,
  className = '',
  showMuteButton = false
}: {
  min: number;
  max: number;
  value: number;
  step?: number;
  onChange: (v: number) => void;
  onChangeEnd?: (v: number) => void;
  label?: string;
  showPercentage?: boolean;
  className?: string;
  showMuteButton?: boolean;
}) {
  const [localValue, setLocalValue] = useState(value);
  const lastPropsValue = useRef(value);
  const lastNonZeroValue = useRef(value > 0 ? value : 100);
  const trackRootRef = useRef<HTMLDivElement>(null);
  const thumbRef = useRef<HTMLDivElement>(null);
  const haloRef = useRef<HTMLDivElement>(null);
  const iconHighRef = useRef<HTMLSpanElement>(null);
  const iconSlashRef = useRef<HTMLSpanElement>(null);
  const lastPctRef = useRef(0);
  const interactingRef = useRef(false);
  const releaseRef = useRef<() => void>(() => {});
  const firstMuteRun = useRef(true);

  useEffect(() => {
    if (value !== lastPropsValue.current) {
      setLocalValue(value);
      lastPropsValue.current = value;
      if (value > 0) {
        lastNonZeroValue.current = value;
      }
    }
  }, [value]);

  const toPct = (v: number) => ((v - min) / (max - min)) * 100;
  const isMuted = localValue === 0;

  useLayoutEffect(() => {
    const p = toPct(value);
    lastPctRef.current = p;
    if (trackRootRef.current) utils.set(trackRootRef.current, { '--slider-pct': p + '%' });
    if (thumbRef.current) utils.set(thumbRef.current, { scale: 1 });
    if (haloRef.current) utils.set(haloRef.current, { scale: 1, opacity: 0 });
    if (iconHighRef.current) utils.set(iconHighRef.current, { opacity: isMuted ? 0 : 1, scale: isMuted ? 0.6 : 1 });
    if (iconSlashRef.current) utils.set(iconSlashRef.current, { opacity: isMuted ? 1 : 0, scale: isMuted ? 1 : 0.6 });
  }, []);

  useEffect(() => {
    const to = toPct(value);
    const from = lastPctRef.current;
    if (from === to) return;
    lastPctRef.current = to;
    if (interactingRef.current || prefersReducedMotion()) {
      if (trackRootRef.current) utils.set(trackRootRef.current, { '--slider-pct': to + '%' });
    } else if (trackRootRef.current) {
      animate(trackRootRef.current, { '--slider-pct': [from + '%', to + '%'], ease: springs.slider });
    }
  }, [value, min, max]);

  useEffect(() => {
    if (firstMuteRun.current) {
      firstMuteRun.current = false;
      return;
    }
    const toHigh = { opacity: isMuted ? 0 : 1, scale: isMuted ? 0.6 : 1 };
    const toSlash = { opacity: isMuted ? 1 : 0, scale: isMuted ? 1 : 0.6 };
    if (prefersReducedMotion()) {
      if (iconHighRef.current) utils.set(iconHighRef.current, toHigh);
      if (iconSlashRef.current) utils.set(iconSlashRef.current, toSlash);
    } else {
      if (iconHighRef.current) animate(iconHighRef.current, { ...toHigh, duration: DUR.micro, ease: EASE_OUT });
      if (iconSlashRef.current) animate(iconSlashRef.current, { ...toSlash, duration: DUR.micro, ease: EASE_OUT });
    }
  }, [isMuted]);

  const grab = () => {
    interactingRef.current = true;
    if (prefersReducedMotion()) {
      if (thumbRef.current) utils.set(thumbRef.current, { scale: 1.25 });
      if (haloRef.current) utils.set(haloRef.current, { scale: 2.4, opacity: 0.16 });
      return;
    }
    if (thumbRef.current) animate(thumbRef.current, { scale: 1.25, ease: springs.press });
    if (haloRef.current) animate(haloRef.current, { scale: 2.4, opacity: 0.16, duration: DUR.micro, ease: EASE_OUT });
  };

  const release = () => {
    interactingRef.current = false;
    if (prefersReducedMotion()) {
      if (thumbRef.current) utils.set(thumbRef.current, { scale: 1 });
      if (haloRef.current) utils.set(haloRef.current, { scale: 1, opacity: 0 });
      return;
    }
    if (thumbRef.current) animate(thumbRef.current, { scale: 1, ease: springs.press });
    if (haloRef.current) animate(haloRef.current, { scale: 1, opacity: 0, duration: DUR.micro, ease: EASE_OUT });
  };

  useEffect(() => {
    releaseRef.current = release;
  });

  useEffect(() => {
    const handleUp = () => {
      if (interactingRef.current) releaseRef.current();
    };
    window.addEventListener('pointerup', handleUp);
    window.addEventListener('pointercancel', handleUp);
    return () => {
      window.removeEventListener('pointerup', handleUp);
      window.removeEventListener('pointercancel', handleUp);
    };
  }, []);

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const val = Number(e.target.value);
    setLocalValue(val);
    if (val > 0) {
      lastNonZeroValue.current = val;
    }
    const p = toPct(val);
    lastPctRef.current = p;
    if (trackRootRef.current) utils.set(trackRootRef.current, { '--slider-pct': p + '%' });
    onChange(val);
  };

  const handleDragEnd = () => {
    if (onChangeEnd) {
      onChangeEnd(localValue);
    }
  };

  const handleMuteClick = () => {
    if (localValue > 0) {
      lastNonZeroValue.current = localValue;
      setLocalValue(0);
      onChange(0);
      if (onChangeEnd) onChangeEnd(0);
    } else {
      const restoreVal = lastNonZeroValue.current;
      setLocalValue(restoreVal);
      onChange(restoreVal);
      if (onChangeEnd) onChangeEnd(restoreVal);
    }
  };

  return (
    <div className={className}>
      {label && (
        <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">
          {label}{showPercentage ? ` — ${localValue}%` : ''}
        </label>
      )}
      <div className="flex items-center gap-4">
        {showMuteButton && (
          <button
            onClick={handleMuteClick}
            className="relative text-textMuted hover:text-white transition-all duration-200 flex items-center justify-center w-10 h-10 rounded-xl bg-surface/50"
          >
            <span ref={iconHighRef} className="absolute inset-0 flex items-center justify-center">
              <SpeakerHigh weight="bold" size={20} className="text-white" />
            </span>
            <span ref={iconSlashRef} className="absolute inset-0 flex items-center justify-center">
              <SpeakerSlash weight="bold" size={20} className="text-danger" />
            </span>
          </button>
        )}
        <div
          ref={trackRootRef}
          className="relative w-full h-10"
        >
          <div
            className="absolute left-0 right-0 rounded-full"
            style={{
              top: '50%',
              height: 4,
              marginTop: -2,
              background:
                'linear-gradient(to right, rgb(var(--z-primary)) var(--slider-pct, 0%), rgba(255,255,255,0.08) var(--slider-pct, 0%))',
              pointerEvents: 'none'
            }}
          />
          <div
            className="absolute w-5 h-5"
            style={{ left: 'var(--slider-pct, 0%)', top: '50%', marginLeft: -10, marginTop: -10, pointerEvents: 'none' }}
          >
            <div
              ref={haloRef}
              className="absolute inset-0 rounded-full"
              style={{ background: 'rgb(var(--z-primary))', opacity: 0, pointerEvents: 'none', willChange: 'transform, opacity' }}
            />
            <div
              ref={thumbRef}
              className="absolute inset-0 rounded-full"
              style={{ background: 'rgb(var(--z-primary))', pointerEvents: 'none', willChange: 'transform' }}
            />
          </div>
          <input
            type="range"
            min={min}
            max={max}
            step={step}
            value={localValue}
            onChange={handleChange}
            onMouseUp={handleDragEnd}
            onTouchEnd={handleDragEnd}
            onPointerDown={grab}
            aria-label={label || undefined}
            className="absolute inset-0 w-full h-full opacity-0 cursor-pointer z-10 m-0"
          />
        </div>
      </div>
    </div>
  );
}
