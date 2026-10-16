import { useState, useEffect, useRef, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { animate, utils, springs, EASE_OUT, DUR, prefersReducedMotion } from '../../lib/anime';

type TitleBarButtonProps = {
  label: string;
  danger?: boolean;
  onClick: () => void;
  children: ReactNode;
};

function TitleBarButton({ label, danger = false, onClick, children }: TitleBarButtonProps) {
  const iconRef = useRef<HTMLSpanElement>(null);
  const pressedRef = useRef(false);

  const pressDown = () => {
    pressedRef.current = true;
    const el = iconRef.current;
    if (!el || prefersReducedMotion()) return;
    animate(el, { scale: 0.82, duration: DUR.micro, ease: EASE_OUT });
  };

  const pressUp = () => {
    if (!pressedRef.current) return;
    pressedRef.current = false;
    const el = iconRef.current;
    if (!el) return;
    if (prefersReducedMotion()) {
      utils.set(el, { scale: 1 });
      return;
    }
    animate(el, { scale: 1, ease: springs.press });
  };

  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      onPointerDown={pressDown}
      onPointerUp={pressUp}
      onPointerLeave={pressUp}
      onPointerCancel={pressUp}
      className={`h-full w-12 flex items-center justify-center text-white/40 hover:text-white transition-colors ${danger ? 'hover:bg-primary/90' : 'hover:bg-white/[0.06]'}`}
    >
      <span ref={iconRef} style={{ willChange: 'transform', display: 'inline-flex' }}>
        {children}
      </span>
    </button>
  );
}

export function TitleBar() {
  const { t } = useTranslation();
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    const cleanup = window.windowControls?.onMaximizeChange(setIsMaximized);
    return () => cleanup?.();
  }, []);

  return (
    <div className="title-bar h-9 bg-[#1a1a1a]/90 backdrop-blur-xl flex items-center justify-between shrink-0 relative z-[2000000] border-b border-white/[0.06]">
      <div className="flex items-center pl-4 gap-2 pointer-events-none">
        <span className="text-[13px] font-black tracking-[0.2em] text-white/30">{t('titlebar.wordmark')}</span>
      </div>
      <div className="flex items-center h-full title-no-drag">
        <TitleBarButton label={t('titlebar.minimize')} onClick={() => window.windowControls?.minimize()}>
          <svg width="12" height="1" viewBox="0 0 12 1" fill="currentColor"><rect width="12" height="1" rx="0.5" /></svg>
        </TitleBarButton>
        <TitleBarButton label={isMaximized ? t('titlebar.restore') : t('titlebar.maximize')} onClick={() => window.windowControls?.maximize()}>
          {isMaximized ? (
            <svg width="11" height="11" viewBox="0 0 11 11" fill="none" stroke="currentColor" strokeWidth="1"><rect x="3" y="0.5" width="7.5" height="7.5" rx="1" /><rect x="0.5" y="3" width="7.5" height="7.5" rx="1" /></svg>
          ) : (
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1"><rect x="0.5" y="0.5" width="9" height="9" rx="1.5" /></svg>
          )}
        </TitleBarButton>
        <TitleBarButton label={t('titlebar.close')} danger onClick={() => window.windowControls?.close()}>
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round"><line x1="1" y1="1" x2="11" y2="11" /><line x1="11" y1="1" x2="1" y2="11" /></svg>
        </TitleBarButton>
      </div>
    </div>
  );
}
