import React, { useState, useEffect, useRef } from 'react';
import { SpeakerHigh, SpeakerSlash } from '@phosphor-icons/react';

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

  useEffect(() => {
    if (value !== lastPropsValue.current) {
      setLocalValue(value);
      lastPropsValue.current = value;
      if (value > 0) {
        lastNonZeroValue.current = value;
      }
    }
  }, [value]);

  const pct = ((localValue - min) / (max - min)) * 100;

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const val = Number(e.target.value);
    setLocalValue(val);
    if (val > 0) {
      lastNonZeroValue.current = val;
    }
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
            className="text-textMuted hover:text-white transition-all duration-200 focus:outline-none flex items-center justify-center w-10 h-10 rounded-xl bg-surface/50 border border-[#303035]"
          >
            {localValue === 0 ? (
              <SpeakerSlash weight="bold" size={20} className="text-danger" />
            ) : (
              <SpeakerHigh weight="bold" size={20} className="text-white" />
            )}
          </button>
        )}
        <input
          type="range"
          min={min}
          max={max}
          step={step}
          value={localValue}
          onChange={handleChange}
          onMouseUp={handleDragEnd}
          onTouchEnd={handleDragEnd}
          className="md3-range w-full"
          style={{ '--slider-pct': `${pct}%` } as React.CSSProperties}
        />
      </div>
    </div>
  );
}
