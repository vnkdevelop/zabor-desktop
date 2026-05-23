import { useState, useEffect, useRef, useMemo, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { Gear as Settings, Microphone as Mic, MicrophoneSlash as MicOff, Headphones, PhoneCall as Phone, Eye, EyeSlash as EyeOff, UserMinus, UserMinus as UserX, Camera, Check, X, SignOut as LogOut, UserPlus, Envelope as Mail, PencilSimple as Edit2, SpeakerHigh as Volume2, PhoneDisconnect as PhoneOff, WifiHigh as Wifi, WifiSlash as WifiOff, Users, SignOut as LeaveIcon, Crown, Globe, Trophy, Plus } from '@phosphor-icons/react';
import { useTranslation, Trans } from 'react-i18next';

import { useAppStore, User, VoiceChannel } from './store/useAppStore';
import { signalRService } from './services/signalr';
import { webrtc } from './services/webrtc';
import { isPackedGif, packGif, unpackGif, getDisplaySrc, getStaticFrameSync, preloadStaticFrame } from './utils/avatar';

import { ACHIEVEMENTS, getAchievementDef, formatProgress, AchievementsPayload, getProgressPercent } from './achievements';
import { translateJoke } from './utils/jokesTranslation';

import { TitleBar } from './components/Layout/TitleBar';
import { Md3Slider } from './components/Shared/Md3Slider';
import { Md3Switch } from './components/Shared/Md3Switch';
import { AvatarImg } from './components/Shared/AvatarImg';

// === Main App ===
export default function App() {
  const { t, i18n } = useTranslation();
  const store = useAppStore();

  const [isAuth, setIsAuth] = useState(false);
  const [language, setLanguage] = useState(i18n.language || 'ru');
  const [isLoading, setIsLoading] = useState(false);
  const [activeTab, setActiveTab] = useState<'channels' | 'friends'>('channels');

  const [serverConnected, setServerConnected] = useState(false);
  const [showErrorText, setShowErrorText] = useState(false);
  const [showInitConnectionError, setShowInitConnectionError] = useState(false);
  const [showReconnectingOverlay, setShowReconnectingOverlay] = useState(false);
  const [appLoading, setAppLoading] = useState(true);
  const [loadingFadeOut, setLoadingFadeOut] = useState(false);
  const disconnectTimerRef = useRef<NodeJS.Timeout | null>(null);

  const [login, setLogin] = useState('');
  const [password, setPassword] = useState('');
  const [showPassword, setShowPassword] = useState(false);
  const [authStep, setAuthStep] = useState<'login' | 'confirm' | 'setup'>('login');
  const [error, setError] = useState('');

  const [displayName, setDisplayName] = useState('');
  const [avatarBase64, setAvatarBase64] = useState<string | null>(null);
  const [avatarColor, setAvatarColor] = useState<string>('#c70060');
  const [editProfileDisplayName, setEditProfileDisplayName] = useState('');
  const [editProfileAvatarBase64, setEditProfileAvatarBase64] = useState<string | null>(null);
  const [editProfileAvatarColor, setEditProfileAvatarColor] = useState<string>('#c70060');
  const [editProfileAboutMe, setEditProfileAboutMe] = useState('');
  const [isEditingProfile, setIsEditingProfile] = useState(false);
  const [isLoginCopied, setIsLoginCopied] = useState(false);

  const [newChannelName, setNewChannelName] = useState('');
  const [editChannelName, setEditChannelName] = useState('');
  const [editChannelId, setEditChannelId] = useState<string | null>(null);

  const [friendName, setFriendName] = useState('');
  const [friendRequestStatus, setFriendRequestStatus] = useState<'idle' | 'loading' | 'sent' | 'notfound' | 'alreadyfriend'>('idle');
  const [profileFriendRequestStatus, setProfileFriendRequestStatus] = useState<'idle' | 'loading' | 'sent'>('idle');
  const [newPassword, setNewPassword] = useState('');
  const [showPrivacyPass, setShowPrivacyPass] = useState(false);
  const [privacyError, setPrivacyError] = useState('');
  const [offlineToast, setOfflineToast] = useState<string | null>(null);

  const [volumeUser, setVolumeUser] = useState<User | null>(null);
  const [volumeUserValue, setVolumeUserValue] = useState<number>(100);

  const [inputVolume, setInputVolume] = useState(100);
  const [outputVolume, setOutputVolume] = useState(100);
  const [audioDevices, setAudioDevices] = useState<{ inputs: MediaDeviceInfo[], outputs: MediaDeviceInfo[] }>({ inputs: [], outputs: [] });
  const [selectedInput, setSelectedInput] = useState('default');
  const [selectedOutput, setSelectedOutput] = useState('default');
  const [noiseSuppression, setNoiseSuppression] = useState(true);
  const [isSwitchingChannel, setIsSwitchingChannel] = useState(false);
  const [autoLaunch, setAutoLaunch] = useState(false);
  const [minimizeToTray, setMinimizeToTray] = useState(true);

  const [contextMenu, setContextMenu] = useState<{
    visible: boolean; x: number; y: number;
    type: 'channel' | 'friend' | 'voiceUser' | 'channelMember'; item: any;
  } | null>(null);
  const [showInvitesPanel, setShowInvitesPanel] = useState(false);
  const [settingsTab, setSettingsTab] = useState<'general' | 'audio' | 'privacy'>('general');
  const [isCalibrating, setIsCalibrating] = useState(false);
  const [calibrationCountdown, setCalibrationCountdown] = useState(10);
  const [calibrationSuccess, setCalibrationSuccess] = useState(false);
  const [inviteFriendSearch, setInviteFriendSearch] = useState('');
  const [sentInvites, setSentInvites] = useState<Set<string>>(new Set());

  const addSentInvite = useCallback((userId: string) => {
    setSentInvites(prev => new Set(prev).add(userId));
    setTimeout(() => {
      setSentInvites(prev => {
        const next = new Set(prev);
        next.delete(userId);
        return next;
      });
    }, 30000);
  }, []);

  useEffect(() => {
    if (!store.incomingChannelInvite) return;
    const timer = setTimeout(() => {
      const inviteName = useAppStore.getState().incomingChannelInvite?.channelName;
      store.setModal('incomingChannelInvite', false);
      store.setIncomingChannelInvite(null);
      signalRService.stopRingtone();
      if (inviteName) {
        useAppStore.getState().setSystemToast(t('toasts.missedChannelInvite', { name: inviteName, defaultValue: `Пропущенный зов в канал: ${inviteName}` }));
        setTimeout(() => useAppStore.getState().setSystemToast(null), 4000);
      }
    }, 30000);
    return () => clearTimeout(timer);
  }, [store.incomingChannelInvite]);

  useEffect(() => {
    if (!store.incomingCall) return;
    const timer = setTimeout(() => {
      const callerName = useAppStore.getState().incomingCall?.callerName;
      store.setModal('incomingCall', false);
      store.setIncomingCall(null);
      signalRService.stopRingtone();
      if (callerName) {
        useAppStore.getState().setSystemToast(t('toasts.missedCall', { name: callerName, defaultValue: `Пропущенный звонок от: ${callerName}` }));
        setTimeout(() => useAppStore.getState().setSystemToast(null), 4000);
      }
    }, 30000);
    return () => clearTimeout(timer);
  }, [store.incomingCall]);



  useEffect(() => {
    if (store.callStatus !== 'calling') return;
    const timer = setTimeout(() => {
      signalRService.endCall();
      setOfflineToast(t('toasts.noAnswer', 'Не отвечает'));
      setTimeout(() => setOfflineToast(null), 4000);
    }, 30000);
    return () => clearTimeout(timer);
  }, [store.callStatus]);

  const [showCropper, setShowCropper] = useState(false);
  const [cropImageSrc, setCropImageSrc] = useState<string | null>(null);
  const [cropScale, setCropScale] = useState(1);
  const [cropPos, setCropPos] = useState({ x: 0, y: 0 });
  const [isDragging, setIsDragging] = useState(false);
  const [dragStart, setDragStart] = useState({ x: 0, y: 0 });
  const imgRef = useRef<HTMLImageElement>(null);
  const profileFileInputRef = useRef<HTMLInputElement>(null);
  const [cropContext, setCropContext] = useState<'setup' | 'profile'>('setup');
  const [cropGifDataUrl, setCropGifDataUrl] = useState<string | null>(null);

  const [ping, setPing] = useState<number>(-1);
  const [showPingTooltip, setShowPingTooltip] = useState(false);
  const [isCopied, setIsCopied] = useState(false);

  const [isIdle, setIsIdle] = useState(false);
  const [joke, setJoke] = useState<string>('');
  const idleTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const settingsSaveTimerRef = useRef<NodeJS.Timeout | null>(null);
  const settingsLoadedRef = useRef(false);
  const credentialsRef = useRef<{ login: string; password: string }>({ login: '', password: '' });
  const initCompleteRef = useRef(false);
  /** true когда у нас есть сохранённые креды, но автологин ещё не выполнен (сервер недоступен или вернул ошибку) */
  const autoLoginPendingRef = useRef(false);
  const loginInputRef = useRef<HTMLInputElement>(null);
  const passwordInputRef = useRef<HTMLInputElement>(null);



  const [controlsShake, setControlsShake] = useState(false);
  const [adminBlockToast, setAdminBlockToast] = useState<string | null>(null);
  const adminBlockTimerRef = useRef<NodeJS.Timeout | null>(null);
  const settingsRef = useRef({
    inputVolume: 100, outputVolume: 100,
    selectedInput: 'default', selectedOutput: 'default',
    noiseSuppression: true, language: i18n.language || 'ru',
    openAtLogin: false, minimizeToTray: true
  });

  useEffect(() => {
    settingsRef.current = { inputVolume, outputVolume, selectedInput, selectedOutput, noiseSuppression, language, openAtLogin: autoLaunch, minimizeToTray };
  }, [inputVolume, outputVolume, selectedInput, selectedOutput, noiseSuppression, language, autoLaunch, minimizeToTray]);

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      const glow = document.getElementById('mouse-glow');
      if (glow) {
        glow.style.setProperty('--mouse-x', `${e.clientX}px`);
        glow.style.setProperty('--mouse-y', `${e.clientY}px`);
        glow.style.opacity = '1';
      }
    };

    const handleMouseLeave = () => {
      const glow = document.getElementById('mouse-glow');
      if (glow) {
        glow.style.opacity = '0';
      }
    };

    window.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseleave', handleMouseLeave);

    return () => {
      window.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseleave', handleMouseLeave);
    };
  }, []);

  // === Callbacks defined early to avoid TDZ in useEffect deps ===

  const saveLocalCache = useCallback(() => {
    try {
      const currentUser = useAppStore.getState().currentUser;
      const creds = credentialsRef.current;
      if (!currentUser || !creds.login || !creds.password) return;
      const data = JSON.stringify({
        login: creds.login,
        password: creds.password,
        userId: currentUser.id,
        settings: {
          inputVolume: settingsRef.current.inputVolume,
          outputVolume: settingsRef.current.outputVolume,
          selectedInput: settingsRef.current.selectedInput,
          selectedOutput: settingsRef.current.selectedOutput,
          noiseSuppression: settingsRef.current.noiseSuppression,
          language: settingsRef.current.language,
          openAtLogin: settingsRef.current.openAtLogin,
          minimizeToTray: settingsRef.current.minimizeToTray
        }
      });
      window.windowControls.saveSession(data).catch(() => { });
    } catch { }
  }, []);

  const softClearCache = useCallback(() => {
  }, []);

  const deepWipeOnLogout = useCallback(async () => {
    try {
      await window.windowControls.clearSession();
      await window.windowControls.wipeAppData();
    } catch { }
    await new Promise(r => setTimeout(r, 300));
  }, []);

  const resetToDefaults = useCallback(() => {
    setInputVolume(100);
    setOutputVolume(100);
    setSelectedInput('default');
    setSelectedOutput('default');
    setNoiseSuppression(true);
    setDisplayName('');
    setAvatarBase64(null);
    setAvatarColor('#c70060');
    setEditProfileAvatarBase64(null);
    setEditProfileAvatarColor('#c70060');
    setEditProfileDisplayName('');
    setEditProfileAboutMe('');
    setIsEditingProfile(false);
    setIsLoginCopied(false);
    webrtc.setInputDevice('default');
    webrtc.setOutputDevice('default');
    localStorage.removeItem('zabor_mic_calibrated');
    localStorage.removeItem('zabor_base_noise_floor');
    localStorage.removeItem('zabor_threshold_on');
    localStorage.removeItem('zabor_threshold_off');
    localStorage.removeItem('zabor_attenuation_limit');
  }, []);

  const applySettings = useCallback((s: {
    inputVolume?: number; outputVolume?: number;
    selectedInput?: string; selectedOutput?: string;
    noiseSuppression?: boolean;
    userVolumes?: Record<string, number>;
    language?: string;
    openAtLogin?: boolean;
    minimizeToTray?: boolean;
  }) => {
    const iv = s.inputVolume ?? 100;
    const ov = s.outputVolume ?? 100;
    setInputVolume(iv);
    setOutputVolume(ov);
    setSelectedInput(s.selectedInput ?? 'default');
    setSelectedOutput(s.selectedOutput ?? 'default');
    setNoiseSuppression(s.noiseSuppression ?? true);
    webrtc.setInputDevice(s.selectedInput ?? 'default');
    webrtc.setOutputDevice(s.selectedOutput ?? 'default');
    webrtc.setInputVolume(iv);
    webrtc.setOutputVolume(ov);

    if (s.language) {
      setLanguage(s.language);
      i18n.changeLanguage(s.language);
    }

    // Восстанавливаем системные настройки из кэша сессии без лишних IPC-вызовов
    if (s.openAtLogin !== undefined) {
      setAutoLaunch(s.openAtLogin);
      window.windowControls.setAutoLaunch(s.openAtLogin).catch(() => { });
    }
    if (s.minimizeToTray !== undefined) {
      setMinimizeToTray(s.minimizeToTray);
      window.windowControls.setMinimizeToTray(s.minimizeToTray).catch(() => { });
    }

    // Восстанавливаем индивидуальные громкости пользователей
    if (s.userVolumes && typeof s.userVolumes === 'object') {
      const store = useAppStore.getState();
      Object.entries(s.userVolumes).forEach(([userId, volume]) => {
        store.setUserVolume(userId, volume);
        // Применяем к уже существующим аудио-элементам (если пользователь в канале)
        webrtc.setUserVolume(userId, volume);
      });
    }

    // Синхронно обновляем settingsRef.current для исключения гонки при вызове saveLocalCache
    settingsRef.current = {
      inputVolume: iv,
      outputVolume: ov,
      selectedInput: s.selectedInput ?? 'default',
      selectedOutput: s.selectedOutput ?? 'default',
      noiseSuppression: s.noiseSuppression ?? true,
      language: s.language ?? settingsRef.current.language,
      openAtLogin: s.openAtLogin ?? settingsRef.current.openAtLogin,
      minimizeToTray: s.minimizeToTray ?? settingsRef.current.minimizeToTray
    };
  }, []);

  const containerRef = useRef<HTMLDivElement>(null);
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 });

  useEffect(() => {
    credentialsRef.current = { login, password };
  }, [login, password]);

  useEffect(() => {
    if (!containerRef.current) return;

    let rafId: number | null = null;
    const el = containerRef.current;

    const measure = () => {
      const rect = el.getBoundingClientRect();
      if (rect.width > 0 && rect.height > 0) {
        setContainerSize(prev => {
          if (Math.abs(prev.width - rect.width) < 2 && Math.abs(prev.height - rect.height) < 2) return prev;
          return { width: rect.width, height: rect.height };
        });
      }
    };

    measure();

    const observer = new ResizeObserver(() => {
      if (rafId) cancelAnimationFrame(rafId);
      rafId = requestAnimationFrame(measure);
    });

    observer.observe(el);

    return () => {
      observer.disconnect();
      if (rafId) cancelAnimationFrame(rafId);
    };
  }, [store.currentChannelId, store.currentCallUser?.id]);

  const getCardSize = (count: number, cw: number, ch: number) => {
    if (count === 0 || cw === 0 || ch === 0) return { w: 320, h: 180, avatarSize: 96 };
    const gap = 24;
    const ratio = 16 / 9;
    let bestW = 0;
    for (let cols = 1; cols <= count; cols++) {
      const rows = Math.ceil(count / cols);
      const wCols = (cw - gap * (cols - 1)) / cols;
      const hRows = (ch - gap * (rows - 1)) / rows;
      let testW = wCols;
      let testH = testW / ratio;
      if (testH > hRows) { testH = hRows; testW = testH * ratio; }
      if (testW > bestW) bestW = testW;
    }
    const finalW = Math.max(100, Math.min(bestW, 800));
    const finalH = finalW / ratio;
    const avatarSize = Math.max(48 + 7, Math.min(120 + 7, finalH * 0.4 + 7));
    return { w: Math.floor(finalW), h: Math.floor(finalH), avatarSize: Math.floor(avatarSize) };
  };

  const activeUserCount = store.currentCallUser ? 1 : store.voiceUsers.length;

  const cardSize = useMemo(() => {
    const { w, h, avatarSize } = getCardSize(activeUserCount, containerSize.width, containerSize.height);
    return { w, h, avatarSize };
  }, [activeUserCount, containerSize.width, containerSize.height]);

  useEffect(() => {
    const unsubConnection = signalRService.onConnectionUpdate((isConnected) => {
      setServerConnected(isConnected);
      if (!initCompleteRef.current) return; // Не мешаем init-потоку

      if (isConnected) {
        // Соединение восстановлено — отменяем таймер перехода в лоадинг
        if (disconnectTimerRef.current) {
          clearTimeout(disconnectTimerRef.current);
          disconnectTimerRef.current = null;
        }
        setShowErrorText(false);
        setShowInitConnectionError(false);
        setShowReconnectingOverlay(false);

        if (autoLoginPendingRef.current) {
          // Автологин ещё не выполнен — пробуем сейчас (сервер только что стал доступен)
          autoLoginPendingRef.current = false;
          const creds = credentialsRef.current;
          if (creds.login && creds.password) {
            signalRService.login(creds.login, creds.password).then(async (result) => {
              if (result === 'ok') {
                const needsLoadSettings = !isAuth;
                const [serverSettings, jokeText] = await Promise.all([
                  needsLoadSettings ? signalRService.loadAudioSettings() : Promise.resolve(null),
                  signalRService.getJokeOfTheDay().catch(() => '__NO_JOKE__')
                ]);
                if (serverSettings) applySettings(serverSettings);
                setJoke(jokeText || '__NO_JOKE__');
                setServerConnected(true);
                setIsAuth(true);
                saveLocalCache();
                setTimeout(() => { settingsLoadedRef.current = true; }, 1000);
                setLoadingFadeOut(true);
                setTimeout(() => setAppLoading(false), 650);
              } else if (result === 'invalid') {
                // Неверные credentials — не ретраим, показываем экран логина
                autoLoginPendingRef.current = false;
                setLoadingFadeOut(true);
                setTimeout(() => setAppLoading(false), 650);
              } else {
                // 'network' — сеть упала снова, ждём следующего reconnect
                autoLoginPendingRef.current = true;
                setShowErrorText(true);
                setShowReconnectingOverlay(true);
              }
            });
          }
        } else {
          // Пользователь уже залогинен — просто скрываем лоадинг
          setLoadingFadeOut(true);
          setTimeout(() => setAppLoading(false), 650);
        }
      } else if (isAuth) {
        // СигналР сам попытается переподключиться и вызовет Login.
        // Не сбрасываем UI (appLoading), чтобы не было мигания или потери контекста.
        autoLoginPendingRef.current = true;

        // Запускаем 3-секундный таймер перед показом оверлея реконнекта
        if (!disconnectTimerRef.current) {
          disconnectTimerRef.current = setTimeout(() => {
            setShowReconnectingOverlay(true);
            setShowErrorText(true);
          }, 3000);
        }
      }
    });
    const unsubPing = signalRService.onPingUpdate((newPing) => setPing(newPing));
    return () => {
      unsubConnection();
      unsubPing();
      if (disconnectTimerRef.current) clearTimeout(disconnectTimerRef.current);
    };
  }, [isAuth, applySettings, saveLocalCache]);

  useEffect(() => {
    const init = async () => {
      // Запускаем калибровку микрофона в фоне (длительность определяется автоматически: 5с в первый раз, 2с в последующие)
      const calibrationPromise = webrtc.calibrateMic().catch(err => {
        console.warn('[Calibration] Mic calibration failed on startup:', err);
        return null;
      });

      // 1. Загружаем сессию с диска
      let cachedCredentials: { login: string; password: string; userId?: string } | null = null;

      try {
        const raw = await window.windowControls.loadSession();
        if (raw) {
          const parsed = JSON.parse(raw);
          cachedCredentials = {
            login: parsed.login,
            password: parsed.password,
            userId: parsed.userId
          };

          if (parsed.settings) applySettings(parsed.settings);

          credentialsRef.current = { login: parsed.login, password: parsed.password };
          setLogin(parsed.login);
          setPassword(parsed.password);
        }
      } catch { }

      // 2. Если нет кредов — сразу показываем экран логина
      if (!cachedCredentials) {
        await calibrationPromise;
        initCompleteRef.current = true;
        setLoadingFadeOut(true);
        setTimeout(() => setAppLoading(false), 650);
        return;
      }

      // 3. Подключаемся к серверу (с таймером на текст ошибки)
      setShowInitConnectionError(false);
      const errorTimer = setTimeout(() => setShowInitConnectionError(true), 10000);

      let connected = await signalRService.connect();
      let retries = 0;
      while (!connected && retries < 3) {
        await new Promise(r => setTimeout(r, 2000));
        connected = await signalRService.connect();
        retries++;
      }
      clearTimeout(errorTimer);

      setShowErrorText(false);
      setShowInitConnectionError(false);

      // 4. Если так и не подключились — остаёмся на экране загрузки.
      //    Сессия НЕ трогается: она валидна, просто сервер недоступен.
      //    onConnectionUpdate(true) сам запустит автологин когда сервер вернётся.
      if (!connected) {
        autoLoginPendingRef.current = true;
        await calibrationPromise;
        initCompleteRef.current = true;
        setShowInitConnectionError(true);
        return;
      }

      // 5. Автологин
      const loginResult = await signalRService.login(
        cachedCredentials.login,
        cachedCredentials.password
      );

      if (loginResult === 'ok') {
        const serverUser = useAppStore.getState().currentUser;
        if (cachedCredentials.userId && serverUser && cachedCredentials.userId !== serverUser.id) {
          resetToDefaults();
        }

        const [serverSettings, jokeText] = await Promise.all([
          signalRService.loadAudioSettings(),
          signalRService.getJokeOfTheDay().catch(() => '__NO_JOKE__')
        ]);

        if (serverSettings) applySettings(serverSettings);
        setJoke(jokeText || '__NO_JOKE__');
        setServerConnected(true);
        setIsAuth(true);

        saveLocalCache();
        setTimeout(() => { settingsLoadedRef.current = true; }, 1000);

        initCompleteRef.current = true;
        await calibrationPromise;
        setTimeout(() => {
          setLoadingFadeOut(true);
          setTimeout(() => setAppLoading(false), 650);
        }, 300);
      } else if (loginResult === 'invalid') {
        // Неверные credentials — очищаем сессию и показываем экран логина
        await window.windowControls.clearSession().catch(() => { });
        await calibrationPromise;
        initCompleteRef.current = true;
        setLoadingFadeOut(true);
        setTimeout(() => setAppLoading(false), 650);
      } else {
        // 'network' — сеть прервалась во время login(), ждём reconnect
        autoLoginPendingRef.current = true;
        await calibrationPromise;
        initCompleteRef.current = true;
        setShowErrorText(true);
        setShowReconnectingOverlay(true);
        setShowInitConnectionError(true);
      }
    };

    init();
  }, []);

  useEffect(() => {
    const resetIdle = () => {
      setIsIdle(false);
      if (idleTimeoutRef.current) clearTimeout(idleTimeoutRef.current);
      idleTimeoutRef.current = setTimeout(() => setIsIdle(true), 5000);
    };
    const setIdleTrue = () => setIsIdle(true);
    window.addEventListener('mousemove', resetIdle);
    window.addEventListener('keydown', resetIdle);
    window.addEventListener('focus', resetIdle);
    window.addEventListener('blur', setIdleTrue);
    resetIdle();
    return () => {
      window.removeEventListener('mousemove', resetIdle);
      window.removeEventListener('keydown', resetIdle);
      window.removeEventListener('focus', resetIdle);
      window.removeEventListener('blur', setIdleTrue);
      if (idleTimeoutRef.current) clearTimeout(idleTimeoutRef.current);
    };
  }, []);

  useEffect(() => {
    const handleClick = () => setContextMenu(null);
    document.addEventListener('click', handleClick);
    return () => document.removeEventListener('click', handleClick);
  }, []);

  useEffect(() => {
    const has = store.channelInvites.length > 0 || store.friendRequests.length > 0;
    if (!has) setShowInvitesPanel(false);
  }, [store.channelInvites.length, store.friendRequests.length]);

  useEffect(() => {
    Object.values(store.channelUsersMap).flat().forEach(u => preloadStaticFrame(u.avatarBase64));
    store.friends.forEach(f => preloadStaticFrame(f.avatarBase64));
    store.voiceUsers.forEach(u => preloadStaticFrame(u.avatarBase64));
    preloadStaticFrame(store.currentUser?.avatarBase64);
    preloadStaticFrame(store.currentCallUser?.avatarBase64);
  }, [store.channelUsersMap, store.friends, store.voiceUsers, store.currentUser?.avatarBase64, store.currentCallUser?.avatarBase64]);

  // (saveLocalCache, softClearCache, deepWipeOnLogout, resetToDefaults, applySettings moved above — before first useEffect)

  const userVolumesSaveTimerRef = useRef<NodeJS.Timeout | null>(null);

  useEffect(() => {
    if (!settingsLoadedRef.current || !isAuth) return;

    if (settingsSaveTimerRef.current) clearTimeout(settingsSaveTimerRef.current);
    settingsSaveTimerRef.current = setTimeout(() => {
      const s = settingsRef.current;
      signalRService.saveAudioSettings({
        inputVolume: s.inputVolume,
        outputVolume: s.outputVolume,
        selectedInput: s.selectedInput,
        selectedOutput: s.selectedOutput,
        noiseSuppression: s.noiseSuppression,
        userVolumes: useAppStore.getState().userVolumes,
        language: s.language
      });
      saveLocalCache();
    }, 500);
  }, [inputVolume, outputVolume, selectedInput, selectedOutput, noiseSuppression, isAuth, language]);

  // Сохраняем системные настройки (autoLaunch / minimizeToTray) в локальный кэш сессии при изменении
  useEffect(() => {
    if (!settingsLoadedRef.current || !isAuth) return;
    saveLocalCache();
  }, [autoLaunch, minimizeToTray, isAuth, saveLocalCache]);

  // Сохраняем индивидуальные громкости пользователей на сервере при их изменении
  useEffect(() => {
    if (!settingsLoadedRef.current || !isAuth) return;

    if (userVolumesSaveTimerRef.current) clearTimeout(userVolumesSaveTimerRef.current);
    userVolumesSaveTimerRef.current = setTimeout(() => {
      const s = settingsRef.current;
      signalRService.saveAudioSettings({
        inputVolume: s.inputVolume,
        outputVolume: s.outputVolume,
        selectedInput: s.selectedInput,
        selectedOutput: s.selectedOutput,
        noiseSuppression: s.noiseSuppression,
        userVolumes: useAppStore.getState().userVolumes
      });
    }, 800);
  }, [store.userVolumes, isAuth]);

  useEffect(() => {
    if (!isAuth || !serverConnected || joke) return;

    let cancelled = false;

    signalRService.getJokeOfTheDay().then((j: string) => {
      if (!cancelled) {
        setJoke(j || '__NO_JOKE__');
      }
    }).catch(() => {
      if (!cancelled) {
        setJoke('__NO_JOKE__');
      }
    });

    return () => {
      cancelled = true;
    };
  }, [isAuth, serverConnected, joke]);

  const closeAndResetModals = useCallback(() => {
    setNewChannelName('');
    setEditChannelName('');
    setEditChannelId(null);

    setFriendName('');
    setFriendRequestStatus('idle');
    setNewPassword('');
    setError('');
    setPrivacyError('');
    setShowPrivacyPass(false);

    setEditProfileAvatarBase64(null);
    setEditProfileAvatarColor('#c70060');
    setEditProfileAboutMe('');
    setIsEditingProfile(false);
    setIsLoginCopied(false);

    setInviteFriendSearch('');
    setSentInvites(new Set());



    setContextMenu(null);
    setShowInvitesPanel(false);

    setShowCropper(false);
    setCropGifDataUrl(null);
    setCropImageSrc(null);
    setCropScale(1);
    setCropPos({ x: 0, y: 0 });
    setIsDragging(false);

    store.closeAllModals();
  }, [store]);

  const validateInput = useCallback((str: string) => {
    if (str.length < 4) return t('validation.minChars', 'Минимум 4 символа');
    if (str.length > 25) return t('validation.maxChars', 'Максимум 25 символов');
    if (!/^[a-zA-Z0-9!@#$%^&*()_+={}\[\]:;"'<>,.?/\\|-]+$/.test(str)) return t('validation.latinOnly', 'Только латиница и цифры');
    return "";
  }, [t]);

  const validateName = useCallback((str: string) => {
    if (str.trim().length === 0) return t('validation.emptyName', 'Имя не может быть пустым');
    if (str.length > 20) return t('validation.maxNameChars', 'Максимум 20 символов');
    return "";
  }, [t]);

  const getPingColor = useCallback(() => {
    if (ping < 0) return '#ef4444';
    if (ping < 50) return '#22c55e';
    if (ping < 100) return '#84cc16';
    if (ping < 200) return '#eab308';
    return '#ef4444';
  }, [ping]);

  const handleCopyUsername = useCallback(() => {
    if (store.currentUser?.username) {
      navigator.clipboard.writeText(store.currentUser.username);
      setIsCopied(true);
      setTimeout(() => setIsCopied(false), 2000);
    }
  }, [store.currentUser?.username]);

  const handleAuth = useCallback(async () => {
    setError('');
    const loginErr = validateInput(login);
    const passErr = validateInput(password);
    if (loginErr) { setError(`${t('auth.login', 'ЛОГИН')}: ${loginErr}`); return; }
    if (passErr && authStep === 'login') { setError(`${t('auth.password', 'ПАРОЛЬ')}: ${passErr}`); return; }
    if (authStep === 'setup') {
      const nameErr = validateName(displayName);
      if (nameErr) { setError(nameErr); return; }
    }

    setIsLoading(true);
    try {
      const connected = await signalRService.connect();
      if (!connected) { setError(t('validation.connectionError', 'Ошибка подключения к серверу')); return; }

      if (authStep === 'login') {
        const exists = await signalRService.checkUserExists(login);
        if (exists) {
          settingsLoadedRef.current = false;
          resetToDefaults();

          const loginResult = await signalRService.login(login, password);
          if (loginResult === 'ok') {
            const [serverSettings, jokeText] = await Promise.all([
              signalRService.loadAudioSettings(),
              signalRService.getJokeOfTheDay().catch(() => '__NO_JOKE__')
            ]);

            if (serverSettings) applySettings(serverSettings);
            setJoke(jokeText || '__NO_JOKE__');
            setServerConnected(true);
            setIsAuth(true);
            credentialsRef.current = { login, password };

            saveLocalCache();
            setTimeout(() => { settingsLoadedRef.current = true; }, 1000);

          } else if (loginResult === 'invalid') {
            setError(t('validation.invalidPassword', 'Неверный пароль!'));
          } else {
            setError(t('validation.networkError', 'Ошибка сети, попробуйте ещё раз'));
          }
        } else {
          setAuthStep('confirm');
        }
      } else if (authStep === 'setup') {
        settingsLoadedRef.current = false;
        resetToDefaults();

        const success = await signalRService.register(
          login, password, displayName.trim(), avatarBase64, avatarColor
        );
        if (success) {
          const jokeText = await signalRService.getJokeOfTheDay().catch(() => '__NO_JOKE__');

          setJoke(jokeText || '__NO_JOKE__');
          setServerConnected(true);
          setIsAuth(true);
          credentialsRef.current = { login, password };

          saveLocalCache();
          setTimeout(() => { settingsLoadedRef.current = true; }, 1000);

        } else {
          setError(t('validation.registerError', 'Ошибка регистрации'));
        }
      }
    } catch {
      setError(t('validation.connectError', 'Ошибка подключения'));
    } finally {
      setIsLoading(false);
    }
  }, [login, password, authStep, displayName, avatarBase64, avatarColor,
    validateInput, validateName, saveLocalCache, softClearCache,
    resetToDefaults, applySettings, t]);

  const handleManualCalibration = useCallback(async () => {
    if (isCalibrating) return;

    const isStreamActive = store.currentChannelId !== null || store.callStatus !== 'idle';
    if (isStreamActive) {
      store.setSystemToast(t('toasts.micBusyCalibration', 'Нельзя калибровать микрофон во время разговора'));
      setTimeout(() => {
        const currentStore = useAppStore.getState();
        if (currentStore.systemToast === t('toasts.micBusyCalibration', 'Нельзя калибровать микрофон во время разговора')) {
          currentStore.setSystemToast(null);
        }
      }, 4000);
      return;
    }

    setIsCalibrating(true);
    setCalibrationSuccess(false);
    setCalibrationCountdown(10);

    let secondsLeft = 10;
    const interval = setInterval(() => {
      secondsLeft--;
      setCalibrationCountdown(secondsLeft);
      if (secondsLeft <= 0) {
        clearInterval(interval);
      }
    }, 1000);

    try {
      await webrtc.calibrateMic(10000);
      setCalibrationSuccess(true);
      setTimeout(() => setCalibrationSuccess(false), 4000);
    } catch (err) {
      console.warn('Manual calibration failed:', err);
      store.setSystemToast(t('toasts.calibrationFailed', 'Не удалось получить доступ к микрофону'));
      setTimeout(() => {
        const currentStore = useAppStore.getState();
        if (currentStore.systemToast === t('toasts.calibrationFailed', 'Не удалось получить доступ к микрофону')) {
          currentStore.setSystemToast(null);
        }
      }, 4000);
    } finally {
      clearInterval(interval);
      setIsCalibrating(false);
    }
  }, [isCalibrating, store, t]);

  const handleLogout = useCallback(async () => {
    settingsLoadedRef.current = false;


    closeAndResetModals();

    if (store.currentCallUser) {
      await signalRService.endCall();
    }

    if (store.currentChannelId) {
      await signalRService.leaveChannel();
    }

    webrtc.stopLocalStream();
    signalRService.disconnect();

    await deepWipeOnLogout();

    resetToDefaults();

    store.setCurrentUser(null);
    store.setChannels([]);
    store.setFriends([]);
    store.setFriendRequests([]);
    store.setChannelInvites([]);
    store.setVoiceUsers([]);
    store.setCurrentChannelId(null);
    store.setCallStatus('idle');
    store.setCurrentCallUser(null);
    store.setFullChannelState({});

    setJoke('');



    setDisplayName('');
    setAvatarBase64(null);
    setAvatarColor('#c70060');

    credentialsRef.current = { login: '', password: '' };

    setLogin('');
    setPassword('');
    setShowPassword(false);
    setError('');
    setAuthStep('login');
    setIsAuth(false);


    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        const loginInput = document.querySelector(
          'input[type="text"]'
        ) as HTMLInputElement | null;

        loginInput?.focus();
      });
    });
  }, [
    closeAndResetModals,
    deepWipeOnLogout,
    resetToDefaults,
    store.currentCallUser,
    store.currentChannelId
  ]);

  const handleAutoLaunchToggle = useCallback(async (enabled: boolean) => {
    const prev = autoLaunch;
    setAutoLaunch(enabled); // Optimistic UI
    try {
      await window.windowControls.setAutoLaunch(enabled);
    } catch {
      setAutoLaunch(prev); // Откат при ошибке
    }
  }, [autoLaunch]);

  const changePassword = useCallback(async () => {
    setPrivacyError('');
    const passErr = validateInput(newPassword);
    if (passErr) { setPrivacyError(passErr); return; }
    if (newPassword && newPassword !== password) {
      const success = await signalRService.changePassword(newPassword);
      if (success) {
        setPassword(newPassword);
        credentialsRef.current = { ...credentialsRef.current, password: newPassword };
        saveLocalCache();
        closeAndResetModals();
      }
      else setPrivacyError(t('settings.privacy.changePasswordFailed', 'Не удалось сменить пароль'));
    }
  }, [newPassword, password, validateInput, saveLocalCache, closeAndResetModals, t]);

  const saveProfileChanges = useCallback(async () => {
    const user = store.currentUser;
    if (!user) return;
    const nameErr = validateName(editProfileDisplayName);
    if (nameErr) { setError(nameErr); return; }
    const finalAvatar = editProfileAvatarBase64 ?? user.avatarBase64;
    const finalColor = editProfileAvatarBase64 ? editProfileAvatarColor : user.avatarColor;
    const updatedUser = { ...user, displayName: editProfileDisplayName.trim(), avatarBase64: finalAvatar, avatarColor: finalColor, aboutMe: editProfileAboutMe.trim() };
    store.setCurrentUser(updatedUser);
    if (store.currentChannelId) store.setVoiceUsers(store.voiceUsers.map(u => u.id === user.id ? updatedUser : u));
    store.setFriends(store.friends.map(f => f.id === user.id ? updatedUser : f));
    saveLocalCache();
    setIsEditingProfile(false);
    signalRService.updateProfile(editProfileDisplayName.trim(), finalAvatar, finalColor, editProfileAboutMe.trim());
  }, [store.currentUser, editProfileDisplayName, editProfileAvatarBase64, editProfileAvatarColor, editProfileAboutMe, validateName, saveLocalCache]);

  const handleCreateChannel = useCallback(async () => {
    const nameErr = validateName(newChannelName);
    if (nameErr) { setError(nameErr); return; }
    closeAndResetModals();
    signalRService.createChannel(newChannelName.trim());
  }, [newChannelName, validateName, closeAndResetModals]);

  const saveChannelEdit = useCallback(async () => {
    if (!editChannelId) return;
    const nameErr = validateName(editChannelName);
    if (nameErr) { setError(nameErr); return; }
    const id = editChannelId;
    const name = editChannelName.trim();
    closeAndResetModals();
    signalRService.updateChannel(id, name);
  }, [editChannelId, editChannelName, validateName, closeAndResetModals]);

  const handleChannelClick = useCallback(async (channelId: string) => {
    if (store.currentChannelId === channelId) return;
    if (store.currentChannelId || store.currentCallUser) {
      store.setPendingChannelSwitch(channelId); store.setModal('channelSwitch', true); return;
    }
    const status = await signalRService.joinChannel(channelId);
    if (status === 'full') store.setModal('channelFull', true);
  }, [store.currentChannelId, store.currentCallUser]);

  const confirmChannelSwitch = useCallback(async () => {
    if (!store.pendingChannelSwitch) return;
    const targetId = store.pendingChannelSwitch;
    store.setModal('channelSwitch', false); store.setPendingChannelSwitch(null);
    if (store.currentCallUser) { await signalRService.endCall(); }
    else { webrtc.stopLocalStream(); store.setCurrentChannelId(null); store.setVoiceUsers([]); await signalRService.leaveChannel(); }
    const status = await signalRService.joinChannel(targetId);
    if (status === 'full') store.setModal('channelFull', true);
  }, [store.pendingChannelSwitch, store.currentCallUser]);

  const handleAddFriend = useCallback(async () => {
    if (!friendName.trim() || friendRequestStatus === 'loading' || friendRequestStatus === 'sent') return;
    setFriendRequestStatus('loading');
    const user = await signalRService.getUserByUsername(friendName.trim());
    if (user) {
      closeAndResetModals();
      store.setSelectedProfileUser(user, 'none');
      store.setModal('profile', true);
    } else {
      setFriendRequestStatus('notfound');
    }
  }, [friendName, friendRequestStatus, store, closeAndResetModals]);

  const handleAcceptChannelInvite = useCallback(async (channelId: string) => {
    signalRService.acceptChannelInvite(channelId);
    if (store.currentChannelId || store.currentCallUser) {
      store.setPendingChannelSwitch(channelId); store.setModal('channelSwitch', true); return;
    }
    const status = await signalRService.joinChannel(channelId);
    if (status === 'full') store.setModal('channelFull', true);
  }, [store.currentChannelId, store.currentCallUser]);

  const handleDeclineChannelInvite = useCallback((channelId: string) => {
    signalRService.declineChannelInvite(channelId);
  }, []);

  const handleInviteToChannel = useCallback(async (friendId: string) => {
    const ch = store.selectedChannelForInvite;
    if (!ch) return;
    if (store.currentChannelId !== ch.id) return;
    await signalRService.sendChannelInvite(friendId, ch.id, ch.name);
    addSentInvite(friendId);
  }, [store.selectedChannelForInvite, store.currentChannelId, addSentInvite]);

  const openChannelMembers = useCallback(async (ch: VoiceChannel) => {
    const currentStore = useAppStore.getState();
    currentStore.setSelectedChannelForMembers(ch);

    // Instant cache-hit (fallback to empty array)
    const cached = currentStore.channelMembersCache?.[ch.id] || [];
    currentStore.setChannelMembers(cached);
    currentStore.setModal('channelMembers', true);

    try {
      // Silent background sync
      const members = await signalRService.getChannelMembersList(ch.id);
      if (members && Array.isArray(members)) {
        useAppStore.getState().setChannelMembers(members);
        useAppStore.getState().setChannelMembersCache(ch.id, members);
      }
    } catch (e) {
      console.error("Failed to sync channel members", e);
    }
  }, []);

  const handleKickConfirm = useCallback(async () => {
    const ch = store.selectedChannelForMembers;
    const u = store.userToKick;
    store.setModal('kickConfirm', false);
    store.setUserToKick(null);
    if (ch && u) signalRService.kickFromChannel(ch.id, u.id);
  }, [store.selectedChannelForMembers, store.userToKick]);

  const showAdminBlockFeedback = useCallback(() => {
    setControlsShake(true);
    setTimeout(() => setControlsShake(false), 600);

    if (adminBlockTimerRef.current) clearTimeout(adminBlockTimerRef.current);

    setAdminBlockToast(t('toasts.adminRestricted', 'Администратор запретил это действие'));
    adminBlockTimerRef.current = setTimeout(() => {
      setAdminBlockToast('__hiding__');
      setTimeout(() => setAdminBlockToast(null), 400);
    }, 2500);
  }, [t]);

  const toggleMute = useCallback(() => {
    if (!store.currentUser) return;

    if (store.currentUser.isServerMuted || store.currentUser.isServerDeafened) {
      showAdminBlockFeedback();
      return;
    }

    if (store.currentUser.isDeafened) return;

    const nextMuted = !store.currentUser.isMuted;
    store.setCurrentUser({ ...store.currentUser, isMuted: nextMuted });
    signalRService.toggleState(nextMuted, store.currentUser.isDeafened);
  }, [store.currentUser, showAdminBlockFeedback]);

  const toggleDeafen = useCallback(() => {
    if (!store.currentUser) return;

    if (store.currentUser.isServerDeafened) {
      showAdminBlockFeedback();
      return;
    }

    const nextDeafened = !store.currentUser.isDeafened;
    const nextMuted = nextDeafened ? true : store.currentUser.isMuted;
    store.setCurrentUser({ ...store.currentUser, isDeafened: nextDeafened, isMuted: nextMuted });
    signalRService.toggleState(nextMuted, nextDeafened);
    webrtc.setDeafened(nextDeafened);
  }, [store.currentUser, showAdminBlockFeedback]);


  const handleAcceptCall = useCallback(async () => {
    if (store.incomingCall) await signalRService.acceptCall(store.incomingCall.callerId);
  }, [store.incomingCall]);

  const handleDeclineCall = useCallback(async () => {
    if (store.incomingCall) await signalRService.declineCall(store.incomingCall.callerId);
  }, [store.incomingCall]);

  const handleEndCall = useCallback(async () => {
    await signalRService.endCall();
  }, []);

  const openMyAchievements = useCallback(async () => {
    store.setAchievementsData(null); // Показать "Загрузка..."
    store.setAchievementsViewUserId(null);
    store.setModal('achievements', true);
    const data = await signalRService.getMyAchievements();
    store.setAchievementsData(data);
  }, []);

  const openUserAchievements = useCallback(async (userId: string) => {
    store.setAchievementsData(null);
    store.setAchievementsViewUserId(userId);
    store.setModal('achievements', true);
    const data = await signalRService.getUserAchievements(userId);
    store.setAchievementsData(data);
  }, []);

  const handleContextMenu = useCallback((e: React.MouseEvent, type: 'channel' | 'friend' | 'voiceUser' | 'channelMember', item: any) => {
    e.preventDefault();
    if ((type === 'voiceUser' || type === 'channelMember') && item.id === store.currentUser?.id) return;
    setContextMenu({ visible: true, x: e.clientX, y: e.clientY, type, item });
  }, [store.currentUser?.id]);

  const loadDevices = useCallback(async () => { setAudioDevices(await webrtc.getAudioDevices()); }, []);

  const rgbToHsl = useCallback((r: number, g: number, b: number) => {
    r /= 255; g /= 255; b /= 255;
    const max = Math.max(r, g, b), min = Math.min(r, g, b);
    let h = 0, s = 0; const l = (max + min) / 2;
    if (max !== min) {
      const d = max - min; s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
      switch (max) { case r: h = ((g - b) / d + (g < b ? 6 : 0)) / 6; break; case g: h = ((b - r) / d + 2) / 6; break; case b: h = ((r - g) / d + 4) / 6; break; }
    }
    return { h, s, l };
  }, []);

  const hslToRgb = useCallback((h: number, s: number, l: number) => {
    let r: number, g: number, b: number;
    if (s === 0) { r = g = b = l; } else {
      const hue2rgb = (p: number, q: number, t: number) => { if (t < 0) t += 1; if (t > 1) t -= 1; if (t < 1 / 6) return p + (q - p) * 6 * t; if (t < 1 / 2) return q; if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6; return p; };
      const q = l < 0.5 ? l * (1 + s) : l + s - l * s, p = 2 * l - q;
      r = hue2rgb(p, q, h + 1 / 3); g = hue2rgb(p, q, h); b = hue2rgb(p, q, h - 1 / 3);
    }
    return { r: Math.round(r * 255), g: Math.round(g * 255), b: Math.round(b * 255) };
  }, []);

  const getDominantColor = useCallback((ctx: CanvasRenderingContext2D, width: number, height: number): string => {
    try {
      const imageData = ctx.getImageData(0, 0, width, height).data;
      const pixels: { r: number; g: number; b: number; brightness: number; saturation: number }[] = [];
      for (let i = 0; i < imageData.length; i += 16) {
        const r = imageData[i], g = imageData[i + 1], b = imageData[i + 2], a = imageData[i + 3];
        if (a < 128 || (r < 20 && g < 20 && b < 20) || (r > 235 && g > 235 && b > 235)) continue;
        const max = Math.max(r, g, b), min = Math.min(r, g, b);
        const brightness = (max + min) / 2;
        const saturation = max === min ? 0 : (max - min) / (255 - Math.abs(max + min - 255));
        pixels.push({ r, g, b, brightness, saturation });
      }
      if (pixels.length === 0) return '#c70060';
      const saturatedPixels = pixels.filter(p => p.saturation > 0.3 && p.brightness > 40 && p.brightness < 220);
      const targetPixels = saturatedPixels.length > 0 ? saturatedPixels : pixels;
      const colorClusters = new Map<string, { count: number; r: number; g: number; b: number }>();
      for (const pixel of targetPixels) {
        const key = `${Math.round(pixel.r / 32) * 32},${Math.round(pixel.g / 32) * 32},${Math.round(pixel.b / 32) * 32}`;
        if (!colorClusters.has(key)) colorClusters.set(key, { count: 0, r: 0, g: 0, b: 0 });
        const cluster = colorClusters.get(key)!;
        cluster.count++; cluster.r += pixel.r; cluster.g += pixel.g; cluster.b += pixel.b;
      }
      let maxCount = 0; let dominantCluster: { r: number; g: number; b: number } | null = null;
      for (const cluster of colorClusters.values()) {
        if (cluster.count > maxCount) { maxCount = cluster.count; dominantCluster = { r: Math.round(cluster.r / cluster.count), g: Math.round(cluster.g / cluster.count), b: Math.round(cluster.b / cluster.count) }; }
      }
      if (!dominantCluster) return '#c70060';
      const hsl = rgbToHsl(dominantCluster.r, dominantCluster.g, dominantCluster.b);
      hsl.s = Math.min(1, hsl.s * 1.2); hsl.l = Math.max(0.45, Math.min(0.65, hsl.l));
      const finalRgb = hslToRgb(hsl.h, hsl.s, hsl.l);
      return `#${finalRgb.r.toString(16).padStart(2, '0')}${finalRgb.g.toString(16).padStart(2, '0')}${finalRgb.b.toString(16).padStart(2, '0')}`;
    } catch { return '#c70060'; }
  }, [rgbToHsl, hslToRgb]);

  const applyCrop = useCallback(() => {
    if (!imgRef.current) return;
    const img = imgRef.current;

    const canvas = document.createElement('canvas');
    canvas.width = 200;
    canvas.height = 200;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const ratio = Math.min(200 / img.naturalWidth, 200 / img.naturalHeight);
    const baseW = img.naturalWidth * ratio;
    const baseH = img.naturalHeight * ratio;

    ctx.fillStyle = '#000';
    ctx.fillRect(0, 0, 200, 200);
    ctx.translate(100, 100);
    ctx.scale(cropScale, cropScale);
    ctx.translate(cropPos.x / cropScale, cropPos.y / cropScale);
    ctx.drawImage(img, -baseW / 2, -baseH / 2, baseW, baseH);

    const hex = getDominantColor(ctx, 200, 200);

    let base64: string;

    if (cropGifDataUrl) {
      // GIF: пакуем оригинал + параметры кропа
      base64 = packGif(cropGifDataUrl, cropScale, cropPos.x, cropPos.y);
    } else {
      // Статичное изображение: рендерим через canvas
      base64 = canvas.toDataURL('image/png');
    }

    if (cropContext === 'setup') {
      setAvatarBase64(base64);
      setAvatarColor(hex);
    } else {
      setEditProfileAvatarBase64(base64);
      setEditProfileAvatarColor(hex);
    }
    setCropGifDataUrl(null);
    setShowCropper(false);
  }, [cropScale, cropPos, cropContext, cropGifDataUrl, getDominantColor]);


  const onFileChange = useCallback((e: React.ChangeEvent<HTMLInputElement>, context: 'setup' | 'profile') => {
    if (e.target.files?.[0]) {
      const file = e.target.files[0];
      if (file.size > 5 * 1024 * 1024) { alert(t('common.fileTooLarge')); return; }
      if (!file.type.startsWith('image/')) { alert(t('common.onlyImages')); return; }

      if (file.type === 'image/gif') {
        const reader = new FileReader();
        reader.onload = (ev) => {
          const gifDataUrl = ev.target?.result as string;
          if (gifDataUrl) {
            setCropGifDataUrl(gifDataUrl);
            setCropImageSrc(URL.createObjectURL(file));
            setCropScale(1);
            setCropPos({ x: 0, y: 0 });
            setCropContext(context);
            setShowCropper(true);
          }
        };
        reader.readAsDataURL(file);
        e.target.value = '';
        return;
      }

      setCropGifDataUrl(null);
      setCropImageSrc(URL.createObjectURL(file));
      setCropScale(1);
      setCropPos({ x: 0, y: 0 });
      setCropContext(context);
      setShowCropper(true);
    }
    e.target.value = '';
  }, []);

  const renderModal = useCallback((key: keyof typeof store.modals, content: React.ReactNode) => {
    if (!store.modals[key]) return null;

    return (
      <div className="fixed inset-0 z-[150] bg-black/70 backdrop-blur-md flex items-center justify-center p-4">
        {content}
      </div>
    );
  }, [store.modals]);

  const renderCropper = () => {
    if (!showCropper || !cropImageSrc) return null;

    return (
      <div className="fixed inset-0 z-[99999] bg-black/90 flex items-center justify-center p-4">
        <div className="bg-panelBg p-6 rounded-3xl flex flex-col items-center shadow-2xl w-[360px] max-w-full">
          <div className="w-full flex items-center justify-between mb-6">
            <h2 className="text-white text-xl font-bold">{t('auth.cropTitle')}</h2>
            <button
              onClick={() => {
                setShowCropper(false);
                setCropGifDataUrl(null);
                setCropImageSrc(null);
                setCropScale(1);
                setCropPos({ x: 0, y: 0 });
                setIsDragging(false);
              }}
              className="group text-textMuted hover:text-white transition-all duration-200 hover:rotate-90 hover:scale-110 active:scale-90 p-1.5 rounded-lg hover:bg-surface"
            >
              <X weight="bold" size={24} />
            </button>
          </div>

          <div
            className="w-[200px] h-[200px] rounded-full overflow-hidden relative cursor-move bg-black shadow-inner"
            onMouseDown={e => {
              setIsDragging(true);
              setDragStart({ x: e.clientX - cropPos.x, y: e.clientY - cropPos.y });
            }}
            onMouseMove={e => {
              if (isDragging) {
                setCropPos({ x: e.clientX - dragStart.x, y: e.clientY - dragStart.y });
              }
            }}
            onMouseUp={() => setIsDragging(false)}
            onMouseLeave={() => setIsDragging(false)}
            onWheel={e => setCropScale(s => Math.max(0.5, Math.min(5, s + (e.deltaY > 0 ? -0.1 : 0.1))))}
          >
            <img
              ref={imgRef}
              src={cropImageSrc}
              draggable={false}
              style={{
                position: 'absolute',
                left: '50%',
                top: '50%',
                transform: `translate(calc(-50% + ${cropPos.x}px), calc(-50% + ${cropPos.y}px)) scale(${cropScale})`,
                maxWidth: '100%',
                maxHeight: '100%',
                objectFit: 'contain'
              }}
            />
          </div>

          <Md3Slider min={0.5} max={3} step={0.05} value={cropScale} onChange={setCropScale} className="mt-6" />

          <div className="flex gap-4 mt-6 w-full">
            <button
              onClick={() => {
                setShowCropper(false);
                setCropGifDataUrl(null);
                setCropImageSrc(null);
                setCropScale(1);
                setCropPos({ x: 0, y: 0 });
                setIsDragging(false);
              }}
              className="flex-1 py-3 text-textMuted hover:bg-surface rounded-xl font-bold transition-colors"
            >
              {t('common.cancel')}
            </button>
            <button
              onClick={applyCrop}
              className="flex-1 py-3 bg-[#c70060] text-white font-bold rounded-xl hover:opacity-90 transition-opacity"
            >
              {t('common.apply')}
            </button>
          </div>
        </div>
      </div>
    );
  };

  // === Screens ===

  const hasInvites = store.channelInvites.length > 0 || store.friendRequests.length > 0;

  return (
    <>
      <div id="mouse-glow" />
      {/* Auth screen — показывается поверх основного UI, когда пользователь не авторизован */}
      {!isAuth && (
        <div className="fixed inset-0 z-[100000] flex flex-col bg-appBg text-textMain animate-fade-in select-none">
          <TitleBar />
          <div className="flex-1 flex items-center justify-center p-4">
            {authStep === 'login' && (
              <div className="bg-panelBg p-10 rounded-3xl w-[400px] shadow-2xl flex flex-col">
                <h1 className="text-4xl font-black text-center mb-8 tracking-wider text-white">ZABOR</h1>
                <label className="text-xs font-bold text-textMuted mb-2 tracking-wider">{t('auth.login')}</label>
                <input
                  ref={loginInputRef}
                  type="text"
                  value={login}
                  onChange={e => setLogin(e.target.value)}
                  maxLength={25}
                  className="bg-surface text-white rounded-xl p-3 mb-4 outline-none focus:ring-2 focus:ring-[#c70060]"
                />
                <label className="text-xs font-bold text-textMuted mb-2 tracking-wider">{t('auth.password')}</label>
                <div className="relative mb-6">
                  <input
                    ref={passwordInputRef}
                    type={showPassword ? 'text' : 'password'}
                    value={password}
                    onChange={e => setPassword(e.target.value)}
                    maxLength={25}
                    onKeyDown={e => e.key === 'Enter' && handleAuth()}
                    className="w-full bg-surface text-white rounded-xl p-3 outline-none focus:ring-2 focus:ring-[#c70060] pr-10"
                  />
                  <button
                    onClick={() => setShowPassword(!showPassword)}
                    className="absolute right-3 top-3 text-textMuted hover:text-white transition-colors"
                  >
                    {showPassword ? <EyeOff weight="bold" size={20} /> : <Eye weight="bold" size={20} />}
                  </button>
                </div>
                {error && <p className="text-danger text-sm mb-4 text-center font-medium">{error}</p>}
                <button onClick={handleAuth} disabled={isLoading} className="bg-[#c70060] text-white font-bold py-3 rounded-xl disabled:opacity-50 hover:opacity-90 transition-opacity">{isLoading ? t('auth.loading') : t('auth.continue')}</button>
              </div>
            )}
            {authStep === 'confirm' && (
              <div className="bg-panelBg p-8 rounded-3xl w-[400px] text-center shadow-2xl">
                <h2 className="text-2xl font-bold mb-4 text-white">{t('auth.accountNotFound')}</h2>
                <p className="text-textMuted mb-8">{t('auth.createNewProfile')}</p>
                <div className="flex gap-4">
                  <button onClick={() => setAuthStep('login')} className="flex-1 bg-surface text-white py-3 rounded-xl font-bold hover:bg-surfaceHover transition-colors">{t('auth.no')}</button>
                  <button onClick={() => { setAuthStep('setup'); setDisplayName(login); }} className="flex-1 bg-[#c70060] text-white py-3 rounded-xl font-bold hover:opacity-90 transition-opacity">{t('auth.yes')}</button>
                </div>
              </div>
            )}
            {authStep === 'setup' && (
              <div className="bg-panelBg p-10 rounded-3xl w-[400px] flex flex-col shadow-2xl">
                <h1 className="text-2xl font-bold text-center mb-2 text-white">{t('auth.createProfile')}</h1>
                <p className="text-sm text-textMuted text-center mb-8">{t('auth.howOthersSeeYou')}</p>
                <label className="w-[103px] h-[103px] rounded-full mx-auto mb-8 flex items-center justify-center cursor-pointer relative shadow-lg hover:opacity-80 transition-opacity">
                  {avatarBase64 ? <AvatarImg src={avatarBase64} size={103} bgColor={avatarColor} /> : <div className="w-full h-full rounded-full flex items-center justify-center" style={{ backgroundColor: avatarColor }}><Camera weight="bold" size={32} className="text-white" /></div>}
                  <input type="file" accept="image/*" className="hidden" onChange={e => onFileChange(e, 'setup')} />
                </label>
                <label className="text-xs font-bold text-textMuted mb-2 tracking-wider">{t('auth.displayName')}</label>
                <input type="text" value={displayName} onChange={e => setDisplayName(e.target.value)} maxLength={20} placeholder={t('auth.max20chars')} className="bg-surface text-white rounded-xl p-3 mb-6 outline-none focus:ring-2 focus:ring-[#c70060]" />
                {error && <p className="text-danger text-sm mb-4 text-center font-medium">{error}</p>}
                <button onClick={handleAuth} disabled={isLoading} className="bg-[#c70060] text-white font-bold py-3 rounded-xl disabled:opacity-50 hover:opacity-90 transition-opacity">{isLoading ? t('auth.creating') : t('auth.create')}</button>
              </div>
            )}
            {renderCropper()}
          </div>
        </div>
      )}

      {/* Loading overlays — рендерятся поверх основного UI, чтобы интерфейс монтировался заранее */}
      {appLoading && (
        <div className={`fixed inset-0 z-[100000] flex flex-col bg-appBg transition-opacity duration-[600ms] select-none ${loadingFadeOut ? 'opacity-0 pointer-events-none' : 'opacity-100'}`}>
          <TitleBar />
          <div className="flex-1 flex items-center justify-center">
            <div className="flex flex-col items-center gap-6">
              <h1 className="text-5xl font-black text-white tracking-widest animate-pulse">ZABOR</h1>
              <div className="w-10 h-10 border-4 border-[#c70060] border-t-transparent rounded-full animate-spin" />
              {showInitConnectionError && (
                <div className="flex flex-col items-center mt-2 animate-fade-in">
                  <p className="text-danger font-bold text-center">{t('main.connection.noConnection')}</p>
                  {signalRService.lastConnectionError && (
                    <p className="text-white/60 text-xs mt-1 text-center max-w-[300px] break-words">
                      {signalRService.lastConnectionError}
                    </p>
                  )}
                </div>
              )}
            </div>
          </div>
        </div>
      )}

      {!appLoading && !serverConnected && isAuth && (
        <div className="fixed inset-0 z-[100000] flex flex-col bg-appBg select-none">
          <TitleBar />
          <div className="flex-1 flex items-center justify-center">
            <div className="flex flex-col items-center gap-6">
              <h1 className="text-5xl font-black text-white tracking-widest animate-pulse">ZABOR</h1>
              <div className="w-10 h-10 border-4 border-[#c70060] border-t-transparent rounded-full animate-spin" />
              {showErrorText && (
                <div className="flex flex-col items-center mt-4 animate-fade-in">
                  <p className="text-danger font-bold text-center">{t('main.connection.reconnecting')}</p>
                  {signalRService.lastConnectionError && (
                    <p className="text-white/60 text-xs mt-1 text-center max-w-[300px] break-words">
                      {signalRService.lastConnectionError}
                    </p>
                  )}
                </div>
              )}
            </div>
          </div>
        </div>
      )}

      <div className="flex flex-col h-screen w-screen bg-appBg text-textMain overflow-hidden relative select-none">
        <TitleBar />
        <div className="flex flex-1 overflow-hidden">



          <div className="w-80 bg-panelBg flex flex-col border-r border-[#303035] relative shrink-0">

            {showInvitesPanel && (
              <div className="absolute inset-0 bg-panelBg z-[60] flex flex-col animate-fade-in">
                <div className="flex items-center justify-between p-4 border-b border-[#303035]">
                  <span className="text-sm font-bold text-white tracking-wider">{t('main.notifications.title')}</span>
                  <button onClick={() => setShowInvitesPanel(false)} className="group text-textMuted hover:text-white transition-all duration-200 hover:rotate-90 hover:scale-110 active:scale-90 p-1.5 rounded-lg hover:bg-surface"><X weight="bold" size={20} /></button>
                </div>
                <div className="flex-1 overflow-y-auto p-4 space-y-3">
                  {store.friendRequests.map(req => (
                    <div key={req.id} className="bg-surface p-4 rounded-xl">
                      <div className="flex items-center gap-3 mb-3">
                        <div className="w-[47px] h-[47px] shrink-0 relative">
                          <AvatarImg src={req.avatarBase64} size={47} bgColor={req.avatarColor} />
                        </div>
                        <div className="flex-1 min-w-0">
                          <p className="text-white font-semibold text-sm truncate">{req.displayName}</p>
                          <p className="text-textMuted text-xs font-medium">{t('main.notifications.friendRequest')}</p>
                        </div>
                      </div>
                      <div className="flex gap-2">
                        <button onClick={() => signalRService.acceptFriendRequest(req.id)} className="flex-1 bg-success/20 text-success py-2 rounded-xl text-sm font-bold hover:bg-success/30 transition-colors">{t('main.notifications.accept')}</button>
                        <button onClick={() => signalRService.declineFriendRequest(req.id)} className="flex-1 bg-danger/20 text-danger py-2 rounded-xl text-sm font-bold hover:bg-danger/30 transition-colors">{t('main.notifications.decline')}</button>
                      </div>
                    </div>
                  ))}
                  {store.channelInvites.map(inv => (
                    <div key={inv.channelId} className="bg-surface p-4 rounded-xl">
                      <div className="mb-3">
                        <p className="text-white font-semibold text-sm truncate">{inv.channelName}</p>
                        <p className="text-textMuted text-xs font-medium">{t('main.notifications.channelInvite', { name: inv.senderName })}</p>
                      </div>
                      <div className="flex gap-2">
                        <button onClick={() => { handleAcceptChannelInvite(inv.channelId); setShowInvitesPanel(false); }} className="flex-1 bg-success/20 text-success py-2 rounded-xl text-sm font-bold hover:bg-success/30 transition-colors">{t('main.notifications.join')}</button>
                        <button onClick={() => handleDeclineChannelInvite(inv.channelId)} className="flex-1 bg-danger/20 text-danger py-2 rounded-xl text-sm font-bold hover:bg-danger/30 transition-colors">{t('main.notifications.decline')}</button>
                      </div>
                    </div>
                  ))}
                  {store.friendRequests.length === 0 && store.channelInvites.length === 0 && (
                    <div className="flex flex-col items-center justify-center py-16 text-textMuted">
                      <Mail weight="bold" size={40} className="mb-4 opacity-20" />
                      <p className="font-medium text-sm">{t('main.notifications.none')}</p>
                    </div>
                  )}
                </div>
              </div>
            )}

            <div className="flex-1 overflow-y-auto p-4 pb-20">
              {activeTab === 'channels' && (
                <div className="animate-fade-in">
                  <div className="flex justify-between items-center mb-4 px-2">
                    <span className="text-xs font-bold text-textMuted tracking-wider">{t('main.voice.voiceChannels')}</span>
                    <button
                      onClick={() => store.setModal('createChannel', true)}
                      className="text-textMuted hover:text-white transition-all duration-200 hover:scale-110 active:scale-95 w-8 h-8 rounded-lg hover:bg-surface flex items-center justify-center focus:outline-none"
                      title={t('modals.createChannel.title', 'Создать канал')}
                    >
                      <Plus weight="bold" size={18} />
                    </button>
                  </div>
                  {store.channels.map(ch => {
                    const channelUsers = store.channelUsersMap[ch.id] || [];
                    return (
                      <div key={ch.id} className="mb-2">
                        <button onClick={() => handleChannelClick(ch.id)} onContextMenu={e => handleContextMenu(e, 'channel', ch)}
                          className={`w-full text-left px-2 py-3 rounded-xl flex items-center justify-between group transition-all duration-200 hover:scale-[1.02] active:scale-[0.98] focus:outline-none ${store.currentChannelId === ch.id ? 'bg-[#333]' : 'hover:bg-surfaceHover'}`}>
                          <div className="flex items-center gap-3 overflow-hidden">
                            <div className={`w-1.5 h-1.5 rounded-full shrink-0 ml-2 transition-all duration-300 ${store.currentChannelId === ch.id ? 'bg-[#c70060] shadow-[0_0_8px_#c70060]' : 'bg-textMuted'}`} />
                            <span className="font-medium text-[15px] truncate select-none text-white">{ch.name}</span>
                          </div>
                          <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity pr-2 shrink-0">
                            {store.currentChannelId === ch.id && (
                              <div onClick={async e => {
                                e.stopPropagation();
                                e.preventDefault();
                                store.setSelectedChannelForInvite(ch);
                                try {
                                  const members = await signalRService.getChannelMembersList(ch.id);
                                  if (members && Array.isArray(members)) {
                                    store.setChannelMembersCache(ch.id, members);
                                  }
                                } catch (err) {
                                  console.error("Failed to load channel members:", err);
                                }
                                store.setModal('inviteToChannel', true);
                              }} className="text-textMuted hover:text-white p-1 rounded hover:bg-black/20" title={t('common.invite', 'Пригласить')}><UserPlus weight="bold" size={16} /></div>
                            )}
                            <div onClick={e => { e.stopPropagation(); e.preventDefault(); openChannelMembers(ch); }} className="text-textMuted hover:text-white p-1 rounded hover:bg-black/20" title={t('common.channelMembers', 'Участники канала')}><Users weight="bold" size={16} /></div>
                          </div>
                        </button>
                        {channelUsers.length > 0 && (
                          <div className="flex items-center -space-x-2 px-8 mt-1.5 pointer-events-none">
                            {[...channelUsers].sort((a, b) => {
                              const currentUserId = store.currentUser?.id;
                              if (a.id === currentUserId) return -1;
                              if (b.id === currentUserId) return 1;
                              const nameA = a.displayName.toLowerCase();
                              const nameB = b.displayName.toLowerCase();
                              if (nameA < nameB) return -1;
                              if (nameA > nameB) return 1;
                              return a.id.localeCompare(b.id);
                            }).map((u, i) => (
                              <div key={u.id} className="w-[31px] h-[31px] rounded-full border-2 border-panelBg relative shrink-0 overflow-hidden" style={{ zIndex: 100 - i }} title={u.displayName}>
                                <AvatarImg src={u.avatarBase64} size={31} bgColor={u.avatarColor} animate={false} />
                              </div>
                            ))}
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              )}
              {activeTab === 'friends' && (
                <div className="animate-fade-in">
                  <div className="flex justify-between items-center mb-4 px-2">
                    <span className="text-xs font-bold text-textMuted tracking-wider">{t('main.tabs.friends')}</span>
                    <button
                      onClick={() => store.setModal('addFriend', true)}
                      className="text-textMuted hover:text-white transition-all duration-200 hover:scale-110 active:scale-95 w-8 h-8 rounded-lg hover:bg-surface flex items-center justify-center focus:outline-none"
                      title={t('modals.addFriend.title', 'Добавить друга')}
                    >
                      <Plus weight="bold" size={18} />
                    </button>
                  </div>
                  {store.friends.map(f => (
                    <div key={f.id} onContextMenu={e => handleContextMenu(e, 'friend', f)}
                      onClick={() => { store.setSelectedProfileUser(f, 'friends'); setEditProfileDisplayName(f.displayName); setEditProfileAboutMe(f.aboutMe || ''); setIsEditingProfile(false); store.setModal('profile', true); signalRService.viewProfile(f.id); }}
                      className="px-3 py-2 rounded-xl mb-1 cursor-pointer hover:bg-surfaceHover flex items-center gap-3 transition-all duration-200 hover:scale-[1.02] active:scale-[0.98]">
                      <div className="relative w-[47px] h-[47px] shrink-0">
                        <div className="w-full h-full relative">
                          <AvatarImg src={f.avatarBase64} size={47} bgColor={f.avatarColor} />
                        </div>
                        <div className={`absolute -bottom-0.5 -right-0.5 w-4 h-4 rounded-full border-[3px] border-panelBg ${f.isOnline ? 'bg-success' : 'bg-gray-500'}`} />
                      </div>
                      <span className={`font-semibold text-[15px] truncate ${f.isOnline ? 'text-white' : 'text-textMuted'}`}>{f.displayName}</span>
                    </div>
                  ))}
                </div>
              )}
            </div>

            <div className={`absolute bottom-[145px] right-6 transition-all duration-500 ${hasInvites ? 'translate-y-0 opacity-100 pointer-events-auto' : 'translate-y-[150%] opacity-0 pointer-events-none'}`}>
              <button onClick={() => setShowInvitesPanel(true)} className="w-14 h-14 bg-[#c70060] rounded-full flex items-center justify-center shadow-[0_0_20px_rgba(199,0,96,0.5)] hover:scale-105 transition-transform relative">
                <Mail weight="bold" size={24} color="white" />
                <div className="absolute top-0 right-0 w-4 h-4 bg-danger rounded-full border-2 border-panelBg animate-pulse" />
              </button>
            </div>

            <div className="bg-surface rounded-full mx-4 my-2 p-1 flex relative shrink-0">
              <button onClick={() => setActiveTab('channels')} className={`flex-1 py-2.5 rounded-full font-bold text-sm z-10 transition-all duration-200 hover:scale-[1.02] active:scale-[0.98] ${activeTab === 'channels' ? 'text-white' : 'text-textMuted hover:text-white'}`}>{t('main.tabs.channels')}</button>
              <button onClick={() => setActiveTab('friends')} className={`flex-1 py-2.5 rounded-full font-bold text-sm z-10 transition-all duration-200 hover:scale-[1.02] active:scale-[0.98] ${activeTab === 'friends' ? 'text-white' : 'text-textMuted hover:text-white'}`}>{t('main.tabs.friends')}</button>
              <div className={`absolute top-1 bottom-1 w-[calc(50%-4px)] bg-[#333] rounded-full transition-all duration-300 ease-out ${activeTab === 'channels' ? 'left-1' : 'left-[calc(50%+2px)]'}`} />
            </div>

            <div className="h-[75px] bg-[#09090B] rounded-2xl mx-4 mb-4 flex items-center px-4 shrink-0 shadow-lg">
              <div onClick={() => { store.setSelectedProfileUser(store.currentUser, 'none'); setEditProfileDisplayName(store.currentUser!.displayName); setEditProfileAboutMe(store.currentUser!.aboutMe || ''); setEditProfileAvatarBase64(null); setIsEditingProfile(false); store.setModal('profile', true); }}
                className="relative w-[51px] h-[51px] mr-3 cursor-pointer shrink-0 hover:opacity-80 transition-opacity">
                <AvatarImg src={store.currentUser?.avatarBase64} size={51} bgColor={store.currentUser?.avatarColor} />
                <div className={`absolute -bottom-0.5 -right-0.5 w-4 h-4 rounded-full border-[3px] border-[#09090B] ${serverConnected ? 'bg-success' : 'bg-gray-500'}`} />
              </div>
              <div className="flex-1 min-w-0 flex flex-col justify-center">
                <div className="font-bold text-sm truncate text-white">{store.currentUser?.displayName}</div>
                <div onClick={handleCopyUsername} className="text-xs font-semibold text-textMuted truncate cursor-pointer hover:text-white transition-colors mt-0.5" title={t('main.user.copyUsername')}>
                  {isCopied ? <span className="text-success">{t('main.user.copied')}</span> : store.currentUser?.username}
                </div>
              </div>
              <div className="flex items-center gap-1 shrink-0">


                <button
                  onClick={() => {
                    store.setModal('settings', true);
                    loadDevices();
                    window.windowControls.getAutoLaunch().then(setAutoLaunch).catch(() => { });
                    window.windowControls.getMinimizeToTray().then(setMinimizeToTray).catch(() => { });
                  }}
                  className="group text-textMuted hover:text-white p-2 hover:bg-surface rounded-xl transition-colors"
                >
                  <div className="transition-transform duration-500 group-hover:rotate-90 group-hover:scale-110 group-active:scale-95">
                    <Settings weight="bold" size={20} />
                  </div>
                </button>
              </div>
            </div>
          </div>

          <div className="flex-1 flex flex-col relative bg-[#181818]">

            {store.currentCallUser && (
              <div className="absolute top-0 left-0 right-0 bottom-[120px] p-6 flex items-center justify-center overflow-hidden">
                <div ref={containerRef} className="w-full h-full flex items-center justify-center">
                  <div
                    onContextMenu={e => handleContextMenu(e, 'voiceUser', store.currentCallUser)}
                    className={`relative flex flex-col items-center justify-center overflow-hidden shrink-0 transition-all duration-200
          ${store.callStatus === 'calling' ? 'animate-call-pulse' : ''}
          ${(store.currentCallUser.isSpeaking && store.callStatus === 'connected' && store.webrtcConnections[store.currentCallUser.id])
                        ? 'shadow-[inset_0_0_0_3px_#3BA55C,inset_0_0_0_5px_#181818,0_10px_15px_-3px_rgba(0,0,0,0.5)]'
                        : 'shadow-xl'
                      }`}
                    style={{
                      backgroundColor: store.currentCallUser.avatarColor,
                      width: `${cardSize.w}px`,
                      height: `${cardSize.h}px`,
                      borderRadius: '24px'
                    }}
                  >
                    <div
                      className="relative"
                      style={{
                        width: `${cardSize.avatarSize}px`,
                        height: `${cardSize.avatarSize}px`,
                        marginBottom: '16px'
                      }}
                    >
                      {(store.currentCallUser.isSpeaking && store.callStatus === 'connected' && store.webrtcConnections[store.currentCallUser.id]) && (
                        <div className="absolute inset-0 rounded-full border border-green-500/50 animate-speaking-ripple scale-125 pointer-events-none" />
                      )}
                      <AvatarImg src={store.currentCallUser.avatarBase64} size={cardSize.avatarSize} bgColor="transparent" />
                    </div>

                    {(!store.webrtcConnections[store.currentCallUser.id] && store.callStatus !== 'calling') && (
                      <div className="absolute inset-0 bg-black/40 flex flex-col items-center justify-center z-20 backdrop-blur-[2px]" style={{ borderRadius: '24px' }}>
                        <div className="flex gap-2.5 mb-2">
                          <div className="w-3 h-3 bg-[#c70060] rounded-full animate-pulse" />
                          <div className="w-3 h-3 bg-[#c70060] rounded-full animate-pulse" style={{ animationDelay: '0.15s' }} />
                          <div className="w-3 h-3 bg-[#c70060] rounded-full animate-pulse" style={{ animationDelay: '0.3s' }} />
                        </div>
                        <span className="text-white text-xs font-bold tracking-wider">{t('main.connection.connecting', 'ПОДКЛЮЧЕНИЕ')}</span>
                      </div>
                    )}

                    {store.callStatus === 'calling' && (
                      <div
                        className="absolute inset-0 bg-black/25 flex items-center justify-center"
                        style={{ borderRadius: '24px' }}
                      >
                        <div className="flex gap-2.5">
                          <div className="w-3 h-3 bg-white/90 rounded-full animate-bounce" />
                          <div className="w-3 h-3 bg-white/90 rounded-full animate-bounce" style={{ animationDelay: '0.15s' }} />
                          <div className="w-3 h-3 bg-white/90 rounded-full animate-bounce" style={{ animationDelay: '0.3s' }} />
                        </div>
                      </div>
                    )}

                    <div
                      className={`absolute bottom-4 left-1/2 -translate-x-1/2 transition-all duration-300 ${isIdle && store.callStatus === 'connected'
                        ? 'translate-y-8 opacity-0 pointer-events-none'
                        : 'translate-y-0 opacity-100'
                        }`}
                    >
                      <div
                        className="bg-[#09090B]/80 backdrop-blur-md border border-[#303035]/50 px-4 py-1.5 rounded-full flex items-center gap-2 shadow-lg whitespace-nowrap"
                        style={{ maxWidth: `${cardSize.w - 40}px` }}
                      >
                        <span className="text-white font-bold text-sm truncate">{store.currentCallUser.displayName}</span>

                        {store.callStatus === 'calling' && (
                          <span className="text-textMuted text-xs font-medium">{t('toasts.calling', 'Дозвон...')}</span>
                        )}

                        {store.callStatus === 'connected' && (store.currentCallUser.isMuted || store.currentCallUser.isServerMuted) && (
                          <Mic weight="bold" size={14} className="text-danger shrink-0" />
                        )}
                        {store.callStatus === 'connected' && (store.currentCallUser.isDeafened || store.currentCallUser.isServerDeafened) && (
                          <Headphones weight="bold" size={14} className="text-danger shrink-0" />
                        )}
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            )}

            {!store.currentCallUser && !store.currentChannelId && (
              <div className="flex-1 flex flex-col items-center justify-center px-16">
                <div className="max-w-lg text-center">
                  {joke ? (
                    <>
                      <p className="text-xs text-white/20 mb-3 font-semibold tracking-wider">{t('joke.title', 'ШУТЕЙКА:')}</p>
                      <p className="text-lg text-white/50 font-medium leading-relaxed whitespace-pre-line">
                        {joke === '__NO_JOKE__' ? t('joke.fallback', 'Сегодня сервер шутит молча.') : translateJoke(joke, i18n.language)}
                      </p>
                    </>
                  ) : (
                    <div className="w-6 h-6 border-2 border-white/10 border-t-white/30 rounded-full animate-spin mx-auto" />
                  )}
                </div>
              </div>
            )}

            {!store.currentCallUser && store.currentChannelId && (
              <div className="absolute top-0 left-0 right-0 bottom-[120px] p-6 flex items-center justify-center overflow-hidden">
                <div ref={containerRef} className="w-full h-full flex flex-wrap items-center justify-center gap-6" style={{ alignContent: 'center' }}>
                  {[...store.voiceUsers].sort((a, b) => {
                    const currentUserId = store.currentUser?.id;
                    if (a.id === currentUserId) return -1;
                    if (b.id === currentUserId) return 1;
                    const nameA = a.displayName.toLowerCase();
                    const nameB = b.displayName.toLowerCase();
                    if (nameA < nameB) return -1;
                    if (nameA > nameB) return 1;
                    return a.id.localeCompare(b.id);
                  }).map(user => (
                    <div key={user.id} onContextMenu={e => handleContextMenu(e, 'voiceUser', user)}
                      className={`relative flex flex-col items-center justify-center cursor-pointer transition-all duration-200 overflow-hidden shrink-0 hover:-translate-y-1
                        ${(user.isSpeaking && (store.webrtcConnections[user.id] || user.id === store.currentUser?.id)) ? 'shadow-[inset_0_0_0_3px_#3BA55C,inset_0_0_0_5px_#181818,0_10px_15px_-3px_rgba(0,0,0,0.5)] z-10' : 'shadow-xl'}`}
                      style={{ backgroundColor: user.avatarColor, width: `${cardSize.w}px`, height: `${cardSize.h}px`, borderRadius: '24px' }}>
                      <div className="relative" style={{ width: `${cardSize.avatarSize}px`, height: `${cardSize.avatarSize}px`, marginBottom: '16px' }}>
                        {(user.isSpeaking && (store.webrtcConnections[user.id] || user.id === store.currentUser?.id)) && (
                          <div className="absolute inset-0 rounded-full border border-green-500/50 animate-speaking-ripple scale-125 pointer-events-none" />
                        )}
                        <AvatarImg src={user.avatarBase64} size={cardSize.avatarSize} bgColor="transparent" />
                      </div>
                      {(!store.webrtcConnections[user.id] && user.id !== store.currentUser?.id) && (
                        <div className="absolute inset-0 bg-black/40 flex flex-col items-center justify-center z-20 backdrop-blur-[2px]" style={{ borderRadius: '24px' }}>
                          <div className="flex gap-2.5 mb-2">
                            <div className="w-3 h-3 bg-[#c70060] rounded-full animate-pulse" />
                            <div className="w-3 h-3 bg-[#c70060] rounded-full animate-pulse" style={{ animationDelay: '0.15s' }} />
                            <div className="w-3 h-3 bg-[#c70060] rounded-full animate-pulse" style={{ animationDelay: '0.3s' }} />
                          </div>
                          <span className="text-white text-xs font-bold tracking-wider">{t('main.connection.connecting', 'ПОДКЛЮЧЕНИЕ')}</span>
                        </div>
                      )}
                      <div className={`absolute bottom-4 left-1/2 -translate-x-1/2 transition-all duration-300 ${isIdle ? 'translate-y-8 opacity-0 pointer-events-none' : 'translate-y-0 opacity-100'}`}>
                        <div className="bg-[#09090B]/80 backdrop-blur-md border border-[#303035]/50 px-4 py-1.5 rounded-full flex items-center gap-2 shadow-lg whitespace-nowrap" style={{ maxWidth: `${cardSize.w - 40}px` }}>
                          <span className="text-white font-bold text-sm truncate">{user.displayName}</span>
                          {(user.isMuted || user.isServerMuted) && <Mic weight="bold" size={14} className="text-danger shrink-0" />}
                          {(user.isDeafened || user.isServerDeafened) && <Headphones weight="bold" size={14} className="text-danger shrink-0" />}
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {store.currentCallUser && (
              <div className={[
                "absolute bottom-10 left-1/2 -translate-x-1/2 bg-panelBg px-6 py-4 rounded-full flex gap-4 items-center shadow-2xl border border-[#303035] z-50",
                controlsShake ? "animate-shake" : ""
              ].join(" ")}>
                <button
                  onClick={toggleMute}
                  className={`group w-14 h-14 rounded-full flex items-center justify-center relative transition-colors ${(store.currentUser?.isMuted || store.currentUser?.isServerMuted || store.currentUser?.isServerDeafened)
                    ? 'bg-[#2B2D31] text-white'
                    : 'bg-surface hover:bg-surfaceHover text-white'
                    }`}
                >
                  <div className="flex items-center justify-center transition-transform duration-200 group-active:scale-95 group-hover:scale-110">
                    <Mic weight="bold" size={24} />
                    <div className={`absolute w-[30px] h-[3px] bg-danger rounded-full transition-all duration-300 origin-center ${(store.currentUser?.isMuted || store.currentUser?.isServerMuted || store.currentUser?.isServerDeafened) ? 'scale-100 opacity-100 rotate-45' : 'scale-0 opacity-0 rotate-45'}`} />
                  </div>
                </button>
                <button
                  onClick={toggleDeafen}
                  className={`group w-14 h-14 rounded-full flex items-center justify-center relative transition-colors ${(store.currentUser?.isDeafened || store.currentUser?.isServerDeafened)
                    ? 'bg-[#2B2D31] text-white'
                    : 'bg-surface hover:bg-surfaceHover text-white'
                    }`}
                >
                  <div className="flex items-center justify-center transition-transform duration-200 group-active:scale-95 group-hover:scale-110">
                    <Headphones weight="bold" size={24} />
                    <div className={`absolute w-[30px] h-[3px] bg-danger rounded-full transition-all duration-300 origin-center ${(store.currentUser?.isDeafened || store.currentUser?.isServerDeafened) ? 'scale-100 opacity-100 rotate-45' : 'scale-0 opacity-0 rotate-45'}`} />
                  </div>
                </button>
                <button onClick={handleEndCall} className="group bg-danger hover:bg-red-600 text-white font-bold py-3.5 px-8 rounded-full flex items-center gap-3 transition-colors text-[15px]">
                  <div className="transition-transform duration-300 group-hover:-rotate-12 group-hover:scale-110">
                    <PhoneOff weight="bold" size={20} />
                  </div>
                  {t('main.voice.endCall')}
                </button>
              </div>
            )}

            {store.currentChannelId && !store.currentCallUser && (
              <div className={[
                "absolute bottom-10 left-1/2 -translate-x-1/2 bg-panelBg px-6 py-4 rounded-full flex gap-4 items-center shadow-2xl border border-[#303035] z-50",
                controlsShake ? "animate-shake" : ""
              ].join(" ")}>
                <button
                  onClick={toggleMute}
                  className={`group w-14 h-14 rounded-full flex items-center justify-center relative transition-colors ${(store.currentUser?.isMuted || store.currentUser?.isServerMuted || store.currentUser?.isServerDeafened)
                    ? 'bg-[#2B2D31] text-white'
                    : 'bg-surface hover:bg-surfaceHover text-white'
                    }`}
                >
                  <div className="flex items-center justify-center transition-transform duration-200 group-active:scale-95 group-hover:scale-110">
                    <Mic weight="bold" size={24} />
                    <div className={`absolute w-[30px] h-[3px] bg-danger rounded-full transition-all duration-300 origin-center ${(store.currentUser?.isMuted || store.currentUser?.isServerMuted || store.currentUser?.isServerDeafened) ? 'scale-100 opacity-100 rotate-45' : 'scale-0 opacity-0 rotate-45'}`} />
                  </div>
                </button>
                <button
                  onClick={toggleDeafen}
                  className={`group w-14 h-14 rounded-full flex items-center justify-center relative transition-colors ${(store.currentUser?.isDeafened || store.currentUser?.isServerDeafened)
                    ? 'bg-[#2B2D31] text-white'
                    : 'bg-surface hover:bg-surfaceHover text-white'
                    }`}
                >
                  <div className="flex items-center justify-center transition-transform duration-200 group-active:scale-95 group-hover:scale-110">
                    <Headphones weight="bold" size={24} />
                    <div className={`absolute w-[30px] h-[3px] bg-danger rounded-full transition-all duration-300 origin-center ${(store.currentUser?.isDeafened || store.currentUser?.isServerDeafened) ? 'scale-100 opacity-100 rotate-45' : 'scale-0 opacity-0 rotate-45'}`} />
                  </div>
                </button>
                <button onClick={() => signalRService.leaveChannel()} className="group bg-danger hover:bg-red-600 text-white font-bold py-3.5 px-8 rounded-full flex items-center gap-3 transition-colors text-[15px]">
                  <div className="transition-transform duration-300 group-hover:-rotate-12 group-hover:scale-110">
                    <Phone weight="bold" size={20} />
                  </div>
                  {t('main.voice.endCall')}
                </button>
              </div>
            )}

            <div className="absolute bottom-4 left-4 z-50" onMouseEnter={() => setShowPingTooltip(true)} onMouseLeave={() => setShowPingTooltip(false)}>
              <div className="w-10 h-10 rounded-full bg-surface flex items-center justify-center cursor-pointer hover:bg-surfaceHover transition-colors shadow-lg" style={{ color: getPingColor() }}>
                {ping < 0 ? <WifiOff weight="bold" size={18} /> : <Wifi weight="bold" size={18} />}
              </div>
              {showPingTooltip && (
                <div className="absolute bottom-12 left-0 bg-surface border border-[#303035] rounded-xl px-4 py-2 shadow-xl whitespace-nowrap">
                  <div className="text-xs text-textMuted mb-1 font-bold tracking-wider">{t('main.voice.ping')}</div>
                  <div className="font-bold" style={{ color: getPingColor() }}>{ping < 0 ? t('main.voice.offline') : t('main.voice.pingValue', { ping, defaultValue: `${ping} мс` })}</div>
                </div>
              )}
            </div>
          </div>

        </div>
      </div>

      {renderModal('createChannel',
        <div className="bg-panelBg p-8 rounded-3xl w-[400px] shadow-2xl">
          <h2 className="text-xl font-bold mb-6 text-white">{t('modals.createChannel.title')}</h2>
          <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">{t('modals.createChannel.label')}</label>
          <input type="text" value={newChannelName} onChange={e => setNewChannelName(e.target.value)} maxLength={25} onKeyDown={e => e.key === 'Enter' && handleCreateChannel()} placeholder={t('modals.createChannel.placeholder')} className="w-full bg-surface text-white rounded-xl p-3 mb-6 outline-none focus:ring-2 focus:ring-[#c70060]" />
          {error && <p className="text-danger text-sm mb-4 font-medium">{error}</p>}
          <div className="flex gap-4">
            <button onClick={closeAndResetModals} className="flex-1 bg-surface text-white py-3 rounded-xl font-bold hover:bg-surfaceHover transition-colors">{t('common.cancel')}</button>
            <button onClick={handleCreateChannel} className="flex-1 bg-[#c70060] text-white py-3 rounded-xl font-bold hover:opacity-90 transition-opacity">{t('modals.createChannel.submit')}</button>
          </div>
        </div>
      )}

      {renderModal('channelEdit',
        <div className="bg-panelBg p-8 rounded-3xl w-[400px] shadow-2xl">
          <h2 className="text-xl font-bold mb-6 text-white">{t('modals.renameChannel.title')}</h2>
          <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">{t('modals.renameChannel.label')}</label>
          <input type="text" value={editChannelName} onChange={e => setEditChannelName(e.target.value)} maxLength={25} onKeyDown={e => e.key === 'Enter' && saveChannelEdit()} className="w-full bg-surface text-white rounded-xl p-3 mb-6 outline-none focus:ring-2 focus:ring-[#c70060]" />
          {error && <p className="text-danger text-sm mb-4 font-medium">{error}</p>}
          <div className="flex gap-4">
            <button onClick={closeAndResetModals} className="flex-1 bg-surface text-white py-3 rounded-xl font-bold hover:bg-surfaceHover transition-colors">{t('common.cancel')}</button>
            <button onClick={saveChannelEdit} className="flex-1 bg-[#c70060] text-white py-3 rounded-xl font-bold hover:opacity-90 transition-opacity">{t('modals.renameChannel.submit')}</button>
          </div>
        </div>
      )}

      {renderModal('addFriend',
        <div className="bg-panelBg p-8 rounded-3xl w-[400px] shadow-2xl">
          <h2 className="text-xl font-bold mb-2 text-white">{t('modals.addFriend.title')}</h2>
          <p className="text-textMuted text-sm mb-6 font-medium">{t('modals.addFriend.desc')}</p>
          <input
            type="text"
            value={friendName}
            onChange={e => { setFriendName(e.target.value); if (friendRequestStatus === 'notfound' || friendRequestStatus === 'alreadyfriend') setFriendRequestStatus('idle'); }}
            maxLength={25}
            onKeyDown={e => e.key === 'Enter' && handleAddFriend()}
            placeholder={t('modals.addFriend.placeholder')}
            className={`w-full bg-surface text-white rounded-xl p-3 outline-none focus:ring-2 ${friendRequestStatus === 'notfound' ? 'focus:ring-danger ring-2 ring-danger' :
              friendRequestStatus === 'alreadyfriend' ? 'focus:ring-yellow-400 ring-2 ring-yellow-400' :
                'focus:ring-[#c70060]'
              }`}
          />
          {friendRequestStatus === 'notfound' && (
            <p className="text-danger text-sm mt-2 mb-0 font-medium">{t('modals.addFriend.errorNotFound')}</p>
          )}
          {friendRequestStatus === 'alreadyfriend' && (
            <p className="text-yellow-400 text-sm mt-2 mb-0 font-medium">{t('modals.addFriend.errorAlreadyFriend')}</p>
          )}
          <div className="flex gap-4 mt-4">
            <button
              onClick={() => { closeAndResetModals(); setFriendRequestStatus('idle'); }}
              className="flex-1 bg-surface text-white py-3 rounded-xl font-bold hover:bg-surfaceHover transition-colors"
            >{t('common.cancel')}</button>
            <button
              onClick={handleAddFriend}
              disabled={friendRequestStatus === 'loading' || friendRequestStatus === 'sent'}
              className={`flex-1 py-3 rounded-xl font-bold transition-all ${friendRequestStatus === 'sent'
                ? 'bg-green-600 text-white cursor-default scale-105'
                : friendRequestStatus === 'loading'
                  ? 'bg-[#c70060]/60 text-white cursor-wait'
                  : 'bg-[#c70060] text-white hover:opacity-90'
                }`}
            >
              {friendRequestStatus === 'sent' ? `✓ ${t('modals.addFriend.sent')}` : friendRequestStatus === 'loading' ? '...' : t('modals.addFriend.submit')}
            </button>
          </div>
        </div>
      )}

      {renderModal('settings',
        <div className="bg-panelBg rounded-3xl w-[500px] max-h-[80vh] flex flex-col overflow-hidden shadow-2xl">
          <div className="flex items-center justify-between p-6 pb-0">
            <h2 className="text-xl font-bold text-white">{t('settings.title', 'Настройки')}</h2>
            <button onClick={closeAndResetModals} className="group text-textMuted hover:text-white transition-all duration-200 hover:rotate-90 hover:scale-110 active:scale-90 p-1.5 rounded-lg hover:bg-surface"><X weight="bold" size={24} /></button>
          </div>
          <div className="flex gap-2 px-6 pt-4">
            <button onClick={() => setSettingsTab('general')} className={`px-4 py-2 rounded-xl font-bold text-sm transition-colors ${settingsTab === 'general' ? 'bg-[#c70060] text-white' : 'bg-surface text-textMuted hover:text-white'}`}>{t('settings.tabs.general')}</button>
            <button onClick={() => setSettingsTab('audio')} className={`px-4 py-2 rounded-xl font-bold text-sm transition-colors ${settingsTab === 'audio' ? 'bg-[#c70060] text-white' : 'bg-surface text-textMuted hover:text-white'}`}>{t('settings.tabs.audio')}</button>
            <button onClick={() => setSettingsTab('privacy')} className={`px-4 py-2 rounded-xl font-bold text-sm transition-colors ${settingsTab === 'privacy' ? 'bg-[#c70060] text-white' : 'bg-surface text-textMuted hover:text-white'}`}>{t('settings.tabs.privacy')}</button>
          </div>
          <div className="p-6 overflow-y-auto flex-1">
            {settingsTab === 'general' && (
              <div className="space-y-6">
                <div>
                  <label className="text-xs font-bold text-textMuted mb-3 block tracking-wider">{t('settings.general.system')}</label>

                  <div className="flex items-center justify-between bg-surface p-4 rounded-xl mb-3">
                    <div className="mr-4">
                      <span className="font-semibold text-white text-[15px]">{t('settings.general.language')}</span>
                      <p className="text-xs text-textMuted mt-1">{t('settings.general.languageDesc')}</p>
                    </div>
                    <select
                      value={language}
                      onChange={(e) => {
                        const newLang = e.target.value;
                        setLanguage(newLang);
                        i18n.changeLanguage(newLang);
                      }}
                      className="bg-[#2B2D31] text-white rounded-xl px-3 py-2 outline-none border border-[#303035] focus:ring-2 focus:ring-[#c70060] font-bold text-sm cursor-pointer"
                    >
                      <option value="ru">Русский</option>
                      <option value="en">English</option>
                    </select>
                  </div>

                  <div className="flex items-center justify-between bg-surface p-4 rounded-xl">
                    <div className="mr-4">
                      <span className="font-semibold text-white text-[15px]">{t('settings.general.autoLaunch')}</span>
                      <p className="text-xs text-textMuted mt-1">{t('settings.general.autoLaunchDesc')}</p>
                    </div>
                    <Md3Switch checked={autoLaunch} onChange={handleAutoLaunchToggle} />
                  </div>

                  <div className="flex items-center justify-between bg-surface p-4 rounded-xl mt-3">
                    <div className="mr-4">
                      <span className="font-semibold text-white text-[15px]">{t('settings.general.minimizeToTray')}</span>
                      <p className="text-xs text-textMuted mt-1">{t('settings.general.minimizeToTrayDesc')}</p>
                    </div>
                    <Md3Switch checked={minimizeToTray} onChange={(v) => {
                      setMinimizeToTray(v);
                      window.windowControls.setMinimizeToTray(v).catch(() => { });
                    }} />
                  </div>
                </div>
              </div>
            )}
            {settingsTab === 'audio' && (
              <div className="space-y-6">
                <div>
                  <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">{t('settings.audio.inputDevice')}</label>
                  <select value={selectedInput} onChange={e => { setSelectedInput(e.target.value); webrtc.updateSettings(e.target.value, noiseSuppression); }} className="w-full bg-surface text-white rounded-xl p-3 outline-none focus:ring-2 focus:ring-[#c70060]">
                    <option value="default">{t('settings.audio.default')}</option>
                    {audioDevices.inputs.length === 0 && selectedInput !== 'default' && (
                      <option value={selectedInput}>{t('common.loading')}</option>
                    )}
                    {audioDevices.inputs.map(d => <option key={d.deviceId} value={d.deviceId}>{d.label || t('settings.audio.micFallback')}</option>)}
                  </select>
                </div>
                <div>
                  <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">{t('settings.audio.outputDevice')}</label>
                  <select value={selectedOutput} onChange={e => { setSelectedOutput(e.target.value); webrtc.setOutputDevice(e.target.value); }} className="w-full bg-surface text-white rounded-xl p-3 outline-none focus:ring-2 focus:ring-[#c70060]">
                    <option value="default">{t('settings.audio.default')}</option>
                    {audioDevices.outputs.length === 0 && selectedOutput !== 'default' && (
                      <option value={selectedOutput}>{t('common.loading')}</option>
                    )}
                    {audioDevices.outputs.map(d => <option key={d.deviceId} value={d.deviceId}>{d.label || t('settings.audio.speakerFallback')}</option>)}
                  </select>
                </div>
                <div>
                  <Md3Slider
                    min={0}
                    max={200}
                    step={5}
                    value={inputVolume}
                    label={t('settings.audio.inputVolume')}
                    showPercentage
                    onChange={v => webrtc.setInputVolume(v)}
                    onChangeEnd={setInputVolume}
                  />
                </div>
                <div>
                  <Md3Slider
                    min={0}
                    max={200}
                    step={5}
                    value={outputVolume}
                    label={t('settings.audio.outputVolume')}
                    showPercentage
                    onChange={v => webrtc.setOutputVolume(v)}
                    onChangeEnd={setOutputVolume}
                  />
                </div>
                <div className="flex items-center justify-between bg-surface p-4 rounded-xl">
                  <span className="font-semibold text-white">{t('settings.audio.noiseSuppression')}</span>
                  <Md3Switch checked={noiseSuppression} onChange={v => {
                    setNoiseSuppression(v);
                    webrtc.setNoiseSuppression(v);
                    webrtc.updateSettings(selectedInput, v);
                  }} />
                </div>
                <div className="bg-surface p-4 rounded-xl space-y-3">
                  <div className="flex items-center justify-between">
                    <div>
                      <span className="font-semibold text-white text-[15px]">{t('settings.audio.calibrateSensitivity', 'Калибровка чувствительности')}</span>
                      <p className="text-xs text-textMuted mt-1">{t('settings.audio.calibrateDesc', 'Распознавание уровня шума вашего микрофона')}</p>
                    </div>
                    {isCalibrating && (
                      <span className="text-yellow-500 flex items-center gap-1 text-sm font-bold animate-pulse">
                        {t('settings.audio.doNotSpeak', 'Не говорите!')}
                      </span>
                    )}
                    {calibrationSuccess && (
                      <span className="text-[#22c55e] flex items-center gap-1 text-sm font-bold animate-pulse">
                        <Check weight="bold" size={16} />
                        {t('settings.audio.calibrationComplete', 'Успешно откалибровано!')}
                      </span>
                    )}
                  </div>
                  <button
                    onClick={handleManualCalibration}
                    disabled={isCalibrating}
                    className={`w-full py-3 rounded-xl font-bold flex items-center justify-center gap-2 transition-all duration-300 ${isCalibrating
                      ? 'bg-yellow-600 text-white cursor-default'
                      : calibrationSuccess
                        ? 'bg-[#22c55e] text-white hover:opacity-90'
                        : 'bg-[#c70060] text-white hover:opacity-90 active:scale-95'
                      }`}
                  >
                    {isCalibrating ? (
                      <>
                        <span className="animate-spin rounded-full h-4 w-4 border-2 border-white border-t-transparent"></span>
                        {t('settings.audio.calibrating', { seconds: calibrationCountdown, defaultValue: `Калибровка... ${calibrationCountdown}с` })}
                      </>
                    ) : (
                      <>
                        <Mic weight="bold" size={18} />
                        {t('settings.audio.calibrateButton', 'Откалибровать микрофон')}
                      </>
                    )}
                  </button>
                </div>
              </div>
            )}
            {settingsTab === 'privacy' && (
              <div className="space-y-6">
                <div>
                  <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">{t('settings.privacy.newPassword')}</label>
                  <div className="relative">
                    <input type={showPrivacyPass ? 'text' : 'password'} value={newPassword} onChange={e => { setNewPassword(e.target.value); setPrivacyError(''); }} maxLength={25} placeholder={t('settings.privacy.passwordHint')} className="w-full bg-surface text-white rounded-xl p-3 outline-none pr-10 focus:ring-2 focus:ring-[#c70060]" />
                    <button onClick={() => setShowPrivacyPass(!showPrivacyPass)} className="absolute right-3 top-3 text-textMuted hover:text-white transition-colors">{showPrivacyPass ? <EyeOff weight="bold" size={20} /> : <Eye weight="bold" size={20} />}</button>
                  </div>
                  {privacyError && <p className="text-danger text-sm mt-2 font-medium">{privacyError}</p>}
                  <button onClick={changePassword} className="mt-3 w-full bg-[#c70060] hover:opacity-90 text-white py-3 rounded-xl font-bold transition-opacity">{t('settings.privacy.changePassword')}</button>
                </div>
                <button onClick={handleLogout} className="group w-full bg-danger hover:bg-red-600 text-white py-3 rounded-xl font-bold flex items-center justify-center gap-2 transition-colors">
                  <div className="transition-transform duration-200 group-hover:-translate-x-1">
                    <LogOut weight="bold" size={18} />
                  </div>
                  {t('settings.privacy.logout')}
                </button>
              </div>
            )}
          </div>
        </div>
      )}

      {store.pendingChannelSwitch && (
        <div className="fixed inset-0 z-[999] bg-black/70 backdrop-blur-md flex items-center justify-center p-4">
          <div className="bg-panelBg p-8 rounded-3xl w-[400px] text-center shadow-2xl">
            <div className="flex items-center justify-between mb-4">
              <h2 className="text-xl font-bold text-white">{t('modals.switchChannel.title', 'Сменить канал?')}</h2>
              <button
                onClick={() => store.setPendingChannelSwitch(null)}
                className="group text-textMuted hover:text-white transition-all duration-200 hover:rotate-90 hover:scale-110 active:scale-90 p-1.5 rounded-lg hover:bg-surface"
              >
                <X weight="bold" size={24} />
              </button>
            </div>

            <p className="text-textMuted mb-8 font-medium">
              {t('modals.switchChannel.desc', 'Вы покинете текущий канал и перейдёте в другой.')}
            </p>

            <div className="flex gap-4">
              <button
                onClick={() => store.setPendingChannelSwitch(null)}
                disabled={isSwitchingChannel}
                className="flex-1 bg-surface text-white py-3 rounded-xl font-bold hover:bg-surfaceHover transition-colors"
              >
                {t('modals.switchChannel.stay', 'Остаться')}
              </button>
              <button
                onClick={confirmChannelSwitch}
                disabled={isSwitchingChannel}
                className="flex-1 bg-[#c70060] text-white py-3 rounded-xl font-bold hover:opacity-90 transition-opacity"
              >
                {isSwitchingChannel ? t('modals.switchChannel.switching', 'Переход...') : t('modals.switchChannel.switch', 'Перейти')}
              </button>
            </div>
          </div>
        </div>
      )}

      {renderModal('inviteToChannel',
        <div className="bg-panelBg p-8 rounded-3xl w-[400px] shadow-2xl">
          <h2 className="text-xl font-bold mb-2 text-white">{t('modals.inviteToChannel.title', 'Пригласить в канал')}</h2>
          <p className="text-textMuted text-sm mb-6">{store.selectedChannelForInvite?.name}</p>
          <input type="text" value={inviteFriendSearch} onChange={e => setInviteFriendSearch(e.target.value)} placeholder={t('modals.inviteToChannel.searchPlaceholder', 'Поиск среди друзей...')} className="w-full bg-surface text-white rounded-xl p-3 mb-4 outline-none focus:ring-2 focus:ring-[#c70060]" />
          <div className="max-h-[300px] overflow-y-auto space-y-2">
            {(() => {
              const filteredFriends = store.friends
                .filter(f => f.displayName.toLowerCase().includes(inviteFriendSearch.toLowerCase()))
                .filter(f => !store.channelMembersCache[store.selectedChannelForInvite?.id || '']?.some(m => m.id === f.id));

              return (
                <>
                  {filteredFriends.map(f => (
                    <div key={f.id} className="flex items-center gap-3 p-3 bg-surface rounded-xl hover:bg-surfaceHover transition-colors">
                      <div className="w-[47px] h-[47px] shrink-0 relative"><AvatarImg src={f.avatarBase64} size={47} bgColor={f.avatarColor} /></div>
                      <span className="flex-1 font-semibold text-white truncate">{f.displayName}</span>
                      <button
                        onClick={() => handleInviteToChannel(f.id)}
                        disabled={sentInvites.has(f.id)}
                        className={`py-2 px-4 rounded-xl text-sm font-bold transition-all shrink-0 ${sentInvites.has(f.id)
                          ? 'bg-success/20 text-success cursor-default'
                          : 'bg-success hover:bg-green-600 text-white hover:opacity-90'
                          }`}
                      >
                        {sentInvites.has(f.id) ? t('modals.inviteToChannel.sent', '✓ Отправлено') : t('common.invite', 'Пригласить')}
                      </button>
                    </div>
                  ))}
                  {filteredFriends.length === 0 && (
                    <p className="text-textMuted text-center py-4 font-medium">{t('modals.inviteToChannel.noFriends', 'Друзья не найдены')}</p>
                  )}
                </>
              );
            })()}
          </div>
          <button onClick={closeAndResetModals} className="w-full mt-4 bg-surface text-white py-3 rounded-xl font-bold hover:bg-surfaceHover transition-colors">{t('common.close', 'Закрыть')}</button>
        </div>
      )}

      {renderModal('channelMembers',
        <div className="bg-panelBg p-8 rounded-3xl w-[420px] shadow-2xl">
          <div className="flex items-center justify-between mb-2">
            <h2 className="text-xl font-bold text-white flex items-center gap-3"><Users weight="bold" size={24} /> {t('modals.members.title', 'Участники')}</h2>
            <button onClick={closeAndResetModals} className="group text-textMuted hover:text-white transition-all duration-200 hover:rotate-90 hover:scale-110 active:scale-90 p-1.5 rounded-lg hover:bg-surface"><X weight="bold" size={24} /></button>
          </div>
          <p className="text-textMuted text-sm mb-6 truncate">{store.selectedChannelForMembers?.name}</p>
          <div className="max-h-[350px] overflow-y-auto space-y-2 pr-2">
            {store.channelMembers.length === 0 && (
              <div className="flex justify-center py-8">
                <div className="w-6 h-6 border-2 border-[#c70060] border-t-transparent rounded-full animate-spin" />
              </div>
            )}
            {store.channelMembers.map(m => (
              <div key={m.id} onContextMenu={e => handleContextMenu(e, 'channelMember', m)} className="flex items-center gap-3 p-3 bg-surface rounded-xl hover:bg-surfaceHover transition-colors cursor-pointer">
                <div className="relative w-[47px] h-[47px] shrink-0">
                  <AvatarImg src={m.avatarBase64} size={47} bgColor={m.avatarColor} />
                  <div className={`absolute -bottom-0.5 -right-0.5 w-3.5 h-3.5 rounded-full border-[3px] border-surface ${m.isOnline ? 'bg-success' : 'bg-gray-500'}`} />
                </div>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="font-semibold text-white truncate">{m.displayName}</span>
                    {store.selectedChannelForMembers?.ownerId === m.id && (
                      <span className="text-[10px] font-bold bg-yellow-500/20 text-yellow-500 px-2 py-0.5 rounded-md flex items-center gap-1 shrink-0"><Crown weight="bold" size={12} /> {t('modals.members.creator', 'Создатель')}</span>
                    )}
                  </div>
                  <p className="text-xs text-textMuted truncate">@{m.username}</p>
                </div>

              </div>
            ))}
          </div>
        </div>
      )}

      {renderModal('kickConfirm',
        <div className="bg-panelBg p-8 rounded-3xl w-[400px] text-center shadow-2xl">
          <div className="w-16 h-16 bg-danger/20 rounded-full flex items-center justify-center mx-auto mb-4"><UserX weight="bold" size={32} className="text-danger" /></div>
          <h2 className="text-xl font-bold mb-2 text-white">{t('modals.kick.title')}</h2>
          <p className="text-textMuted mb-8">
            <Trans i18nKey="modals.kick.desc" values={{ name: store.userToKick?.displayName }}>
              Вы уверены, что хотите лишить пользователя <span className="text-white font-bold">{store.userToKick?.displayName}</span> доступа к каналу?
            </Trans>
          </p>
          <div className="flex gap-4">
            <button onClick={() => { store.setModal('kickConfirm', false); store.setUserToKick(null); }} className="flex-1 bg-surface text-white py-3 rounded-xl font-bold hover:bg-surfaceHover transition-colors">{t('common.cancel')}</button>
            <button onClick={handleKickConfirm} className="flex-1 bg-danger text-white py-3 rounded-xl font-bold hover:bg-red-600 transition-colors">{t('modals.kick.submit')}</button>
          </div>
        </div>
      )}

      {renderModal('channelFull',
        <div className="bg-panelBg p-8 rounded-3xl w-[400px] text-center shadow-2xl border border-danger/30">
          <div className="w-20 h-20 bg-danger/20 rounded-full flex items-center justify-center mx-auto mb-4"><Users weight="bold" size={40} className="text-danger" /></div>
          <h2 className="text-xl font-bold mb-4 text-white">{t('modals.channelFull.title', 'Канал переполнен')}</h2>
          <p className="text-textMuted mb-8">{t('modals.channelFull.desc', 'Максимальное количество участников в канале — 10 человек. Подождите, пока кто-то выйдет.')}</p>
          <button onClick={closeAndResetModals} className="w-full bg-surface text-white py-3 rounded-xl font-bold hover:bg-surfaceHover transition-colors">{t('modals.channelFull.gotIt', 'Понятно')}</button>
        </div>
      )}

      {renderModal('userVolume',
        <div className="bg-panelBg p-8 rounded-3xl w-[400px] shadow-2xl">
          <h2 className="text-xl font-bold mb-2 text-white">{t('modals.userVolume.title', 'Громкость пользователя')}</h2>
          <p className="text-textMuted text-sm mb-6 font-medium">{volumeUser?.displayName}</p>
          <div>
            <Md3Slider
              min={0}
              max={200}
              step={5}
              value={volumeUserValue}
              label={t('modals.userVolume.label', 'ГРОМКОСТЬ')}
              showPercentage
              onChange={v => {
                if (volumeUser) webrtc.setUserVolumeRealtime(volumeUser.id, v);
              }}
              onChangeEnd={v => {
                setVolumeUserValue(v);
                if (volumeUser) webrtc.setUserVolume(volumeUser.id, v);
              }}
            />
          </div>
          <button onClick={closeAndResetModals} className="w-full mt-6 bg-surface text-white py-3 rounded-xl font-bold hover:bg-surfaceHover transition-colors">{t('common.close', 'Закрыть')}</button>
        </div>
      )}

      {renderModal('incomingCall',
        <div className="bg-panelBg p-8 rounded-3xl w-[350px] text-center shadow-2xl">
          <div className="w-[87px] h-[87px] mx-auto mb-4 relative">
            <AvatarImg src={store.incomingCall?.callerAvatarBase64 || null} size={87} bgColor={store.incomingCall?.callerAvatarColor} />
          </div>
          <h2 className="text-xl font-bold mb-2 text-white">{store.incomingCall?.callerName}</h2>
          <p className="text-textMuted mb-8 font-medium">{t('toasts.incomingCall', 'Входящий звонок...')}</p>
          <div className="flex gap-4">
            <button onClick={handleDeclineCall} className="flex-1 bg-danger text-white py-3 rounded-xl font-bold hover:bg-red-600 transition-colors flex items-center justify-center gap-2"><PhoneOff weight="bold" size={18} /> {t('main.notifications.decline', 'Отклонить')}</button>
            <button onClick={handleAcceptCall} className="flex-1 bg-success text-white py-3 rounded-xl font-bold hover:bg-green-600 transition-colors flex items-center justify-center gap-2"><Phone weight="bold" size={18} /> {t('main.notifications.accept', 'Принять')}</button>
          </div>
        </div>
      )}

      {renderModal('incomingChannelInvite',
        <div className="bg-panelBg p-8 rounded-3xl w-[350px] text-center shadow-2xl">
          {(() => {
            const invite = store.incomingChannelInvite;
            if (!invite) return null;
            const users = store.channelUsersMap[invite.channelId] || [];
            const displayUsers = users.length > 0 ? users : [{ id: invite.senderId, displayName: invite.senderName, avatarBase64: null, avatarColor: '#c70060', username: '', isOnline: true, isMuted: false, isDeafened: false, isSpeaking: false }];
            return (
              <>
                <div className="flex justify-center mb-4">
                  {displayUsers.slice(0, 3).map((u, i) => (
                    <div key={u.id} className="w-[87px] h-[87px] rounded-full border-[4px] border-panelBg relative shrink-0 shadow-lg" style={{ marginLeft: i === 0 ? 0 : '-1.5rem', zIndex: 10 - i }}>
                      <AvatarImg src={u.avatarBase64 || null} size={87} bgColor={u.avatarColor} />
                    </div>
                  ))}
                </div>
                <h2 className="text-xl font-bold mb-2 text-white truncate px-2">{invite.channelName}</h2>
                <p className="text-textMuted mb-8 font-medium">{t('toasts.incomingChannelInvite', 'Вас зовут в канал')}</p>
                <div className="flex gap-4">
                  <button onClick={() => { store.setModal('incomingChannelInvite', false); store.setIncomingChannelInvite(null); signalRService.stopRingtone(); }} className="flex-1 bg-danger text-white py-3 rounded-xl font-bold hover:bg-red-600 transition-colors flex items-center justify-center gap-2"><PhoneOff weight="bold" size={18} /> {t('common.dismiss', 'Сбросить')}</button>
                  <button onClick={() => { handleAcceptChannelInvite(invite.channelId); store.setModal('incomingChannelInvite', false); store.setIncomingChannelInvite(null); signalRService.stopRingtone(); store.setChannelInvites(store.channelInvites.filter(i => i.channelId !== invite.channelId)); }} className="flex-1 bg-success text-white py-3 rounded-xl font-bold hover:bg-green-600 transition-colors flex items-center justify-center gap-2"><Phone weight="bold" size={18} /> {t('main.notifications.join', 'Войти')}</button>
                </div>
              </>
            );
          })()}
        </div>
      )}

      {store.modals.profile && store.selectedProfileUser && (
        <div className="fixed inset-0 z-[9999] bg-black/70 backdrop-blur-md flex items-center justify-center p-4">
          <div className="bg-panelBg w-[400px] rounded-[32px] overflow-hidden shadow-2xl relative border border-[#303035]">
            <div
              className="h-32 w-full relative transition-colors duration-500"
              style={{ backgroundColor: editProfileAvatarBase64 ? editProfileAvatarColor : store.selectedProfileUser?.avatarColor }}
            >
              <button
                onClick={() => store.closeProfileOnly()}
                className="absolute top-4 right-4 text-white/70 hover:text-white bg-black/20 hover:bg-black/40 p-2 rounded-full backdrop-blur-md transition-all duration-200 hover:rotate-90 hover:scale-110 active:scale-90"
              >
                <X weight="bold" size={20} />
              </button>

              {store.selectedProfileUser?.id === store.currentUser?.id && !isEditingProfile && (
                <button
                  onClick={() => setIsEditingProfile(true)}
                  className="absolute top-4 right-14 text-white/70 hover:text-white bg-black/20 hover:bg-black/40 p-2 rounded-full backdrop-blur-md transition-all"
                  title={t('profile.editTitle', 'Редактировать профиль')}
                >
                  <Edit2 weight="bold" size={20} />
                </button>
              )}
            </div>

            <div className="px-8 pb-8 relative mt-[-56px]">
              <div className="flex items-start gap-6 mb-6 relative z-10">
                {/* Avatar container */}
                <div className="relative group shrink-0">
                  <div className="w-[112px] h-[112px] rounded-full border-[6px] border-panelBg bg-panelBg relative shadow-xl">
                    <AvatarImg
                      src={isEditingProfile ? (editProfileAvatarBase64 || store.selectedProfileUser?.avatarBase64) : store.selectedProfileUser?.avatarBase64}
                      size={100}
                      bgColor={isEditingProfile ? (editProfileAvatarBase64 ? editProfileAvatarColor : store.selectedProfileUser?.avatarColor) : store.selectedProfileUser?.avatarColor}
                    />
                  </div>
                  <div className={`absolute bottom-1.5 right-1.5 w-7 h-7 rounded-full border-[4px] border-panelBg ${store.selectedProfileUser?.id === store.currentUser?.id
                    ? (serverConnected ? 'bg-success' : 'bg-gray-500')
                    : (store.selectedProfileUser?.isOnline ? 'bg-success' : 'bg-gray-500')
                    }`} />

                  {isEditingProfile && (
                    <div
                      className="absolute inset-[6px] bg-black/60 rounded-full opacity-0 group-hover:opacity-100 transition-opacity flex items-center justify-center cursor-pointer backdrop-blur-sm"
                      onClick={() => profileFileInputRef.current?.click()}
                    >
                      <Camera weight="bold" size={32} className="text-white" />
                      <input
                        ref={profileFileInputRef}
                        type="file"
                        accept="image/*"
                        className="hidden"
                        onChange={e => onFileChange(e, 'profile')}
                      />
                    </div>
                  )}
                </div>

                {/* About me thought bubble on the right */}
                {!isEditingProfile && store.selectedProfileUser?.aboutMe && (
                  <div className="flex-1 mt-10 relative animate-fade-in">
                    {/* Little tail circles for the thought bubble */}
                    <div className="absolute w-1.5 h-1.5 rounded-full bg-[#303035] left-[-18px] top-[-12px] opacity-90" />
                    <div className="absolute w-2.5 h-2.5 rounded-full bg-[#303035] left-[-8px] top-[-5px] opacity-90" />

                    {/* Thought bubble container */}
                    <div className="bg-[#2B2D31] border border-[#303035] p-3 rounded-2xl shadow-md min-h-[60px] flex items-center justify-center">
                      <p className="text-white/90 text-sm font-medium leading-relaxed break-words whitespace-pre-wrap text-center w-full">
                        {store.selectedProfileUser.aboutMe}
                      </p>
                    </div>
                  </div>
                )}
              </div>

              <div className="mb-6">
                {isEditingProfile ? (
                  <div className="space-y-4 animate-fade-in">
                    <div>
                      <label className="text-[10px] font-bold text-textMuted mb-2 block tracking-wider uppercase">{t('profile.displayName', 'Отображаемое имя')}</label>
                      <input
                        type="text"
                        value={editProfileDisplayName}
                        onChange={e => {
                          setEditProfileDisplayName(e.target.value);
                          setError('');
                        }}
                        maxLength={20}
                        className="bg-surface w-full p-3 rounded-xl text-white font-bold text-base outline-none focus:ring-2 focus:ring-[#c70060] transition-shadow"
                      />
                      {error && <p className="text-danger text-xs mt-2 font-medium">{error}</p>}
                    </div>
                    <div>
                      <label className="text-[10px] font-bold text-textMuted mb-2 block tracking-wider uppercase">{t('profile.aboutMe', 'О себе')}</label>
                      <textarea
                        value={editProfileAboutMe}
                        onChange={e => setEditProfileAboutMe(e.target.value)}
                        maxLength={150}
                        rows={3}
                        placeholder={t('profile.aboutMePlaceholder', 'Напишите немного о себе...')}
                        className="bg-surface w-full p-3 rounded-xl text-white text-sm outline-none focus:ring-2 focus:ring-[#c70060] resize-none transition-shadow"
                      />
                    </div>
                  </div>
                ) : (
                  <div className="animate-fade-in text-left">
                    <h2 className="text-3xl font-black text-white tracking-tight break-words">{store.selectedProfileUser?.displayName}</h2>
                    <p
                      className={`text-base mt-1.5 font-bold cursor-pointer transition-opacity inline-block ${isLoginCopied ? 'text-success' : 'text-[#c70060] hover:underline hover:opacity-80'}`}
                      title={isLoginCopied ? "" : t('profile.copyLogin', 'Скопировать логин')}
                      onClick={() => {
                        if (store.selectedProfileUser && !isLoginCopied) {
                          navigator.clipboard.writeText(store.selectedProfileUser.username);
                          setIsLoginCopied(true);
                          setTimeout(() => setIsLoginCopied(false), 2000);
                        }
                      }}
                    >
                      {isLoginCopied ? t('profile.loginCopied', 'Скопировано!') : `@${store.selectedProfileUser?.username}`}
                    </p>
                  </div>
                )}
              </div>

              {!isEditingProfile && store.profileSource === 'channelMembers' && (
                <div className="flex flex-col gap-2 mb-4">
                  {store.currentChannelId === store.selectedChannelForMembers?.id && (
                    <button
                      onClick={() => {
                        if (store.selectedProfileUser) {
                          if (sentInvites.has(store.selectedProfileUser.id)) return;
                          if (!store.selectedProfileUser.isOnline) {
                            setOfflineToast(t('profile.userOffline', 'Пользователь не в сети'));
                            setTimeout(() => setOfflineToast(null), 3000);
                          } else if (store.selectedChannelForMembers) {
                            signalRService.callToChannel(
                              store.selectedProfileUser.id,
                              store.selectedChannelForMembers.id,
                              store.selectedChannelForMembers.name
                            );
                            addSentInvite(store.selectedProfileUser!.id);
                          }
                        }
                      }}
                      disabled={store.selectedProfileUser ? sentInvites.has(store.selectedProfileUser.id) : false}
                      className={`w-full py-3 rounded-xl font-bold flex items-center justify-center gap-2 transition-all ${store.selectedProfileUser && sentInvites.has(store.selectedProfileUser.id)
                        ? 'bg-success/20 text-success cursor-default'
                        : 'bg-success text-white hover:opacity-90 hover:shadow-[0_0_25px_rgba(34,197,94,0.5)] active:shadow-[0_0_15px_rgba(34,197,94,0.8)] active:scale-[0.98]'
                        }`}
                    >
                      <Phone weight="bold" size={18} /> {store.selectedProfileUser && sentInvites.has(store.selectedProfileUser.id) ? t('profile.inviting', 'Зовём...') : t('profile.inviteToChannel', 'Позвать в канал')}
                    </button>
                  )}
                  {store.selectedChannelForMembers?.ownerId === store.currentUser?.id && (
                    <button
                      onClick={() => {
                        if (store.selectedProfileUser) {
                          store.setUserToKick(store.selectedProfileUser);
                          store.setModal('kickConfirm', true);
                        }
                        store.closeProfileOnly();
                      }}
                      className="w-full bg-surface text-danger py-3 rounded-xl font-bold hover:bg-[#2B2D31] transition-colors"
                    >
                      {t('profile.kick', 'Исключить из канала')}
                    </button>
                  )}
                </div>
              )}

              {isEditingProfile ? (
                <div className="flex gap-3 pt-4 border-t border-[#303035]">
                  <button
                    onClick={() => {
                      setIsEditingProfile(false);
                      setError('');
                      setEditProfileDisplayName(store.currentUser!.displayName);
                      setEditProfileAboutMe(store.currentUser!.aboutMe || '');
                      setEditProfileAvatarBase64(null);
                    }}
                    className="flex-1 bg-surface text-white py-3.5 rounded-xl font-bold hover:bg-surfaceHover transition-colors"
                  >
                    {t('common.cancel', 'Отмена')}
                  </button>
                  <button
                    onClick={saveProfileChanges}
                    className="flex-1 bg-[#c70060] text-white py-3.5 rounded-xl font-bold hover:shadow-[0_0_25px_rgba(199,0,96,0.5)] active:shadow-[0_0_15px_rgba(199,0,96,0.8)] active:scale-95 transition-all"
                  >
                    {t('common.save', 'Сохранить')}
                  </button>
                </div>
              ) : (
                <div className="flex justify-center items-center gap-4 mt-6">
                  {store.selectedProfileUser?.id === store.currentUser?.id ? (
                    <button
                      onClick={() => {
                        openMyAchievements();
                        store.closeProfileOnly();
                      }}
                      className="w-16 h-16 rounded-2xl bg-surface border border-[#303035] flex items-center justify-center text-[#c70060] hover:bg-[#c70060]/10 hover:border-[#c70060]/40 hover:shadow-[0_0_15px_rgba(199,0,96,0.25)] hover:scale-105 active:scale-95 transition-all"
                      title={t('achievements.title', 'Достижения')}
                    >
                      <Trophy weight="bold" size={28} />
                    </button>
                  ) : store.friends.some(f => f.id === store.selectedProfileUser?.id) ? (
                    <div className="flex justify-center items-center gap-4 w-full">
                      {/* Left: Remove Friend */}
                      <button
                        onClick={() => {
                          if (store.selectedProfileUser) signalRService.removeFriend(store.selectedProfileUser.id);
                          store.closeProfileOnly();
                        }}
                        className="w-16 h-16 rounded-2xl bg-surface border border-[#303035] flex items-center justify-center text-textMuted hover:text-danger hover:bg-danger/10 hover:border-danger/40 hover:shadow-[0_0_15px_rgba(239,68,68,0.25)] hover:scale-105 active:scale-95 transition-all"
                        title={t('profile.removeFriend', 'Удалить из друзей')}
                      >
                        <UserMinus weight="bold" size={28} />
                      </button>

                      {/* Center: Achievements */}
                      <button
                        onClick={() => {
                          if (store.selectedProfileUser) openUserAchievements(store.selectedProfileUser.id);
                          store.closeProfileOnly();
                        }}
                        className="w-16 h-16 rounded-2xl bg-surface border border-[#303035] flex items-center justify-center text-[#c70060] hover:bg-[#c70060]/10 hover:border-[#c70060]/40 hover:shadow-[0_0_15px_rgba(199,0,96,0.25)] hover:scale-105 active:scale-95 transition-all"
                        title={t('achievements.title', 'Достижения')}
                      >
                        <Trophy weight="bold" size={28} />
                      </button>

                      {/* Right: Call */}
                      <button
                        onClick={async () => {
                          if (store.selectedProfileUser) {
                            const ok = await signalRService.startCall(store.selectedProfileUser.id);
                            if (!ok) {
                              setOfflineToast(t('profile.userOffline', 'Пользователь не в сети'));
                              setTimeout(() => setOfflineToast(null), 3000);
                            }
                          }
                          store.closeProfileOnly();
                        }}
                        className="w-16 h-16 rounded-2xl bg-surface border border-[#303035] flex items-center justify-center text-success hover:bg-success/10 hover:border-success/40 hover:shadow-[0_0_15px_rgba(34,197,94,0.25)] hover:scale-105 active:scale-95 transition-all"
                        title={t('profile.call', 'Позвонить')}
                      >
                        <Phone weight="bold" size={28} />
                      </button>
                    </div>
                  ) : (
                    <div className="flex justify-center items-center gap-4 w-full">
                      {/* Left: Achievements */}
                      <button
                        onClick={() => {
                          if (store.selectedProfileUser) openUserAchievements(store.selectedProfileUser.id);
                          store.closeProfileOnly();
                        }}
                        className="w-16 h-16 rounded-2xl bg-surface border border-[#303035] flex items-center justify-center text-[#c70060] hover:bg-[#c70060]/10 hover:border-[#c70060]/40 hover:shadow-[0_0_15px_rgba(199,0,96,0.25)] hover:scale-105 active:scale-95 transition-all"
                        title={t('achievements.title', 'Достижения')}
                      >
                        <Trophy weight="bold" size={28} />
                      </button>

                      {/* Right: Add Friend */}
                      <button
                        onClick={async () => {
                          if (!store.selectedProfileUser || profileFriendRequestStatus === 'loading' || profileFriendRequestStatus === 'sent') return;
                          setProfileFriendRequestStatus('loading');
                          const success = await signalRService.sendFriendRequest(store.selectedProfileUser.username);
                          if (success) {
                            setProfileFriendRequestStatus('sent');
                            setTimeout(() => setProfileFriendRequestStatus('idle'), 2000);
                          } else {
                            setProfileFriendRequestStatus('idle');
                          }
                        }}
                        disabled={profileFriendRequestStatus === 'loading' || profileFriendRequestStatus === 'sent'}
                        className="w-16 h-16 rounded-2xl bg-surface border border-[#303035] flex items-center justify-center text-success hover:bg-success/10 hover:border-success/40 hover:shadow-[0_0_15px_rgba(34,197,94,0.25)] hover:scale-105 active:scale-95 transition-all disabled:opacity-50 disabled:cursor-default"
                        title={profileFriendRequestStatus === 'sent' ? t('modals.addFriend.sent', 'Отправлено') : t('modals.addFriend.title', 'Добавить друга')}
                      >
                        {profileFriendRequestStatus === 'sent' ? <Check weight="bold" size={28} className="text-success animate-bounce" /> : <UserPlus weight="bold" size={28} />}
                      </button>
                    </div>
                  )}
                </div>
              )}
            </div>
          </div>
        </div>
      )}
      {/* Achievement Toast */}
      {store.achievementToast && createPortal((() => {
        const isHiding = store.achievementToast.startsWith('__hiding__');
        const achId = isHiding ? store.achievementToast.replace('__hiding__', '') : store.achievementToast;
        const def = getAchievementDef(achId);
        if (!def) return null;
        return (
          <div className={`fixed top-14 left-1/2 z-[1000000] ${isHiding ? 'animate-toast-top-out' : 'animate-toast-top-in'}`}>
            <div className="bg-[#09090B]/90 backdrop-blur-xl border border-[#c70060]/50 rounded-3xl px-8 py-6 flex items-center gap-6 shadow-[0_0_60px_rgba(199,0,96,0.3)] ring-1 ring-white/10">
              <div className="w-14 h-14 rounded-2xl bg-[#c70060]/20 flex items-center justify-center shrink-0 shadow-inner border border-[#c70060]/30 overflow-hidden relative group">
                <div className="absolute inset-0 bg-gradient-to-br from-[#c70060]/20 to-transparent animate-pulse" />
                <span className="text-4xl relative z-10">{def.icon}</span>
              </div>
              <div className="min-w-0">
                <p className="text-[#c70060] font-black text-xs tracking-[0.2em] uppercase mb-1.5 opacity-90">{t('achievements.toastTitle', 'Достижение получено')}</p>
                <p className="text-white font-black text-xl tracking-tight leading-none">{t(`achievements.${def.id}.title`, def.title)}</p>
                <p className="text-textMuted font-bold text-sm mt-2 line-clamp-1 opacity-80">{t(`achievements.${def.id}.description`, def.description)}</p>
              </div>
            </div>
          </div>
        );
      })(), document.body)}

      {/* Achievements Modal */}
      {renderModal('achievements',
        <div className="bg-panelBg rounded-3xl w-[500px] max-h-[80vh] flex flex-col overflow-hidden shadow-2xl">
          <div className="flex items-center justify-between p-6 pb-4">
            <h2 className="text-xl font-bold text-white flex items-center gap-3">
              <Trophy weight="bold" size={24} />
              {store.achievementsViewUserId ? t('achievements.title', 'Достижения') : t('achievements.myTitle', 'Мои достижения')}
            </h2>
            <button onClick={closeAndResetModals} className="group text-textMuted hover:text-white transition-all duration-200 hover:rotate-90 hover:scale-110 active:scale-90 p-1.5 rounded-lg hover:bg-surface"><X weight="bold" size={24} /></button>
          </div>
          <div className="px-6 overflow-y-auto flex-1 space-y-3 pb-6">
            {(() => {
              const data = store.achievementsData;
              if (!data) return <p className="text-textMuted text-center py-8">{t('common.loading', 'Загрузка...')}</p>;
              const isOwnProfile = !store.achievementsViewUserId;
              const stats = data.stats || {};
              const unlocked = data.unlockedIds || [];

              const categoryOrder: Record<string, number> = { voice: 0, calls: 1, social: 2, hidden: 3 };

              const filtered = ACHIEVEMENTS
                .filter(a => {
                  if (a.hidden && !unlocked.includes(a.id) && !isOwnProfile) return false;
                  return true;
                })
                .sort((a, b) => {
                  const aUnlocked = unlocked.includes(a.id) ? 1 : 0;
                  const bUnlocked = unlocked.includes(b.id) ? 1 : 0;
                  if (aUnlocked !== bUnlocked) return bUnlocked - aUnlocked;

                  const aProgress = Math.min((stats[a.statKey] ?? 0) / a.maxValue, 1);
                  const bProgress = Math.min((stats[b.statKey] ?? 0) / b.maxValue, 1);
                  if (aProgress > 0 && bProgress === 0) return -1;
                  if (aProgress === 0 && bProgress > 0) return 1;
                  if (aProgress > 0 && bProgress > 0) return bProgress - aProgress;

                  return (categoryOrder[a.category] ?? 99) - (categoryOrder[b.category] ?? 99);
                });

              if (filtered.length === 0) return <p className="text-textMuted text-center py-8 font-medium">{t('achievements.empty', 'Нет достижений')}</p>;

              return filtered.map(a => {
                const isUnlocked = unlocked.includes(a.id);
                const statVal = stats[a.statKey] ?? 0;
                const effectiveStatVal = isUnlocked ? Math.max(statVal, a.maxValue) : statVal;
                const progress = getProgressPercent(effectiveStatVal, a.maxValue, a.unit);
                const showHidden = a.hidden && !isUnlocked;
                return (
                  <div key={a.id} className={`p-4 rounded-xl border transition-colors ${isUnlocked ? 'bg-[#c70060]/10 border-[#c70060]/30' : 'bg-surface border-transparent'}`}>
                    <div className="flex items-center gap-4">
                      <span className={`text-3xl ${showHidden ? 'blur-sm' : ''}`}>{showHidden ? '❓' : a.icon}</span>
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2 mb-1">
                          <span className="font-bold text-white truncate">{showHidden ? t('achievements.hiddenTitle', 'Скрытое достижение') : t(`achievements.${a.id}.title`, a.title)}</span>
                          {isUnlocked && <span className="text-[10px] font-bold bg-[#c70060]/20 text-[#c70060] px-2 py-0.5 rounded-md shrink-0">{t('achievements.unlocked', '✓ Получено')}</span>}
                        </div>
                        <p className="text-textMuted text-sm truncate">{showHidden ? t('achievements.hiddenDesc', '???') : t(`achievements.${a.id}.description`, a.description)}</p>
                        {!showHidden && (
                          <div className="mt-2 flex items-center gap-3">
                            <div className="flex-1 h-1.5 bg-black/30 rounded-full overflow-hidden">
                              <div className="h-full rounded-full transition-all duration-500" style={{ width: `${progress * 100}%`, backgroundColor: isUnlocked ? '#c70060' : '#555' }} />
                            </div>
                            <span className="text-xs text-textMuted font-mono shrink-0">{formatProgress(effectiveStatVal, a.maxValue, a.unit)}</span>
                          </div>
                        )}
                      </div>
                    </div>
                  </div>
                );
              });
            })()}
          </div>
          <div className="p-4 pt-0 text-center">
            <span className="text-xs text-textMuted">{t('achievements.summary', { unlocked: store.achievementsData?.unlockedIds?.length ?? 0, total: ACHIEVEMENTS.length, defaultValue: '{{unlocked}} / {{total}} получено' })}</span>
          </div>
        </div>
      )}

      {offlineToast && createPortal(
        <div className="fixed bottom-10 left-1/2 -translate-x-1/2 z-[1000000] animate-toast-in">
          <div className="bg-[#09090B]/90 backdrop-blur-xl border border-danger/40 rounded-3xl px-8 py-5 shadow-[0_0_50px_rgba(239,68,68,0.25)] flex items-center gap-4">
            <div className="w-10 h-10 rounded-full bg-danger/20 flex items-center justify-center shrink-0">
              <WifiOff weight="bold" size={20} className="text-danger" />
            </div>
            <div>
              <p className="text-white font-bold text-base leading-tight">{t('toasts.notification', 'Уведомление')}</p>
              <p className="text-danger/90 font-medium text-sm mt-0.5">{offlineToast}</p>
            </div>
          </div>
        </div>,
        document.body
      )}

      {store.systemToast && createPortal(
        <div className="fixed bottom-10 left-1/2 -translate-x-1/2 z-[1000000] animate-toast-in">
          <div className="bg-[#09090B]/90 backdrop-blur-xl border border-warning/40 rounded-3xl px-8 py-5 shadow-[0_0_50px_rgba(234,179,8,0.25)] flex items-center gap-4">
            <div className="w-10 h-10 rounded-full bg-warning/20 flex items-center justify-center shrink-0">
              <MicOff weight="bold" size={20} className="text-warning" />
            </div>
            <div>
              <p className="text-white font-bold text-base leading-tight">{t('toasts.notification', 'Уведомление')}</p>
              <p className="text-warning/90 font-medium text-sm mt-0.5">{store.systemToast}</p>
            </div>
          </div>
        </div>,
        document.body
      )}



      {adminBlockToast && createPortal((() => {
        const isHiding = adminBlockToast === '__hiding__';
        return (
          <div className={`fixed top-14 left-1/2 z-[1000000] ${isHiding ? 'animate-admin-block-out' : 'animate-admin-block-in'}`}>
            <div className="bg-[#09090B]/90 backdrop-blur-xl border border-danger/50 rounded-3xl px-8 py-6 flex items-center gap-5 shadow-[0_0_60px_rgba(239,68,68,0.3)] ring-1 ring-white/5">
              <div className="w-12 h-12 rounded-2xl bg-danger/20 flex items-center justify-center shrink-0 shadow-inner">
                <MicOff weight="bold" size={24} className="text-danger" />
              </div>
              <div className="min-w-0">
                <p className="text-white font-black text-lg tracking-tight leading-none mb-1">{t('toasts.accessRestricted', 'Доступ ограничен')}</p>
                <p className="text-textMuted font-bold text-sm truncate opacity-90">{typeof adminBlockToast === 'string' && adminBlockToast !== '__hiding__' ? adminBlockToast : t('toasts.adminRestricted', 'Администратор запретил это действие')}</p>
              </div>
            </div>
          </div>
        );
      })(), document.body)}

      {renderCropper()}

      {contextMenu?.visible && (
        <div
          className="fixed z-[999999] bg-surface border border-[#303035] rounded-xl shadow-xl py-2 w-48"
          style={{ top: contextMenu.y, left: contextMenu.x }}
          onClick={e => e.stopPropagation()}
          onContextMenu={e => e.stopPropagation()}
        >
          {contextMenu.type === 'channel' ? (
            <>
              {contextMenu.item.ownerId === store.currentUser?.id && (
                <button onClick={() => { setEditChannelId(contextMenu.item.id); setEditChannelName(contextMenu.item.name); store.setModal('channelEdit', true); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-white hover:bg-surfaceHover flex items-center gap-3 font-medium"><Edit2 weight="bold" size={16} /> {t('contextMenu.rename', 'Переименовать')}</button>
              )}
              <button onClick={() => { signalRService.quitAccessChannel(contextMenu.item.id); setContextMenu(null); }} className="group w-full text-left px-4 py-2 text-danger hover:bg-surfaceHover flex items-center gap-3 font-medium mt-1">
                <div className="transition-transform duration-200 group-hover:translate-x-1">
                  <LeaveIcon weight="bold" size={16} />
                </div>
                {t('contextMenu.leaveChannel', 'Выйти из канала')}</button>
            </>
          ) : contextMenu.type === 'channelMember' ? (
            <>
              <button onClick={() => { store.setSelectedProfileUser(contextMenu.item, 'channelMembers'); store.setModal('profile', true); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-white hover:bg-surfaceHover flex items-center gap-3 font-medium"><Settings weight="bold" size={16} /> {t('contextMenu.profile', 'Профиль')}</button>
              {contextMenu.item.id !== store.currentUser?.id && store.currentChannelId === store.selectedChannelForMembers?.id && (
                <button onClick={() => {
                  if (sentInvites.has(contextMenu.item.id)) return;
                  if (!contextMenu.item.isOnline) {
                    setOfflineToast(t('profile.userOffline', 'Пользователь не в сети'));
                    setTimeout(() => setOfflineToast(null), 3000);
                  } else if (store.selectedChannelForMembers) {
                    signalRService.callToChannel(contextMenu.item.id, store.selectedChannelForMembers.id, store.selectedChannelForMembers.name);
                    addSentInvite(contextMenu.item.id);
                  }
                  setContextMenu(null);
                }}
                  disabled={sentInvites.has(contextMenu.item.id)}
                  className={`w-full text-left px-4 py-2 flex items-center gap-3 font-medium mt-1 ${sentInvites.has(contextMenu.item.id) ? 'text-success cursor-default' : 'text-white hover:bg-surfaceHover'
                    }`}>
                  <Phone weight="bold" size={16} /> {sentInvites.has(contextMenu.item.id) ? t('contextMenu.inviting', 'Зовём...') : t('contextMenu.invite', 'Позвать в канал')}
                </button>
              )}
              {store.selectedChannelForMembers?.ownerId === store.currentUser?.id && contextMenu.item.id !== store.currentUser?.id && (
                <button onClick={() => { store.setUserToKick(contextMenu.item); store.setModal('kickConfirm', true); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-danger hover:bg-surfaceHover flex items-center gap-3 font-medium mt-1"><UserX weight="bold" size={16} /> {t('contextMenu.kick', 'Исключить')}</button>
              )}
            </>
          ) : contextMenu.type === 'voiceUser' ? (
            <>
              <button onClick={() => { setVolumeUser(contextMenu.item); setVolumeUserValue(store.userVolumes[contextMenu.item.id] ?? 100); store.setModal('userVolume', true); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-white hover:bg-surfaceHover flex items-center gap-3 font-medium"><Volume2 weight="bold" size={16} /> {t('contextMenu.volume', 'Громкость')}</button>
              <button onClick={() => { store.setSelectedProfileUser(contextMenu.item, 'voiceUsers'); store.setModal('profile', true); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-white hover:bg-surfaceHover flex items-center gap-3 font-medium mt-1"><Settings weight="bold" size={16} /> {t('contextMenu.profile', 'Профиль')}</button>
            </>
          ) : (
            <>
              <button onClick={() => { store.setSelectedProfileUser(contextMenu.item, 'friends'); store.setModal('profile', true); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-white hover:bg-surfaceHover flex items-center gap-3 font-medium"><Settings weight="bold" size={16} /> {t('contextMenu.profile', 'Профиль')}</button>
              <button onClick={() => { signalRService.removeFriend(contextMenu.item.id); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-danger hover:bg-surfaceHover flex items-center gap-3 font-medium mt-1"><UserMinus weight="bold" size={16} /> {t('contextMenu.remove', 'Удалить')}</button>
            </>
          )}
        </div>
      )}
    </>
  );
}