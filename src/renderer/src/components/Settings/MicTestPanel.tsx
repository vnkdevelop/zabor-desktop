import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Pause, Play } from '@phosphor-icons/react';
import { MIC_TEST_SILENCE_DBFS, webrtc } from '../../services/webrtc';
import { animate, utils, springs, prefersReducedMotion } from '../../lib/anime';

export interface MicTestPanelProps {
  isEnabled: boolean;
  isActive: boolean;
  onHeightChange?: (height: number) => void;
}

type MicTestStage = 'idle' | 'arming' | 'recording' | 'ready';
type MicTestIssue = 'silent' | 'failed' | null;

export const MIC_TEST_PANEL_GAP_PX = 12;

const SEEK_STEP_SECONDS = 0.5;

export function MicTestPanel({ isEnabled, isActive, onHeightChange }: MicTestPanelProps) {
  const { t } = useTranslation();
  const [slidOut, setSlidOut] = useState(false);
  const [stage, setStage] = useState<MicTestStage>('idle');
  const [issue, setIssue] = useState<MicTestIssue>(null);
  const [duration, setDuration] = useState(0);
  const [position, setPosition] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [scrubbing, setScrubbing] = useState(false);
  const trackRef = useRef<HTMLDivElement>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const recordBtnRef = useRef<HTMLButtonElement>(null);
  const dotRef = useRef<HTMLSpanElement>(null);
  const thumbRef = useRef<HTMLSpanElement>(null);
  const pulseAnimRef = useRef<ReturnType<typeof animate> | null>(null);
  const dotAnimRef = useRef<ReturnType<typeof animate> | null>(null);

  const isVisible = isEnabled && isActive;
  const visibleRef = useRef(isVisible);
  visibleRef.current = isVisible;

  useEffect(() => {
    const root = rootRef.current;
    if (!root || !onHeightChange) return;
    const report = () => onHeightChange(root.offsetHeight);
    report();
    const observer = new ResizeObserver(report);
    observer.observe(root);
    return () => observer.disconnect();
  }, [onHeightChange]);

  useEffect(() => {
    if (!isVisible) {
      setSlidOut(false);
      return;
    }
    void webrtc.prepareMicTest();
    let inner = 0;
    const outer = requestAnimationFrame(() => {
      inner = requestAnimationFrame(() => setSlidOut(true));
    });
    return () => {
      cancelAnimationFrame(outer);
      cancelAnimationFrame(inner);
    };
  }, [isVisible]);

  useEffect(() => {
    webrtc.onMicTestEnded(() => {
      setPlaying(false);
      setPosition(webrtc.getMicTestDuration());
    });
    return () => {
      webrtc.onMicTestEnded(null);
      webrtc.disposeMicTest();
    };
  }, []);

  useEffect(() => {
    if (isActive) return;
    webrtc.pauseMicTest();
    setPlaying(false);
  }, [isActive]);

  useEffect(() => {
    if (isEnabled) return;
    webrtc.disposeMicTest();
    setPlaying(false);
    setScrubbing(false);
    setPosition(0);
    setDuration(0);
    setIssue(null);
    setStage('idle');
  }, [isEnabled]);

  useEffect(() => {
    if (!playing || scrubbing) return;
    let frame = requestAnimationFrame(function tick() {
      setPosition(webrtc.getMicTestPosition());
      frame = requestAnimationFrame(tick);
    });
    return () => cancelAnimationFrame(frame);
  }, [playing, scrubbing]);

  const startRecording = useCallback(async () => {
    if (webrtc.isMicTestRecording()) return;
    webrtc.pauseMicTest();
    setPlaying(false);
    setScrubbing(false);
    setPosition(0);
    setDuration(0);
    setIssue(null);
    setStage('arming');
    try {
      const clip = await webrtc.recordMicTest(() => setStage('recording'));
      setDuration(clip.durationSeconds);
      setStage('ready');
      if (clip.peakDb <= MIC_TEST_SILENCE_DBFS) setIssue('silent');
      if (visibleRef.current && webrtc.playMicTest(0)) setPlaying(true);
    } catch (error) {
      if ((error as Error)?.message === 'MIC_TEST_CANCELLED') return;
      console.warn('[MicTest] Recording failed:', error);
      setIssue('failed');
      setStage('idle');
    }
  }, []);

  const togglePlayback = useCallback(() => {
    if (playing) {
      webrtc.pauseMicTest();
      setPosition(webrtc.getMicTestPosition());
      setPlaying(false);
      return;
    }
    if (webrtc.playMicTest()) {
      setPosition(webrtc.getMicTestPosition());
      setPlaying(true);
    }
  }, [playing]);

  const secondsFromClientX = useCallback((clientX: number) => {
    const rect = trackRef.current?.getBoundingClientRect();
    if (!rect || rect.width <= 0) return 0;
    const ratio = (clientX - rect.left) / rect.width;
    return Math.max(0, Math.min(1, ratio)) * webrtc.getMicTestDuration();
  }, []);

  const popThumb = useCallback((to: number) => {
    const el = thumbRef.current;
    if (!el) return;
    if (prefersReducedMotion()) {
      utils.set(el, { scale: to });
      return;
    }
    animate(el, { scale: to, ease: springs.press });
  }, []);

  const handleSeekPointerDown = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    if (webrtc.getMicTestDuration() <= 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    setScrubbing(true);
    setPosition(secondsFromClientX(event.clientX));
    popThumb(1.5);
  }, [secondsFromClientX, popThumb]);

  const handleSeekPointerMove = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    if (!scrubbing) return;
    setPosition(secondsFromClientX(event.clientX));
  }, [scrubbing, secondsFromClientX]);

  const handleSeekPointerUp = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    if (!scrubbing) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    const target = secondsFromClientX(event.clientX);
    setScrubbing(false);
    setPosition(target);
    webrtc.seekMicTest(target);
    setPlaying(webrtc.isMicTestPlaying());
    popThumb(1);
  }, [scrubbing, secondsFromClientX, popThumb]);

  const handleSeekKeyDown = useCallback((event: React.KeyboardEvent<HTMLDivElement>) => {
    const total = webrtc.getMicTestDuration();
    if (total <= 0) return;
    const step = event.key === 'ArrowLeft' ? -SEEK_STEP_SECONDS : event.key === 'ArrowRight' ? SEEK_STEP_SECONDS : 0;
    if (step === 0) return;
    event.preventDefault();
    const target = Math.max(0, Math.min(total, webrtc.getMicTestPosition() + step));
    webrtc.seekMicTest(target);
    setPosition(target);
    setPlaying(webrtc.isMicTestPlaying());
  }, []);

  useEffect(() => {
    const btn = recordBtnRef.current;
    if ((stage === 'arming' || stage === 'recording') && btn && !prefersReducedMotion()) {
      pulseAnimRef.current = animate(btn, { scale: [1, 1.03], loop: true, alternate: true, duration: 900, ease: 'inOutQuad' });
    }
    return () => {
      pulseAnimRef.current?.pause();
      pulseAnimRef.current = null;
      if (btn) btn.style.transform = '';
    };
  }, [stage]);

  useEffect(() => {
    const dot = dotRef.current;
    if (stage === 'recording' && dot && !prefersReducedMotion()) {
      dotAnimRef.current = animate(dot, { scale: [1, 1.55], opacity: [1, 0.4], loop: true, alternate: true, duration: 760, ease: 'inOutSine' });
    }
    return () => {
      dotAnimRef.current?.pause();
      dotAnimRef.current = null;
      if (dot) utils.set(dot, { scale: 1, opacity: 1 });
    };
  }, [stage]);

  useEffect(() => {
    if (stage === 'ready' && thumbRef.current) {
      if (prefersReducedMotion()) utils.set(thumbRef.current, { scale: 1 });
      else animate(thumbRef.current, { scale: [0, 1], ease: springs.pop });
    }
  }, [stage]);

  const progress = duration > 0 ? Math.max(0, Math.min(100, (position / duration) * 100)) : 0;
  const issueText = issue === 'silent'
    ? t('settings.audio.micTestSilent', 'микрофон молчит')
    : issue === 'failed'
      ? t('settings.audio.micTestFailed', 'не удалось записать')
      : '';

  return (
    <div
      ref={rootRef}
      aria-hidden={!isVisible}
      style={{ marginTop: MIC_TEST_PANEL_GAP_PX }}
      className={`absolute left-1/2 top-full w-[156px] -translate-x-1/2 transition-[transform,opacity] duration-[420ms] ease-[cubic-bezier(0.16,1,0.3,1)] ${slidOut ? 'translate-y-0 opacity-100' : 'pointer-events-none translate-y-[calc(-100%-2rem)] opacity-0'
        }`}
    >
      {stage === 'ready' ? (
        <div className="flex h-9 items-center overflow-hidden rounded-xl border border-white/[0.07] border-t-white/[0.14] bg-panelBg/75 backdrop-blur-xl">
          <button
            type="button"
            onClick={togglePlayback}
            tabIndex={isVisible ? 0 : -1}
            aria-label={playing
              ? t('settings.audio.micTestPause', 'пауза')
              : t('settings.audio.micTestPlay', 'воспроизвести')}
            className="flex h-9 w-9 shrink-0 items-center justify-center text-white transition-colors duration-200 hover:bg-white/[0.06]"
          >
            {playing ? <Pause size={14} weight="fill" /> : <Play size={14} weight="fill" />}
          </button>
          <div
            role="slider"
            tabIndex={isVisible ? 0 : -1}
            aria-label={t('settings.audio.micTestSeek', 'позиция воспроизведения')}
            aria-valuemin={0}
            aria-valuemax={Math.round(duration * 10) / 10}
            aria-valuenow={Math.round(position * 10) / 10}
            onPointerDown={handleSeekPointerDown}
            onPointerMove={handleSeekPointerMove}
            onPointerUp={handleSeekPointerUp}
            onPointerCancel={handleSeekPointerUp}
            onKeyDown={handleSeekKeyDown}
            className="flex h-9 flex-1 cursor-pointer touch-none items-center pl-0.5 pr-3 outline-none"
          >
            <div ref={trackRef} className="relative h-1 w-full rounded-full bg-white/[0.12]">
              <div
                className="absolute inset-y-0 left-0 rounded-full bg-primaryHover"
                style={{ width: `${progress}%` }}
              />
              <span
                className="absolute top-1/2 h-2.5 w-2.5 -translate-x-1/2 -translate-y-1/2"
                style={{ left: `${progress}%` }}
              >
                <span ref={thumbRef} className="block h-full w-full rounded-full bg-white" style={{ willChange: 'transform' }} />
              </span>
            </div>
          </div>
        </div>
      ) : (
        <button
          ref={recordBtnRef}
          type="button"
          onClick={() => void startRecording()}
          disabled={stage !== 'idle'}
          tabIndex={isVisible ? 0 : -1}
          style={{ willChange: 'transform' }}
          className={`flex h-9 w-full items-center justify-center gap-1.5 rounded-xl text-xs font-bold transition-colors duration-200 ${stage === 'idle'
            ? 'bg-primary/90 text-white hover:bg-primaryHover active:scale-[0.99]'
            : 'cursor-default bg-primary/20 text-primaryText'
            }`}
        >
          {stage === 'recording' && <span ref={dotRef} className="h-2 w-2 shrink-0 rounded-full bg-primaryHover" style={{ willChange: 'transform, opacity' }} />}
          <span>
            {stage === 'recording'
              ? t('settings.audio.micTestTalk', 'болтайте')
              : stage === 'arming'
                ? t('settings.audio.micTestArming', 'подготовка…')
                : t('settings.audio.micTestListen', 'послушать себя')}
          </span>
        </button>
      )}

      {stage === 'ready' && (
        <button
          type="button"
          onClick={() => void startRecording()}
          tabIndex={isVisible ? 0 : -1}
          className="mx-auto mt-1.5 block rounded-md border border-white/[0.06] bg-panelBg/70 backdrop-blur-md px-2 py-0.5 text-[10px] font-semibold text-textMuted transition-colors duration-200 hover:bg-white/[0.09] hover:text-white"
        >
          {t('settings.audio.micTestRetake', 'перезаписать?')}
        </button>
      )}

      {issueText && (
        <p className="mt-1.5 text-center text-[10px] font-medium leading-snug text-primaryText">{issueText}</p>
      )}
    </div>
  );
}
