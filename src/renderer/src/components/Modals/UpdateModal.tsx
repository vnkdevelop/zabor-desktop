import React from 'react';
import { useTranslation } from 'react-i18next';
import { X, ArrowSquareOut, DownloadSimple, Sparkle, ArrowRight, CheckCircle, Warning, CircleNotch } from '@phosphor-icons/react';
import { useAppStore } from '../../store/useAppStore';

interface UpdateModalProps {
  onClose: () => void;
}

export const UpdateModal: React.FC<UpdateModalProps> = ({ onClose }) => {
  const { t } = useTranslation();
  const updateInfo = useAppStore(state => state.updateInfo);
  const updateProgress = useAppStore(state => state.updateProgress);
  const updateStatus = useAppStore(state => state.updateStatus);
  const updateError = useAppStore(state => state.updateError);
  const setUpdateStatus = useAppStore(state => state.setUpdateStatus);
  const setUpdateError = useAppStore(state => state.setUpdateError);

  if (!updateInfo) return null;

  const formatBytes = (bytes: number): string => {
    if (!bytes || bytes <= 0) return '0 мб';
    const mb = bytes / (1024 * 1024);
    return `${mb.toFixed(1)} мб`;
  };

  const handleStartDownload = async () => {
    if (!updateInfo.downloadUrl) return;
    setUpdateStatus('downloading');
    setUpdateError(null);
    try {
      const res = await window.windowControls.startUpdateDownload(updateInfo.downloadUrl, updateInfo.version);
      if (!res.success && res.error) {
        setUpdateStatus('error');
        setUpdateError(res.error);
      }
    } catch (err: any) {
      setUpdateStatus('error');
      setUpdateError(err?.message || 'ошибка скачивания');
    }
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

  const isDownloading = updateStatus === 'downloading';
  const isDownloaded = updateStatus === 'downloaded';
  const isInstalling = updateStatus === 'installing';

  if (isInstalling) {
    return (
      <div className="glass-modal w-[420px] max-w-full p-8 flex flex-col items-center text-center relative overflow-hidden animate-modal-in">
        <div className="w-14 h-14 rounded-2xl bg-primary/20 flex items-center justify-center text-primary mb-5">
          <CircleNotch weight="bold" size={30} className="animate-spin" />
        </div>
        <h2 className="text-xl font-bold text-white leading-tight mb-2">
          {t('modals.update.installingTitle', 'устанавливаю обновление')}
        </h2>
        <p className="text-sm text-textMuted leading-relaxed">
          {t('modals.update.installingDesc', 'закрываю zabor и обновляю. приложение перезапустится само на новой версии.')}
        </p>
        <div className="w-full h-1.5 rounded-full bg-black/40 overflow-hidden relative mt-6">
          <div className="h-full w-1/3 bg-primary/90 rounded-full animate-update-indeterminate" />
        </div>
      </div>
    );
  }

  return (
    <div className="glass-modal w-[520px] max-w-full p-6 flex flex-col relative overflow-hidden animate-modal-in">
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center gap-2.5">
          <div className="w-8 h-8 rounded-xl bg-primary/20 flex items-center justify-center text-primary">
            <Sparkle weight="bold" size={18} />
          </div>
          <div>
            <h2 className="text-xl font-bold text-white leading-tight">
              {t('modals.update.title', 'доступно обновление')}
            </h2>
            <div className="flex items-center gap-2 mt-0.5 text-xs font-semibold text-textMuted">
              <span>v{updateInfo.currentVersion}</span>
              <ArrowRight weight="bold" size={12} className="text-primary" />
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
          onClick={onClose}
          disabled={isDownloading}
          className={`group text-textMuted hover:text-white transition-colors duration-200 p-1.5 rounded-lg hover:bg-surface/70 ${
            isDownloading ? 'opacity-40 cursor-not-allowed' : ''
          }`}
          title={t('common.close', 'закрыть')}
        >
          <X weight="bold" size={20} />
        </button>
      </div>

      {updateInfo.releaseNotes ? (
        <div className="mb-5">
          <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">
            {t('modals.update.whatsNew', 'что нового')}
          </label>
          <div className="glass-field rounded-xl p-3.5 max-h-[160px] overflow-y-auto text-sm text-white/90 whitespace-pre-wrap leading-relaxed">
            {updateInfo.releaseNotes}
          </div>
        </div>
      ) : (
        <div className="mb-5 text-sm text-textMuted">
          {t('modals.update.noNotes', 'вышла новая версия zabor с улучшениями и исправлениями.')}
        </div>
      )}

      {isDownloading && (
        <div className="mb-5 p-3.5 rounded-xl bg-surface/50 border border-white/[0.06]">
          <div className="flex items-center justify-between text-xs font-semibold mb-2">
            <span className="text-white">{t('modals.update.downloading', 'скачивание обновления...')}</span>
            <span className="text-primaryText font-bold">
              {updateProgress ? `${updateProgress.percent}%` : '0%'}
            </span>
          </div>

          <div className="w-full h-2 rounded-full bg-black/40 overflow-hidden relative">
            <div
              className="h-full bg-primary/90 rounded-full transition-all duration-150 ease-out"
              style={{ width: `${updateProgress?.percent || 0}%` }}
            />
          </div>

          {updateProgress && updateProgress.total > 0 && (
            <div className="flex justify-between text-[11px] text-textMuted mt-1.5 font-medium">
              <span>{formatBytes(updateProgress.transferred)}</span>
              <span>{formatBytes(updateProgress.total)}</span>
            </div>
          )}
        </div>
      )}

      {isDownloaded && (
        <div className="mb-5 p-3.5 rounded-xl bg-success/10 border border-success/20 flex items-center gap-3">
          <CheckCircle weight="bold" size={22} className="text-success shrink-0" />
          <div className="text-xs text-white">
            <p className="font-bold">{t('modals.update.downloadedTitle', 'обновление готово к установке')}</p>
            <p className="text-textMuted mt-0.5">
              {t('modals.update.downloadedDesc', 'zabor закроется и запустит установщик.')}
            </p>
          </div>
        </div>
      )}

      {updateStatus === 'error' && updateError && (
        <div className="mb-5 p-3.5 rounded-xl bg-danger/10 border border-danger/20 flex items-center gap-3">
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

        <button
          onClick={onClose}
          disabled={isDownloading}
          className={`px-4 py-2.5 rounded-xl text-sm font-bold text-white bg-surface/70 hover:bg-surfaceHover/80 transition-colors ${
            isDownloading ? 'opacity-50 cursor-not-allowed' : ''
          }`}
        >
          {t('modals.update.later', 'позже')}
        </button>

        {isDownloaded ? (
          <button
            onClick={handleInstall}
            className="px-5 py-2.5 rounded-xl text-sm font-bold text-white bg-success hover:bg-green-600 transition-colors active:scale-[0.98]"
          >
            {t('modals.update.install', 'установить')}
          </button>
        ) : (
          <button
            onClick={handleStartDownload}
            disabled={isDownloading}
            className={`px-5 py-2.5 rounded-xl text-sm font-bold text-white bg-primary/90 hover:opacity-90 transition-all flex items-center gap-2 active:scale-[0.98] ${
              isDownloading ? 'opacity-70 cursor-wait' : ''
            }`}
          >
            <DownloadSimple weight="bold" size={16} />
            <span>
              {isDownloading
                ? t('modals.update.downloadingBtn', 'загрузка...')
                : t('modals.update.updateNow', 'обновить')}
            </span>
          </button>
        )}
      </div>
    </div>
  );
};
