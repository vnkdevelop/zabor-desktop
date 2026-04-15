import { useState, useEffect, useRef, useMemo, useCallback } from 'react';
import { createPortal } from 'react-dom';
import {
  Settings, Mic, MicOff, Headphones, Phone, Eye, EyeOff, UserMinus, Camera,
  Check, X, LogOut, UserPlus, Mail, Edit2, Volume2,
  PhoneOff, Wifi, WifiOff, Users, LogOut as LeaveIcon, Crown, UserX
} from 'lucide-react';
import { useAppStore, User, VoiceChannel } from './store/useAppStore';
import { signalRService } from './services/signalr';
import { webrtc } from './services/webrtc';
import { isPackedGif, packGif, unpackGif, getDisplaySrc, getStaticFrameSync, preloadStaticFrame } from './utils/avatar';
import { Trophy } from 'lucide-react';
import { ACHIEVEMENTS, getAchievementDef, formatProgress, AchievementsPayload } from './achievements';

import { TitleBar } from './components/Layout/TitleBar';
import { Md3Slider } from './components/Shared/Md3Slider';
import { Md3Switch } from './components/Shared/Md3Switch';
import { AvatarImg } from './components/Shared/AvatarImg';

// === Main App ===
export default function App() {
  const store = useAppStore();

  const [isAuth, setIsAuth] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [activeTab, setActiveTab] = useState<'channels' | 'friends'>('channels');

  const [serverConnected, setServerConnected] = useState(false);
  const [showErrorText, setShowErrorText] = useState(false);
  const [showInitConnectionError, setShowInitConnectionError] = useState(false);
  const [appLoading, setAppLoading] = useState(true);
  const [loadingFadeOut, setLoadingFadeOut] = useState(false);

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

  const [newChannelName, setNewChannelName] = useState('');
  const [editChannelName, setEditChannelName] = useState('');
  const [editChannelId, setEditChannelId] = useState<string | null>(null);

  const [friendName, setFriendName] = useState('');
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

  const [contextMenu, setContextMenu] = useState<{
    visible: boolean; x: number; y: number;
    type: 'channel' | 'friend' | 'voiceUser' | 'channelMember'; item: any;
  } | null>(null);
  const [showInvitesPanel, setShowInvitesPanel] = useState(false);
  const [settingsTab, setSettingsTab] = useState<'general' | 'audio' | 'privacy'>('general');
  const [inviteFriendSearch, setInviteFriendSearch] = useState('');
  const [sentInvites, setSentInvites] = useState<Set<string>>(new Set());

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
  const loginInputRef = useRef<HTMLInputElement>(null);
  const passwordInputRef = useRef<HTMLInputElement>(null);

  const [isAdmin, setIsAdmin] = useState(false);
  const [adminUsers, setAdminUsers] = useState<any[]>([]);
  const [adminSearch, setAdminSearch] = useState('');
  const [adminLoading, setAdminLoading] = useState(false);
  const [adminActionUserId, setAdminActionUserId] = useState<string | null>(null);
  const [adminError, setAdminError] = useState('');
  const [adminSelectedUser, setAdminSelectedUser] = useState<any | null>(null);
  const [adminDetailsLoading, setAdminDetailsLoading] = useState(false);
  const [adminEditDisplayName, setAdminEditDisplayName] = useState('');
  const [adminEditAchievements, setAdminEditAchievements] = useState('');
  const [adminCopiedLogin, setAdminCopiedLogin] = useState<string | null>(null);
  const [adminRenameChannelId, setAdminRenameChannelId] = useState<string | null>(null);
  const [adminRenameChannelName, setAdminRenameChannelName] = useState('');

  const [controlsShake, setControlsShake] = useState(false);
  const [adminBlockToast, setAdminBlockToast] = useState<string | null>(null);
  const adminBlockTimerRef = useRef<NodeJS.Timeout | null>(null);

  const settingsRef = useRef({
    inputVolume: 100, outputVolume: 100,
    selectedInput: 'default', selectedOutput: 'default',
    noiseSuppression: true
  });

  useEffect(() => {
    settingsRef.current = { inputVolume, outputVolume, selectedInput, selectedOutput, noiseSuppression };
  }, [inputVolume, outputVolume, selectedInput, selectedOutput, noiseSuppression]);

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
        setShowErrorText(false);
        setLoadingFadeOut(true);
        setTimeout(() => setAppLoading(false), 600);
      } else if (isAuth) {
        setAppLoading(true);
        setLoadingFadeOut(false);
        setTimeout(() => setShowErrorText(true), 5000);
      }
    });
    const unsubPing = signalRService.onPingUpdate((newPing) => setPing(newPing));
    return () => { unsubConnection(); unsubPing(); };
  }, [isAuth]);

  useEffect(() => {
    const init = async () => {
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
        initCompleteRef.current = true;
        setLoadingFadeOut(true);
        setTimeout(() => setAppLoading(false), 300);
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

      // 4. Автологин
      const loginSuccess = await signalRService.login(
        cachedCredentials.login,
        cachedCredentials.password
      );

      if (loginSuccess) {
        const serverUser = useAppStore.getState().currentUser;
        if (cachedCredentials.userId && serverUser && cachedCredentials.userId !== serverUser.id) {
          resetToDefaults();
        }

        const [isAdminChecked, serverSettings, jokeText] = await Promise.all([
          signalRService.isCurrentUserAdmin().catch(() => false),
          signalRService.loadAudioSettings(),
          signalRService.getJokeOfTheDay().catch(() => 'Сегодня сервер шутит молча.')
        ]);

        if (serverSettings) applySettings(serverSettings);
        setIsAdmin(isAdminChecked);
        setJoke(jokeText || 'Сегодня сервер шутит молча.');
        setIsAuth(true);

        saveLocalCache();
        setTimeout(() => { settingsLoadedRef.current = true; }, 1000);

        initCompleteRef.current = true;
        setTimeout(() => {
          setLoadingFadeOut(true);
          setTimeout(() => setAppLoading(false), 600);
        }, 700);
      } else {
        // Пароль изменён или аккаунт удалён
        await window.windowControls.clearSession();
        resetToDefaults();
        store.setCurrentUser(null);
        store.setChannels([]);
        store.setFriends([]);
        store.setFriendRequests([]);
        store.setChannelInvites([]);
        credentialsRef.current = { login: '', password: '' };
        setLogin('');
        setPassword('');
        setIsAuth(false);
        
        initCompleteRef.current = true;
        setTimeout(() => {
          setLoadingFadeOut(true);
          setTimeout(() => setAppLoading(false), 600);
        }, 700);
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
          noiseSuppression: settingsRef.current.noiseSuppression
        }
      });
      window.windowControls.saveSession(data).catch(() => { });
    } catch { }
  }, []);

  const loadAdminUsers = useCallback(async () => {
    setAdminLoading(true);
    setAdminError('');
    try {
      const users = await signalRService.adminGetAllUsers();
      setAdminUsers(users);
    } catch {
      setAdminError('Не удалось загрузить список пользователей');
    } finally {
      setAdminLoading(false);
    }
  }, []);

  const loadAdminUserDetails = useCallback(async (userId: string) => {
    setAdminDetailsLoading(true);
    setAdminError('');
    try {
      const details = await signalRService.adminGetUserDetails(userId);
      setAdminSelectedUser(details);

      if (details) {
        setAdminEditDisplayName(details.displayName ?? '');
        setAdminEditAchievements((details.achievements?.unlockedIds || []).join(', '));
      }
    } catch {
      setAdminError('Не удалось загрузить профиль пользователя');
    } finally {
      setAdminDetailsLoading(false);
    }
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
    webrtc.setInputDevice('default');
    webrtc.setOutputDevice('default');
  }, []);

  const applySettings = useCallback((s: {
    inputVolume?: number; outputVolume?: number;
    selectedInput?: string; selectedOutput?: string;
    noiseSuppression?: boolean;
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
  }, []);

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
        noiseSuppression: s.noiseSuppression
      });
      saveLocalCache();
    }, 500);
  }, [inputVolume, outputVolume, selectedInput, selectedOutput, noiseSuppression, isAuth]);

  useEffect(() => {
    if (!isAuth || !serverConnected || joke) return;

    let cancelled = false;

    signalRService.getJokeOfTheDay().then((j: string) => {
      if (!cancelled) {
        setJoke(j || 'Сегодня сервер шутит молча.');
      }
    }).catch(() => {
      if (!cancelled) {
        setJoke('Сегодня сервер шутит молча.');
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
    setNewPassword('');
    setError('');
    setPrivacyError('');
    setShowPrivacyPass(false);

    setEditProfileAvatarBase64(null);
    setEditProfileAvatarColor('#c70060');

    setInviteFriendSearch('');
    setSentInvites(new Set());

    setAdminSearch('');
    setAdminError('');
    setAdminActionUserId(null);
    setAdminSelectedUser(null);
    setAdminDetailsLoading(false);
    setAdminEditDisplayName('');
    setAdminCopiedLogin(null);
    setAdminRenameChannelId(null);
    setAdminRenameChannelName('');

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
    if (str.length < 4) return "Минимум 4 символа";
    if (str.length > 25) return "Максимум 25 символов";
    if (!/^[a-zA-Z0-9!@#$%^&*()_+={}\[\]:;"'<>,.?/\\|-]+$/.test(str)) return "Только латиница и цифры";
    return "";
  }, []);

  const validateName = useCallback((str: string) => {
    if (str.trim().length === 0) return "Имя не может быть пустым";
    if (str.length > 20) return "Максимум 20 символов";
    return "";
  }, []);

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
    if (loginErr) { setError(`Логин: ${loginErr}`); return; }
    if (passErr && authStep === 'login') { setError(`Пароль: ${passErr}`); return; }
    if (authStep === 'setup') {
      const nameErr = validateName(displayName);
      if (nameErr) { setError(nameErr); return; }
    }

    setIsLoading(true);
    try {
      const connected = await signalRService.connect();
      if (!connected) { setError("Ошибка подключения к серверу"); return; }

      if (authStep === 'login') {
        const exists = await signalRService.checkUserExists(login);
        if (exists) {
          settingsLoadedRef.current = false;
          resetToDefaults();

          const success = await signalRService.login(login, password);
          if (success) {
            const [isAdminChecked, serverSettings, jokeText] = await Promise.all([
              signalRService.isCurrentUserAdmin().catch(() => false),
              signalRService.loadAudioSettings(),
              signalRService.getJokeOfTheDay().catch(() => 'Сегодня сервер шутит молча.')
            ]);

            if (serverSettings) applySettings(serverSettings);
            setIsAdmin(isAdminChecked);
            setJoke(jokeText || 'Сегодня сервер шутит молча.');
            setIsAuth(true);
            credentialsRef.current = { login, password };

            saveLocalCache();
            setTimeout(() => { settingsLoadedRef.current = true; }, 1000);

          } else {
            setError("Неверный пароль!");
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
          const [isAdminChecked, jokeText] = await Promise.all([
            signalRService.isCurrentUserAdmin().catch(() => false),
            signalRService.getJokeOfTheDay().catch(() => 'Сегодня сервер шутит молча.')
          ]);

          setIsAdmin(isAdminChecked);
          setJoke(jokeText || 'Сегодня сервер шутит молча.');
          setIsAuth(true);
          credentialsRef.current = { login, password };

          saveLocalCache();
          setTimeout(() => { settingsLoadedRef.current = true; }, 1000);

        } else {
          setError("Ошибка регистрации");
        }
      }
    } catch {
      setError("Ошибка подключения");
    } finally {
      setIsLoading(false);
    }
  }, [login, password, authStep, displayName, avatarBase64, avatarColor,
    validateInput, validateName, saveLocalCache, softClearCache,
    resetToDefaults, applySettings]);

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

    setIsAdmin(false);
    setAdminUsers([]);
    setAdminSearch('');

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
      if (success) { setPassword(newPassword); saveLocalCache(); closeAndResetModals(); }
      else setPrivacyError("Не удалось сменить пароль");
    }
  }, [newPassword, password, validateInput, saveLocalCache, closeAndResetModals]);

  const saveProfileChanges = useCallback(async () => {
    const user = store.currentUser;
    if (!user) return;
    const nameErr = validateName(editProfileDisplayName);
    if (nameErr) { setError(nameErr); return; }
    const finalAvatar = editProfileAvatarBase64 ?? user.avatarBase64;
    const finalColor = editProfileAvatarBase64 ? editProfileAvatarColor : user.avatarColor;
    const updatedUser = { ...user, displayName: editProfileDisplayName.trim(), avatarBase64: finalAvatar, avatarColor: finalColor };
    store.setCurrentUser(updatedUser);
    if (store.currentChannelId) store.setVoiceUsers(store.voiceUsers.map(u => u.id === user.id ? updatedUser : u));
    store.setFriends(store.friends.map(f => f.id === user.id ? updatedUser : f));
    saveLocalCache(); store.closeProfileOnly();
    signalRService.updateProfile(editProfileDisplayName.trim(), finalAvatar, finalColor);
  }, [store.currentUser, editProfileDisplayName, editProfileAvatarBase64, editProfileAvatarColor, validateName, saveLocalCache]);

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
    if (!friendName.trim()) return;
    const name = friendName.trim();
    closeAndResetModals();
    const success = await signalRService.sendFriendRequest(name);
    if (!success) {
      setOfflineToast('Пользователь не найден');
      setTimeout(() => setOfflineToast(null), 3000);
    }
  }, [friendName, closeAndResetModals]);

  const handleAcceptChannelInvite = useCallback(async (channelId: string) => {
    store.setChannelInvites(store.channelInvites.filter(i => i.channelId !== channelId));
    if (store.currentChannelId || store.currentCallUser) {
      store.setPendingChannelSwitch(channelId); store.setModal('channelSwitch', true); return;
    }
    const status = await signalRService.joinChannel(channelId);
    if (status === 'full') store.setModal('channelFull', true);
  }, [store.channelInvites, store.currentChannelId, store.currentCallUser]);

  const handleDeclineChannelInvite = useCallback((channelId: string) => {
    store.setChannelInvites(store.channelInvites.filter(i => i.channelId !== channelId));
  }, [store.channelInvites]);

  const handleInviteToChannel = useCallback(async (friendId: string) => {
    const ch = store.selectedChannelForInvite;
    if (!ch) return;
    await signalRService.sendChannelInvite(friendId, ch.id, ch.name);
    setSentInvites(prev => new Set(prev).add(friendId));
  }, [store.selectedChannelForInvite]);

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

    setAdminBlockToast('Администратор запретил это действие');
    adminBlockTimerRef.current = setTimeout(() => {
      setAdminBlockToast('__hiding__');
      setTimeout(() => setAdminBlockToast(null), 400);
    }, 2500);
  }, []);

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
      if (file.size > 5 * 1024 * 1024) { alert('Макс 5 МБ'); return; }
      if (!file.type.startsWith('image/')) { alert('Только изображения'); return; }

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
            <h2 className="text-white text-xl font-bold">Обрезка аватара</h2>
            <button
              onClick={() => {
                setShowCropper(false);
                setCropGifDataUrl(null);
                setCropImageSrc(null);
                setCropScale(1);
                setCropPos({ x: 0, y: 0 });
                setIsDragging(false);
              }}
              className="text-textMuted hover:text-white transition-colors"
            >
              <X size={24} />
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
              Отмена
            </button>
            <button
              onClick={applyCrop}
              className="flex-1 py-3 bg-[#c70060] text-white font-bold rounded-xl hover:opacity-90 transition-opacity"
            >
              Применить
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
      {/* Auth screen — показывается поверх основного UI, когда пользователь не авторизован */}
      {!isAuth && !appLoading && (
        <div className="fixed inset-0 z-[100000] flex flex-col bg-appBg text-textMain animate-fade-in select-none">
          <TitleBar />
          <div className="flex-1 flex items-center justify-center p-4">
            {authStep === 'login' && (
              <div className="bg-panelBg p-10 rounded-3xl w-[400px] shadow-2xl flex flex-col">
                <h1 className="text-4xl font-black text-center mb-8 tracking-wider text-white">ZABOR</h1>
                <label className="text-xs font-bold text-textMuted mb-2 tracking-wider">ЛОГИН</label>
                <input
                  ref={loginInputRef}
                  type="text"
                  value={login}
                  onChange={e => setLogin(e.target.value)}
                  maxLength={25}
                  className="bg-surface text-white rounded-xl p-3 mb-4 outline-none focus:ring-2 focus:ring-[#c70060]"
                />
                <label className="text-xs font-bold text-textMuted mb-2 tracking-wider">ПАРОЛЬ</label>
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
                    {showPassword ? <EyeOff size={20} /> : <Eye size={20} />}
                  </button>
                </div>
                {error && <p className="text-danger text-sm mb-4 text-center font-medium">{error}</p>}
                <button onClick={handleAuth} disabled={isLoading} className="bg-[#c70060] text-white font-bold py-3 rounded-xl disabled:opacity-50 hover:opacity-90 transition-opacity">{isLoading ? 'ЗАГРУЗКА...' : 'ПРОДОЛЖИТЬ'}</button>
              </div>
            )}
            {authStep === 'confirm' && (
              <div className="bg-panelBg p-8 rounded-3xl w-[400px] text-center shadow-2xl">
                <h2 className="text-2xl font-bold mb-4 text-white">Аккаунт не найден</h2>
                <p className="text-textMuted mb-8">Создать новый профиль с таким логином?</p>
                <div className="flex gap-4">
                  <button onClick={() => setAuthStep('login')} className="flex-1 bg-surface text-white py-3 rounded-xl font-bold hover:bg-surfaceHover transition-colors">Нет</button>
                  <button onClick={() => { setAuthStep('setup'); setDisplayName(login); }} className="flex-1 bg-[#c70060] text-white py-3 rounded-xl font-bold hover:opacity-90 transition-opacity">Да</button>
                </div>
              </div>
            )}
            {authStep === 'setup' && (
              <div className="bg-panelBg p-10 rounded-3xl w-[400px] flex flex-col shadow-2xl">
                <h1 className="text-2xl font-bold text-center mb-2 text-white">Создать профиль</h1>
                <p className="text-sm text-textMuted text-center mb-8">Как вас будут видеть другие?</p>
                <label className="w-[103px] h-[103px] rounded-full mx-auto mb-8 flex items-center justify-center cursor-pointer relative shadow-lg hover:opacity-80 transition-opacity">
                  {avatarBase64 ? <AvatarImg src={avatarBase64} size={103} bgColor={avatarColor} /> : <div className="w-full h-full rounded-full flex items-center justify-center" style={{ backgroundColor: avatarColor }}><Camera size={32} className="text-white" /></div>}
                  <input type="file" accept="image/*" className="hidden" onChange={e => onFileChange(e, 'setup')} />
                </label>
                <label className="text-xs font-bold text-textMuted mb-2 tracking-wider">ОТОБРАЖАЕМОЕ ИМЯ</label>
                <input type="text" value={displayName} onChange={e => setDisplayName(e.target.value)} maxLength={20} placeholder="Максимум 20 символов" className="bg-surface text-white rounded-xl p-3 mb-6 outline-none focus:ring-2 focus:ring-[#c70060]" />
                {error && <p className="text-danger text-sm mb-4 text-center font-medium">{error}</p>}
                <button onClick={handleAuth} disabled={isLoading} className="bg-[#c70060] text-white font-bold py-3 rounded-xl disabled:opacity-50 hover:opacity-90 transition-opacity">{isLoading ? 'СОЗДАНИЕ...' : 'СОЗДАТЬ'}</button>
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
                  <p className="text-danger font-bold text-center">Нет соединения с сервером</p>
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
                  <p className="text-danger font-bold text-center">Нет соединения с сервером. Переподключение...</p>
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

      <div className="flex flex-col h-screen w-screen bg-appBg text-textMain overflow-hidden relative animate-fade-in select-none">
        <TitleBar />
        <div className="flex flex-1 overflow-hidden">



          <div className="w-80 bg-panelBg flex flex-col border-r border-[#303035] relative shrink-0">

            {showInvitesPanel && (
              <div className="absolute inset-0 bg-panelBg z-[60] flex flex-col animate-fade-in">
                <div className="flex items-center justify-between p-4 border-b border-[#303035]">
                  <span className="text-sm font-bold text-white tracking-wider">УВЕДОМЛЕНИЯ</span>
                  <button onClick={() => setShowInvitesPanel(false)} className="text-textMuted hover:text-white transition-colors"><X size={20} /></button>
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
                          <p className="text-textMuted text-xs font-medium">Запрос в друзья</p>
                        </div>
                      </div>
                      <div className="flex gap-2">
                        <button onClick={() => signalRService.acceptFriendRequest(req.id)} className="flex-1 bg-success/20 text-success py-2 rounded-xl text-sm font-bold hover:bg-success/30 transition-colors">Принять</button>
                        <button onClick={() => signalRService.declineFriendRequest(req.id)} className="flex-1 bg-danger/20 text-danger py-2 rounded-xl text-sm font-bold hover:bg-danger/30 transition-colors">Отклонить</button>
                      </div>
                    </div>
                  ))}
                  {store.channelInvites.map(inv => (
                    <div key={inv.channelId} className="bg-surface p-4 rounded-xl">
                      <div className="mb-3">
                        <p className="text-white font-semibold text-sm truncate">{inv.channelName}</p>
                        <p className="text-textMuted text-xs font-medium">Приглашение от {inv.senderName}</p>
                      </div>
                      <div className="flex gap-2">
                        <button onClick={() => { handleAcceptChannelInvite(inv.channelId); setShowInvitesPanel(false); }} className="flex-1 bg-success/20 text-success py-2 rounded-xl text-sm font-bold hover:bg-success/30 transition-colors">Войти</button>
                        <button onClick={() => handleDeclineChannelInvite(inv.channelId)} className="flex-1 bg-danger/20 text-danger py-2 rounded-xl text-sm font-bold hover:bg-danger/30 transition-colors">Отклонить</button>
                      </div>
                    </div>
                  ))}
                  {store.friendRequests.length === 0 && store.channelInvites.length === 0 && (
                    <div className="flex flex-col items-center justify-center py-16 text-textMuted">
                      <Mail size={40} className="mb-4 opacity-20" />
                      <p className="font-medium text-sm">Нет уведомлений</p>
                    </div>
                  )}
                </div>
              </div>
            )}

            <div className="flex-1 overflow-y-auto p-4 pb-20">
              {activeTab === 'channels' && (
                <div className="animate-fade-in">
                  <div className="flex justify-between items-center mb-4 px-2">
                    <span className="text-xs font-bold text-textMuted tracking-wider">ГОЛОСОВЫЕ КАНАЛЫ</span>
                    <button onClick={() => store.setModal('createChannel', true)} className="text-textMuted hover:text-white text-xl transition-colors">+</button>
                  </div>
                  {store.channels.map(ch => {
                    const channelUsers = store.channelUsersMap[ch.id] || [];
                    return (
                      <div key={ch.id} className="mb-2">
                        <button onClick={() => handleChannelClick(ch.id)} onContextMenu={e => handleContextMenu(e, 'channel', ch)}
                          className={`w-full text-left px-2 py-3 rounded-xl flex items-center justify-between group transition-colors focus:outline-none ${store.currentChannelId === ch.id ? 'bg-[#333]' : 'hover:bg-surfaceHover'}`}>
                          <div className="flex items-center gap-3 overflow-hidden">
                            <div className="w-1.5 h-1.5 rounded-full bg-textMuted shrink-0 ml-2" />
                            <span className="font-medium text-[15px] truncate select-none text-white">{ch.name}</span>
                          </div>
                          <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity pr-2 shrink-0">
                            <div onClick={e => { e.stopPropagation(); e.preventDefault(); store.setSelectedChannelForInvite(ch); store.setModal('inviteToChannel', true); }} className="text-textMuted hover:text-white p-1 rounded hover:bg-black/20" title="Пригласить"><UserPlus size={16} /></div>
                            <div onClick={e => { e.stopPropagation(); e.preventDefault(); openChannelMembers(ch); }} className="text-textMuted hover:text-white p-1 rounded hover:bg-black/20" title="Участники канала"><Users size={16} /></div>
                          </div>
                        </button>
                        {channelUsers.length > 0 && (
                          <div className="flex items-center -space-x-2 px-8 mt-1.5 pointer-events-none">
                            {channelUsers.map(u => (
                              <div key={u.id} className="w-[31px] h-[31px] rounded-full border-2 border-panelBg relative shrink-0 overflow-hidden" title={u.displayName}>
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
                    <span className="text-xs font-bold text-textMuted tracking-wider">ДРУЗЬЯ</span>
                    <button onClick={() => store.setModal('addFriend', true)} className="text-textMuted hover:text-white text-xl transition-colors">+</button>
                  </div>
                  {store.friends.map(f => (
                    <div key={f.id} onContextMenu={e => handleContextMenu(e, 'friend', f)}
                      onClick={() => { store.setSelectedProfileUser(f, 'friends'); setEditProfileDisplayName(f.displayName); store.setModal('profile', true); signalRService.viewProfile(f.id); }}
                      className="px-3 py-2 rounded-xl mb-1 cursor-pointer hover:bg-surfaceHover flex items-center gap-3 transition-colors">
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
                <Mail size={24} color="white" />
                <div className="absolute top-0 right-0 w-4 h-4 bg-danger rounded-full border-2 border-panelBg animate-pulse" />
              </button>
            </div>

            <div className="bg-surface rounded-full mx-4 my-2 p-1 flex relative shrink-0">
              <button onClick={() => setActiveTab('channels')} className={`flex-1 py-2.5 rounded-full font-bold text-sm z-10 transition-colors ${activeTab === 'channels' ? 'text-white' : 'text-textMuted hover:text-white'}`}>Каналы</button>
              <button onClick={() => setActiveTab('friends')} className={`flex-1 py-2.5 rounded-full font-bold text-sm z-10 transition-colors ${activeTab === 'friends' ? 'text-white' : 'text-textMuted hover:text-white'}`}>Друзья</button>
              <div className={`absolute top-1 bottom-1 w-[calc(50%-4px)] bg-[#333] rounded-full transition-all duration-300 ease-out ${activeTab === 'channels' ? 'left-1' : 'left-[calc(50%+2px)]'}`} />
            </div>

            <div className="h-[75px] bg-[#09090B] rounded-2xl mx-4 mb-4 flex items-center px-4 shrink-0 shadow-lg">
                <div onClick={() => { store.setSelectedProfileUser(store.currentUser, 'none'); setEditProfileDisplayName(store.currentUser!.displayName); setEditProfileAvatarBase64(null); store.setModal('profile', true); }}
                className="relative w-[51px] h-[51px] mr-3 cursor-pointer shrink-0 hover:opacity-80 transition-opacity">
                  <AvatarImg src={store.currentUser?.avatarBase64} size={51} bgColor={store.currentUser?.avatarColor} />
                  <div className={`absolute -bottom-1 -right-1 w-4 h-4 rounded-full border-[3px] border-[#09090B] ${serverConnected ? 'bg-success' : 'bg-gray-500'}`} />
                </div>
              <div className="flex-1 min-w-0 flex flex-col justify-center">
                <div className="font-bold text-sm truncate text-white">{store.currentUser?.displayName}</div>
                <div onClick={handleCopyUsername} className="text-xs font-semibold text-textMuted truncate cursor-pointer hover:text-white transition-colors mt-0.5" title="Нажмите, чтобы скопировать">
                  {isCopied ? <span className="text-success">Скопировано!</span> : store.currentUser?.username}
                </div>
              </div>
              <div className="flex items-center gap-1 shrink-0">
                {isAdmin && (
                  <button
                    onClick={() => {
                      store.setModal('adminConsole', true);
                      loadAdminUsers();
                    }}
                    className="text-textMuted hover:text-white p-2 hover:bg-surface rounded-xl transition-colors"
                    title="Админ-консоль"
                  >
                    <Crown size={20} />
                  </button>
                )}

                <button
                  onClick={() => {
                    store.setModal('settings', true);
                    loadDevices();
                    window.windowControls.getAutoLaunch().then(setAutoLaunch).catch(() => { });
                  }}
                  className="text-textMuted hover:text-white p-2 hover:bg-surface rounded-xl transition-colors"
                >
                  <Settings size={20} />
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
          ${store.currentCallUser.isSpeaking && store.callStatus === 'connected'
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
                      <AvatarImg src={store.currentCallUser.avatarBase64} size={cardSize.avatarSize} bgColor="transparent" />
                    </div>

                    {(!store.webrtcConnections[store.currentCallUser.id] && store.callStatus !== 'calling') && (
                      <div className="absolute inset-0 bg-black/40 flex flex-col items-center justify-center z-20 backdrop-blur-[2px]" style={{ borderRadius: '24px' }}>
                        <div className="flex gap-2.5 mb-2">
                          <div className="w-3 h-3 bg-[#c70060] rounded-full animate-pulse" />
                          <div className="w-3 h-3 bg-[#c70060] rounded-full animate-pulse" style={{ animationDelay: '0.15s' }} />
                          <div className="w-3 h-3 bg-[#c70060] rounded-full animate-pulse" style={{ animationDelay: '0.3s' }} />
                        </div>
                        <span className="text-white text-xs font-bold tracking-wider">ПОДКЛЮЧЕНИЕ</span>
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
                          <span className="text-textMuted text-xs font-medium">Дозвон...</span>
                        )}

                        {store.callStatus === 'connected' && (store.currentCallUser.isMuted || store.currentCallUser.isServerMuted) && (
                          <Mic size={14} className="text-danger shrink-0" />
                        )}
                        {store.callStatus === 'connected' && (store.currentCallUser.isDeafened || store.currentCallUser.isServerDeafened) && (
                          <Headphones size={14} className="text-danger shrink-0" />
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
                      <p className="text-xs text-white/20 mb-3 font-semibold tracking-wider">ШУТЕЙКА:</p>
                      <p className="text-lg text-white/50 font-medium leading-relaxed whitespace-pre-line">
                        {joke}
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
                  {[...store.voiceUsers].sort((a, b) => a.displayName.localeCompare(b.displayName)).map(user => (
                    <div key={user.id} onContextMenu={e => handleContextMenu(e, 'voiceUser', user)}
                      className={`relative flex flex-col items-center justify-center cursor-pointer transition-all duration-200 overflow-hidden shrink-0 hover:-translate-y-1
                        ${user.isSpeaking ? 'shadow-[inset_0_0_0_3px_#3BA55C,inset_0_0_0_5px_#181818,0_10px_15px_-3px_rgba(0,0,0,0.5)] z-10' : 'shadow-xl'}`}
                      style={{ backgroundColor: user.avatarColor, width: `${cardSize.w}px`, height: `${cardSize.h}px`, borderRadius: '24px' }}>
                      <div className="relative" style={{ width: `${cardSize.avatarSize}px`, height: `${cardSize.avatarSize}px`, marginBottom: '16px' }}>
                        <AvatarImg src={user.avatarBase64} size={cardSize.avatarSize} bgColor="transparent" />
                      </div>
                      {(!store.webrtcConnections[user.id] && user.id !== store.currentUser?.id) && (
                        <div className="absolute inset-0 bg-black/40 flex flex-col items-center justify-center z-20 backdrop-blur-[2px]" style={{ borderRadius: '24px' }}>
                          <div className="flex gap-2.5 mb-2">
                            <div className="w-3 h-3 bg-[#c70060] rounded-full animate-pulse" />
                            <div className="w-3 h-3 bg-[#c70060] rounded-full animate-pulse" style={{ animationDelay: '0.15s' }} />
                            <div className="w-3 h-3 bg-[#c70060] rounded-full animate-pulse" style={{ animationDelay: '0.3s' }} />
                          </div>
                          <span className="text-white text-xs font-bold tracking-wider">ПОДКЛЮЧЕНИЕ</span>
                        </div>
                      )}
                      <div className={`absolute bottom-4 left-1/2 -translate-x-1/2 transition-all duration-300 ${isIdle ? 'translate-y-8 opacity-0 pointer-events-none' : 'translate-y-0 opacity-100'}`}>
                        <div className="bg-[#09090B]/80 backdrop-blur-md border border-[#303035]/50 px-4 py-1.5 rounded-full flex items-center gap-2 shadow-lg whitespace-nowrap" style={{ maxWidth: `${cardSize.w - 40}px` }}>
                          <span className="text-white font-bold text-sm truncate">{user.displayName}</span>
                          {(user.isMuted || user.isServerMuted) && <Mic size={14} className="text-danger shrink-0" />}
                          {(user.isDeafened || user.isServerDeafened) && <Headphones size={14} className="text-danger shrink-0" />}
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
                  className={`w-14 h-14 rounded-full flex items-center justify-center relative transition-colors ${(store.currentUser?.isMuted || store.currentUser?.isServerMuted || store.currentUser?.isServerDeafened)
                    ? 'bg-[#2B2D31] text-white'
                    : 'bg-surface hover:bg-surfaceHover text-white'
                    }`}
                >
                  <Mic size={24} />
                  {(store.currentUser?.isMuted || store.currentUser?.isServerMuted || store.currentUser?.isServerDeafened) && (
                    <div className="absolute w-[30px] h-[3px] bg-danger rotate-45 rounded-full" />
                  )}
                </button>
                <button
                  onClick={toggleDeafen}
                  className={`w-14 h-14 rounded-full flex items-center justify-center relative transition-colors ${(store.currentUser?.isDeafened || store.currentUser?.isServerDeafened)
                    ? 'bg-[#2B2D31] text-white'
                    : 'bg-surface hover:bg-surfaceHover text-white'
                    }`}
                >
                  <Headphones size={24} />
                  {(store.currentUser?.isDeafened || store.currentUser?.isServerDeafened) && (
                    <div className="absolute w-[30px] h-[3px] bg-danger rotate-45 rounded-full" />
                  )}
                </button>
                <button onClick={handleEndCall} className="bg-danger hover:bg-red-600 text-white font-bold py-3.5 px-8 rounded-full flex items-center gap-3 transition-colors text-[15px]">
                  <PhoneOff size={20} /> Завершить
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
                  className={`w-14 h-14 rounded-full flex items-center justify-center relative transition-colors ${(store.currentUser?.isMuted || store.currentUser?.isServerMuted || store.currentUser?.isServerDeafened)
                    ? 'bg-[#2B2D31] text-white'
                    : 'bg-surface hover:bg-surfaceHover text-white'
                    }`}
                >
                  <Mic size={24} />
                  {(store.currentUser?.isMuted || store.currentUser?.isServerMuted || store.currentUser?.isServerDeafened) && (
                    <div className="absolute w-[30px] h-[3px] bg-danger rotate-45 rounded-full" />
                  )}
                </button>
                <button
                  onClick={toggleDeafen}
                  className={`w-14 h-14 rounded-full flex items-center justify-center relative transition-colors ${(store.currentUser?.isDeafened || store.currentUser?.isServerDeafened)
                    ? 'bg-[#2B2D31] text-white'
                    : 'bg-surface hover:bg-surfaceHover text-white'
                    }`}
                >
                  <Headphones size={24} />
                  {(store.currentUser?.isDeafened || store.currentUser?.isServerDeafened) && (
                    <div className="absolute w-[30px] h-[3px] bg-danger rotate-45 rounded-full" />
                  )}
                </button>
                <button onClick={() => signalRService.leaveChannel()} className="bg-danger hover:bg-red-600 text-white font-bold py-3.5 px-8 rounded-full flex items-center gap-3 transition-colors text-[15px]">
                  <Phone size={20} /> Завершить
                </button>
              </div>
            )}

            <div className="absolute bottom-4 left-4 z-50" onMouseEnter={() => setShowPingTooltip(true)} onMouseLeave={() => setShowPingTooltip(false)}>
              <div className="w-10 h-10 rounded-full bg-surface flex items-center justify-center cursor-pointer hover:bg-surfaceHover transition-colors shadow-lg" style={{ color: getPingColor() }}>
                {ping < 0 ? <WifiOff size={18} /> : <Wifi size={18} />}
              </div>
              {showPingTooltip && (
                <div className="absolute bottom-12 left-0 bg-surface border border-[#303035] rounded-xl px-4 py-2 shadow-xl whitespace-nowrap">
                  <div className="text-xs text-textMuted mb-1 font-bold tracking-wider">ПИНГ</div>
                  <div className="font-bold" style={{ color: getPingColor() }}>{ping < 0 ? 'Офлайн' : `${ping} мс`}</div>
                </div>
              )}
            </div>
          </div>

        </div>
      </div>

      {renderModal('createChannel',
        <div className="bg-panelBg p-8 rounded-3xl w-[400px] shadow-2xl">
          <h2 className="text-xl font-bold mb-6 text-white">Создать канал</h2>
          <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">НАЗВАНИЕ КАНАЛА</label>
          <input type="text" value={newChannelName} onChange={e => setNewChannelName(e.target.value)} maxLength={25} onKeyDown={e => e.key === 'Enter' && handleCreateChannel()} placeholder="Максимум 25 символов" className="w-full bg-surface text-white rounded-xl p-3 mb-6 outline-none focus:ring-2 focus:ring-[#c70060]" />
          {error && <p className="text-danger text-sm mb-4 font-medium">{error}</p>}
          <div className="flex gap-4">
            <button onClick={closeAndResetModals} className="flex-1 bg-surface text-white py-3 rounded-xl font-bold hover:bg-surfaceHover transition-colors">Отмена</button>
            <button onClick={handleCreateChannel} className="flex-1 bg-[#c70060] text-white py-3 rounded-xl font-bold hover:opacity-90 transition-opacity">Создать</button>
          </div>
        </div>
      )}

      {renderModal('channelEdit',
        <div className="bg-panelBg p-8 rounded-3xl w-[400px] shadow-2xl">
          <h2 className="text-xl font-bold mb-6 text-white">Переименовать канал</h2>
          <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">НОВОЕ НАЗВАНИЕ</label>
          <input type="text" value={editChannelName} onChange={e => setEditChannelName(e.target.value)} maxLength={25} onKeyDown={e => e.key === 'Enter' && saveChannelEdit()} className="w-full bg-surface text-white rounded-xl p-3 mb-6 outline-none focus:ring-2 focus:ring-[#c70060]" />
          {error && <p className="text-danger text-sm mb-4 font-medium">{error}</p>}
          <div className="flex gap-4">
            <button onClick={closeAndResetModals} className="flex-1 bg-surface text-white py-3 rounded-xl font-bold hover:bg-surfaceHover transition-colors">Отмена</button>
            <button onClick={saveChannelEdit} className="flex-1 bg-[#c70060] text-white py-3 rounded-xl font-bold hover:opacity-90 transition-opacity">Сохранить</button>
          </div>
        </div>
      )}

      {renderModal('addFriend',
        <div className="bg-panelBg p-8 rounded-3xl w-[400px] shadow-2xl">
          <h2 className="text-xl font-bold mb-2 text-white">Добавить друга</h2>
          <p className="text-textMuted text-sm mb-6 font-medium">Введите логин пользователя</p>
          <input type="text" value={friendName} onChange={e => { setFriendName(e.target.value); setError(''); }} maxLength={25} onKeyDown={e => e.key === 'Enter' && handleAddFriend()} placeholder="Логин" className="w-full bg-surface text-white rounded-xl p-3 mb-4 outline-none focus:ring-2 focus:ring-[#c70060]" />
          {error && <p className="text-danger text-sm mb-4 font-medium">{error}</p>}
          <div className="flex gap-4">
            <button onClick={closeAndResetModals} className="flex-1 bg-surface text-white py-3 rounded-xl font-bold hover:bg-surfaceHover transition-colors">Отмена</button>
            <button onClick={handleAddFriend} className="flex-1 bg-[#c70060] text-white py-3 rounded-xl font-bold hover:opacity-90 transition-opacity">Отправить</button>
          </div>
        </div>
      )}

      {renderModal('settings',
        <div className="bg-panelBg rounded-3xl w-[500px] max-h-[80vh] flex flex-col overflow-hidden shadow-2xl">
          <div className="flex items-center justify-between p-6 pb-0">
            <h2 className="text-xl font-bold text-white">Настройки</h2>
            <button onClick={closeAndResetModals} className="text-textMuted hover:text-white transition-colors"><X size={24} /></button>
          </div>
          <div className="flex gap-2 px-6 pt-4">
            <button onClick={() => setSettingsTab('general')} className={`px-4 py-2 rounded-xl font-bold text-sm transition-colors ${settingsTab === 'general' ? 'bg-[#c70060] text-white' : 'bg-surface text-textMuted hover:text-white'}`}>Общие</button>
            <button onClick={() => setSettingsTab('audio')} className={`px-4 py-2 rounded-xl font-bold text-sm transition-colors ${settingsTab === 'audio' ? 'bg-[#c70060] text-white' : 'bg-surface text-textMuted hover:text-white'}`}>Звук</button>
            <button onClick={() => setSettingsTab('privacy')} className={`px-4 py-2 rounded-xl font-bold text-sm transition-colors ${settingsTab === 'privacy' ? 'bg-[#c70060] text-white' : 'bg-surface text-textMuted hover:text-white'}`}>Безопасность</button>
          </div>
          <div className="p-6 overflow-y-auto flex-1">
            {settingsTab === 'general' && (
              <div className="space-y-6">
                <div>
                  <label className="text-xs font-bold text-textMuted mb-3 block tracking-wider">СИСТЕМА</label>
                  <div className="flex items-center justify-between bg-surface p-4 rounded-xl">
                    <div className="mr-4">
                      <span className="font-semibold text-white text-[15px]">Запускать вместе с Windows</span>
                      <p className="text-xs text-textMuted mt-1">ZABOR откроется автоматически при включении компьютера</p>
                    </div>
                    <Md3Switch checked={autoLaunch} onChange={handleAutoLaunchToggle} />
                  </div>
                </div>
              </div>
            )}
            {settingsTab === 'audio' && (
              <div className="space-y-6">
                <div>
                  <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">УСТРОЙСТВО ВВОДА</label>
                  <select value={selectedInput} onChange={e => { setSelectedInput(e.target.value); webrtc.updateSettings(e.target.value, noiseSuppression); }} className="w-full bg-surface text-white rounded-xl p-3 outline-none focus:ring-2 focus:ring-[#c70060]">
                    <option value="default">По умолчанию</option>
                    {audioDevices.inputs.length === 0 && selectedInput !== 'default' && (
                      <option value={selectedInput}>Загрузка...</option>
                    )}
                    {audioDevices.inputs.map(d => <option key={d.deviceId} value={d.deviceId}>{d.label || 'Микрофон'}</option>)}
                  </select>
                </div>
                <div>
                  <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">УСТРОЙСТВО ВЫВОДА</label>
                  <select value={selectedOutput} onChange={e => { setSelectedOutput(e.target.value); webrtc.setOutputDevice(e.target.value); }} className="w-full bg-surface text-white rounded-xl p-3 outline-none focus:ring-2 focus:ring-[#c70060]">
                    <option value="default">По умолчанию</option>
                    {audioDevices.outputs.length === 0 && selectedOutput !== 'default' && (
                      <option value={selectedOutput}>Загрузка...</option>
                    )}
                    {audioDevices.outputs.map(d => <option key={d.deviceId} value={d.deviceId}>{d.label || 'Динамики'}</option>)}
                  </select>
                </div>
                <div>
                  <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">ГРОМКОСТЬ МИКРОФОНА — {inputVolume}%</label>
                  <Md3Slider min={0} max={200} step={5} value={inputVolume} onChange={v => { setInputVolume(v); webrtc.setInputVolume(v); }} />
                </div>
                <div>
                  <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">ГРОМКОСТЬ ЗВУКА — {outputVolume}%</label>
                  <Md3Slider min={0} max={200} step={5} value={outputVolume} onChange={v => { setOutputVolume(v); webrtc.setOutputVolume(v); }} />
                </div>
                <div className="flex items-center justify-between bg-surface p-4 rounded-xl">
                  <span className="font-semibold text-white">Шумоподавление</span>
                  <Md3Switch checked={noiseSuppression} onChange={v => {
                    setNoiseSuppression(v);
                    webrtc.setNoiseSuppression(v);
                    webrtc.updateSettings(selectedInput, v);
                  }} />
                </div>
              </div>
            )}
            {settingsTab === 'privacy' && (
              <div className="space-y-6">
                <div>
                  <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">НОВЫЙ ПАРОЛЬ</label>
                  <div className="relative">
                    <input type={showPrivacyPass ? 'text' : 'password'} value={newPassword} onChange={e => { setNewPassword(e.target.value); setPrivacyError(''); }} maxLength={25} placeholder="Максимум 25 символов" className="w-full bg-surface text-white rounded-xl p-3 outline-none pr-10 focus:ring-2 focus:ring-[#c70060]" />
                    <button onClick={() => setShowPrivacyPass(!showPrivacyPass)} className="absolute right-3 top-3 text-textMuted hover:text-white transition-colors">{showPrivacyPass ? <EyeOff size={20} /> : <Eye size={20} />}</button>
                  </div>
                  {privacyError && <p className="text-danger text-sm mt-2 font-medium">{privacyError}</p>}
                  <button onClick={changePassword} className="mt-3 w-full bg-[#c70060] hover:opacity-90 text-white py-3 rounded-xl font-bold transition-opacity">Сменить пароль</button>
                </div>
                <button onClick={handleLogout} className="w-full bg-danger hover:bg-red-600 text-white py-3 rounded-xl font-bold flex items-center justify-center gap-2 transition-colors"><LogOut size={18} /> Выйти из аккаунта</button>
              </div>
            )}
          </div>
        </div>
      )}

      {store.pendingChannelSwitch && (
        <div className="fixed inset-0 z-[999] bg-black/70 backdrop-blur-md flex items-center justify-center p-4">
          <div className="bg-panelBg p-8 rounded-3xl w-[400px] text-center shadow-2xl">
            <div className="flex items-center justify-between mb-4">
              <h2 className="text-xl font-bold text-white">Сменить канал?</h2>
              <button
                onClick={() => store.setPendingChannelSwitch(null)}
                className="text-textMuted hover:text-white transition-colors"
              >
                <X size={24} />
              </button>
            </div>

            <p className="text-textMuted mb-8 font-medium">
              Вы покинете текущий канал и перейдёте в другой.
            </p>

            <div className="flex gap-4">
              <button
                onClick={() => store.setPendingChannelSwitch(null)}
                disabled={isSwitchingChannel}
                className="flex-1 bg-surface text-white py-3 rounded-xl font-bold hover:bg-surfaceHover transition-colors"
              >
                Остаться
              </button>
              <button
                onClick={confirmChannelSwitch}
                disabled={isSwitchingChannel}
                className="flex-1 bg-[#c70060] text-white py-3 rounded-xl font-bold hover:opacity-90 transition-opacity"
              >
                {isSwitchingChannel ? 'Переход...' : 'Перейти'}
              </button>
            </div>
          </div>
        </div>
      )}

      {renderModal('inviteToChannel',
        <div className="bg-panelBg p-8 rounded-3xl w-[400px] shadow-2xl">
          <h2 className="text-xl font-bold mb-2 text-white">Пригласить в канал</h2>
          <p className="text-textMuted text-sm mb-6">{store.selectedChannelForInvite?.name}</p>
          <input type="text" value={inviteFriendSearch} onChange={e => setInviteFriendSearch(e.target.value)} placeholder="Поиск среди друзей..." className="w-full bg-surface text-white rounded-xl p-3 mb-4 outline-none focus:ring-2 focus:ring-[#c70060]" />
          <div className="max-h-[300px] overflow-y-auto space-y-2">
            {store.friends.filter(f => f.displayName.toLowerCase().includes(inviteFriendSearch.toLowerCase())).map(f => (
              <div key={f.id} className="flex items-center gap-3 p-3 bg-surface rounded-xl hover:bg-surfaceHover transition-colors">
                <div className="w-[47px] h-[47px] shrink-0 relative"><AvatarImg src={f.avatarBase64} size={47} bgColor={f.avatarColor} /></div>
                <span className="flex-1 font-semibold text-white truncate">{f.displayName}</span>
                <button
                  onClick={() => handleInviteToChannel(f.id)}
                  disabled={sentInvites.has(f.id)}
                  className={`py-2 px-4 rounded-xl text-sm font-bold transition-all shrink-0 ${sentInvites.has(f.id)
                    ? 'bg-success/20 text-success cursor-default'
                    : 'bg-[#c70060] hover:opacity-90 text-white'
                    }`}
                >
                  {sentInvites.has(f.id) ? '✓ Отправлено' : 'Пригласить'}
                </button>
              </div>
            ))}
            {store.friends.filter(f => f.displayName.toLowerCase().includes(inviteFriendSearch.toLowerCase())).length === 0 && (
              <p className="text-textMuted text-center py-4 font-medium">Друзья не найдены</p>
            )}
          </div>
          <button onClick={closeAndResetModals} className="w-full mt-4 bg-surface text-white py-3 rounded-xl font-bold hover:bg-surfaceHover transition-colors">Закрыть</button>
        </div>
      )}

      {renderModal('channelMembers',
        <div className="bg-panelBg p-8 rounded-3xl w-[420px] shadow-2xl">
          <div className="flex items-center justify-between mb-2">
            <h2 className="text-xl font-bold text-white flex items-center gap-3"><Users size={24} /> Участники</h2>
            <button onClick={closeAndResetModals} className="text-textMuted hover:text-white transition-colors"><X size={24} /></button>
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
                      <span className="text-[10px] font-bold bg-yellow-500/20 text-yellow-500 px-2 py-0.5 rounded-md flex items-center gap-1 shrink-0"><Crown size={12} /> Создатель</span>
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
          <div className="w-16 h-16 bg-danger/20 rounded-full flex items-center justify-center mx-auto mb-4"><UserX size={32} className="text-danger" /></div>
          <h2 className="text-xl font-bold mb-2 text-white">Исключить пользователя?</h2>
          <p className="text-textMuted mb-8">Вы уверены, что хотите лишить пользователя <span className="text-white font-bold">{store.userToKick?.displayName}</span> доступа к каналу?</p>
          <div className="flex gap-4">
            <button onClick={() => { store.setModal('kickConfirm', false); store.setUserToKick(null); }} className="flex-1 bg-surface text-white py-3 rounded-xl font-bold hover:bg-surfaceHover transition-colors">Отмена</button>
            <button onClick={handleKickConfirm} className="flex-1 bg-danger text-white py-3 rounded-xl font-bold hover:bg-red-600 transition-colors">Исключить</button>
          </div>
        </div>
      )}

      {renderModal('channelFull',
        <div className="bg-panelBg p-8 rounded-3xl w-[400px] text-center shadow-2xl border border-danger/30">
          <div className="w-20 h-20 bg-danger/20 rounded-full flex items-center justify-center mx-auto mb-4"><Users size={40} className="text-danger" /></div>
          <h2 className="text-xl font-bold mb-4 text-white">Канал переполнен</h2>
          <p className="text-textMuted mb-8">Максимальное количество участников в канале — 10 человек. Подождите, пока кто-то выйдет.</p>
          <button onClick={closeAndResetModals} className="w-full bg-surface text-white py-3 rounded-xl font-bold hover:bg-surfaceHover transition-colors">Понятно</button>
        </div>
      )}

      {renderModal('userVolume',
        <div className="bg-panelBg p-8 rounded-3xl w-[400px] shadow-2xl">
          <h2 className="text-xl font-bold mb-2 text-white">Громкость пользователя</h2>
          <p className="text-textMuted text-sm mb-6 font-medium">{volumeUser?.displayName}</p>
          <div>
            <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">ГРОМКОСТЬ — {volumeUserValue}%</label>
            <Md3Slider min={0} max={200} step={5} value={volumeUserValue} onChange={v => {
              setVolumeUserValue(v);
              if (volumeUser) webrtc.setUserVolume(volumeUser.id, v);
            }} />
          </div>
          <button onClick={closeAndResetModals} className="w-full mt-6 bg-surface text-white py-3 rounded-xl font-bold hover:bg-surfaceHover transition-colors">Закрыть</button>
        </div>
      )}

      {renderModal('incomingCall',
        <div className="bg-panelBg p-8 rounded-3xl w-[350px] text-center shadow-2xl">
          <div className="w-[87px] h-[87px] mx-auto mb-4 relative">
            <AvatarImg src={store.incomingCall?.callerAvatarBase64 || null} size={87} bgColor={store.incomingCall?.callerAvatarColor} />
          </div>
          <h2 className="text-xl font-bold mb-2 text-white">{store.incomingCall?.callerName}</h2>
          <p className="text-textMuted mb-8 font-medium">Входящий звонок...</p>
          <div className="flex gap-4">
            <button onClick={handleDeclineCall} className="flex-1 bg-danger text-white py-3 rounded-xl font-bold hover:bg-red-600 transition-colors flex items-center justify-center gap-2"><PhoneOff size={18} /> Отклонить</button>
            <button onClick={handleAcceptCall} className="flex-1 bg-success text-white py-3 rounded-xl font-bold hover:bg-green-600 transition-colors flex items-center justify-center gap-2"><Phone size={18} /> Принять</button>
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
                   {displayUsers.slice(0, 4).map((u, i) => (
                     <div key={u.id} className="w-[87px] h-[87px] rounded-full border-[4px] border-panelBg relative shrink-0 shadow-lg" style={{ marginLeft: i === 0 ? 0 : '-1.5rem', zIndex: 10 - i }}>
                        <AvatarImg src={u.avatarBase64 || null} size={87} bgColor={u.avatarColor} />
                     </div>
                   ))}
                 </div>
                 <h2 className="text-xl font-bold mb-2 text-white truncate px-2">{invite.channelName}</h2>
                 <p className="text-textMuted mb-8 font-medium">Вас зовут в канал</p>
                 <div className="flex gap-4">
                   <button onClick={() => { store.setModal('incomingChannelInvite', false); store.setIncomingChannelInvite(null); signalRService.stopRingtone(); }} className="flex-1 bg-danger text-white py-3 rounded-xl font-bold hover:bg-red-600 transition-colors flex items-center justify-center gap-2"><PhoneOff size={18} /> Сбросить</button>
                   <button onClick={() => { handleAcceptChannelInvite(invite.channelId); store.setModal('incomingChannelInvite', false); store.setIncomingChannelInvite(null); signalRService.stopRingtone(); store.setChannelInvites(store.channelInvites.filter(i => i.channelId !== invite.channelId)); }} className="flex-1 bg-success text-white py-3 rounded-xl font-bold hover:bg-green-600 transition-colors flex items-center justify-center gap-2"><Phone size={18} /> Войти</button>
                 </div>
               </>
             );
          })()}
        </div>
      )}

      {store.modals.profile && store.selectedProfileUser && (
        <div className="fixed inset-0 z-[9999] bg-black/70 backdrop-blur-md flex items-center justify-center p-4">
          <div className="bg-panelBg w-[350px] rounded-3xl overflow-hidden shadow-2xl relative">
            <div
              className="h-28 w-full relative"
              style={{ backgroundColor: editProfileAvatarBase64 ? editProfileAvatarColor : store.selectedProfileUser?.avatarColor }}
            >
              <button
                onClick={() => {
                  const uid = store.selectedProfileUser!.id;
                  if (uid === store.currentUser?.id) openMyAchievements();
                  else openUserAchievements(uid);
                  store.closeProfileOnly();
                }}
                className="absolute bottom-3 right-4 w-10 h-10 rounded-xl bg-transparent border-2 border-white/90 flex items-center justify-center hover:bg-white/5 transition-colors shadow-lg"
                title="Достижения"
              >
                <span
                  className="w-full h-full flex items-center justify-center text-white text-[20px] leading-none select-none"
                  style={{
                    WebkitFontSmoothing: 'antialiased',
                    MozOsxFontSmoothing: 'grayscale',
                    textRendering: 'geometricPrecision'
                  }}
                >
                  ✦
                </span>
              </button>

              <button
                onClick={() => store.closeProfileOnly()}
                className="absolute top-4 right-4 text-textMuted hover:text-white transition-colors"
              >
                <X size={20} />
              </button>
            </div>

            <div className="px-6 pb-8 relative">
              <div className="absolute -top-12 left-6 w-[103px] h-[103px]">
                <div className="w-full h-full rounded-full border-[6px] border-panelBg bg-panelBg relative">
                  <AvatarImg
                    src={editProfileAvatarBase64 || store.selectedProfileUser?.avatarBase64}
                    size={91}
                    bgColor={editProfileAvatarBase64 ? editProfileAvatarColor : store.selectedProfileUser?.avatarColor}
                  />
                </div>
                <div className={`absolute bottom-0.5 right-0.5 w-6 h-6 rounded-full border-[4px] border-panelBg ${store.selectedProfileUser?.id === store.currentUser?.id
                  ? (serverConnected ? 'bg-success' : 'bg-gray-500')
                  : (store.selectedProfileUser?.isOnline ? 'bg-success' : 'bg-gray-500')
                  }`} />
              </div>

              <div className="pt-14 mb-6">
                {store.selectedProfileUser?.id === store.currentUser?.id ? (
                  <>
                    <label className="text-[10px] font-bold text-textMuted mb-2 block tracking-wider">ОТОБРАЖАЕМОЕ ИМЯ</label>
                    <input
                      type="text"
                      value={editProfileDisplayName}
                      onChange={e => {
                        setEditProfileDisplayName(e.target.value);
                        setError('');
                      }}
                      maxLength={20}
                      className="bg-surface w-full p-3 rounded-xl text-white font-bold text-lg mb-3 outline-none focus:ring-2 focus:ring-[#c70060]"
                    />
                    {error && <p className="text-danger text-xs mb-2 font-medium">{error}</p>}
                    <p className="text-textMuted text-sm font-medium">@{store.selectedProfileUser?.username}</p>
                  </>
                ) : (
                  <>
                    <h2 className="text-2xl font-bold text-white truncate">{store.selectedProfileUser?.displayName}</h2>
                    <p className="text-textMuted text-sm mt-1 font-medium">@{store.selectedProfileUser?.username}</p>
                  </>
                )}
              </div>

              {store.selectedProfileUser?.id === store.currentUser?.id ? (
                <div className="flex flex-col gap-3">
                  <input
                    ref={profileFileInputRef}
                    type="file"
                    accept="image/*"
                    className="hidden"
                    onChange={e => onFileChange(e, 'profile')}
                  />
                  <button
                    onClick={() => profileFileInputRef.current?.click()}
                    className="w-full bg-surface text-white py-3.5 rounded-xl font-bold flex items-center justify-center hover:bg-surfaceHover transition-colors"
                  >
                    <Camera size={18} className="mr-2" /> Сменить аватар
                  </button>
                  <button
                    onClick={saveProfileChanges}
                    className="w-full bg-[#c70060] text-white py-3.5 rounded-xl font-bold hover:opacity-90 transition-opacity"
                  >
                    Сохранить
                  </button>
                </div>
              ) : (
                <div className="flex flex-col gap-3">
                  {store.profileSource === 'channelMembers' ? (
                    <>
                      <button
                        onClick={() => {
                          if (store.selectedProfileUser) {
                            if (!store.selectedProfileUser.isOnline) {
                              setOfflineToast('Пользователь не в сети');
                              setTimeout(() => setOfflineToast(null), 3000);
                            } else if (store.selectedChannelForMembers) {
                              signalRService.sendChannelInvite(
                                store.selectedProfileUser.id,
                                store.selectedChannelForMembers.id,
                                store.selectedChannelForMembers.name
                              );
                              setSentInvites(prev => new Set(prev).add(store.selectedProfileUser!.id));
                            }
                          }
                          store.closeProfileOnly();
                        }}
                        className="w-full bg-[#c70060] text-white py-3.5 rounded-xl font-bold flex items-center justify-center gap-2 hover:opacity-90 transition-opacity"
                      >
                        <UserPlus size={18} /> Позвать
                      </button>
                      {store.selectedChannelForMembers?.ownerId === store.currentUser?.id && (
                        <button
                          onClick={() => {
                            if (store.selectedProfileUser) {
                              store.setUserToKick(store.selectedProfileUser);
                              store.setModal('kickConfirm', true);
                            }
                            store.closeProfileOnly();
                          }}
                          className="w-full bg-surface text-danger py-3.5 rounded-xl font-bold hover:bg-[#2B2D31] transition-colors"
                        >
                          Исключить из канала
                        </button>
                      )}
                    </>
                  ) : (
                    <>
                      <button
                        onClick={async () => {
                          if (store.selectedProfileUser) {
                            const ok = await signalRService.startCall(store.selectedProfileUser.id);
                            if (!ok) {
                              setOfflineToast('Пользователь не в сети');
                              setTimeout(() => setOfflineToast(null), 3000);
                            }
                          }
                          store.closeProfileOnly();
                        }}
                        className="w-full bg-success text-white py-3.5 rounded-xl font-bold flex items-center justify-center gap-2 hover:bg-green-600 transition-colors"
                      >
                        <Phone size={18} /> Позвонить
                      </button>
                      <button
                        onClick={() => {
                          if (store.selectedProfileUser) signalRService.removeFriend(store.selectedProfileUser.id);
                          store.closeProfileOnly();
                        }}
                        className="w-full bg-surface text-danger py-3.5 rounded-xl font-bold hover:bg-[#2B2D31] transition-colors"
                      >
                        Удалить из друзей
                      </button>
                    </>
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
                <p className="text-[#c70060] font-black text-xs tracking-[0.2em] uppercase mb-1.5 opacity-90">Достижение получено</p>
                <p className="text-white font-black text-xl tracking-tight leading-none">{def.title}</p>
                <p className="text-textMuted font-bold text-sm mt-2 line-clamp-1 opacity-80">{def.description}</p>
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
              <Trophy size={24} />
              {store.achievementsViewUserId ? 'Достижения' : 'Мои достижения'}
            </h2>
            <button onClick={closeAndResetModals} className="text-textMuted hover:text-white transition-colors"><X size={24} /></button>
          </div>
          <div className="px-6 overflow-y-auto flex-1 space-y-3 pb-6">
            {(() => {
              const data = store.achievementsData;
              if (!data) return <p className="text-textMuted text-center py-8">Загрузка...</p>;
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

              if (filtered.length === 0) return <p className="text-textMuted text-center py-8 font-medium">Нет достижений</p>;

              return filtered.map(a => {
                const isUnlocked = unlocked.includes(a.id);
                const statVal = stats[a.statKey] ?? 0;
                const progress = Math.min(statVal / a.maxValue, 1);
                const showHidden = a.hidden && !isUnlocked;
                return (
                  <div key={a.id} className={`p-4 rounded-xl border transition-colors ${isUnlocked ? 'bg-[#c70060]/10 border-[#c70060]/30' : 'bg-surface border-transparent'}`}>
                    <div className="flex items-center gap-4">
                      <span className={`text-3xl ${showHidden ? 'blur-sm' : ''}`}>{showHidden ? '❓' : a.icon}</span>
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2 mb-1">
                          <span className="font-bold text-white truncate">{showHidden ? 'Скрытое достижение' : a.title}</span>
                          {isUnlocked && <span className="text-[10px] font-bold bg-[#c70060]/20 text-[#c70060] px-2 py-0.5 rounded-md shrink-0">✓ Получено</span>}
                        </div>
                        <p className="text-textMuted text-sm truncate">{showHidden ? '???' : a.description}</p>
                        {!showHidden && (
                          <div className="mt-2 flex items-center gap-3">
                            <div className="flex-1 h-1.5 bg-black/30 rounded-full overflow-hidden">
                              <div className="h-full rounded-full transition-all duration-500" style={{ width: `${progress * 100}%`, backgroundColor: isUnlocked ? '#c70060' : '#555' }} />
                            </div>
                            <span className="text-xs text-textMuted font-mono shrink-0">{formatProgress(statVal, a.maxValue, a.unit)}</span>
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
            <span className="text-xs text-textMuted">{store.achievementsData?.unlockedIds?.length ?? 0} / {ACHIEVEMENTS.length} получено</span>
          </div>
        </div>
      )}

      {offlineToast && createPortal(
        <div className="fixed bottom-10 left-1/2 -translate-x-1/2 z-[1000000] animate-toast-in">
          <div className="bg-[#09090B]/90 backdrop-blur-xl border border-danger/40 rounded-3xl px-8 py-5 shadow-[0_0_50px_rgba(239,68,68,0.25)] flex items-center gap-4">
            <div className="w-10 h-10 rounded-full bg-danger/20 flex items-center justify-center shrink-0">
               <WifiOff size={20} className="text-danger" />
            </div>
            <div>
              <p className="text-white font-bold text-base leading-tight">Уведомление</p>
              <p className="text-danger/90 font-medium text-sm mt-0.5">{offlineToast}</p>
            </div>
          </div>
        </div>,
        document.body
      )}

      {renderModal('adminConsole',
        <div className="bg-panelBg rounded-3xl w-[760px] max-h-[82vh] flex flex-col overflow-hidden shadow-2xl">
          <div className="flex items-center justify-between p-6 pb-4">
            <h2 className="text-xl font-bold text-white flex items-center gap-3">
              <Crown size={24} className="text-[#c70060]" />
              Админ-консоль
            </h2>

            <div className="flex items-center gap-2">
              <button
                onClick={loadAdminUsers}
                disabled={adminLoading}
                className="bg-surface text-white px-4 py-2 rounded-xl text-sm font-semibold hover:bg-surfaceHover transition-colors disabled:opacity-50"
              >
                Обновить
              </button>
              <button
                onClick={closeAndResetModals}
                className="text-textMuted hover:text-white transition-colors"
              >
                <X size={24} />
              </button>
            </div>
          </div>

          <div className="px-6 pb-4">
            <input
              type="text"
              value={adminSearch}
              onChange={e => setAdminSearch(e.target.value)}
              placeholder="Поиск по логину / имени"
              className="w-full bg-surface text-white rounded-xl p-3 outline-none focus:ring-2 focus:ring-[#c70060]"
            />
          </div>

          {adminError && (
            <div className="px-6 pb-3">
              <div className="bg-danger/10 border border-danger/30 rounded-xl px-4 py-3 text-danger text-sm font-medium">
                {adminError}
              </div>
            </div>
          )}

          <div className="px-6 overflow-y-auto flex-1 space-y-2 pb-6 custom-scrollbar">
            {adminLoading ? (
              <div className="flex justify-center py-12">
                <div className="w-8 h-8 border-2 border-[#c70060] border-t-transparent rounded-full animate-spin" />
              </div>
            ) : (
              adminUsers
                .filter(u =>
                  u.username.toLowerCase().includes(adminSearch.toLowerCase()) ||
                  u.displayName.toLowerCase().includes(adminSearch.toLowerCase())
                )
                .map(u => (
                  <div key={u.id} className="bg-surface rounded-xl p-4 flex items-center gap-4">
                    <div className="w-[47px] h-[47px] shrink-0 relative">
                      <AvatarImg src={u.avatarBase64} size={47} bgColor={u.avatarColor} />
                    </div>

                    <div className="flex-1 min-w-0">
                      <p className="text-white font-semibold truncate">{u.displayName}</p>
                      <p className="text-textMuted text-xs truncate">@{u.username}</p>
                      <p className="text-textMuted text-[11px] mt-1 truncate">
                        {u.currentChannelId
                          ? `В канале: ${u.currentChannelId}`
                          : u.currentCallUserId
                            ? `В звонке: ${u.currentCallUserId}`
                            : 'Не в голосе'}
                      </p>
                    </div>

                    <div className="text-xs text-textMuted shrink-0 min-w-[70px] text-right">
                      {u.isOnline ? 'Онлайн' : 'Оффлайн'}
                    </div>

                    <button
                      disabled={adminActionUserId === u.id}
                      onClick={async () => {
                        setAdminSelectedUser(null);
                        setAdminDetailsLoading(true);
                        setAdminError('');
                        store.setModal('adminUserSettings', true);

                        try {
                          const details = await signalRService.adminGetUserDetails(u.id);

                          if (!details) {
                            setAdminError('Не удалось загрузить профиль пользователя');
                          } else {
                            setAdminSelectedUser(details);
                            setAdminEditDisplayName(details.displayName ?? '');
                          }
                        } catch (e) {
                          setAdminError('Не удалось загрузить профиль пользователя');
                        } finally {
                          setAdminDetailsLoading(false);
                        }
                      }}
                      className="bg-surfaceHover hover:bg-white/10 text-white px-3 py-2 rounded-lg text-sm font-semibold transition-colors disabled:opacity-50"
                    >
                      Настройки
                    </button>
                  </div>
                ))
            )}

            {!adminLoading && adminUsers.length > 0 && adminUsers.filter(u =>
              u.username.toLowerCase().includes(adminSearch.toLowerCase()) ||
              u.displayName.toLowerCase().includes(adminSearch.toLowerCase())
            ).length === 0 && (
                <p className="text-textMuted text-center py-8 font-medium">Пользователи не найдены</p>
              )}
          </div>
        </div>
      )}

      {renderModal('adminUserSettings',
        <div className="bg-panelBg rounded-3xl w-[760px] max-h-[84vh] flex flex-col overflow-hidden shadow-2xl">
          <div className="flex items-center justify-between p-6 pb-4 border-b border-white/5">
            <h2 className="text-xl font-bold text-white flex items-center gap-3">
              <Settings size={24} />
              Профиль: {adminSelectedUser?.username}
            </h2>
            <button
              onClick={() => {
                setAdminSelectedUser(null);
                setAdminEditDisplayName('');
                setAdminRenameChannelId(null);
                store.setModal('adminUserSettings', false);
              }}
              className="text-textMuted hover:text-white transition-colors"
            >
              <X size={24} />
            </button>
          </div>

          {adminError && (
            <div className="px-6 pt-4">
              <div className="bg-danger/10 border border-danger/30 rounded-xl px-4 py-3 text-danger text-sm font-medium">
                {adminError}
              </div>
            </div>
          )}

          <div className="px-6 overflow-y-auto flex-1 py-6 custom-scrollbar">
            {adminDetailsLoading ? (
              <div className="flex justify-center py-12">
                <div className="w-8 h-8 border-2 border-[#c70060] border-t-transparent rounded-full animate-spin" />
              </div>
            ) : adminSelectedUser ? (
              <div className="space-y-6">
                {/* ПРОФИЛЬ */}
                <section className="bg-surface rounded-2xl p-5">
                  <div className="flex items-center gap-4 mb-5">
                    <div className="w-[71px] h-[71px] shrink-0 relative">
                      <AvatarImg src={adminSelectedUser.avatarBase64} size={71} bgColor={adminSelectedUser.avatarColor} />
                    </div>
                    <div className="min-w-0">
                      <p className="text-white font-bold text-lg truncate">{adminSelectedUser.displayName}</p>
                      <p className="text-textMuted text-sm truncate">@{adminSelectedUser.username}</p>
                      <p className="text-textMuted text-xs mt-1">
                        {adminSelectedUser.isOnline ? <span className="text-success">Онлайн</span> : 'Оффлайн'}
                      </p>
                    </div>
                  </div>

                  <div className="space-y-4">
                    <div>
                      <label className="text-[10px] font-bold text-textMuted tracking-wider block mb-2">
                        ОТОБРАЖАЕМОЕ ИМЯ
                      </label>
                      <input
                        type="text"
                        value={adminEditDisplayName}
                        onChange={e => setAdminEditDisplayName(e.target.value)}
                        className="w-full bg-surfaceHover text-white rounded-xl p-3 outline-none focus:ring-2 focus:ring-[#c70060]"
                      />
                    </div>

                    <button
                      onClick={async () => {
                        if (!adminEditDisplayName.trim()) return;
                        const ok = await signalRService.adminUpdateUser({
                          userId: adminSelectedUser.id,
                          displayName: adminEditDisplayName.trim()
                        });

                        if (ok) {
                          await loadAdminUserDetails(adminSelectedUser.id);
                          await loadAdminUsers();
                        } else {
                          setAdminError('Не удалось изменить имя');
                        }
                      }}
                      className="w-full bg-surfaceHover hover:bg-white/10 text-white py-3 rounded-xl font-semibold transition-colors"
                    >
                      Сохранить имя
                    </button>
                  </div>
                </section>

                {/* ГОЛОС И КАНАЛЫ */}
                <section className="bg-surface rounded-2xl p-5">
                  <div className="flex items-center justify-between mb-3">
                    <h3 className="text-white font-bold">Голос и каналы</h3>
                    {adminSelectedUser.currentChannelId && (
                      <button
                        onClick={async () => {
                          const ok = await signalRService.adminKickFromCurrentChannel(adminSelectedUser.id);
                          if (ok) {
                            await loadAdminUserDetails(adminSelectedUser.id);
                            await loadAdminUsers();
                          }
                        }}
                        className="text-xs bg-danger/20 text-danger px-3 py-1.5 rounded-lg hover:bg-danger hover:text-white transition-colors font-semibold"
                      >
                        Исключить из канала
                      </button>
                    )}
                  </div>

                  <p className="text-textMuted text-sm mb-4 bg-surfaceHover p-3 rounded-xl">
                    {adminSelectedUser.currentChannelId
                      ? `Сейчас в канале: ${adminSelectedUser.currentChannelId}`
                      : adminSelectedUser.currentCallUserId
                        ? `Сейчас в звонке: ${adminSelectedUser.currentCallUserId}`
                        : 'Сейчас не в голосе'}
                  </p>

                  <div className="grid grid-cols-2 gap-3 mb-6">
                    {/* КНОПКА МЬЮТА */}
                    <button
                      onClick={async () => {
                        const newState = !adminSelectedUser.isMuted;

                        // Оптимистичный UI
                        setAdminSelectedUser({ ...adminSelectedUser, isMuted: newState });

                        const ok = await signalRService.adminSetGlobalVoiceState({
                          userId: adminSelectedUser.id,
                          isMuted: newState
                        });

                        if (ok) {
                          loadAdminUsers();
                        } else {
                          loadAdminUserDetails(adminSelectedUser.id); // Откат
                        }
                      }}
                      className={`py-3 rounded-xl font-semibold flex items-center justify-center gap-2 transition-colors ${adminSelectedUser.isMuted ? 'bg-danger text-white hover:bg-red-600' : 'bg-surfaceHover text-white hover:bg-white/10'
                        }`}
                    >
                      {adminSelectedUser.isMuted ? <MicOff size={18} /> : <Mic size={18} />}
                      {adminSelectedUser.isMuted ? 'Микрофон выключен' : 'Глобально замьютить'}
                    </button>

                    {/* КНОПКА ГЛУШЕНИЯ (DEAFEN) */}
                    <button
                      onClick={async () => {
                        const newState = !adminSelectedUser.isDeafened;

                        // Оптимистичный UI (если глушим уши, микрофон тоже отключается)
                        setAdminSelectedUser({
                          ...adminSelectedUser,
                          isDeafened: newState,
                          isMuted: newState ? true : adminSelectedUser.isMuted
                        });

                        const ok = await signalRService.adminSetGlobalVoiceState({
                          userId: adminSelectedUser.id,
                          isDeafened: newState
                        });

                        if (ok) {
                          loadAdminUsers();
                        } else {
                          loadAdminUserDetails(adminSelectedUser.id); // Откат
                        }
                      }}
                      className={`py-3 rounded-xl font-semibold flex items-center justify-center gap-2 transition-colors ${adminSelectedUser.isDeafened ? 'bg-danger text-white hover:bg-red-600' : 'bg-surfaceHover text-white hover:bg-white/10'
                        }`}
                    >
                      <div className="relative flex items-center justify-center">
                        <Headphones size={18} className={adminSelectedUser.isDeafened ? 'opacity-60' : ''} />
                        {adminSelectedUser.isDeafened && (
                          <div className="absolute w-[22px] h-[2.5px] bg-white rotate-45 rounded-full" />
                        )}
                      </div>
                      {adminSelectedUser.isDeafened ? 'Звук выключен' : 'Глобально заглушить'}
                    </button>
                  </div>

                  <label className="text-[10px] font-bold text-textMuted mb-2 block tracking-wider">ДОСТУПНЫЕ КАНАЛЫ</label>
                  <div className="space-y-2 max-h-[200px] overflow-y-auto pr-2 custom-scrollbar">
                    {adminSelectedUser.accessibleChannels?.length > 0 ? (
                      adminSelectedUser.accessibleChannels.map((ch: any) => (
                        <div key={ch.id} className="bg-surfaceHover rounded-xl px-4 py-2 flex items-center justify-between min-h-[50px]">

                          {adminRenameChannelId === ch.id ? (
                            // РЕЖИМ РЕДАКТИРОВАНИЯ
                            <div className="flex items-center gap-2 w-full">
                              <input
                                autoFocus
                                type="text"
                                value={adminRenameChannelName}
                                onChange={e => setAdminRenameChannelName(e.target.value)}
                                onKeyDown={async e => {
                                  if (e.key === 'Enter') {
                                    const newName = adminRenameChannelName.trim();
                                    if (!newName) return;
                                    const ok = await signalRService.adminRenameChannel(ch.id, newName);
                                    if (ok) {
                                      await loadAdminUserDetails(adminSelectedUser.id);
                                      setAdminRenameChannelId(null);
                                    }
                                  }
                                }}
                                className="flex-1 bg-panelBg text-white rounded-lg px-3 py-1.5 outline-none focus:ring-2 focus:ring-[#c70060] text-sm"
                              />
                              <button
                                onClick={async () => {
                                  const newName = adminRenameChannelName.trim();
                                  if (!newName) return;
                                  const ok = await signalRService.adminRenameChannel(ch.id, newName);
                                  if (ok) {
                                    await loadAdminUserDetails(adminSelectedUser.id);
                                    setAdminRenameChannelId(null);
                                  }
                                }}
                                className="text-success hover:bg-success/20 p-1.5 rounded-lg transition-colors"
                                title="Сохранить"
                              >
                                <Check size={18} />
                              </button>
                              <button
                                onClick={() => setAdminRenameChannelId(null)}
                                className="text-danger hover:bg-danger/20 p-1.5 rounded-lg transition-colors"
                                title="Отмена"
                              >
                                <X size={18} />
                              </button>
                            </div>
                          ) : (
                            // РЕЖИМ ПРОСМОТРА
                            <>
                              <div className="min-w-0 pr-4">
                                <p className="text-white text-sm font-semibold truncate">{ch.name}</p>
                                <p className="text-textMuted text-[11px] truncate">{ch.id}</p>
                              </div>
                              <button
                                onClick={() => {
                                  setAdminRenameChannelId(ch.id);
                                  setAdminRenameChannelName(ch.name);
                                }}
                                className="bg-panelBg hover:bg-[#333] text-textMuted hover:text-white px-3 py-1.5 rounded-lg text-xs font-semibold transition-colors shrink-0"
                              >
                                Изменить
                              </button>
                            </>
                          )}
                        </div>
                      ))
                    ) : (
                      <p className="text-textMuted text-sm">Нет доступных каналов</p>
                    )}
                  </div>
                </section>

                {/* Достижения (Инлайн и 100% прогресс) */}
                <section className="bg-surface rounded-2xl p-5">
                  <div className="flex items-center justify-between mb-4">
                    <h3 className="text-white font-bold">Достижения</h3>
                    <span className="text-sm font-semibold text-[#c70060] bg-[#c70060]/10 px-3 py-1 rounded-full">
                      Разблокировано: {adminSelectedUser.achievements?.unlockedIds?.length ?? 0}
                    </span>
                  </div>

                  <div className="space-y-2 max-h-[320px] overflow-y-auto pr-1 custom-scrollbar">
                    {ACHIEVEMENTS.map(achievement => {
                      const unlocked = (adminSelectedUser.achievements?.unlockedIds || []).includes(achievement.id);
                      return (
                        <label
                          key={achievement.id}
                          className={`flex items-start gap-3 rounded-xl px-4 py-3 cursor-pointer transition-colors border ${unlocked ? 'bg-[#c70060]/10 border-[#c70060]/30' : 'bg-surfaceHover border-transparent hover:bg-white/5'
                            }`}
                        >
                          <input
                            type="checkbox"
                            checked={unlocked}
                            onChange={async (e) => {
                              const isGranted = e.target.checked;

                              // ДОБАВЛЕН ЯВНЫЙ ТИП <string> ЗДЕСЬ:
                              const currentUnlocked = new Set<string>(adminSelectedUser.achievements?.unlockedIds || []);
                              const currentStats = { ...(adminSelectedUser.achievements?.stats || {}) };

                              // Выставляем 100% прогресса или сбрасываем в 0
                              if (isGranted) {
                                currentUnlocked.add(achievement.id);
                                currentStats[achievement.statKey] = achievement.maxValue;
                              } else {
                                currentUnlocked.delete(achievement.id);
                                currentStats[achievement.statKey] = 0;
                              }

                              const nextUnlocked = Array.from(currentUnlocked);
                              const nextAchievements = {
                                stats: currentStats,
                                unlockedIds: nextUnlocked,
                                visitedChannelIds: adminSelectedUser.achievements?.visitedChannelIds || []
                              };

                              // Оптимистичный UI
                              setAdminSelectedUser({
                                ...adminSelectedUser,
                                achievements: nextAchievements
                              });

                              // Отправляем на сервер
                              const ok = await signalRService.adminUpdateAchievements({
                                userId: adminSelectedUser.id,
                                unlockedIds: nextUnlocked,
                                stats: currentStats
                              });

                              if (!ok) {
                                setAdminError('Ошибка синхронизации достижений');
                                loadAdminUserDetails(adminSelectedUser.id); // Откат
                              }
                            }}
                            className="mt-1.5 w-4 h-4 accent-[#c70060] cursor-pointer"
                          />
                          <div className="flex-1 min-w-0">
                            <div className="flex items-center gap-2">
                              <span className="text-lg">{achievement.icon}</span>
                              <span className={`font-semibold truncate ${unlocked ? 'text-white' : 'text-textMuted'}`}>
                                {achievement.title}
                              </span>
                            </div>
                            <p className="text-textMuted text-xs mt-1 leading-tight">{achievement.description}</p>
                          </div>
                        </label>
                      );
                    })}
                  </div>
                </section>

                {/* DANGER ZONE */}
                <section className="pt-4 border-t border-white/5">
                  <button
                    onClick={async () => {
                      const confirmed = confirm(`Удалить пользователя ${adminSelectedUser.displayName}? Это действие необратимо.`);
                      if (!confirmed) return;

                      const ok = await signalRService.adminDeleteUser(adminSelectedUser.id);
                      if (ok) {
                        store.setFriends(store.friends.filter(f => f.id !== adminSelectedUser.id));
                        setAdminSelectedUser(null);
                        store.setModal('adminUserSettings', false);
                        await loadAdminUsers();
                      } else {
                        setAdminError('Не удалось удалить пользователя');
                      }
                    }}
                    className="w-full bg-danger/10 text-danger hover:bg-danger hover:text-white py-4 rounded-xl font-bold transition-colors"
                  >
                    УДАЛИТЬ ПОЛЬЗОВАТЕЛЯ НАВСЕГДА
                  </button>
                </section>
              </div>
            ) : null}
          </div>
        </div>
      )}

      {adminBlockToast && createPortal((() => {
        const isHiding = adminBlockToast === '__hiding__';
        return (
          <div className={`fixed top-14 left-1/2 z-[1000000] ${isHiding ? 'animate-admin-block-out' : 'animate-admin-block-in'}`}>
            <div className="bg-[#09090B]/90 backdrop-blur-xl border border-danger/50 rounded-3xl px-8 py-6 flex items-center gap-5 shadow-[0_0_60px_rgba(239,68,68,0.3)] ring-1 ring-white/5">
              <div className="w-12 h-12 rounded-2xl bg-danger/20 flex items-center justify-center shrink-0 shadow-inner">
                <MicOff size={24} className="text-danger" />
              </div>
              <div className="min-w-0">
                <p className="text-white font-black text-lg tracking-tight leading-none mb-1">Доступ ограничен</p>
                <p className="text-textMuted font-bold text-sm truncate opacity-90">{typeof adminBlockToast === 'string' && adminBlockToast !== '__hiding__' ? adminBlockToast : 'Администратор запретил это действие'}</p>
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
                <button onClick={() => { setEditChannelId(contextMenu.item.id); setEditChannelName(contextMenu.item.name); store.setModal('channelEdit', true); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-white hover:bg-surfaceHover flex items-center gap-3 font-medium"><Edit2 size={16} /> Переименовать</button>
              )}
              <button onClick={() => { signalRService.quitAccessChannel(contextMenu.item.id); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-danger hover:bg-surfaceHover flex items-center gap-3 font-medium mt-1"><LeaveIcon size={16} /> Выйти из канала</button>
            </>
          ) : contextMenu.type === 'channelMember' ? (
            <>
              <button onClick={() => { store.setSelectedProfileUser(contextMenu.item, 'channelMembers'); store.setModal('profile', true); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-white hover:bg-surfaceHover flex items-center gap-3 font-medium"><Settings size={16} /> Профиль</button>
              {contextMenu.item.id !== store.currentUser?.id && (
                <button onClick={() => {
                  if (!contextMenu.item.isOnline) {
                    setOfflineToast('Пользователь не в сети');
                    setTimeout(() => setOfflineToast(null), 3000);
                  } else if (store.selectedChannelForMembers) {
                    signalRService.sendChannelInvite(contextMenu.item.id, store.selectedChannelForMembers.id, store.selectedChannelForMembers.name);
                    setSentInvites(prev => new Set(prev).add(contextMenu.item.id));
                  }
                  setContextMenu(null);
                }} className="w-full text-left px-4 py-2 text-white hover:bg-surfaceHover flex items-center gap-3 font-medium mt-1">
                  <UserPlus size={16} /> Позвать в канал
                </button>
              )}
              {store.selectedChannelForMembers?.ownerId === store.currentUser?.id && contextMenu.item.id !== store.currentUser?.id && (
                <button onClick={() => { store.setUserToKick(contextMenu.item); store.setModal('kickConfirm', true); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-danger hover:bg-surfaceHover flex items-center gap-3 font-medium mt-1"><UserX size={16} /> Исключить</button>
              )}
            </>
          ) : contextMenu.type === 'voiceUser' ? (
            <>
              <button onClick={() => { setVolumeUser(contextMenu.item); setVolumeUserValue(store.userVolumes[contextMenu.item.id] ?? 100); store.setModal('userVolume', true); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-white hover:bg-surfaceHover flex items-center gap-3 font-medium"><Volume2 size={16} /> Громкость</button>
              <button onClick={() => { store.setSelectedProfileUser(contextMenu.item, 'voiceUsers'); store.setModal('profile', true); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-white hover:bg-surfaceHover flex items-center gap-3 font-medium mt-1"><Settings size={16} /> Профиль</button>
            </>
          ) : (
            <>
              <button onClick={() => { store.setSelectedProfileUser(contextMenu.item, 'friends'); store.setModal('profile', true); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-white hover:bg-surfaceHover flex items-center gap-3 font-medium"><Settings size={16} /> Профиль</button>
              <button onClick={() => { signalRService.removeFriend(contextMenu.item.id); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-danger hover:bg-surfaceHover flex items-center gap-3 font-medium mt-1"><UserMinus size={16} /> Удалить</button>
            </>
          )}
        </div>
      )}
    </>
  );
}