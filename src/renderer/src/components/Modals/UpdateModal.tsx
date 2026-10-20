import React, { useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { X, ArrowSquareOut, DownloadSimple, Sparkle, ArrowRight, CheckCircle, Warning } from '@phosphor-icons/react';
import { useAppStore } from '../../store/useAppStore';
import { play, animate, spring, utils, springs, EASE_OUT, DUR, prefersReducedMotion } from '../../lib/anime';

interface UpdateModalProps {
  onClose: () => void;
}

const PROGRESS_BAR_SPRING = spring({ stiffness: 110, damping: 12, mass: 1 });
const PROGRESS_ENTER_SPRING = spring({ stiffness: 170, damping: 13, mass: 1 });

export const UpdateModal: React.FC<UpdateModalProps> = ({ onClose }) => {
  const { t } = useTranslation();
  const updateInfo = useAppStore(state => state.updateInfo);
  const updateProgress = useAppStore(state => state.updateProgress);
  const updateStatus = useAppStore(state => state.updateStatus);
  const updateError = useAppStore(state => state.updateError);
  const setUpdateStatus = useAppStore(state => state.setUpdateStatus);
  const setUpdateProgress = useAppStore(state => state.setUpdateProgress);
  const setUpdateError = useAppStore(state => state.setUpdateError);

  const rootRef = useRef<HTMLDivElement>(null);
  const badgeRef = useRef<HTMLDivElement>(null);
  const arrowRef = useRef<HTMLSpanElement>(null);
  const cardRef = useRef<HTMLDivElement>(null);
  const whatsNewRef = useRef<HTMLDivElement>(null);
  const progressWrapRef = useRef<HTMLDivElement>(null);
  const progressTrackRef = useRef<HTMLDivElement>(null);
  const progressFillRef = useRef<HTMLDivElement>(null);
  const actionBtnWrapRef = useRef<HTMLDivElement>(null);
  const prevViewModeRef = useRef<'notes' | 'progress'>('notes');
  const prevBtnModeRef = useRef<'initial' | 'downloading' | 'downloaded'>('initial');

  const isDownloading = updateStatus === 'downloading';
  const isDownloaded = updateStatus === 'downloaded';
  const isInstalling = updateStatus === 'installing';
  const isProgressView = isDownloading || isDownloaded || isInstalling;

  const displayPercent = isDownloaded || isInstalling
    ? 100
    : Math.min(100, Math.max(0, updateProgress?.percent ?? 0));

  useEffect(() => {
    const root = rootRef.current;
    const badge = badgeRef.current;
    const arrow = arrowRef.current;
    if (prefersReducedMotion()) {
      if (root) utils.set(root, { opacity: 1, scale: 1, translateY: 0 });
      if (badge) utils.set(badge, { opacity: 1, scale: 1, rotate: 0 });
      if (arrow) utils.set(arrow, { opacity: 1, scaleX: 1 });
      return;
    }
    if (root) {
      utils.set(root, { opacity: 0, scale: 0.96, translateY: 8 });
      animate(root, { opacity: [0, 1], scale: [0.96, 1], translateY: [8, 0], duration: DUR.enter, ease: EASE_OUT });
    }
    if (badge) {
      utils.set(badge, { opacity: 0, scale: 0.6, rotate: -25 });
      void play(badge, { opacity: [0, 1], scale: [0.6, 1], rotate: [-25, 0], ease: springs.pop, delay: 60 });
    }
    if (arrow) {
      utils.set(arrow, { opacity: 0, scaleX: 0.2 });
      void play(arrow, { opacity: [0, 1], scaleX: [0.2, 1], duration: DUR.base, ease: EASE_OUT, delay: 150 });
    }
  }, []);

  useEffect(() => {
    const nextViewMode: 'notes' | 'progress' = isProgressView ? 'progress' : 'notes';
    const changed = prevViewModeRef.current !== nextViewMode;
    prevViewModeRef.current = nextViewMode;

    if (nextViewMode === 'progress') {
      const wrap = progressWrapRef.current;
      const track = progressTrackRef.current;
      if (!wrap) return;
      if (prefersReducedMotion()) {
        utils.set(wrap, { opacity: 1, scale: 1, translateY: 0 });
        if (track) utils.set(track, { scaleX: 1, scaleY: 1 });
        return;
      }
      utils.remove(wrap);
      utils.set(wrap, { opacity: 0, scale: 0.92, translateY: 10 });
      animate(wrap, {
        opacity: [0, 1],
        scale: [0.92, 1],
        translateY: [10, 0],
        ease: PROGRESS_ENTER_SPRING
      });
      if (track) {
        utils.remove(track);
        utils.set(track, { scaleX: 0.85, scaleY: 0.55 });
        animate(track, {
          scaleX: [0.85, 1],
          scaleY: [0.55, 1],
          ease: springs.pop,
          delay: 40
        });
      }
    } else if (changed) {
      const notes = whatsNewRef.current;
      if (!notes) return;
      if (prefersReducedMotion()) {
        utils.set(notes, { opacity: 1, scale: 1, translateY: 0 });
        return;
      }
      utils.remove(notes);
      utils.set(notes, { opacity: 0, scale: 0.96, translateY: 6 });
      animate(notes, {
        opacity: [0, 1],
        scale: [0.96, 1],
        translateY: [6, 0],
        ease: springs.card
      });
    }
  }, [isProgressView]);

  useEffect(() => {
    if (!isProgressView) return;
    const fill = progressFillRef.current;
    if (!fill) return;
    const targetShift = `${displayPercent / 2}%`;
    if (prefersReducedMotion()) {
      utils.set(fill, { translateX: targetShift });
      return;
    }
    utils.remove(fill);
    animate(fill, {
      translateX: targetShift,
      ease: PROGRESS_BAR_SPRING
    });
  }, [displayPercent, isProgressView]);

  useEffect(() => {
    const nextBtnMode: 'initial' | 'downloading' | 'downloaded' =
      isDownloaded || isInstalling ? 'downloaded' : isDownloading ? 'downloading' : 'initial';
    if (prevBtnModeRef.current === nextBtnMode) return;
    prevBtnModeRef.current = nextBtnMode;
    const btnWrap = actionBtnWrapRef.current;
    if (!btnWrap) return;
    if (prefersReducedMotion()) {
      utils.set(btnWrap, { opacity: 1, scale: 1, translateY: 0 });
      return;
    }
    utils.remove(btnWrap);
    utils.set(btnWrap, { opacity: 0, scale: 0.88, translateY: 4 });
    animate(btnWrap, {
      opacity: [0, 1],
      scale: [0.88, 1],
      translateY: [4, 0],
      ease: springs.pop
    });
  }, [isDownloading, isDownloaded, isInstalling]);

  useEffect(() => {
    if (updateStatus !== 'error' || !updateError) return;
    const card = cardRef.current;
    if (!card) return;
    if (prefersReducedMotion()) {
      utils.set(card, { opacity: 1, scale: 1, translateY: 0 });
      return;
    }
    utils.set(card, { opacity: 0, scale: 0.95, translateY: 6 });
    void play(card, { opacity: [0, 1], scale: [0.95, 1], translateY: [6, 0], duration: DUR.base, ease: EASE_OUT });
  }, [updateStatus, updateError]);

  if (!updateInfo) return null;

  const formatBytes = (bytes: number): string => {
    if (!bytes || bytes <= 0) return `0 ${t('updater.mb', 'мб')}`;
    const mb = bytes / (1024 * 1024);
    return `${mb.toFixed(1)} ${t('updater.mb', 'мб')}`;
  };

  const handleStartDownload = async () => {
    if (!updateInfo.downloadUrl) return;
    setUpdateProgress({ percent: 0, transferred: 0, total: updateInfo.fileSize || 0 });
    setUpdateStatus('downloading');
    setUpdateError(null);
    try {
      const res = await window.windowControls.startUpdateDownload(updateInfo.downloadUrl, updateInfo.version);
      if (!res.success && res.error && useAppStore.getState().updateStatus === 'downloading') {
        setUpdateStatus('error');
        setUpdateError(res.error);
      }
    } catch (err: any) {
      if (useAppStore.getState().updateStatus === 'downloading') {
        setUpdateStatus('error');
        setUpdateError(err?.message || 'ошибка скачивания');
      }
    }
  };

  const handleCancelDownload = async () => {
    try {
      await window.windowControls.cancelUpdateDownload();
    } catch {}
    setUpdateStatus('available');
    setUpdateProgress(null);
    setUpdateError(null);
  };

  const handleCloseModal = () => {
    if (isDownloading) {
      void window.windowControls.cancelUpdateDownload().catch(() => {});
      setUpdateStatus('available');
      setUpdateProgress(null);
    }
    onClose();
  };

  const handleInstall = async () => {
    setUpdateStatus('installing');
    setUpdateError(null);
    try {
      const res = await window.windowControls.installUpdate();
      if (res && !res.success && (res.error || res.message)) {
        setUpdateStatus('error');
        setUpdateError(res.error || res.message || 'ошибка установки');
      }
    } catch (err: any) {
      setUpdateStatus('error');
      setUpdateError(err?.message || 'ошибка установки');
    }
  };

  const handleOpenReleasePage = () => {
    if (updateInfo.releaseUrl) {
      window.windowControls.openExternalUrl(updateInfo.releaseUrl);
    }
  };

  const totalBytes = (updateProgress && updateProgress.total > 0) ? updateProgress.total : (updateInfo.fileSize || 0);
  const transferredBytes = isDownloaded || isInstalling
    ? totalBytes
    : (updateProgress?.transferred ?? 0);

  return (
    <div ref={rootRef} className="glass-modal w-[520px] max-w-full p-6 flex flex-col relative overflow-hidden opacity-0" style={{ willChange: 'transform, opacity' }}>
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center gap-2.5">
          <div ref={badgeRef} className="w-8 h-8 rounded-xl bg-primary/20 flex items-center justify-center text-primary opacity-0" style={{ willChange: 'transform, opacity' }}>
            <Sparkle weight="bold" size={18} />
          </div>
          <div>
            <h2 className="text-xl font-bold text-white leading-tight">
              {t('modals.update.title', 'доступно обновление')}
            </h2>
            <div className="flex items-center gap-2 mt-0.5 text-xs font-semibold text-textMuted">
              <span>v{updateInfo.currentVersion}</span>
              <span ref={arrowRef} className="opacity-0 inline-flex" style={{ transformOrigin: 'left center', willChange: 'transform, opacity' }}>
                <ArrowRight weight="bold" size={12} className="text-primary" />
              </span>
              <span className="text-primaryText font-bold">v{updateInfo.version}</span>
              {updateInfo.fileSize > 0 && (
                <>
                  <span className="text-white/20">•</span>
                  <span>{formatBytes(updateInfo.fileSize)}</span>
                </>
              )}
            </div>
          </div>
        </div>

        <button
          onClick={handleCloseModal}
          disabled={isInstalling}
          className={`group text-textMuted hover:text-white transition-colors duration-200 p-1.5 rounded-lg hover:bg-surface/70 ${
            isInstalling ? 'opacity-40 cursor-not-allowed' : ''
          }`}
          title={t('common.close', 'закрыть')}
        >
          <X weight="bold" size={20} />
        </button>
      </div>

      {isProgressView ? (
        <div
          ref={progressWrapRef}
          className="mb-5 glass-field rounded-xl p-4"
          style={{ willChange: 'transform, opacity' }}
        >
          <div className="flex items-center justify-between text-xs font-bold mb-2.5">
            <span className="text-white flex items-center gap-2">
              {(isDownloaded || isInstalling) && (
                <CheckCircle weight="bold" size={16} className="text-primaryText shrink-0" />
              )}
              <span>
                {isInstalling
                  ? t('modals.update.installingTitle', 'устанавливаю обновление')
                  : isDownloaded
                    ? t('modals.update.downloadedTitle', 'обновление готово к установке')
                    : t('modals.update.downloading', 'скачивание обновления...')}
              </span>
            </span>
            <span className="text-primaryText font-black text-sm tabular-nums">
              {displayPercent}%
            </span>
          </div>

          <div
            ref={progressTrackRef}
            className="w-full h-2.5 rounded-full bg-black/50 overflow-hidden relative border border-white/[0.07]"
            style={{ transformOrigin: 'center center', willChange: 'transform' }}
          >
            <div
              ref={progressFillRef}
              className="absolute inset-y-0 -left-[200%] w-[200%] rounded-full bg-primary overflow-hidden"
              style={{ transform: 'translateX(0%)', willChange: 'transform' }}
            >
              {isDownloading && (
                <div className="absolute inset-y-0 left-1/2 w-1/4 bg-gradient-to-r from-transparent via-white/35 to-transparent animate-update-indeterminate" />
              )}
            </div>
          </div>

          <div className="flex items-center justify-between text-[11px] text-textMuted mt-2 font-medium tabular-nums">
            <span>
              {isDownloaded || isInstalling
                ? t('modals.update.downloadedDesc', 'zabor закроется и запустит установщик.')
                : totalBytes > 0
                  ? `${formatBytes(transferredBytes)} / ${formatBytes(totalBytes)}`
                  : formatBytes(transferredBytes)}
            </span>
            {(isDownloaded || isInstalling) && totalBytes > 0 && (
              <span>{formatBytes(totalBytes)}</span>
            )}
          </div>
        </div>
      ) : (
        <div ref={whatsNewRef} className="mb-5" style={{ willChange: 'transform, opacity' }}>
          {updateInfo.releaseNotes ? (
            <>
              <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">
                {t('modals.update.whatsNew', 'что нового')}
              </label>
              <div className="glass-field rounded-xl p-3.5 max-h-[160px] overflow-y-auto text-sm text-white/90 whitespace-pre-wrap leading-relaxed">
                {updateInfo.releaseNotes}
              </div>
            </>
          ) : (
            <div className="text-sm text-textMuted">
              {t('modals.update.noNotes', 'вышла новая версия zabor с улучшениями и исправлениями.')}
            </div>
          )}
        </div>
      )}

      {updateStatus === 'error' && updateError && (
        <div ref={cardRef} className="mb-5 p-3.5 rounded-xl bg-danger/10 border border-danger/20 flex items-center gap-3 opacity-0" style={{ willChange: 'transform, opacity' }}>
          <Warning weight="bold" size={22} className="text-danger shrink-0" />
          <div className="text-xs text-danger flex-1 truncate">
            <p className="font-bold">{t('modals.update.errorTitle', 'не удалось загрузить обновление')}</p>
            <p className="text-textMuted mt-0.5 truncate">{updateError}</p>
          </div>
        </div>
      )}

      <div className="flex items-center gap-3">
        <button
          onClick={handleOpenReleasePage}
          className="px-4 py-2.5 rounded-xl text-xs font-bold text-textMuted hover:text-white bg-surface/70 hover:bg-surfaceHover/80 transition-colors flex items-center gap-1.5"
          title="GitHub"
        >
          <ArrowSquareOut weight="bold" size={16} />
          <span>{t('modals.update.viewOnGithub', 'на GitHub')}</span>
        </button>

        <div className="flex-1" />

        <div ref={actionBtnWrapRef} className="flex items-center gap-3" style={{ willChange: 'transform, opacity' }}>
          {isDownloading ? (
            <button
              onClick={handleCancelDownload}
              className="px-5 py-2.5 rounded-xl text-sm font-bold text-white bg-surface/70 hover:bg-surfaceHover/80 transition-colors active:scale-[0.98]"
            >
              {t('common.cancel', 'отмена')}
            </button>
          ) : isDownloaded || isInstalling ? (
            <button
              onClick={handleInstall}
              disabled={isInstalling}
              className={`px-5 py-2.5 rounded-xl text-sm font-bold text-white bg-primary/90 hover:opacity-90 transition-all flex items-center gap-2 active:scale-[0.98] ${
                isInstalling ? 'opacity-70 cursor-wait' : ''
              }`}
            >
              <span>{t('modals.update.install', 'установить')}</span>
            </button>
          ) : (
            <>
              <button
                onClick={onClose}
                className="px-4 py-2.5 rounded-xl text-sm font-bold text-white bg-surface/70 hover:bg-surfaceHover/80 transition-colors active:scale-[0.98]"
              >
                {t('modals.update.later', 'позже')}
              </button>

              <button
                onClick={handleStartDownload}
                className="px-5 py-2.5 rounded-xl text-sm font-bold text-white bg-primary/90 hover:opacity-90 transition-all flex items-center gap-2 active:scale-[0.98]"
              >
                <DownloadSimple weight="bold" size={16} />
                <span>{t('modals.update.updateNow', 'обновить')}</span>
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
};
