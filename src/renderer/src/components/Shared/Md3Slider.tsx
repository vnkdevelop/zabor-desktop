import React, { useState, useEffect, useRef } from 'react';

export function Md3Slider({
  min,
  max,
  value,
  step = 1,
  onChange,
  onChangeEnd,
  label,
  showPercentage = false,
  className = ''
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
}) {
  const [localValue, setLocalValue] = useState(value);
  const lastPropsValue = useRef(value);

  // Sync internal state if prop value changes from outside
  useEffect(() => {
    if (value !== lastPropsValue.current) {
      setLocalValue(value);
      lastPropsValue.current = value;
    }
  }, [value]);

  const pct = ((localValue - min) / (max - min)) * 100;

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const val = Number(e.target.value);
    setLocalValue(val);
    onChange(val);
  };

  const handleDragEnd = () => {
    if (onChangeEnd) {
      onChangeEnd(localValue);
    }
  };

  return (
    <div className={className}>
      {label && (
        <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">
          {label}{showPercentage ? ` — ${localValue}%` : ''}
        </label>
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
  );
}
