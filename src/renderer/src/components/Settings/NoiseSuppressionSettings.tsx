import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, Sliders, Sparkle } from '@phosphor-icons/react';
import { Md3Switch } from '../Shared/Md3Switch';
import {
  MAX_SUPPRESSION_STRENGTH_DB,
  MIN_SUPPRESSION_STRENGTH_DB,
  VAD_CALIBRATION_ENABLED,
  webrtc,
  type SmartNoiseModel
} from '../../services/webrtc';

export type NoiseSuppressionMode = 'smart' | 'manual';
export type CalibrationPhase = 'idle' | 'preparing' | 'voice' | 'checking';
export type { SmartNoiseModel };

export interface NoiseSuppressionSettingsProps {
  isEnabled: boolean;
  onEnabledChange: (enabled: boolean) => void;
  mode: NoiseSuppressionMode;
  onModeChange: (mode: NoiseSuppressionMode) => void;
  manualThreshold: number;
  onManualThresholdChange: (val: number) => void;
  smartModel: SmartNoiseModel;
  onSmartModelChange: (model: SmartNoiseModel) => void;
  speechAnalyzer: boolean;
  onSpeechAnalyzerChange: (enabled: boolean) => void;
  echoCancellation: boolean;
  onEchoCancellationChange: (enabled: boolean) => void;
  suppressionStrength: number;
  onSuppressionStrengthChange: (db: number) => void;
  onStartCalibration?: () => void;
  isCalibrating?: boolean;
  calibrationCountdown?: number;
  calibrationPhase?: CalibrationPhase;
  calibrationSuccess?: boolean;
  liveMicLevel?: number;
  ultraLowLatency?: boolean;
  onUltraLowLatencyChange?: (enabled: boolean) => void;
  className?: string;
}

const MIN_THRESHOLD_DB = -60;
const MAX_THRESHOLD_DB = -12;
const METER_SEGMENTS = 32;

export function NoiseSuppressionSettings({
  isEnabled,
  onEnabledChange,
  mode,
  onModeChange,
  manualThreshold,
  onManualThresholdChange,
  smartModel,
  onSmartModelChange,
  speechAnalyzer,
  onSpeechAnalyzerChange,
  echoCancellation,
  onEchoCancellationChange,
  suppressionStrength,
  onSuppressionStrengthChange,
  onStartCalibration,
  isCalibrating = false,
  calibrationCountdown = 0,
  calibrationPhase = 'idle',
  calibrationSuccess = false,
  liveMicLevel: externalMicLevel,
  ultraLowLatency = false,
  onUltraLowLatencyChange,
  className = ''
}: NoiseSuppressionSettingsProps) {
  const { t } = useTranslation();
  const [internalMicLevelDb, setInternalMicLevelDb] = useState(-100);

  const needsMeter = isEnabled && mode === 'manual';
  useEffect(() => {
    if (!needsMeter || externalMicLevel !== undefined) return;
    return webrtc.subscribeMicLevel(setInternalMicLevelDb);
  }, [needsMeter, externalMicLevel]);

  const micLevelDb = Math.max(-100, Math.min(0, externalMicLevel ?? internalMicLevelDb));
  const threshold = Math.max(MIN_THRESHOLD_DB, Math.min(MAX_THRESHOLD_DB, manualThreshold));
  const strength = Math.max(
    MIN_SUPPRESSION_STRENGTH_DB,
    Math.min(MAX_SUPPRESSION_STRENGTH_DB, Math.round(suppressionStrength))
  );
  const meterPosition = Math.max(0, Math.min(100, ((micLevelDb - MIN_THRESHOLD_DB) / (MAX_THRESHOLD_DB - MIN_THRESHOLD_DB)) * 100));

  const idleCalibrationText = t('settings.audio.calibrateButton', 'калибровка');
  const calibrationText = calibrationPhase === 'voice'
    ? `${t('settings.audio.calibrationPhrase', '«съешь ещё этих мягких французских булок»')}${calibrationCountdown > 0 ? ` · ${calibrationCountdown}` : ''}`
    : calibrationPhase === 'checking'
      ? t('settings.audio.checking', 'проверяем')
      : calibrationPhase === 'preparing'
        ? t('settings.audio.preparing', 'подготовка')
        : idleCalibrationText;

  const modelOptionClass =
    'group flex flex-col items-center justify-center gap-1.5 rounded-lg py-2.5 transition-colors duration-200 hover:bg-white/[0.035]';

  const modelRadioClass = (isActive: boolean) =>
    `flex h-[18px] w-[18px] items-center justify-center rounded-full border-2 transition-colors duration-200 ${isActive
      ? 'border-primaryHover'
      : 'border-white/20 group-hover:border-white/35'
    }`;

  const modelRadioDotClass = (isActive: boolean) =>
    `h-2.5 w-2.5 rounded-full bg-primaryHover transition-transform duration-200 ease-[cubic-bezier(0.34,1.56,0.64,1)] ${isActive ? 'scale-100' : 'scale-0'
    }`;

  const modelLabelClass = (isActive: boolean) =>
    `text-xs font-semibold transition-colors duration-200 ${isActive ? 'text-white' : 'text-textMuted group-hover:text-white'
    }`;

  const analyzerAvailable = !isEnabled || mode === 'smart';

  return (
    <div className={`space-y-3 ${className}`}>
      <section
        className={`rounded-2xl border glass-row transition-[border-color,background-color] duration-300 ${ultraLowLatency ? 'border-primary/50' : 'border-white/[0.07] hover:border-white/[0.13]'
          }`}
      >
        <div className="flex min-h-[66px] items-center justify-between gap-3 px-4 py-3">
          <div className="min-w-0">
            <div className="text-[15px] font-bold tracking-wide text-white">
              {t('settings.audio.ultraLowLatency', 'ультранизкая задержка')}
            </div>
            <p className="mt-1 text-xs leading-snug text-textMuted">
              {t('settings.audio.ultraLowLatencyDesc', 'минимально возможная задержка голоса без тяжёлых фильтров и обработок.')}
            </p>
          </div>
          <Md3Switch checked={ultraLowLatency} onChange={onUltraLowLatencyChange} />
        </div>
      </section>

      <div className={ultraLowLatency ? 'pointer-events-none opacity-40 space-y-3' : 'space-y-3'}>
      <section
        aria-disabled={!analyzerAvailable}
        className={`rounded-2xl border glass-row transition-[border-color,background-color,opacity] duration-300 ${speechAnalyzer && analyzerAvailable ? 'border-white/[0.13]' : 'border-white/[0.07]'
          } ${analyzerAvailable ? 'hover:border-white/[0.13]' : 'pointer-events-none opacity-45'}`}
      >
        <div className="flex min-h-[66px] items-center justify-between gap-3 px-4 py-3">
          <div className="min-w-0">
            <div className="text-[15px] font-bold tracking-wide text-white">
              {t('settings.audio.speechAnalyzer', 'анализатор речи (нестабильно)')}
            </div>
            <p className="mt-1 text-xs leading-snug text-textMuted">
              {analyzerAvailable
                ? t('settings.audio.speechAnalyzerHint', 'нейросеть отделяет голос от других звуков, может плохо работать с некоторыми микрофонами, немного увеличивает задержку')
                : t('settings.audio.speechAnalyzerManualHint', 'доступен только в умном режиме — в режиме «ручное» речь определяет ваш порог')}
            </p>
          </div>
          <Md3Switch checked={speechAnalyzer} onChange={onSpeechAnalyzerChange} />
        </div>
      </section>

      <section
        className={`rounded-2xl border glass-row transition-[border-color,background-color] duration-300 ${echoCancellation ? 'border-white/[0.13]' : 'border-white/[0.07] hover:border-white/[0.13]'
          }`}
      >
        <div className="flex min-h-[66px] items-center justify-between px-4">
          <span className="text-[15px] font-bold tracking-wide text-white">{t('settings.audio.echoCancellation', 'эхоподавление')}</span>
          <Md3Switch checked={echoCancellation} onChange={onEchoCancellationChange} />
        </div>
      </section>

      <section
        className={`overflow-hidden rounded-2xl border glass-row transition-[border-color,background-color] duration-300 ${isEnabled ? 'border-white/[0.13]' : 'border-white/[0.07] hover:border-white/[0.13]'
          }`}
      >
        <div className="flex min-h-[66px] items-center justify-between px-4">
          <span className="text-[15px] font-bold tracking-wide text-white">{t('settings.audio.noiseSuppression', 'шумоподавление')}</span>
          <Md3Switch checked={isEnabled} onChange={onEnabledChange} />
        </div>

        <div
          aria-hidden={!isEnabled}
          className={`grid transition-[grid-template-rows,opacity] duration-300 ease-[cubic-bezier(0.4,0,0.2,1)] ${isEnabled ? 'grid-rows-[1fr] opacity-100' : 'pointer-events-none grid-rows-[0fr] opacity-0'
            }`}
        >
          <div className="min-h-0 overflow-hidden">
            <div className="space-y-4 border-t border-white/[0.07] px-4 pb-4 pt-3">
              <div className="rounded-xl bg-black/30 p-1">
                <div className="grid grid-cols-2 gap-1" role="radiogroup" aria-label={t('settings.audio.noiseSuppressionMode', 'режим шумоподавления')}>
                  <button
                    type="button"
                    role="radio"
                    aria-checked={mode === 'smart'}
                    onClick={() => onModeChange('smart')}
                    className={`flex h-10 items-center justify-center gap-2 rounded-lg text-sm font-semibold transition-colors duration-200 ${mode === 'smart'
                      ? 'bg-white/[0.09] text-white'
                      : 'text-textMuted hover:bg-white/[0.035] hover:text-white'
                      }`}
                  >
                    <Sparkle size={16} weight={mode === 'smart' ? 'fill' : 'regular'} className={mode === 'smart' ? 'text-primaryHover' : ''} />
                    {t('settings.audio.smartMode', 'умное')}
                  </button>
                  <button
                    type="button"
                    role="radio"
                    aria-checked={mode === 'manual'}
                    onClick={() => onModeChange('manual')}
                    className={`flex h-10 items-center justify-center gap-2 rounded-lg text-sm font-semibold transition-colors duration-200 ${mode === 'manual'
                      ? 'bg-white/[0.09] text-white'
                      : 'text-textMuted hover:bg-white/[0.035] hover:text-white'
                      }`}
                  >
                    <Sliders size={16} weight="bold" className={mode === 'manual' ? 'text-primaryHover' : ''} />
                    {t('settings.audio.manualMode', 'ручное')}
                  </button>
                </div>

                <div
                  aria-hidden={mode !== 'smart'}
                  className={`grid transition-[grid-template-rows,opacity] duration-300 ease-[cubic-bezier(0.4,0,0.2,1)] ${mode === 'smart'
                    ? 'grid-rows-[1fr] opacity-100'
                    : 'pointer-events-none grid-rows-[0fr] opacity-0'
                    }`}
                >
                  <div className="min-h-0 overflow-hidden">
                    <div className="grid grid-cols-2 gap-1 pt-1" role="radiogroup" aria-label={t('settings.audio.noiseModel', 'модель шумоподавления')}>
                      <button
                        type="button"
                        role="radio"
                        aria-checked={smartModel === 'deepfilter'}
                        tabIndex={mode === 'smart' ? 0 : -1}
                        onClick={() => onSmartModelChange('deepfilter')}
                        className={modelOptionClass}
                      >
                        <span className={modelRadioClass(smartModel === 'deepfilter')}>
                          <span className={modelRadioDotClass(smartModel === 'deepfilter')} />
                        </span>
                        <span className={modelLabelClass(smartModel === 'deepfilter')}>
                          {t('settings.audio.model1', 'модель 1')}
                        </span>
                      </button>
                      <button
                        type="button"
                        role="radio"
                        aria-checked={smartModel === 'rnnoise'}
                        tabIndex={mode === 'smart' ? 0 : -1}
                        onClick={() => onSmartModelChange('rnnoise')}
                        className={modelOptionClass}
                      >
                        <span className={modelRadioClass(smartModel === 'rnnoise')}>
                          <span className={modelRadioDotClass(smartModel === 'rnnoise')} />
                        </span>
                        <span className={modelLabelClass(smartModel === 'rnnoise')}>
                          {t('settings.audio.model2', 'модель 2')}
                        </span>
                      </button>
                    </div>
                  </div>
                </div>
              </div>

              {mode === 'smart' && (smartModel === 'deepfilter' || VAD_CALIBRATION_ENABLED) && (
                <div className="space-y-3">
                  {smartModel === 'deepfilter' && (
                    <div className="space-y-2">
                      <div className="px-0.5 text-xs">
                        <span className="font-semibold text-textMuted">{t('settings.audio.suppressionStrength', 'сила подавления')}</span>
                      </div>
                      <div className="relative flex h-9 items-center">
                        <div
                          className="pointer-events-none absolute top-1/2 h-1.5 -translate-y-1/2 overflow-hidden rounded-full bg-white/[0.08]"
                          style={{ left: '9px', right: '9px' }}
                        >
                          <div
                            className="h-full rounded-full bg-primaryHover"
                            style={{
                              width: `${((strength - MIN_SUPPRESSION_STRENGTH_DB) /
                                (MAX_SUPPRESSION_STRENGTH_DB - MIN_SUPPRESSION_STRENGTH_DB)) * 100}%`
                            }}
                          />
                        </div>
                        <input
                          type="range"
                          min={MIN_SUPPRESSION_STRENGTH_DB}
                          max={MAX_SUPPRESSION_STRENGTH_DB}
                          step={1}
                          value={strength}
                          onChange={(event) => onSuppressionStrengthChange(Number(event.target.value))}
                          aria-label={t('settings.audio.suppressionStrength', 'сила подавления')}
                          className="threshold-range absolute inset-0 h-full w-full cursor-pointer appearance-none bg-transparent outline-none"
                        />
                      </div>
                      <div className="flex items-center justify-between px-0.5 text-[11px] font-medium text-textMuted">
                        <span>{t('settings.audio.suppressionStrengthWeaker', 'слабже')}</span>
                        <span className="font-bold text-white">{strength} dB</span>
                        <span>{t('settings.audio.suppressionStrengthStronger', 'сильнее')}</span>
                      </div>
                    </div>
                  )}

                  {VAD_CALIBRATION_ENABLED && (
                    <button
                      type="button"
                      onClick={() => !isCalibrating && !calibrationSuccess && onStartCalibration?.()}
                      disabled={isCalibrating || calibrationSuccess}
                      className={`relative flex min-h-10 w-full items-center justify-center overflow-hidden rounded-xl border px-3 py-2 text-xs font-bold transition-all duration-200 ${isCalibrating
                        ? 'cursor-default border-primary/25 bg-primary/10 text-primaryText'
                        : calibrationSuccess
                          ? 'cursor-default border-primaryHover/40 bg-primaryHover/15 text-white select-none'
                          : 'border-primary bg-primary/90 text-white hover:bg-primaryHover active:scale-[0.99]'
                        }`}
                    >
                      {isCalibrating ? (
                        <div className="flex items-center justify-center gap-2">
                          <span className="h-3.5 w-3.5 shrink-0 animate-spin rounded-full border-2 border-primaryText border-t-transparent" />
                          <span className="text-center leading-snug">{calibrationText}</span>
                        </div>
                      ) : calibrationSuccess ? (
                        <>
                          <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
                            <Check size={52} weight="bold" className="animate-checkmark-pop text-primaryHover" />
                          </div>
                          <span className="relative z-10 font-bold tracking-wide text-white">{t('settings.audio.calibrationSuccess', 'успешно')}</span>
                        </>
                      ) : (
                        <span>{idleCalibrationText}</span>
                      )}
                    </button>
                  )}
                </div>
              )}

              {mode === 'manual' && (
                <div className="space-y-2">
                  <div className="px-0.5 text-xs">
                    <span className="font-semibold text-textMuted">{t('settings.audio.threshold', 'порог')}</span>
                  </div>
                  <div className="relative flex h-9 items-center">
                    <div className="absolute grid h-5 grid-cols-[repeat(32,minmax(0,1fr))] items-center gap-[2px]" style={{ left: '9px', right: '9px' }} aria-hidden="true">
                      {Array.from({ length: METER_SEGMENTS }, (_, index) => {
                        const isActive = ((index + 0.5) / METER_SEGMENTS) * 100 <= meterPosition;
                        return (
                          <span
                            key={index}
                            className={`h-full rounded-[2px] transition-colors duration-75 ${isActive
                              ? index > 26 ? 'bg-[#ef4444]' : 'bg-[#22c55e]'
                              : 'bg-white/[0.08]'
                              }`}
                          />
                        );
                      })}
                    </div>
                    <input
                      type="range"
                      min={MIN_THRESHOLD_DB}
                      max={MAX_THRESHOLD_DB}
                      step={1}
                      value={threshold}
                      onChange={(event) => onManualThresholdChange(Number(event.target.value))}
                      aria-label={t('settings.audio.thresholdLabel', 'порог срабатывания микрофона')}
                      className="threshold-range absolute inset-0 h-full w-full cursor-pointer appearance-none bg-transparent outline-none"
                    />
                  </div>
                  <div className="flex items-center justify-between px-0.5 text-[11px] font-medium text-textMuted">
                    <span>{MIN_THRESHOLD_DB} dB</span>
                    <span className="font-bold text-white">{threshold} dB</span>
                    <span>{MAX_THRESHOLD_DB} dB</span>
                  </div>
                </div>
              )}
            </div>
          </div>
        </div>
      </section>
      </div>
    </div>
  );
}
