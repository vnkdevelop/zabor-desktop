import React, { useState, useRef, useEffect } from 'react';
import { CaretDown, Check } from '@phosphor-icons/react';
import { animate, play, stagger, utils, EASE, springs, prefersReducedMotion } from '../../lib/anime';
import { usePresence } from '../../hooks/usePresence';

export interface GlassSelectOption {
  value: string;
  label: string;
}

export interface GlassSelectProps {
  value: string;
  onChange: (value: string) => void;
  options: GlassSelectOption[];
  disabled?: boolean;
  className?: string;
  compact?: boolean;
  placeholder?: string;
}

export function GlassSelect({
  value,
  onChange,
  options,
  disabled = false,
  className = '',
  compact = false,
  placeholder = ''
}: GlassSelectProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [direction, setDirection] = useState<'down' | 'up'>('down');
  const [highlightedIndex, setHighlightedIndex] = useState(-1);

  const containerRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const popupRef = useRef<HTMLDivElement | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const caretRef = useRef<HTMLSpanElement>(null);

  const selectedOption = options.find((opt) => opt.value === value);
  const displayLabel = selectedOption?.label ?? placeholder ?? value;

  const { mounted, ref: presenceRef } = usePresence(isOpen, {
    onEnter: (el) => {
      play(el, {
        opacity: [0, 1],
        translateY: [direction === 'up' ? 6 : -6, 0],
        scale: [0.98, 1],
        duration: 220,
        ease: EASE
      });
      const rows = Array.from(el.querySelectorAll<HTMLElement>('[data-select-item]'));
      if (rows.length) {
        play(rows, {
          opacity: [0, 1],
          translateY: [direction === 'up' ? 4 : -4, 0],
          duration: 180,
          ease: EASE,
          delay: stagger(18)
        });
      }
      const check = el.querySelector<HTMLElement>('[data-select-check]');
      if (check) play(check, { scale: [0, 1], ease: springs.pop });
    },
    onExit: (el) =>
      play(el, {
        opacity: [1, 0],
        translateY: [0, direction === 'up' ? 6 : -6],
        scale: [1, 0.98],
        duration: 140,
        ease: EASE
      })
  });

  const pressTo = (scale: number) => {
    const el = triggerRef.current;
    if (!el || prefersReducedMotion()) return;
    animate(el, { scale, ease: springs.press });
  };

  useEffect(() => {
    const el = caretRef.current;
    if (!el) return;
    if (prefersReducedMotion()) {
      utils.set(el, { rotate: isOpen ? 180 : 0 });
      return;
    }
    animate(el, { rotate: isOpen ? 180 : 0, ease: springs.knob });
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen) return;

    if (triggerRef.current) {
      const rect = triggerRef.current.getBoundingClientRect();
      const spaceBelow = window.innerHeight - rect.bottom;
      if (spaceBelow < 220 && rect.top > spaceBelow) {
        setDirection('up');
      } else {
        setDirection('down');
      }
    }

    const selectedIdx = options.findIndex((opt) => opt.value === value);
    setHighlightedIndex(selectedIdx >= 0 ? selectedIdx : 0);

    const SAFE_MARGIN = 32;

    const handleMouseMove = (e: MouseEvent) => {
      const trigger = triggerRef.current;
      const popup = popupRef.current;
      if (!trigger && !popup) return;

      const triggerRect = trigger ? trigger.getBoundingClientRect() : null;
      const popupRect = popup ? popup.getBoundingClientRect() : null;

      let minX = Infinity;
      let maxX = -Infinity;
      let minY = Infinity;
      let maxY = -Infinity;

      if (triggerRect && triggerRect.width > 0) {
        minX = Math.min(minX, triggerRect.left);
        maxX = Math.max(maxX, triggerRect.right);
        minY = Math.min(minY, triggerRect.top);
        maxY = Math.max(maxY, triggerRect.bottom);
      }

      if (popupRect && popupRect.width > 0) {
        minX = Math.min(minX, popupRect.left);
        maxX = Math.max(maxX, popupRect.right);
        minY = Math.min(minY, popupRect.top);
        maxY = Math.max(maxY, popupRect.bottom);
      }

      if (minX === Infinity) return;

      const isInsideOrNear =
        e.clientX >= minX - SAFE_MARGIN &&
        e.clientX <= maxX + SAFE_MARGIN &&
        e.clientY >= minY - SAFE_MARGIN &&
        e.clientY <= maxY + SAFE_MARGIN;

      if (!isInsideOrNear) {
        setIsOpen(false);
      }
    };

    const handleClickOutside = (event: MouseEvent | TouchEvent) => {
      if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    };

    const handleWindowKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setIsOpen(false);
        triggerRef.current?.focus();
      }
    };

    const handleWheel = (e: WheelEvent) => {
      if (listRef.current && listRef.current.contains(e.target as Node)) {
        return;
      }
      setIsOpen(false);
    };

    const handleScroll = (e: Event) => {
      if (listRef.current && listRef.current.contains(e.target as Node)) {
        return;
      }
      setIsOpen(false);
    };

    const handleWindowBlur = () => {
      setIsOpen(false);
    };

    const handleMouseLeaveDoc = (e: MouseEvent) => {
      if (!e.relatedTarget && !(e as any).toElement) {
        setIsOpen(false);
      }
    };

    document.addEventListener('mousedown', handleClickOutside);
    document.addEventListener('touchstart', handleClickOutside);
    document.addEventListener('keydown', handleWindowKeyDown);
    window.addEventListener('mousemove', handleMouseMove);
    window.addEventListener('wheel', handleWheel, { passive: true });
    window.addEventListener('scroll', handleScroll, { capture: true, passive: true });
    window.addEventListener('blur', handleWindowBlur);
    document.addEventListener('mouseleave', handleMouseLeaveDoc);

    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
      document.removeEventListener('touchstart', handleClickOutside);
      document.removeEventListener('keydown', handleWindowKeyDown);
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('wheel', handleWheel);
      window.removeEventListener('scroll', handleScroll, { capture: true });
      window.removeEventListener('blur', handleWindowBlur);
      document.removeEventListener('mouseleave', handleMouseLeaveDoc);
    };
  }, [isOpen, value, options]);

  useEffect(() => {
    if (isOpen && listRef.current && highlightedIndex >= 0) {
      const items = listRef.current.querySelectorAll<HTMLButtonElement>('[data-select-item]');
      items[highlightedIndex]?.scrollIntoView({ block: 'nearest' });
    }
  }, [highlightedIndex, isOpen]);

  const handleTriggerKeyDown = (e: React.KeyboardEvent<HTMLButtonElement>) => {
    if (disabled) return;

    if (!isOpen) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp' || e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        setIsOpen(true);
      }
      return;
    }

    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setHighlightedIndex((prev) => (options.length === 0 ? -1 : (prev + 1) % options.length));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setHighlightedIndex((prev) => (options.length === 0 ? -1 : (prev - 1 + options.length) % options.length));
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      if (highlightedIndex >= 0 && highlightedIndex < options.length) {
        onChange(options[highlightedIndex].value);
        setIsOpen(false);
      }
    } else if (e.key === 'Tab') {
      setIsOpen(false);
    }
  };

  return (
    <div
      ref={containerRef}
      className={`relative ${compact ? 'w-auto shrink-0 min-w-[130px]' : 'w-full'} ${
        mounted ? 'z-30' : 'z-0'
      } ${className}`}
    >
      <button
        ref={triggerRef}
        type="button"
        disabled={disabled}
        onClick={() => !disabled && setIsOpen((prev) => !prev)}
        onKeyDown={handleTriggerKeyDown}
        onPointerDown={() => !disabled && pressTo(0.97)}
        onPointerUp={() => !disabled && pressTo(1)}
        onPointerLeave={() => !disabled && pressTo(1)}
        aria-haspopup="listbox"
        aria-expanded={isOpen}
        className={`group relative flex items-center justify-between text-left transition-colors duration-150 outline-none select-none ${
          compact
            ? 'w-full glass-field text-white rounded-xl px-3 py-2 text-sm font-semibold'
            : 'w-full glass-field text-white rounded-xl px-3.5 py-3 text-[14px]'
        } ${isOpen ? 'ring-2 ring-primary bg-white/[0.06]' : 'hover:bg-white/[0.06] focus:ring-2 focus:ring-primary'} ${
          disabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'
        }`}
      >
        <span className="truncate pr-2 lowercase font-medium">{displayLabel}</span>
        <span ref={caretRef} className="ml-2 shrink-0 inline-flex" style={{ willChange: 'transform' }}>
          <CaretDown
            weight="bold"
            size={compact ? 13 : 15}
            className={`transition-colors duration-200 ${
              isOpen ? 'text-primary' : 'text-textMuted group-hover:text-white'
            }`}
          />
        </span>
      </button>

      {mounted && (
        <div
          ref={(el) => {
            popupRef.current = el;
            presenceRef.current = el;
          }}
          className={`absolute z-50 ${
            compact ? 'min-w-[140px] right-0' : 'left-0 right-0'
          } ${
            direction === 'up' ? 'bottom-full mb-1.5' : 'top-full mt-1.5'
          } glass-sheet rounded-xl p-1.5 shadow-none overflow-hidden opacity-0`}
          style={{
            transformOrigin: direction === 'up' ? 'bottom center' : 'top center',
            willChange: 'transform, opacity'
          }}
        >
          <div
            ref={listRef}
            role="listbox"
            tabIndex={-1}
            className="max-h-56 overflow-y-auto space-y-0.5"
            style={{ scrollbarGutter: 'stable' }}
          >
            {options.map((opt, index) => {
              const isSelected = opt.value === value;
              const isHighlighted = highlightedIndex === index;

              return (
                <button
                  type="button"
                  key={opt.value}
                  data-select-item
                  role="option"
                  aria-selected={isSelected}
                  onClick={(e) => {
                    e.stopPropagation();
                    onChange(opt.value);
                    setIsOpen(false);
                    triggerRef.current?.focus();
                  }}
                  onMouseEnter={() => setHighlightedIndex(index)}
                  className={`w-full text-left px-3 py-2 rounded-lg flex items-center justify-between text-sm transition-colors duration-100 cursor-pointer select-none ${
                    isSelected
                      ? 'bg-primary/20 text-white font-semibold'
                      : isHighlighted
                        ? 'bg-surfaceHover/80 text-white font-medium'
                        : 'text-white/80 hover:bg-surfaceHover/80 hover:text-white font-medium'
                  }`}
                >
                  <span className="truncate pr-2 lowercase">{opt.label}</span>
                  {isSelected && (
                    <span data-select-check className="shrink-0 ml-2 inline-flex" style={{ willChange: 'transform' }}>
                      <Check weight="bold" size={14} className="text-primary" />
                    </span>
                  )}
                </button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
