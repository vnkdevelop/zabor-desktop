import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { X, CircleNotch, Warning } from '@phosphor-icons/react';
import type { UpdateProgress, UpdateStatus } from '../global';

export const UpdaterWindow: React.FC = () => {
  const { t } = useTranslation();
  const [status, setStatus] = useState<UpdateStatus>('downloading');
  const [progress, setProgress] = useState<UpdateProgress>({ percent: 0, transferred: 0, total: 0 });
  const [version, setVersion] = useState<string>('');
  const [downloadUrl, setDownloadUrl] = useState<string>('');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let isMounted = true;

    window.windowControls?.getUpdateStatus?.().then((snapshot) => {
      if (!isMounted || !snapshot) return;
      if (snapshot.updateInfo?.version) setVersion(snapshot.updateInfo.version);
      if (snapshot.updateInfo?.downloadUrl) setDownloadUrl(snapshot.updateInfo.downloadUrl);
      if (snapshot.progress) setProgress(snapshot.progress);

      if (snapshot.status === 'installing') {
        setStatus('installing');
      } else if (snapshot.status === 'error') {
        setStatus('error');
        setError(snapshot.error || null);
      } else if (snapshot.status === 'downloaded') {
        setStatus('installing');
        window.windowControls?.installUpdate?.().then((res) => {
          if (res && !res.success && isMounted) {
            setStatus('error');
            setError(res.error || res.message || null);
          }
        });
      } else {
        setStatus('downloading');
      }
    }).catch((err) => {
      console.error('[UpdaterWindow] Failed to get update status:', err);
    });

    const progressListener = (p: UpdateProgress) => {
      if (!isMounted) return;
      setProgress(p);
      setStatus('downloading');
    };

    const cleanupProgress = window.windowControls?.onUpdateProgress
      ? window.windowControls.onUpdateProgress(progressListener)
      : window.windowControls?.onUpdateDownloadProgress?.(progressListener);

    const cleanupDownloaded = window.windowControls?.onUpdateDownloaded?.(async () => {
      if (!isMounted) return;
      setStatus('installing');
      try {
        const res = await window.windowControls?.installUpdate?.();
        if (res && !res.success && isMounted) {
          setStatus('error');
          setError(res.error || res.message || null);
        }
      } catch (e: any) {
        if (isMounted) {
          setStatus('error');
          setError(e?.message || null);
        }
      }
    });

    const cleanupInstalling = window.windowControls?.onUpdateInstalling?.(() => {
      if (!isMounted) return;
      setStatus('installing');
    });

    const cleanupError = window.windowControls?.onUpdateError?.((err) => {
      if (!isMounted) return;
      setStatus('error');
      setError(err);
    });

    return () => {
      isMounted = false;
      cleanupProgress?.();
      cleanupDownloaded?.();
      cleanupInstalling?.();
      cleanupError?.();
    };
  }, []);

  const formatBytes = (bytes: number): string => {
    if (!bytes || bytes <= 0) return `0 ${t('updater.mb', 'мб')}`;
    const mb = bytes / (1024 * 1024);
    return `${mb.toFixed(1)} ${t('updater.mb', 'мб')}`;
  };

  const handleClose = () => {
    window.windowControls?.close?.();
  };

  const handleCancel = async () => {
    try {
      await window.windowControls?.cancelUpdateDownload?.();
    } finally {
      window.windowControls?.close?.();
    }
  };

  const handleRetry = async () => {
    let targetUrl = downloadUrl;
    let targetVersion = version;

    if (!targetUrl || !targetVersion) {
      const snapshot = await window.windowControls?.getUpdateStatus?.();
      if (snapshot?.updateInfo) {
        targetUrl = snapshot.updateInfo.downloadUrl;
        targetVersion = snapshot.updateInfo.version;
        setDownloadUrl(targetUrl);
        setVersion(targetVersion);
      }
    }

    if (!targetUrl) return;

    setStatus('downloading');
    setError(null);
    setProgress({ percent: 0, transferred: 0, total: 0 });

    try {
      const res = await window.windowControls?.startUpdateDownload?.(targetUrl, targetVersion);
      if (res && !res.success && res.error) {
        setStatus('error');
        setError(res.error);
      }
    } catch (err: any) {
      setStatus('error');
      setError(err?.message || null);
    }
  };

  const percent = Math.min(100, Math.max(0, progress.percent || 0));
  const cleanVersion = version ? version.replace(/^v/i, '') : '';
  const displayVersion = cleanVersion ? `${t('updater.version', 'версия')} ${cleanVersion}` : '';

  return (
    <div
      className="w-screen h-screen flex flex-col select-none overflow-hidden bg-[#09090B] text-white border border-white/[0.07] border-t-white/[0.14]"
      style={{ boxShadow: 'none' }}
    >
      {/* Titlebar */}
      <div className="title-bar h-9 bg-[#1a1a1a]/90 backdrop-blur-xl flex items-center justify-between shrink-0 relative z-50 border-b border-white/[0.06]">
        <div className="flex items-center pl-3.5 gap-2 pointer-events-none">
          <span className="text-[12px] font-black tracking-[0.2em] text-white/30 lowercase">
            {t('updater.title', 'zabor')}
          </span>
        </div>
        <div className="flex items-center h-full title-no-drag">
          <button
            type="button"
            aria-label={t('updater.close', 'закрыть')}
            title={t('updater.close', 'закрыть')}
            onClick={status === 'downloading' ? handleCancel : handleClose}
            className="h-full w-10 flex items-center justify-center text-white/40 hover:text-white hover:bg-primary/90 transition-colors"
          >
            <X weight="bold" size={14} />
          </button>
        </div>
      </div>

      {/* Content Area */}
      <div className="flex-1 flex flex-col justify-between p-5">
        {status === 'error' ? (
          <>
            <div className="flex items-center gap-2 text-danger font-bold text-sm lowercase">
              <Warning weight="bold" size={18} className="shrink-0" />
              <span>{t('updater.errorTitle', 'не удалось загрузить обновление')}</span>
            </div>

            <div className="glass-field rounded-xl p-3 text-xs text-textMuted/90 break-words max-h-[72px] overflow-y-auto leading-relaxed border border-danger/30">
              {error || t('updater.errorUnknown', 'произошла непредвиденная ошибка')}
            </div>

            <div className="flex items-center justify-end gap-2.5 title-no-drag">
              <button
                type="button"
                onClick={handleClose}
                className="px-4 py-1.5 rounded-xl text-xs font-bold text-white/80 bg-surface/70 hover:bg-surfaceHover/80 hover:text-white transition-colors active:scale-[0.98] border border-white/[0.07] lowercase"
              >
                {t('updater.close', 'закрыть')}
              </button>
              <button
                type="button"
                onClick={handleRetry}
                className="px-4 py-1.5 rounded-xl text-xs font-bold text-white bg-primary/90 hover:opacity-90 transition-opacity active:scale-[0.98] lowercase"
              >
                {t('updater.retry', 'повторить')}
              </button>
            </div>
          </>
        ) : status === 'installing' ? (
          <>
            <div className="flex items-center justify-between">
              <span className="text-sm font-bold text-white lowercase">
                {t('updater.installing', 'устанавливаю обновление...')}
              </span>
              {displayVersion && (
                <span className="text-xs font-bold text-primaryText lowercase">
                  {displayVersion}
                </span>
              )}
            </div>

            <div className="flex flex-col gap-3 my-auto">
              <div className="w-full h-2 rounded-full bg-white/[0.08] overflow-hidden relative border border-white/[0.06]">
                <div className="h-full w-1/3 bg-primary rounded-full animate-update-indeterminate" />
              </div>
              <div className="flex items-center justify-center gap-2 text-xs text-textMuted">
                <CircleNotch weight="bold" size={14} className="animate-spin text-primary shrink-0" />
                <span className="lowercase">
                  {t('updater.installingDesc', 'приложение перезапустится автоматически')}
                </span>
              </div>
            </div>

            <div className="flex items-center justify-between text-[11px] text-textMuted/60">
              <span className="lowercase">{t('updater.pleaseWait', 'пожалуйста, подождите...')}</span>
            </div>
          </>
        ) : (
          <>
            <div className="flex items-center justify-between">
              <span className="text-sm font-bold text-white lowercase">
                {t('updater.downloading', 'скачивание обновления...')}
              </span>
              {displayVersion && (
                <span className="text-xs font-bold text-primaryText lowercase">
                  {displayVersion}
                </span>
              )}
            </div>

            <div className="flex flex-col gap-2 my-auto">
              <div className="flex items-center justify-between text-xs">
                <span className="text-textMuted font-medium lowercase">
                  {progress.total > 0
                    ? `${formatBytes(progress.transferred)} / ${formatBytes(progress.total)}`
                    : formatBytes(progress.transferred)}
                </span>
                <span className="text-sm font-black text-white">{percent}%</span>
              </div>

              <div className="w-full h-2 rounded-full bg-white/[0.08] overflow-hidden relative border border-white/[0.06]">
                <div
                  className="h-full bg-primary rounded-full transition-all duration-150 ease-out"
                  style={{ width: `${percent}%` }}
                />
                <div
                  className="absolute inset-y-0 left-0 w-1/3 bg-gradient-to-r from-transparent via-white/30 to-transparent"
                  style={{ animation: 'update-indeterminate 1.4s linear infinite', willChange: 'transform' }}
                />
              </div>
            </div>

            <div className="flex items-center justify-end title-no-drag">
              <button
                type="button"
                onClick={handleCancel}
                className="px-4 py-1.5 rounded-xl text-xs font-bold text-white/80 bg-surface/70 hover:bg-surfaceHover/80 hover:text-white transition-colors active:scale-[0.98] border border-white/[0.07] lowercase"
              >
                {t('updater.cancel', 'отмена')}
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
};

export default UpdaterWindow;
