import React, { useEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import { DownloadSimple, CircleNotch } from '@phosphor-icons/react';
import { useAppStore } from '../../store/useAppStore';
import { usePresence } from '../../hooks/usePresence';
import { animate, createTimeline, play, utils, EASE, EASE_OUT, DUR, prefersReducedMotion } from '../../lib/anime';

export const UpdateProgressOverlay: React.FC = () => {
  const { t } = useTranslation();
  const updateStatus = useAppStore(state => state.updateStatus);
  const updateProgress = useAppStore(state => state.updateProgress);
  const updateInfo = useAppStore(state => state.updateInfo);

  const isDownloading = updateStatus === 'downloading';
  const isInstalling = updateStatus === 'installing';
  const open = isDownloading || isInstalling;

  const modeRef = useRef<'downloading' | 'installing'>('downloading');
  if (isDownloading) modeRef.current = 'downloading';
  else if (isInstalling) modeRef.current = 'installing';
  const mode = modeRef.current;

  const barRef = useRef<HTMLDivElement>(null);
  const sheenRef = useRef<HTMLDivElement>(null);
  const sliverRef = useRef<HTMLDivElement>(null);
  const spinRef = useRef<HTMLSpanElement>(null);

  const formatBytes = (bytes: number): string => {
    if (!bytes || bytes <= 0) return '0 мб';
    const mb = bytes / (1024 * 1024);
    return `${mb.toFixed(1)} мб`;
  };

  const percent = updateProgress?.percent ?? 0;

  const { mounted, ref } = usePresence(open, {
    onEnter: (el) => {
      const panel = el.querySelector<HTMLElement>('[data-overlay-panel]');
      if (prefersReducedMotion()) {
        utils.set(el, { opacity: 1 });
        if (panel) utils.set(panel, { opacity: 1, scale: 1, translateY: 0 });
        return;
      }
      utils.set(el, { opacity: 0 });
      if (panel) utils.set(panel, { opacity: 0, scale: 0.95, translateY: 6 });
      const tl = createTimeline();
      tl.add(el, { opacity: [0, 1], duration: DUR.base, ease: EASE }, 0);
      if (panel) {
        tl.add(panel, { opacity: [0, 1], scale: [0.95, 1], translateY: [6, 0], duration: DUR.base, ease: EASE }, 0);
      }
    },
    onExit: (el) => {
      const panel = el.querySelector<HTMLElement>('[data-overlay-panel]');
      return Promise.all([
        play(el, { opacity: [1, 0], duration: DUR.micro, ease: EASE }),
        panel ? play(panel, { opacity: [1, 0], scale: [1, 0.97], duration: DUR.micro, ease: EASE }) : Promise.resolve(),
      ]).then(() => undefined);
    },
  });

  useEffect(() => {
    if (!mounted || mode !== 'downloading') return;
    const el = barRef.current;
    if (el) utils.set(el, { scaleX: 0 });
  }, [mounted, mode]);

  useEffect(() => {
    if (!mounted || mode !== 'downloading') return;
    const el = barRef.current;
    if (!el) return;
    const target = percent / 100;
    if (prefersReducedMotion()) {
      utils.set(el, { scaleX: target });
      return;
    }
    const a = animate(el, { scaleX: target, duration: DUR.slow, ease: EASE_OUT });
    return () => { a.pause(); };
  }, [mounted, mode, percent]);

  useEffect(() => {
    if (!mounted || mode !== 'downloading' || prefersReducedMotion()) return;
    const el = sheenRef.current;
    if (!el) return;
    const a = animate(el, { translateX: ['-120%', '220%'], duration: 1200, loop: true, ease: 'linear' });
    return () => { a.pause(); };
  }, [mounted, mode]);

  useEffect(() => {
    if (!mounted || mode !== 'installing' || prefersReducedMotion()) return;
    const el = sliverRef.current;
    if (!el) return;
    const a = animate(el, { translateX: ['-120%', '320%'], duration: 1100, loop: true, ease: 'inOutSine' });
    return () => { a.pause(); };
  }, [mounted, mode]);

  useEffect(() => {
    if (!mounted || mode !== 'installing' || prefersReducedMotion()) return;
    const el = spinRef.current;
    if (!el) return;
    const a = animate(el, { rotate: [0, 360], duration: 900, loop: true, ease: 'linear' });
    return () => { a.pause(); };
  }, [mounted, mode]);

  if (!mounted) return null;

  return createPortal(
    <div
      ref={ref}
      className="fixed inset-0 z-[100001] bg-black/70 backdrop-blur-md flex items-center justify-center p-3 pt-[3.75rem]"
      style={{ opacity: 0, willChange: 'opacity' }}
    >
      <div
        data-overlay-panel
        className="glass-modal w-[420px] max-w-full p-8 flex flex-col items-center text-center relative overflow-hidden"
        style={{ opacity: 0, willChange: 'transform, opacity' }}
      >
        <div className="w-14 h-14 rounded-2xl bg-primary/20 flex items-center justify-center text-primary mb-5">
          {mode === 'installing' ? (
            <span ref={spinRef} className="inline-flex" style={{ willChange: 'transform' }}>
              <CircleNotch weight="bold" size={30} />
            </span>
          ) : (
            <DownloadSimple weight="bold" size={28} />
          )}
        </div>

        <h2 className="text-xl font-bold text-white leading-tight mb-1.5">
          {mode === 'installing'
            ? t('modals.update.installingTitle', 'устанавливаю обновление')
            : t('modals.update.downloadingTitle', 'скачиваю обновление')}
        </h2>

        {updateInfo?.version && (
          <div className="text-xs font-bold text-primaryText mb-4">v{updateInfo.version}</div>
        )}

        <p className="text-sm text-textMuted leading-relaxed mb-6">
          {mode === 'installing'
            ? t('modals.update.installingDesc', 'закрываю zabor и обновляю. приложение перезапустится само на новой версии.')
            : t('modals.update.downloadingDesc', 'загружаю новую версию. пожалуйста, не закрывайте окно.')}
        </p>

        {mode === 'downloading' ? (
          <div className="w-full">
            <div className="flex items-center justify-between text-xs font-semibold mb-2">
              <span className="text-white">{t('modals.update.downloading', 'скачивание обновления...')}</span>
              <span className="text-primaryText font-bold">{percent}%</span>
            </div>
            <div className="w-full h-2 rounded-full bg-black/40 overflow-hidden relative">
              <div
                ref={barRef}
                className="h-full w-full bg-primary/90 rounded-full"
                style={{ transformOrigin: 'left center', willChange: 'transform' }}
              />
              <div
                ref={sheenRef}
                className="absolute inset-y-0 left-0 w-1/3 bg-gradient-to-r from-transparent via-white/40 to-transparent"
                style={{ willChange: 'transform' }}
              />
            </div>
            {updateProgress && updateProgress.total > 0 && (
              <div className="flex justify-between text-[11px] text-textMuted mt-1.5 font-medium">
                <span>{formatBytes(updateProgress.transferred)}</span>
                <span>{formatBytes(updateProgress.total)}</span>
              </div>
            )}
          </div>
        ) : (
          <div className="w-full h-1.5 rounded-full bg-black/40 overflow-hidden relative">
            <div
              ref={sliverRef}
              className="absolute inset-y-0 left-0 w-1/3 bg-primary/90 rounded-full"
              style={{ willChange: 'transform' }}
            />
          </div>
        )}
      </div>
    </div>,
    document.body
  );
};
