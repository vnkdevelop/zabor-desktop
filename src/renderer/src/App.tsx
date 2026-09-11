import { useState, useEffect, useRef, useMemo, useCallback, memo } from 'react';
import { createPortal } from 'react-dom';
import { Gear as Settings, Microphone as Mic, MicrophoneSlash as MicOff, Headphones, PhoneCall as Phone, Eye, EyeSlash as EyeOff, UserMinus, UserMinus as UserX, Camera, Check, X, SignOut as LogOut, UserPlus, Envelope as Mail, PencilSimple as Edit2, SpeakerHigh as Volume2, SpeakerSlash, PhoneDisconnect as PhoneOff, WifiHigh as Wifi, WifiSlash as WifiOff, Users, SignOut as LeaveIcon, Crown, Globe, Trophy, Plus, Key, UserCircleMinus, UserCheck, Desktop, CornersIn, CornersOut, Sparkle, Trash, ArrowsClockwise, ChatCircle } from '@phosphor-icons/react';
import { useTranslation, Trans } from 'react-i18next';

import { useAppStore, User, VoiceChannel } from './store/useAppStore';
import { useSpeakingStore } from './store/useSpeakingStore';
import { signalRService } from './services/signalr';
import { webrtc, CalibrationError } from './services/webrtc';
import { isPackedGif, packGif, unpackGif, getDisplaySrc, getStaticFrameSync, preloadStaticFrame } from './utils/avatar';
import i18n from './i18n';

import { ACHIEVEMENTS, getAchievementDef, formatProgress, AchievementsPayload, getProgressPercent, AchievementDef } from './achievements';
import { translateJoke } from './utils/jokesTranslation';

import { TitleBar } from './components/Layout/TitleBar';
import { Md3Slider } from './components/Shared/Md3Slider';
import { Md3Switch } from './components/Shared/Md3Switch';
import { GlassSelect } from './components/Shared/GlassSelect';
import { AvatarImg } from './components/Shared/AvatarImg';
import { StreamPicker } from './components/Stream/StreamPicker';
import { StreamCard } from './components/Stream/StreamCard';
import { NoiseSuppressionSettings, type CalibrationPhase, type SmartNoiseModel } from './components/Settings/NoiseSuppressionSettings';
import { MIC_TEST_PANEL_GAP_PX, MicTestPanel } from './components/Settings/MicTestPanel';
import { UpdateModal } from './components/Modals/UpdateModal';
import { ChatPanel } from './components/Chat/ChatPanel';
import { ActiveSessionPip } from './components/Chat/ActiveSessionPip';
import { chatPeer } from './services/chatPeer';
import { useChatStore } from './store/useChatStore';

const lastNonZeroUserVolumes = new Map<string, number>();
const lastNonZeroVolumes = new Map<string, number>();

const VoiceUserCard = memo(({ user, cardSize, isIdle, t, handleContextMenu, webrtcConnections, currentUserId }: {
  user: User;
  cardSize: any;
  isIdle: boolean;
  t: any;
  handleContextMenu: any;
  webrtcConnections: any;
  currentUserId: string | undefined;
}) => {
  const isSpeaking = useSpeakingStore(state => state.speaking[user.id] ?? false);
  const isLocal = user.id === currentUserId;
  const isConnected = webrtcConnections[user.id] || isLocal;

  return (
    <div onContextMenu={e => handleContextMenu(e, 'voiceUser', user)}
      className={`relative flex flex-col items-center justify-center cursor-pointer transition-all duration-200 overflow-hidden shrink-0 hover:-translate-y-1 animate-avatar-in
        ${(isSpeaking && isConnected) ? 'shadow-[inset_0_0_0_3px_#3BA55C,inset_0_0_0_5px_#181818,0_10px_15px_-3px_rgba(0,0,0,0.5)] z-10' : ''}`}
      style={{ backgroundColor: user.avatarColor, width: `${cardSize.w}px`, height: `${cardSize.h}px`, borderRadius: '24px' }}>
      <div className="relative" style={{ width: `${cardSize.avatarSize}px`, height: `${cardSize.avatarSize}px`, marginBottom: cardSize.avatarSize <= 48 ? '4px' : '16px' }}>
        <AvatarImg src={user.avatarBase64} size={cardSize.avatarSize} bgColor="transparent" />
      </div>
      {(!webrtcConnections[user.id] && !isLocal) && (
        <div className="absolute inset-0 bg-black/40 flex flex-col items-center justify-center z-20 backdrop-blur-[2px]" style={{ borderRadius: '24px' }}>
          <div className="flex gap-2.5">
            <div className="w-3 h-3 bg-primary/90 rounded-full animate-pulse" />
            <div className="w-3 h-3 bg-primary/90 rounded-full animate-pulse" style={{ animationDelay: '0.15s' }} />
            <div className="w-3 h-3 bg-primary/90 rounded-full animate-pulse" style={{ animationDelay: '0.3s' }} />
          </div>
        </div>
      )}
      {isConnected && (
        <div className={`absolute ${cardSize.avatarSize <= 48 ? 'bottom-2' : 'bottom-4'} left-1/2 -translate-x-1/2 transition-all duration-300 ${isIdle ? 'translate-y-8 opacity-0 pointer-events-none' : 'translate-y-0 opacity-100'}`}>
          <div className={`bg-[#09090B]/80 backdrop-blur-md border border-[#303035]/50 rounded-full flex items-center gap-1.5 whitespace-nowrap ${cardSize.avatarSize <= 48 ? 'px-2 py-0.5' : 'px-4 py-1.5'
            }`} style={{ maxWidth: `${cardSize.w - 20}px` }}>
            <span className={`text-white font-bold truncate ${cardSize.avatarSize <= 48 ? 'text-[11px]' : 'text-sm'}`}>{user.displayName}</span>
            {(user.isMuted || user.isServerMuted) && <MicOff weight="bold" size={cardSize.avatarSize <= 48 ? 10 : 14} className="text-danger shrink-0" />}
            {(user.isDeafened || user.isServerDeafened) && <SpeakerSlash weight="bold" size={cardSize.avatarSize <= 48 ? 10 : 14} className="text-danger shrink-0" />}
          </div>
        </div>
      )}
    </div>
  );
});
VoiceUserCard.displayName = 'VoiceUserCard';

const CallUserCard = memo(({ currentCallUser, callStatus, cardSize, webrtcConnections, handleContextMenu, t, isIdle }: {
  currentCallUser: User;
  callStatus: string;
  cardSize: any;
  webrtcConnections: any;
  handleContextMenu: any;
  t: any;
  isIdle: boolean;
}) => {
  const isSpeaking = useSpeakingStore(state => state.speaking[currentCallUser.id] ?? false);
  const isConnected = webrtcConnections[currentCallUser.id];

  return (
    <div
      onContextMenu={e => handleContextMenu(e, 'voiceUser', currentCallUser)}
      className={`relative flex flex-col items-center justify-center overflow-hidden shrink-0 transition-all duration-200 animate-avatar-in
        ${(isSpeaking && callStatus === 'connected' && isConnected)
          ? 'shadow-[inset_0_0_0_3px_#3BA55C,inset_0_0_0_5px_#181818,0_10px_15px_-3px_rgba(0,0,0,0.5)] z-10'
          : ''
        }`}
      style={{
        backgroundColor: currentCallUser.avatarColor,
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
          marginBottom: cardSize.avatarSize <= 48 ? '4px' : '16px'
        }}
      >
        <AvatarImg src={currentCallUser.avatarBase64} size={cardSize.avatarSize} bgColor="transparent" />
      </div>

      {(!isConnected && callStatus !== 'calling') && (
        <div className="absolute inset-0 bg-black/40 flex flex-col items-center justify-center z-20 backdrop-blur-[2px]" style={{ borderRadius: '24px' }}>
          <div className="flex gap-2.5">
            <div className="w-3 h-3 bg-primary/90 rounded-full animate-pulse" />
            <div className="w-3 h-3 bg-primary/90 rounded-full animate-pulse" style={{ animationDelay: '0.15s' }} />
            <div className="w-3 h-3 bg-primary/90 rounded-full animate-pulse" style={{ animationDelay: '0.3s' }} />
          </div>
        </div>
      )}

      {callStatus === 'calling' && (
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

      {(isConnected || callStatus === 'calling') && (
        <div
          className={`absolute ${cardSize.avatarSize <= 48 ? 'bottom-2' : 'bottom-4'} left-1/2 -translate-x-1/2 transition-all duration-300 ${isIdle && callStatus === 'connected'
            ? 'translate-y-8 opacity-0 pointer-events-none'
            : 'translate-y-0 opacity-100'
            }`}
        >
          <div
            className={`bg-[#09090B]/80 backdrop-blur-md border border-[#303035]/50 rounded-full flex items-center gap-1.5 whitespace-nowrap ${cardSize.avatarSize <= 48 ? 'px-2 py-0.5' : 'px-4 py-1.5'}`}
            style={{ maxWidth: `${cardSize.w - 20}px` }}
          >
            <span className={`text-white font-bold truncate ${cardSize.avatarSize <= 48 ? 'text-[11px]' : 'text-sm'}`}>{currentCallUser.displayName}</span>

            {callStatus === 'calling' && (
              <span className="text-textMuted text-xs font-medium">{t('toasts.calling', 'дозвон...')}</span>
            )}

            {callStatus === 'connected' && (currentCallUser.isMuted || currentCallUser.isServerMuted) && (
              <MicOff weight="bold" size={cardSize.avatarSize <= 48 ? 10 : 14} className="text-danger shrink-0" />
            )}
            {callStatus === 'connected' && (currentCallUser.isDeafened || currentCallUser.isServerDeafened) && (
              <SpeakerSlash weight="bold" size={cardSize.avatarSize <= 48 ? 10 : 14} className="text-danger shrink-0" />
            )}
          </div>
        </div>
      )}
    </div>
  );
});
CallUserCard.displayName = 'CallUserCard';

export default function App() {
  const { t, i18n } = useTranslation();
  const store = useAppStore();
  const speakingMap = useSpeakingStore(state => state.speaking);

  const [isAuth, setIsAuth] = useState(false);
  const [language, setLanguage] = useState(i18n.language || 'ru');
  const [isLoading, setIsLoading] = useState(false);
  const [activeTab, setActiveTab] = useState<'channels' | 'friends'>('channels');
  const [showStreamPicker, setShowStreamPicker] = useState(false);
  const [volumeType, setVolumeType] = useState<'voice' | 'stream'>('voice');
  const [showOverlays, setShowOverlays] = useState(true);
  const [callView, setCallView] = useState<'full' | 'chat'>('full');
  const previousCallUserIdRef = useRef<string | null>(null);
  const selectedChatFriendId = useChatStore(state => state.selectedFriendId);
  const selectChatFriend = useChatStore(state => state.selectFriend);
  const unreadChats = useChatStore(state => state.unread);
  const hasUnreadChats = Object.values(unreadChats).some(Boolean);
  const overlayTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const [windowSize, setWindowSize] = useState({
    width: typeof window !== 'undefined' ? window.innerWidth : 1280,
    height: typeof window !== 'undefined' ? window.innerHeight : 720
  });

  const [manualUpdateChecking, setManualUpdateChecking] = useState(false);
  const [manualUpdateStatus, setManualUpdateStatus] = useState<string | null>(null);
  const [appVersion, setAppVersion] = useState<string>('');

  useEffect(() => {
    window.windowControls?.getAppVersion?.().then(v => {
      if (v) setAppVersion(v);
    }).catch(() => {});

    const unsubAvailable = window.windowControls?.onUpdateAvailable?.((info) => {
      store.setUpdateInfo(info);
      store.setUpdateStatus('available');
      store.setModal('update', true);
    });

    const unsubProgress = window.windowControls?.onUpdateDownloadProgress?.((progress) => {
      store.setUpdateProgress(progress);
      store.setUpdateStatus('downloading');
    });

    const unsubDownloaded = window.windowControls?.onUpdateDownloaded?.(() => {
      store.setUpdateStatus('downloaded');
    });

    const unsubError = window.windowControls?.onUpdateError?.((err) => {
      store.setUpdateStatus('error');
      store.setUpdateError(err);
    });

    return () => {
      unsubAvailable?.();
      unsubProgress?.();
      unsubDownloaded?.();
      unsubError?.();
    };
  }, []);

  const handleManualCheckUpdates = useCallback(async () => {
    setManualUpdateChecking(true);
    setManualUpdateStatus(null);
    try {
      const res = await window.windowControls?.checkForUpdates?.();
      setManualUpdateChecking(false);
      if (res?.updateAvailable && res?.updateInfo) {
        store.setUpdateInfo(res.updateInfo);
        store.setUpdateStatus('available');
        store.setModal('update', true);
        setManualUpdateStatus(t('settings.general.updateAvailableNotice', { version: res.updateInfo.version }));
      } else if (res?.error) {
        setManualUpdateStatus(t('settings.general.checkFailed'));
      } else {
        setManualUpdateStatus(t('settings.general.latestVersionInstalled'));
      }
    } catch {
      setManualUpdateChecking(false);
      setManualUpdateStatus(t('settings.general.checkFailed'));
    }
  }, [t, store]);

  useEffect(() => {
    const handleResize = () => {
      setWindowSize({ width: window.innerWidth, height: window.innerHeight });
    };
    window.addEventListener('resize', handleResize);
    return () => window.removeEventListener('resize', handleResize);
  }, []);

  useEffect(() => {
    if (store.isStreamFullscreen) {
      setShowOverlays(true);
      if (overlayTimeoutRef.current) clearTimeout(overlayTimeoutRef.current);
      overlayTimeoutRef.current = setTimeout(() => {
        setShowOverlays(false);
      }, 3000);

      const onMove = () => {
        setShowOverlays(true);
        if (overlayTimeoutRef.current) clearTimeout(overlayTimeoutRef.current);
        overlayTimeoutRef.current = setTimeout(() => {
          setShowOverlays(false);
        }, 3000);
      };
      window.addEventListener('mousemove', onMove);
      return () => {
        window.removeEventListener('mousemove', onMove);
        if (overlayTimeoutRef.current) clearTimeout(overlayTimeoutRef.current);
      };
    } else {
      setShowOverlays(true);
    }
    return undefined;
  }, [store.isStreamFullscreen]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!document.hasFocus() || event.repeat) return;
      const target = event.target as HTMLElement | null;
      if (target && (target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName))) return;
      if (event.code === 'KeyF' && !event.ctrlKey && !event.metaKey && !event.altKey) {
        if (!store.activeStreamId) return;
        event.preventDefault();
        store.setStreamFullscreen(!store.isStreamFullscreen);
      } else if (event.key === 'Escape' && store.isStreamFullscreen) {
        event.preventDefault();
        store.setStreamFullscreen(false);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [store.activeStreamId, store.isStreamFullscreen, store.setStreamFullscreen]);

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
  const [captchaData, setCaptchaData] = useState<{ id: string; imageBase64: string } | null>(null);
  const [captchaAnswer, setCaptchaAnswer] = useState('');
  const [isCaptchaLoading, setIsCaptchaLoading] = useState(false);

  const [displayName, setDisplayName] = useState('');
  const [avatarBase64, setAvatarBase64] = useState<string | null>(null);
  const [avatarColor, setAvatarColor] = useState<string>('#C81E70');
  const [editProfileDisplayName, setEditProfileDisplayName] = useState('');
  const [editProfileAvatarBase64, setEditProfileAvatarBase64] = useState<string | null>(null);
  const [editProfileAvatarColor, setEditProfileAvatarColor] = useState<string>('#C81E70');
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
  const [micThresholdMode, setMicThresholdMode] = useState<'auto' | 'manual'>('auto');
  const [manualThresholdValue, setManualThresholdValue] = useState(-42);
  const [smartNoiseModel, setSmartNoiseModel] = useState<SmartNoiseModel>(() => webrtc.getSmartNoiseModel());
  const [suppressionStrength, setSuppressionStrength] = useState(() => webrtc.getSuppressionStrength());
  const [speechAnalyzerEnabled, setSpeechAnalyzerEnabled] = useState(() => webrtc.isSpeechAnalyzerEnabled());
  const [echoCancellationEnabled, setEchoCancellationEnabled] = useState(() => webrtc.isEchoCancellationEnabled());
  const [isSwitchingChannel, setIsSwitchingChannel] = useState(false);
  const [autoLaunch, setAutoLaunch] = useState(false);
  const [minimizeToTray, setMinimizeToTray] = useState(true);

  const [contextMenu, setContextMenu] = useState<{
    visible: boolean; x: number; y: number;
    type: 'channel' | 'friend' | 'voiceUser' | 'channelMember' | 'stream'; item: any;
  } | null>(null);
  const contextMenuRef = useRef<HTMLDivElement | null>(null);
  const [showInvitesPanel, setShowInvitesPanel] = useState(false);
  const [settingsTab, setSettingsTab] = useState<'general' | 'audio' | 'privacy'>('general');
  const [relayOnlyIce, setRelayOnlyIce] = useState(() => webrtc.isRelayOnlyIce());
  const [showDeleteAccountModal, setShowDeleteAccountModal] = useState(false);
  const [deleteConfirmText, setDeleteConfirmText] = useState('');
  const [deleteAccountError, setDeleteAccountError] = useState('');
  const [isDeletingAccount, setIsDeletingAccount] = useState(false);
  const [micTestPanelHeight, setMicTestPanelHeight] = useState(0);
  const micTestPanelOpen = noiseSuppression && settingsTab === 'audio';
  const micTestPanelReserve = micTestPanelOpen && micTestPanelHeight > 0 ? micTestPanelHeight + MIC_TEST_PANEL_GAP_PX : 0;
  const [isCalibrating, setIsCalibrating] = useState(false);
  const [calibrationCountdown, setCalibrationCountdown] = useState(10);
  const [calibrationPhase, setCalibrationPhase] = useState<CalibrationPhase>('idle');
  const [calibrationSuccess, setCalibrationSuccess] = useState(false);
  const [inviteFriendSearch, setInviteFriendSearch] = useState('');
  const [sentInvites, setSentInvites] = useState<Set<string>>(new Set());
  const [inviteLoadingChannelId, setInviteLoadingChannelId] = useState<string | null>(null);
  const [isChannelMembersLoading, setIsChannelMembersLoading] = useState(false);
  const inviteRequestIdRef = useRef(0);
  const membersRequestIdRef = useRef(0);

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

  const removeSentInvite = useCallback((userId: string) => {
    setSentInvites(prev => {
      const next = new Set(prev);
      next.delete(userId);
      return next;
    });
  }, []);

  useEffect(() => {
    setProfileFriendRequestStatus('idle');
    const profile = store.selectedProfileUser;
    if (!store.modals.profile || !profile || profile.id === store.currentUser?.id) return;

    let cancelled = false;
    const profileId = profile.id;
    const profileSource = store.profileSource;
    signalRService.getUserByUsername(profile.username).then(freshUser => {
      const latestStore = useAppStore.getState();
      if (!cancelled && freshUser && latestStore.modals.profile && latestStore.selectedProfileUser?.id === profileId) {
        latestStore.setSelectedProfileUser(freshUser, profileSource);
      }
    }).catch(err => console.error("Failed to fetch fresh user profile:", err));

    return () => { cancelled = true; };
  }, [store.modals.profile, store.selectedProfileUser?.id, store.selectedProfileUser?.username, store.currentUser?.id]);

  useEffect(() => {
    if (!store.modals.settings) {
      setCalibrationSuccess(false);
    }
  }, [store.modals.settings]);

  useEffect(() => {
    if (!store.incomingChannelInvite) return;
    const timer = setTimeout(() => {
      const inviteName = useAppStore.getState().incomingChannelInvite?.channelName;
      store.setModal('incomingChannelInvite', false);
      store.setIncomingChannelInvite(null);
      signalRService.stopRingtone();
      if (inviteName) {
        useAppStore.getState().setSystemToast(t('toasts.missedChannelInvite', { name: inviteName, defaultValue: `пропущенный зов в канал: ${inviteName}` }));
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
        useAppStore.getState().setSystemToast(t('toasts.missedCall', { name: callerName, defaultValue: `пропущенный звонок от: ${callerName}` }));
        setTimeout(() => useAppStore.getState().setSystemToast(null), 4000);
      }
    }, 30000);
    return () => clearTimeout(timer);
  }, [store.incomingCall]);

  useEffect(() => {
    if (store.callStatus !== 'calling') return;
    const timer = setTimeout(() => {
      signalRService.endCall();
      setOfflineToast(t('toasts.noAnswer', 'не отвечает'));
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
  const isLoggingOutRef = useRef(false);

  const autoLoginPendingRef = useRef(false);
  const autoLoginInFlightRef = useRef(false);
  const autoLoginAttemptsRef = useRef(0);
  const autoLoginRetryTimerRef = useRef<NodeJS.Timeout | null>(null);
  const attemptAutoLoginRef = useRef<() => void>(() => { });
  const loginInputRef = useRef<HTMLInputElement>(null);
  const passwordInputRef = useRef<HTMLInputElement>(null);

  const [controlsShake, setControlsShake] = useState(false);
  const [adminBlockToast, setAdminBlockToast] = useState<string | null>(null);
  const adminBlockTimerRef = useRef<NodeJS.Timeout | null>(null);
  const settingsRef = useRef({
    inputVolume: 100, outputVolume: 100,
    selectedInput: 'default', selectedOutput: 'default',
    noiseSuppression: true, language: i18n.language || 'ru',
    openAtLogin: false, minimizeToTray: true,
    micThresholdMode: 'auto' as 'auto' | 'manual',
    manualThresholdValue: -42
  });

  useEffect(() => {
    settingsRef.current = {
      inputVolume, outputVolume, selectedInput, selectedOutput, noiseSuppression, language,
      openAtLogin: autoLaunch, minimizeToTray, micThresholdMode, manualThresholdValue
    };
  }, [inputVolume, outputVolume, selectedInput, selectedOutput, noiseSuppression, language, autoLaunch, minimizeToTray, micThresholdMode, manualThresholdValue]);

  useEffect(() => {
    let glowElement: HTMLElement | null = null;
    let frameHandle = 0;
    let pendingX = 0;
    let pendingY = 0;

    const resolveGlow = () => {
      if (!glowElement || !glowElement.isConnected) glowElement = document.getElementById('mouse-glow');
      return glowElement;
    };

    const applyPendingPosition = () => {
      frameHandle = 0;
      const glow = resolveGlow();
      if (!glow) return;
      glow.style.transform = `translate3d(${pendingX}px, ${pendingY}px, 0)`;
      glow.style.opacity = '1';
    };

    const handleMouseMove = (e: MouseEvent) => {
      pendingX = e.clientX;
      pendingY = e.clientY;
      if (frameHandle === 0) frameHandle = requestAnimationFrame(applyPendingPosition);
    };

    const handleMouseLeave = () => {
      if (frameHandle !== 0) {
        cancelAnimationFrame(frameHandle);
        frameHandle = 0;
      }
      const glow = resolveGlow();
      if (glow) glow.style.opacity = '0';
    };

    window.addEventListener('mousemove', handleMouseMove, { passive: true });
    document.addEventListener('mouseleave', handleMouseLeave);

    return () => {
      if (frameHandle !== 0) cancelAnimationFrame(frameHandle);
      window.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseleave', handleMouseLeave);
    };
  }, []);

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
          minimizeToTray: settingsRef.current.minimizeToTray,
          micThresholdMode: settingsRef.current.micThresholdMode,
          manualThresholdValue: settingsRef.current.manualThresholdValue
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
    setMicThresholdMode('auto');
    setManualThresholdValue(-42);
    setDisplayName('');
    setAvatarBase64(null);
    setAvatarColor('#C81E70');
    setEditProfileAvatarBase64(null);
    setEditProfileAvatarColor('#C81E70');
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
    localStorage.removeItem('zabor_threshold_mode');
    localStorage.removeItem('zabor_manual_threshold_value');
  }, []);

  const applySettings = useCallback((s: {
    inputVolume?: number; outputVolume?: number;
    selectedInput?: string; selectedOutput?: string;
    noiseSuppression?: boolean;
    userVolumes?: Record<string, number>;
    language?: string;
    openAtLogin?: boolean;
    minimizeToTray?: boolean;
    micThresholdMode?: 'auto' | 'manual';
    manualThresholdValue?: number;
  }) => {
    const iv = s.inputVolume ?? 100;
    const ov = s.outputVolume ?? 100;
    setInputVolume(iv);
    setOutputVolume(ov);
    const rawInput = s.selectedInput ?? 'default';
    const rawOutput = s.selectedOutput ?? 'default';
    const normalizedInput = rawInput === 'communications' ? 'default' : rawInput;
    const normalizedOutput = rawOutput === 'communications' ? 'default' : rawOutput;
    setSelectedInput(normalizedInput);
    setSelectedOutput(normalizedOutput);
    setNoiseSuppression(s.noiseSuppression ?? true);

    const mode = s.micThresholdMode ?? 'auto';
    const savedThreshold = s.manualThresholdValue ?? -42;
    const val = savedThreshold < 0 ? Math.max(-60, Math.min(-12, savedThreshold)) : -42;
    setMicThresholdMode(mode);
    setManualThresholdValue(val);
    webrtc.setMicThresholdParams(mode, val);
    webrtc.warmUpSmartNoiseSuppression(s.noiseSuppression ?? true);

    webrtc.setInputDevice(normalizedInput);
    webrtc.setOutputDevice(normalizedOutput);
    webrtc.setInputVolume(iv);
    webrtc.setOutputVolume(ov);

    if (s.language) {
      setLanguage(s.language);
      i18n.changeLanguage(s.language);
    }

    if (s.openAtLogin !== undefined) {
      setAutoLaunch(s.openAtLogin);
      window.windowControls.setAutoLaunch(s.openAtLogin).catch(() => { });
    }
    if (s.minimizeToTray !== undefined) {
      setMinimizeToTray(s.minimizeToTray);
      window.windowControls.setMinimizeToTray(s.minimizeToTray).catch(() => { });
    }

    if (s.userVolumes && typeof s.userVolumes === 'object') {
      const store = useAppStore.getState();
      Object.entries(s.userVolumes).forEach(([userId, volume]) => {
        store.setUserVolume(userId, volume);
        webrtc.setUserVolume(userId, volume);
      });
    }

    settingsRef.current = {
      inputVolume: iv,
      outputVolume: ov,
      selectedInput: normalizedInput,
      selectedOutput: normalizedOutput,
      noiseSuppression: s.noiseSuppression ?? true,
      language: s.language ?? settingsRef.current.language,
      openAtLogin: s.openAtLogin ?? settingsRef.current.openAtLogin,
      minimizeToTray: s.minimizeToTray ?? settingsRef.current.minimizeToTray,
      micThresholdMode: mode,
      manualThresholdValue: val
    };
  }, []);

  const containerRef = useRef<HTMLDivElement>(null);
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 });
  const [streamRatio, setStreamRatio] = useState(16 / 9);

  useEffect(() => {
    if (!store.activeStreamId) {
      setStreamRatio(16 / 9);
      return;
    }
    const relevantUsers = store.currentCallUser
      ? (store.currentUser ? [store.currentCallUser, store.currentUser] : [store.currentCallUser])
      : store.voiceUsers;

    const activeStream = relevantUsers.map(user => {
      const isStreaming = user.isStreaming || (user.id === store.currentUser?.id && !!webrtc.localVideoStream);
      if (isStreaming) {
        const stream = user.id === store.currentUser?.id ? webrtc.localVideoStream : store.remoteVideoStreams[user.id];
        return { user, stream };
      }
      return null;
    }).find(item => item && store.activeStreamId === item.user.id);

    if (activeStream?.stream) {
      const videoTrack = activeStream.stream.getVideoTracks()[0];
      if (videoTrack) {
        const settings = videoTrack.getSettings();
        if (settings.width && settings.height) {
          setStreamRatio(settings.width / settings.height);
          return;
        }
      }
    } else {
      if (store.isStreamFullscreen) store.setStreamFullscreen(false);
      store.setActiveStreamId(null);
    }
    setStreamRatio(16 / 9);
  }, [store.activeStreamId, store.remoteVideoStreams, store.voiceUsers, store.currentCallUser, store.currentUser]);

  useEffect(() => {
    const relevantUsers = store.currentCallUser ? [store.currentCallUser] : store.voiceUsers;
    relevantUsers.forEach(user => {
      webrtc.updateRemoteStreamVolume(user.id);
    });
  }, [store.activeStreamId, store.voiceUsers, store.currentCallUser]);

  useEffect(() => {
    webrtc.scheduleStreamViewInterestReport();
  }, [store.activeStreamId, store.remoteVideoStreams]);

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

  const activeUserCount = useMemo(() => {
    if (store.currentCallUser) {
      let count = 1;
      const remoteStream = (store.currentCallUser.isStreaming || !!store.remoteVideoStreams[store.currentCallUser.id]) ? store.remoteVideoStreams[store.currentCallUser.id] : null;
      if (remoteStream) count++;
      if ((store.currentUser?.isStreaming || !!webrtc.localVideoStream) && webrtc.localVideoStream) count++;
      return count;
    }
    let count = store.voiceUsers.length;
    store.voiceUsers.forEach(u => {
      const isStreaming = u.isStreaming || (u.id === store.currentUser?.id && !!webrtc.localVideoStream);
      if (isStreaming) {
        const stream = u.id === store.currentUser?.id ? webrtc.localVideoStream : store.remoteVideoStreams[u.id];
        if (stream) count++;
      }
    });
    return count;
  }, [store.voiceUsers, store.currentCallUser, store.currentUser?.id, store.currentUser?.isStreaming, store.remoteVideoStreams, showStreamPicker]);

  const cardSize = useMemo(() => {
    const { w, h, avatarSize } = getCardSize(activeUserCount, containerSize.width, containerSize.height);
    return { w, h, avatarSize };
  }, [activeUserCount, containerSize.width, containerSize.height]);

  const inviteChannelId = store.selectedChannelForInvite?.id;

  const invitableFriends = useMemo(() => {
    if (!inviteChannelId) return [];
    const memberIds = new Set((store.channelMembersCache[inviteChannelId] || []).map(m => m.id));
    const query = inviteFriendSearch.trim().toLowerCase();
    return store.friends.filter(f =>
      !memberIds.has(f.id) &&
      (!query || f.displayName.toLowerCase().includes(query))
    );
  }, [inviteChannelId, store.channelMembersCache, store.friends, inviteFriendSearch]);

  const attemptAutoLogin = useCallback(async () => {
    if (autoLoginInFlightRef.current) return;
    const creds = credentialsRef.current;
    if (!creds.login || !creds.password) return;
    autoLoginInFlightRef.current = true;
    autoLoginPendingRef.current = false;
    setServerConnected(false);
    try {
      const result = await signalRService.login(creds.login, creds.password);
      if (result === 'ok') {
        autoLoginAttemptsRef.current = 0;
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
        return;
      }
      if (result === 'invalid') {
        autoLoginAttemptsRef.current = 0;
        setLoadingFadeOut(true);
        setTimeout(() => setAppLoading(false), 650);
        return;
      }
      if (result === 'throttled') {
        autoLoginAttemptsRef.current = 0;
        autoLoginPendingRef.current = false;
        setShowErrorText(true);
        setLoadingFadeOut(true);
        setTimeout(() => setAppLoading(false), 650);
        return;
      }
      autoLoginPendingRef.current = true;
      setShowErrorText(true);
      setShowReconnectingOverlay(true);
      const attempt = ++autoLoginAttemptsRef.current;
      const delay = Math.min(2000 * 2 ** Math.min(attempt - 1, 4), 30000) + Math.floor(Math.random() * 1000);
      if (autoLoginRetryTimerRef.current) clearTimeout(autoLoginRetryTimerRef.current);
      autoLoginRetryTimerRef.current = setTimeout(() => {
        autoLoginRetryTimerRef.current = null;
        attemptAutoLoginRef.current();
      }, delay);
    } finally {
      autoLoginInFlightRef.current = false;
    }
  }, [isAuth, applySettings, saveLocalCache]);

  useEffect(() => { attemptAutoLoginRef.current = () => { void attemptAutoLogin(); }; }, [attemptAutoLogin]);

  useEffect(() => () => {
    if (autoLoginRetryTimerRef.current) clearTimeout(autoLoginRetryTimerRef.current);
  }, []);

  useEffect(() => {
    const unsubConnection = signalRService.onConnectionUpdate((isConnected) => {
      if (!initCompleteRef.current) {
        setServerConnected(isConnected);
        return;
      }

      if (isConnected) {

        if (disconnectTimerRef.current) {
          clearTimeout(disconnectTimerRef.current);
          disconnectTimerRef.current = null;
        }
        setShowErrorText(false);
        setShowInitConnectionError(false);
        setShowReconnectingOverlay(false);

        if (autoLoginPendingRef.current) {
          autoLoginAttemptsRef.current = 0;
          if (autoLoginRetryTimerRef.current) {
            clearTimeout(autoLoginRetryTimerRef.current);
            autoLoginRetryTimerRef.current = null;
          }
          void attemptAutoLogin();
        } else {
          setServerConnected(true);
          setLoadingFadeOut(true);
          setTimeout(() => setAppLoading(false), 650);
        }
      } else if (isAuth) {
        setServerConnected(false);
        autoLoginPendingRef.current = true;
        if (autoLoginRetryTimerRef.current) {
          clearTimeout(autoLoginRetryTimerRef.current);
          autoLoginRetryTimerRef.current = null;
        }
        store.closeAllModals();
        store.setIncomingCall(null);
        store.setIncomingChannelInvite(null);
        setContextMenu(null);
        setShowInvitesPanel(false);
        signalRService.stopRingtone();

        if (!disconnectTimerRef.current) {
          disconnectTimerRef.current = setTimeout(() => {
            setShowReconnectingOverlay(true);
            setShowErrorText(true);
          }, 3000);
        }
      }
    });
    const unsubPing = signalRService.onPingUpdate(setPing);
    return () => {
      unsubConnection();
      unsubPing();
      if (disconnectTimerRef.current) clearTimeout(disconnectTimerRef.current);
    };
  }, [isAuth, attemptAutoLogin, store.closeAllModals, store.setIncomingCall, store.setIncomingChannelInvite]);

  useEffect(() => {
    const init = async () => {
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

      const micPromise = webrtc.prewarmLocalStream().catch(err => {
        console.warn('[Calibration] Mic calibration failed on startup:', err);
        return false;
      });

      if (!cachedCredentials) {
        await micPromise;
        initCompleteRef.current = true;
        setLoadingFadeOut(true);
        setTimeout(() => setAppLoading(false), 650);
        return;
      }

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

      if (!connected) {
        autoLoginPendingRef.current = true;
        await micPromise;
        initCompleteRef.current = true;
        setShowInitConnectionError(true);
        return;
      }

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

        if (serverSettings) {
          const previousInput = settingsRef.current.selectedInput;
          const previousNoiseSuppression = settingsRef.current.noiseSuppression;
          applySettings(serverSettings);
          await micPromise;

          const nextInput = serverSettings.selectedInput === 'communications'
            ? 'default'
            : (serverSettings.selectedInput ?? 'default');
          const nextNoiseSuppression = serverSettings.noiseSuppression ?? true;
          if (nextInput !== previousInput || nextNoiseSuppression !== previousNoiseSuppression) {
            await webrtc.updateSettings(nextInput, nextNoiseSuppression);
          }
        }
        setJoke(jokeText || '__NO_JOKE__');
        setServerConnected(true);
        setIsAuth(true);

        saveLocalCache();
        setTimeout(() => { settingsLoadedRef.current = true; }, 1000);

        initCompleteRef.current = true;
        await micPromise;
        setTimeout(() => {
          setLoadingFadeOut(true);
          setTimeout(() => setAppLoading(false), 650);
        }, 300);
      } else if (loginResult === 'throttled') {
        await micPromise;
        initCompleteRef.current = true;
        setShowErrorText(true);
        setLoadingFadeOut(true);
        setTimeout(() => setAppLoading(false), 650);
      } else if (loginResult === 'invalid') {

        await window.windowControls.clearSession().catch(() => { });
        await micPromise;
        initCompleteRef.current = true;
        setLoadingFadeOut(true);
        setTimeout(() => setAppLoading(false), 650);
      } else {

        autoLoginPendingRef.current = true;
        await micPromise;
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
    if (!contextMenu?.visible) return;

    const SAFE_MARGIN = 32;

    const handleMouseMove = (e: MouseEvent) => {
      if (!contextMenuRef.current) return;
      const rect = contextMenuRef.current.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) return;

      const isInsideOrNear =
        e.clientX >= rect.left - SAFE_MARGIN &&
        e.clientX <= rect.right + SAFE_MARGIN &&
        e.clientY >= rect.top - SAFE_MARGIN &&
        e.clientY <= rect.bottom + SAFE_MARGIN;

      if (!isInsideOrNear) {
        setContextMenu(null);
      }
    };

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setContextMenu(null);
      }
    };

    const handleScrollOrWheel = () => {
      setContextMenu(null);
    };

    const handleWindowBlur = () => {
      setContextMenu(null);
    };

    const handleMouseLeaveDoc = (e: MouseEvent) => {
      if (!e.relatedTarget && !(e as any).toElement) {
        setContextMenu(null);
      }
    };

    const handlePointerDown = (e: MouseEvent) => {
      if (contextMenuRef.current && !contextMenuRef.current.contains(e.target as Node)) {
        setContextMenu(null);
      }
    };

    window.addEventListener('mousemove', handleMouseMove);
    window.addEventListener('keydown', handleKeyDown);
    window.addEventListener('wheel', handleScrollOrWheel, { passive: true });
    window.addEventListener('scroll', handleScrollOrWheel, { capture: true, passive: true });
    window.addEventListener('blur', handleWindowBlur);
    window.addEventListener('mousedown', handlePointerDown);
    document.addEventListener('mouseleave', handleMouseLeaveDoc);

    return () => {
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('keydown', handleKeyDown);
      window.removeEventListener('wheel', handleScrollOrWheel);
      window.removeEventListener('scroll', handleScrollOrWheel, { capture: true });
      window.removeEventListener('blur', handleWindowBlur);
      window.removeEventListener('mousedown', handlePointerDown);
      document.removeEventListener('mouseleave', handleMouseLeaveDoc);
    };
  }, [contextMenu?.visible]);

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
        language: s.language,
        micThresholdMode: s.micThresholdMode,
        manualThresholdValue: s.manualThresholdValue
      });
      saveLocalCache();
    }, 500);
  }, [inputVolume, outputVolume, selectedInput, selectedOutput, noiseSuppression, isAuth, language, micThresholdMode, manualThresholdValue]);

  useEffect(() => {
    if (!settingsLoadedRef.current || !isAuth) return;
    saveLocalCache();
  }, [autoLaunch, minimizeToTray, isAuth, saveLocalCache]);

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
        userVolumes: useAppStore.getState().userVolumes,
        micThresholdMode: s.micThresholdMode,
        manualThresholdValue: s.manualThresholdValue
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
    setEditProfileAvatarColor('#C81E70');
    setEditProfileAboutMe('');
    setIsEditingProfile(false);
    setIsLoginCopied(false);

    setInviteFriendSearch('');
    setSentInvites(new Set());
    setInviteLoadingChannelId(null);
    setIsChannelMembersLoading(false);
    inviteRequestIdRef.current++;
    membersRequestIdRef.current++;

    setContextMenu(null);
    setShowInvitesPanel(false);

    setShowCropper(false);
    setCropGifDataUrl(null);
    setCropImageSrc(null);
    setCropScale(1);
    setCropPos({ x: 0, y: 0 });
    setIsDragging(false);
    setCalibrationSuccess(false);

    setShowDeleteAccountModal(false);
    setDeleteConfirmText('');
    setDeleteAccountError('');
    setIsDeletingAccount(false);

    store.closeAllModals();
  }, [store]);

  const validateInput = useCallback((str: string) => {
    if (str.length < 4) return t('validation.minChars', 'минимум 4 символа');
    if (str.length > 25) return t('validation.maxChars', 'максимум 25 символов');
    if (!/^[a-zA-Z0-9!@#$%^&*()_+={}\[\]:;"'<>,.?/\\|-]+$/.test(str)) return t('validation.latinOnly', 'только латиница и цифры');
    return "";
  }, [t]);

  const validateName = useCallback((str: string) => {
    if (str.trim().length === 0) return t('validation.emptyName', 'имя не может быть пустым');
    if (str.length > 20) return t('validation.maxNameChars', 'максимум 20 символов');
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

  const fetchCaptcha = useCallback(async () => {
    setIsCaptchaLoading(true);
    setCaptchaAnswer('');
    try {
      const challenge = await signalRService.getRegistrationCaptcha();
      setCaptchaData(challenge);
    } catch {
      setCaptchaData(null);
    } finally {
      setIsCaptchaLoading(false);
    }
  }, []);

  useEffect(() => {
    if (authStep === 'setup') {
      fetchCaptcha();
    }
  }, [authStep, fetchCaptcha]);

  const handleAuth = useCallback(async () => {
    setError('');
    const loginErr = validateInput(login);
    const passErr = validateInput(password);
    if (loginErr) { setError(`${t('auth.login', 'логин')}: ${loginErr}`); return; }
    if (passErr && authStep === 'login') { setError(`${t('auth.password', 'пароль')}: ${passErr}`); return; }
    if (authStep === 'setup') {
      const nameErr = validateName(displayName);
      if (nameErr) { setError(nameErr); return; }
      if (!captchaAnswer.trim()) { setError(t('validation.captchaRequired', 'введите код с картинки')); return; }
    }

    setIsLoading(true);
    try {
      const connected = await signalRService.connect();
      if (!connected) { setError(t('validation.connectionError', 'ошибка подключения к серверу')); return; }

      if (authStep === 'login') {
        const exists = await signalRService.checkUserExists(login);
        if (signalRService.lastAuthThrottleMessage) {
          setError(signalRService.lastAuthThrottleMessage);
          return;
        }
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

          } else if (loginResult === 'throttled') {
            setError(signalRService.lastAuthThrottleMessage ?? t('validation.networkError', 'ошибка сети, попробуйте ещё раз'));
          } else if (loginResult === 'invalid') {
            setError(t('validation.invalidPassword', 'неверный пароль!'));
          } else {
            setError(t('validation.networkError', 'ошибка сети, попробуйте ещё раз'));
          }
        } else {
          setAuthStep('confirm');
        }
      } else if (authStep === 'setup') {
        settingsLoadedRef.current = false;

        const success = await signalRService.register(
          login, password, displayName.trim(), avatarBase64, avatarColor, captchaData?.id, captchaAnswer.trim()
        );
        if (success) {
          resetToDefaults();
          const jokeText = await signalRService.getJokeOfTheDay().catch(() => '__NO_JOKE__');

          setJoke(jokeText || '__NO_JOKE__');
          setServerConnected(true);
          setIsAuth(true);
          credentialsRef.current = { login, password };

          saveLocalCache();
          setTimeout(() => { settingsLoadedRef.current = true; }, 1000);

        } else {
          setCaptchaAnswer('');
          fetchCaptcha();
          if (signalRService.lastRegisterError === 'CAPTCHA_INVALID') {
            setError(t('validation.invalidCaptcha', 'неверный код с картинки'));
          } else if (signalRService.lastRegisterError === 'CAPTCHA_EXPIRED') {
            setError(t('validation.expiredCaptcha', 'код устарел, обновите картинку'));
          } else if (signalRService.lastRegisterError === 'CAPTCHA_REQUIRED') {
            setError(t('validation.captchaRequired', 'введите код с картинки'));
          } else {
            setError(signalRService.lastAuthThrottleMessage ?? t('validation.registerError', 'ошибка регистрации'));
          }
        }
      }
    } catch {
      if (authStep === 'setup') {
        setCaptchaAnswer('');
      }
      setError(t('validation.connectError', 'ошибка подключения'));
    } finally {
      setIsLoading(false);
    }
  }, [login, password, authStep, displayName, avatarBase64, avatarColor,
    captchaData, captchaAnswer, fetchCaptcha,
    validateInput, validateName, saveLocalCache, softClearCache,
    resetToDefaults, applySettings, t]);

  const handleManualCalibration = useCallback(async () => {
    if (isCalibrating || !webrtc.isCalibrationAvailable()) return;

    setIsCalibrating(true);
    setCalibrationSuccess(false);
    setCalibrationPhase('preparing');
    setCalibrationCountdown(0);

    let interval: ReturnType<typeof setInterval> | null = null;

    try {
      await webrtc.calibrateMic(4500, () => {
        const startedAt = Date.now();
        setCalibrationPhase('voice');
        setCalibrationCountdown(5);
        interval = setInterval(() => {
          const elapsed = Math.floor((Date.now() - startedAt) / 1000);
          if (elapsed < 5) {
            setCalibrationPhase('voice');
            setCalibrationCountdown(5 - elapsed);
          } else {
            setCalibrationPhase('checking');
            setCalibrationCountdown(0);
            if (interval) clearInterval(interval);
          }
        }, 200);
      });
      setCalibrationSuccess(true);
    } catch (err) {
      const code = err instanceof CalibrationError ? err.code : null;
      console.error('Manual calibration failed:', {
        code: code ?? (err instanceof Error ? err.message : String(err)),
        detail: err instanceof CalibrationError ? err.detail : '',
        ...webrtc.getCalibrationDiagnostics()
      });
      const message = code === 'CALIBRATION_ENGINE_UNAVAILABLE'
        ? t('toasts.calibrationEngineUnavailable', 'шумоподавление не запустилось на этом устройстве. перезапустите приложение.')
        : code === 'CALIBRATION_NO_MIC'
          ? t('toasts.calibrationNoMic', 'микрофон недоступен. проверьте устройство ввода и повторите.')
          : code === 'CALIBRATION_BUSY'
            ? t('toasts.calibrationBusy', 'калибровка уже идёт. дождитесь её окончания.')
            : code === 'CALIBRATION_TIMEOUT'
              ? t('toasts.calibrationTimeout', 'микрофон перестал отдавать звук. проверьте устройство и повторите.')
              : code === 'CALIBRATION_NEEDS_VOICE'
                ? t('toasts.calibrationNeedVoice', 'говорите обычным голосом на протяжении всей калибровки и повторите.')
                : t('toasts.calibrationFailedRetry', 'калибровка не удалась. повторите.');
      store.setSystemToast(message);
      setTimeout(() => {
        const currentStore = useAppStore.getState();
        if (currentStore.systemToast === message) {
          currentStore.setSystemToast(null);
        }
      }, 4000);
    } finally {
      if (interval) clearInterval(interval);
      setIsCalibrating(false);
      setCalibrationPhase('idle');
    }
  }, [isCalibrating, store, t]);

  const handleLogout = useCallback(async () => {
    if (isLoggingOutRef.current) return;
    isLoggingOutRef.current = true;
    try {
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

      store.logout();
      store.setFullChannelState({});
      store.clearChannelMemberData();

      setJoke('');

      setDisplayName('');
      setAvatarBase64(null);
      setAvatarColor('#C81E70');

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
    } finally {
      isLoggingOutRef.current = false;
    }
  }, [
    closeAndResetModals,
    deepWipeOnLogout,
    resetToDefaults,
    store,
    store.currentCallUser,
    store.currentChannelId
  ]);

  useEffect(() => {
    const unsub = signalRService.onForceLogout(() => {
      void handleLogout();
    });
    return unsub;
  }, [handleLogout]);

  const handleAutoLaunchToggle = useCallback(async (enabled: boolean) => {
    const prev = autoLaunch;
    setAutoLaunch(enabled);
    try {
      await window.windowControls.setAutoLaunch(enabled);
    } catch {
      setAutoLaunch(prev);
    }
  }, [autoLaunch]);

  const closeChangePasswordModal = useCallback(() => {
    setNewPassword('');
    setPrivacyError('');
    setShowPrivacyPass(false);
    store.setModal('privacy', false);
  }, [store]);

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
        closeChangePasswordModal();
      }
      else setPrivacyError(t('settings.privacy.changePasswordFailed', 'не удалось сменить пароль'));
    }
  }, [newPassword, password, validateInput, saveLocalCache, closeChangePasswordModal, t]);

  const handleDeleteAccount = useCallback(async () => {
    const expectedWord = t('settings.privacy.deleteAccountConfirmWord', 'подтверждаю').trim().toLowerCase();
    const normalizedInput = deleteConfirmText.trim().toLowerCase();
    if (normalizedInput !== expectedWord && normalizedInput !== 'подтверждаю' && normalizedInput !== 'confirm') {
      return;
    }
    setIsDeletingAccount(true);
    setDeleteAccountError('');
    try {
      const ok = await signalRService.deleteAccount();
      if (ok) {
        setShowDeleteAccountModal(false);
        setDeleteConfirmText('');
        setDeleteAccountError('');
        await handleLogout();
      } else {
        setDeleteAccountError(t('settings.privacy.deleteAccountFailed', 'не удалось удалить аккаунт'));
      }
    } catch {
      setDeleteAccountError(t('settings.privacy.deleteAccountFailed', 'не удалось удалить аккаунт'));
    } finally {
      setIsDeletingAccount(false);
    }
  }, [deleteConfirmText, t, handleLogout]);

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

  const reportChannelJoinStatus = useCallback((status: 'ok' | 'network' | 'mic_failed' | 'full') => {
    if (status === 'ok' || status === 'mic_failed') return;
    if (status === 'full') { store.setModal('channelFull', true); return; }
    if (useAppStore.getState().isJoiningChannel) return;
    const message = t('toasts.channelJoinFailed', 'не удалось войти в канал: сервер не ответил. попробуйте снова.');
    store.setSystemToast(message);
    setTimeout(() => {
      const currentStore = useAppStore.getState();
      if (currentStore.systemToast === message) currentStore.setSystemToast(null);
    }, 4000);
  }, [store, t]);

  const handleChannelClick = useCallback(async (channelId: string) => {
    if (store.currentChannelId === channelId) return;
    if (store.currentChannelId || store.currentCallUser) {
      store.setPendingChannelSwitch(channelId); store.setModal('channelSwitch', true); return;
    }
    const status = await signalRService.joinChannel(channelId);
    reportChannelJoinStatus(status);
  }, [store.currentChannelId, store.currentCallUser, reportChannelJoinStatus]);

  const confirmChannelSwitch = useCallback(async () => {
    if (!store.pendingChannelSwitch || isSwitchingChannel) return;
    const targetId = store.pendingChannelSwitch;
    setIsSwitchingChannel(true);
    store.setPendingChannelSwitch(null);
    store.setModal('channelSwitch', false);
    try {
      let status: 'ok' | 'network' | 'mic_failed' | 'full';
      if (store.currentCallUser) {
        await signalRService.endCall();
        status = await signalRService.joinChannel(targetId);
      } else {
        status = await signalRService.switchChannel(targetId);
      }
      reportChannelJoinStatus(status);
    } finally {
      setIsSwitchingChannel(false);
    }
  }, [store.pendingChannelSwitch, store.currentCallUser, isSwitchingChannel, reportChannelJoinStatus]);

  const cancelChannelSwitch = useCallback(() => {
    store.setPendingChannelSwitch(null);
    store.setModal('channelSwitch', false);
  }, []);

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
    const accepted = await signalRService.acceptChannelInvite(channelId);
    if (!accepted) return;
    if (store.currentChannelId || store.currentCallUser) {
      store.setPendingChannelSwitch(channelId); store.setModal('channelSwitch', true); return;
    }
    const status = await signalRService.joinChannel(channelId);
    reportChannelJoinStatus(status);
  }, [store.currentChannelId, store.currentCallUser, reportChannelJoinStatus]);

  const handleDeclineChannelInvite = useCallback((channelId: string) => {
    signalRService.declineChannelInvite(channelId);
  }, []);

  const handleInviteToChannel = useCallback(async (friendId: string) => {
    const ch = store.selectedChannelForInvite;
    if (!ch) return;
    if (store.currentChannelId !== ch.id) return;
    addSentInvite(friendId);
    const sent = await signalRService.sendChannelInvite(friendId, ch.id, ch.name);
    if (!sent) removeSentInvite(friendId);
  }, [store.selectedChannelForInvite, store.currentChannelId, addSentInvite, removeSentInvite]);

  const openChannelMembers = useCallback(async (ch: VoiceChannel) => {
    const currentStore = useAppStore.getState();
    const requestId = ++membersRequestIdRef.current;
    currentStore.setSelectedChannelForMembers(ch);
    currentStore.setChannelMembers(currentStore.channelMembersCache[ch.id] || []);
    currentStore.setModal('channelMembers', true);
    setIsChannelMembersLoading(true);

    try {
      const members = await signalRService.getChannelMembersList(ch.id);
      const latestStore = useAppStore.getState();
      if (requestId === membersRequestIdRef.current && latestStore.selectedChannelForMembers?.id === ch.id && Array.isArray(members)) {
        latestStore.setChannelMembers(members);
        latestStore.setChannelMembersCache(ch.id, members);
      }
    } catch (e) {
      console.error("Failed to sync channel members", e);
    } finally {
      if (requestId === membersRequestIdRef.current) setIsChannelMembersLoading(false);
    }
  }, []);

  const openInviteToChannel = useCallback(async (ch: VoiceChannel) => {
    if (inviteLoadingChannelId || useAppStore.getState().currentChannelId !== ch.id) return;
    const requestId = ++inviteRequestIdRef.current;
    const currentStore = useAppStore.getState();
    currentStore.setSelectedChannelForInvite(ch);
    setInviteLoadingChannelId(ch.id);
    try {
      const members = await signalRService.getChannelMembersList(ch.id);
      if (requestId !== inviteRequestIdRef.current || !Array.isArray(members)) return;
      const latestStore = useAppStore.getState();
      if (latestStore.currentChannelId !== ch.id) return;
      latestStore.setChannelMembersCache(ch.id, members);
      latestStore.setModal('inviteToChannel', true);
    } catch (error) {
      console.error('Failed to prepare channel invite list', error);
    } finally {
      if (requestId === inviteRequestIdRef.current) setInviteLoadingChannelId(null);
    }
  }, [inviteLoadingChannelId]);

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

    setAdminBlockToast(t('toasts.adminRestricted', 'администратор запретил это действие'));
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

  const handleStopStream = useCallback(async () => {
    try {
      const myId = store.currentUser?.id || '';
      if (store.activeStreamId === myId) {
        store.setActiveStreamId(null);
        if (store.isStreamFullscreen) store.setStreamFullscreen(false);
      }
      webrtc.stopScreenShare();
      await signalRService.stopStream();
      store.updateUserStatus(myId, { isStreaming: false, streamQuality: undefined });
    } catch (e) {
      console.error(e);
    }
  }, [store]);

  const handleStartStream = useCallback(async (sourceId: string, quality: 'low' | 'high' | 'camera', includeAudio: boolean) => {
    try {
      setShowStreamPicker(false);
      const ok = await signalRService.startStream(quality);
      if (ok) {
        await webrtc.startScreenShare(sourceId, quality, includeAudio);
        store.updateUserStatus(store.currentUser?.id || '', { isStreaming: true, streamQuality: quality });
      }
    } catch (e) {
      console.error(e);
      webrtc.stopScreenShare();
      await signalRService.stopStream();
      store.updateUserStatus(store.currentUser?.id || '', { isStreaming: false, streamQuality: undefined });
    }
  }, [store]);

  const openMyAchievements = useCallback(async () => {
    store.setAchievementsData(null);
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

  const handleContextMenu = useCallback((e: React.MouseEvent, type: 'channel' | 'friend' | 'voiceUser' | 'channelMember' | 'stream', item: any) => {
    e.preventDefault();
    if ((type === 'voiceUser' || type === 'channelMember' || type === 'stream') && item.id === store.currentUser?.id) return;
    const menuWidth = 192;
    const menuHeight = 180;
    const x = Math.max(8, Math.min(e.clientX, window.innerWidth - menuWidth - 8));
    const y = Math.max(8, Math.min(e.clientY, window.innerHeight - menuHeight - 8));
    setContextMenu({ visible: true, x, y, type, item });
  }, [store.currentUser?.id]);

  const loadDevices = useCallback(async () => {
    const devices = await webrtc.getAudioDevices();
    setAudioDevices(devices);
    try {
      const result = await webrtc.handleAudioDeviceChange();
      setAudioDevices({ inputs: result.inputs, outputs: result.outputs });
      setSelectedInput(result.inputDeviceId);
      setSelectedOutput(result.outputDeviceId);
    } catch (error) {
      console.warn('[Audio] Failed to validate audio devices:', error);
    }
  }, []);

  useEffect(() => {
    if (!store.modals.settings) return;
    void loadDevices();
  }, [store.modals.settings, loadDevices]);

  useEffect(() => {
    let deviceChangeTimer: NodeJS.Timeout | null = null;
    let disposed = false;

    const handleDeviceChange = () => {
      if (deviceChangeTimer) clearTimeout(deviceChangeTimer);
      deviceChangeTimer = setTimeout(async () => {
        try {
          const result = await webrtc.handleAudioDeviceChange();
          if (disposed) return;
          setAudioDevices({ inputs: result.inputs, outputs: result.outputs });
          setSelectedInput(result.inputDeviceId);
          setSelectedOutput(result.outputDeviceId);
        } catch (error) {
          console.warn('[Audio] Failed to reconcile changed devices:', error);
        }
      }, 300);
    };

    navigator.mediaDevices?.addEventListener('devicechange', handleDeviceChange);
    return () => {
      disposed = true;
      if (deviceChangeTimer) clearTimeout(deviceChangeTimer);
      navigator.mediaDevices?.removeEventListener('devicechange', handleDeviceChange);
    };
  }, []);

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
      if (pixels.length === 0) return '#C81E70';
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
      if (!dominantCluster) return '#C81E70';
      const hsl = rgbToHsl(dominantCluster.r, dominantCluster.g, dominantCluster.b);
      hsl.s = Math.min(1, hsl.s * 1.2); hsl.l = Math.max(0.45, Math.min(0.65, hsl.l));
      const finalRgb = hslToRgb(hsl.h, hsl.s, hsl.l);
      return `#${finalRgb.r.toString(16).padStart(2, '0')}${finalRgb.g.toString(16).padStart(2, '0')}${finalRgb.b.toString(16).padStart(2, '0')}`;
    } catch { return '#C81E70'; }
  }, [rgbToHsl, hslToRgb]);

  const applyCrop = useCallback(() => {
    if (!imgRef.current) return;
    const img = imgRef.current;

    const scaleFactor = 2;
    const targetSize = 200 * scaleFactor;
    const canvas = document.createElement('canvas');
    canvas.width = targetSize;
    canvas.height = targetSize;
    const ctx = canvas.getContext('2d', { willReadFrequently: true });
    if (!ctx) return;
    ctx.imageSmoothingEnabled = true;
    ctx.imageSmoothingQuality = 'high';

    const ratio = Math.min(targetSize / img.naturalWidth, targetSize / img.naturalHeight);
    const baseW = img.naturalWidth * ratio;
    const baseH = img.naturalHeight * ratio;

    ctx.fillStyle = '#000';
    ctx.fillRect(0, 0, targetSize, targetSize);
    ctx.translate(targetSize / 2, targetSize / 2);
    ctx.scale(cropScale, cropScale);
    ctx.translate((cropPos.x * scaleFactor) / cropScale, (cropPos.y * scaleFactor) / cropScale);
    ctx.drawImage(img, -baseW / 2, -baseH / 2, baseW, baseH);

    const hex = getDominantColor(ctx, targetSize, targetSize);

    let base64: string;

    if (cropGifDataUrl) {

      base64 = packGif(cropGifDataUrl, cropScale, cropPos.x, cropPos.y);
    } else {

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
      <div className="fixed inset-0 z-[100000] bg-black/70 backdrop-blur-md flex items-center justify-center p-3 pt-[3.75rem]">
        {content}
      </div>
    );
  }, [store.modals]);

  const renderCropper = () => {
    if (!showCropper || !cropImageSrc) return null;

    return (
      <div className="fixed inset-0 z-[100005] bg-black/90 flex items-center justify-center p-4 pt-[3.75rem]">
        <div className="glass-modal p-6 flex flex-col items-center w-[360px] max-w-full">
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
              className="group text-textMuted hover:text-white transition-colors duration-200 p-1.5 rounded-lg hover:bg-surface/70"
            >
              <X weight="bold" size={24} />
            </button>
          </div>

          <div
            className="w-[200px] h-[200px] rounded-full overflow-hidden relative cursor-move bg-black shrink-0"
            style={{
              WebkitMaskImage: 'radial-gradient(circle closest-side at 50% 50%, #000 calc(100% - 1px), transparent 100%)',
              maskImage: 'radial-gradient(circle closest-side at 50% 50%, #000 calc(100% - 1px), transparent 100%)',
              WebkitBackfaceVisibility: 'hidden',
              WebkitTransform: 'translate3d(0, 0, 0)',
            }}
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
              className="flex-1 py-3 text-textMuted hover:bg-surface/70 rounded-xl font-bold transition-colors"
            >
              {t('common.cancel')}
            </button>
            <button
              onClick={applyCrop}
              className="flex-1 py-3 bg-primary/90 text-white font-bold rounded-xl hover:opacity-90 transition-opacity"
            >
              {t('common.apply')}
            </button>
          </div>
        </div>
      </div>
    );
  };

  const hasInvites = store.channelInvites.length > 0 || store.friendRequests.length > 0;
  const selectedChatFriend = activeTab === 'friends' ? store.friends.find(friend => friend.id === selectedChatFriendId) ?? null : null;
  const chatVisible = Boolean(selectedChatFriend && store.currentUser);
  const callHidden = Boolean(store.currentCallUser && activeTab === 'friends' && callView === 'chat' && chatVisible);

  useEffect(() => {
    const currentCallUserId = store.currentCallUser?.id ?? null;
    if (currentCallUserId && previousCallUserIdRef.current !== currentCallUserId) {
      setCallView('full');
    } else if (!currentCallUserId && previousCallUserIdRef.current) {
      setCallView('full');
    }
    previousCallUserIdRef.current = currentCallUserId;
  }, [store.currentCallUser?.id]);

  const openCallChat = useCallback(() => {
    const callUser = store.currentCallUser;
    if (!callUser) return;
    selectChatFriend(callUser.id);
    setActiveTab('friends');
    setCallView('chat');
  }, [store.currentCallUser, selectChatFriend]);

  useEffect(() => {
    if (store.currentUser?.id && isAuth) void chatPeer.initialize();
  }, [store.currentUser?.id, isAuth]);

  useEffect(() => {
    if (!isAuth) return;
    chatPeer.preconnect(store.friends.filter(friend => friend.isOnline).map(friend => friend.id));
  }, [isAuth, store.friends]);

  return (
    <>
      <div id="mouse-glow" />
      {!isAuth && (
        <div className="fixed inset-0 z-[100000] flex flex-col bg-appBg text-textMain animate-fade-in select-none">
          <TitleBar />
          <div className="flex-1 flex items-center justify-center p-4">
            {authStep === 'login' && (
              <div className="glass-modal p-10 w-[400px] flex flex-col">
                <h1 className="text-4xl font-black text-center mb-8 tracking-wider text-white">zabor</h1>
                <label className="text-xs font-bold text-textMuted mb-2 tracking-wider">{t('auth.login')}</label>
                <input
                  ref={loginInputRef}
                  type="text"
                  value={login}
                  onChange={e => setLogin(e.target.value)}
                  maxLength={25}
                  className="glass-field text-white rounded-xl p-3 mb-4 outline-none focus:ring-2 focus:ring-primary"
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
                    className="w-full glass-field text-white rounded-xl p-3 outline-none focus:ring-2 focus:ring-primary pr-10"
                  />
                  <button
                    onClick={() => setShowPassword(!showPassword)}
                    className="absolute right-3 top-3 text-textMuted hover:text-white transition-colors"
                  >
                    {showPassword ? <EyeOff weight="bold" size={20} /> : <Eye weight="bold" size={20} />}
                  </button>
                </div>
                {error && <p className="text-danger text-sm mb-4 text-center font-medium">{error}</p>}
                <button onClick={handleAuth} disabled={isLoading} className="bg-primary/90 text-white font-bold py-3 rounded-xl disabled:opacity-50 hover:opacity-90 transition-opacity">{isLoading ? t('auth.loading') : t('auth.continue')}</button>
              </div>
            )}
            {authStep === 'confirm' && (
              <div className="glass-modal p-8 w-[400px] text-center">
                <h2 className="text-2xl font-bold mb-4 text-white">{t('auth.accountNotFound')}</h2>
                <p className="text-textMuted mb-8">{t('auth.createNewProfile')}</p>
                <div className="flex gap-4">
                  <button onClick={() => setAuthStep('login')} className="flex-1 bg-surface/70 text-white py-3 rounded-xl font-bold hover:bg-surfaceHover/80 transition-colors">{t('auth.no')}</button>
                  <button onClick={() => { setAuthStep('setup'); setDisplayName(login); setCaptchaAnswer(''); }} className="flex-1 bg-primary/90 text-white py-3 rounded-xl font-bold hover:opacity-90 transition-opacity">{t('auth.yes')}</button>
                </div>
              </div>
            )}
            {authStep === 'setup' && (
              <div className="glass-modal p-8 w-[400px] flex flex-col">
                <h1 className="text-2xl font-bold text-center mb-1 text-white">{t('auth.createProfile')}</h1>
                <p className="text-sm text-textMuted text-center mb-5">{t('auth.howOthersSeeYou')}</p>
                <label className="w-[88px] h-[88px] rounded-full mx-auto mb-5 flex items-center justify-center cursor-pointer relative hover:opacity-80 transition-opacity">
                  {avatarBase64 ? <AvatarImg src={avatarBase64} size={88} bgColor={avatarColor} /> : <div className="w-full h-full rounded-full flex items-center justify-center" style={{ backgroundColor: avatarColor }}><Camera weight="bold" size={28} className="text-white" /></div>}
                  <input type="file" accept="image/*" className="hidden" onChange={e => onFileChange(e, 'setup')} />
                </label>
                <label className="text-xs font-bold text-textMuted mb-1.5 tracking-wider">{t('auth.displayName')}</label>
                <input type="text" value={displayName} onChange={e => setDisplayName(e.target.value)} onKeyDown={e => e.key === 'Enter' && handleAuth()} maxLength={20} placeholder={t('auth.max20chars')} className="glass-field text-white rounded-xl p-3 mb-4 outline-none focus:ring-2 focus:ring-primary" />
                <label className="text-xs font-bold text-textMuted mb-1.5 tracking-wider">{t('auth.captcha')}</label>
                <div className="flex gap-2 items-center mb-6 w-full">
                  <input
                    type="text"
                    value={captchaAnswer}
                    onChange={e => setCaptchaAnswer(e.target.value.toUpperCase())}
                    maxLength={8}
                    placeholder={t('auth.captchaPlaceholder')}
                    onKeyDown={e => e.key === 'Enter' && handleAuth()}
                    className="glass-field flex-1 min-w-0 text-white rounded-xl h-[52px] px-3 outline-none focus:ring-2 focus:ring-primary tracking-widest text-center uppercase font-mono font-bold text-base"
                  />
                  <div
                    onClick={!isCaptchaLoading ? fetchCaptcha : undefined}
                    title={t('auth.refreshCaptcha')}
                    className="w-[154px] h-[52px] px-2.5 rounded-xl bg-surface/60 hover:bg-surfaceHover/80 border border-white/[0.07] border-t-white/[0.14] flex items-center justify-between cursor-pointer select-none active:scale-[0.98] transition-transform shrink-0 group"
                  >
                    <div className="w-[112px] h-9 flex items-center justify-center shrink-0 overflow-hidden">
                      {captchaData?.imageBase64 ? (
                        <img
                          src={captchaData.imageBase64}
                          alt={t('auth.captcha')}
                          className={`h-9 w-[112px] object-contain pointer-events-none select-none transition-opacity duration-150 ${isCaptchaLoading ? 'opacity-30' : 'opacity-100'}`}
                        />
                      ) : isCaptchaLoading ? (
                        <div className="w-4 h-4 border-2 border-primary border-t-transparent rounded-full animate-spin" />
                      ) : (
                        <span className="text-xs text-textMuted px-1">{t('validation.connectError')}</span>
                      )}
                    </div>
                    <ArrowsClockwise weight="bold" size={16} className={`text-textMuted group-hover:text-white transition-colors shrink-0 ${isCaptchaLoading ? 'animate-spin text-primary' : ''}`} />
                  </div>
                </div>
                {error && <p className="text-danger text-sm mb-4 text-center font-medium">{error}</p>}
                <button onClick={handleAuth} disabled={isLoading} className="bg-primary/90 text-white font-bold py-3 rounded-xl disabled:opacity-50 hover:opacity-90 active:scale-[0.98] transition-all">{isLoading ? t('auth.creating') : t('auth.create')}</button>
              </div>
            )}
            {renderCropper()}
          </div>
        </div>
      )}

      {appLoading && (
        <div className={`fixed inset-0 z-[100000] flex flex-col bg-appBg transition-opacity duration-[600ms] select-none ${loadingFadeOut ? 'opacity-0 pointer-events-none' : 'opacity-100'}`}>
          <TitleBar />
          <div className="flex-1 flex items-center justify-center">
            <div className="flex flex-col items-center">
              <h1 className="text-5xl font-black text-white tracking-widest animate-pulse">ZABOR</h1>
              {showInitConnectionError && (
                <div className="flex flex-col items-center mt-2 animate-fade-in">
                  <p className="text-danger font-bold text-center">
                    {t(signalRService.isClientRejected ? 'main.connection.clientRejectedTitle' : 'main.connection.noConnection')}
                  </p>
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
            <div className="flex flex-col items-center">
              <h1 className="text-5xl font-black text-white tracking-widest animate-pulse">ZABOR</h1>
              {showErrorText && (
                <div className="flex flex-col items-center mt-4 animate-fade-in">
                  <p className="text-danger font-bold text-center">
                    {t(signalRService.isClientRejected ? 'main.connection.clientRejectedTitle' : 'main.connection.reconnecting')}
                  </p>
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
        <div className="flex flex-1 overflow-hidden relative">
          {!store.isStreamFullscreen && (
            <div className="w-[344px] shrink-0 relative flex flex-col p-3 gap-3">
              <div className="glass-panel relative flex flex-col flex-1 min-h-0 overflow-hidden">

                {showInvitesPanel && (
                  <div className="absolute inset-0 bg-panelBg/[0.94] backdrop-blur-2xl z-[60] flex flex-col animate-fade-in">
                    <div className="flex items-center justify-between p-4 border-b border-white/[0.07]">
                      <span className="text-sm font-bold text-white tracking-wider">{t('main.notifications.title')}</span>
                      <button onClick={() => setShowInvitesPanel(false)} className="group text-textMuted hover:text-white transition-colors duration-200 p-1.5 rounded-lg hover:bg-white/[0.06]"><X weight="bold" size={20} /></button>
                    </div>
                    <div className="flex-1 overflow-y-auto p-4 space-y-3">
                      {store.friendRequests.map(req => (
                        <div key={req.id} className="bg-surface/70 p-4 rounded-xl">
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
                        <div key={inv.channelId} className="bg-surface/70 p-4 rounded-xl">
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
                          className="text-textMuted hover:text-white transition-all duration-200 hover:scale-110 active:scale-95 w-8 h-8 rounded-lg hover:bg-surface/70 flex items-center justify-center"
                          title={t('modals.createChannel.title', 'создать канал')}
                        >
                          <Plus weight="bold" size={18} />
                        </button>
                      </div>
                      {store.channels.map(ch => {
                        const channelUsers = store.channelUsersMap[ch.id] || [];
                        return (
                          <div key={ch.id} className="mb-2">
                            <div
                              onContextMenu={e => handleContextMenu(e, 'channel', ch)}
                              onClick={() => handleChannelClick(ch.id)}
                              role="button"
                              tabIndex={0}
                              onKeyDown={e => {
                                if (e.key === 'Enter' || e.key === ' ') {
                                  e.preventDefault();
                                  handleChannelClick(ch.id);
                                }
                              }}
                              className={`w-full px-2 py-3 rounded-xl flex items-center justify-between group transition-colors duration-200 cursor-pointer select-none active:scale-[0.99] ${store.currentChannelId === ch.id ? 'bg-[#333]/75' : 'hover:bg-surfaceHover/80'}`}
                            >
                              <div className="flex flex-1 items-center gap-3 overflow-hidden text-left min-w-0 pr-2">
                                <div className={`w-1.5 h-1.5 rounded-full shrink-0 ml-2 transition-all duration-300 ${store.currentChannelId === ch.id ? 'bg-primary/90' : 'bg-textMuted'}`} />
                                <span className="font-medium text-[15px] truncate text-white">{ch.name}</span>
                              </div>
                              <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity pr-2 shrink-0" onClick={e => e.stopPropagation()}>
                                {store.currentChannelId === ch.id && (
                                  <button
                                    type="button"
                                    disabled={inviteLoadingChannelId !== null}
                                    onClick={e => {
                                      e.stopPropagation();
                                      openInviteToChannel(ch);
                                    }}
                                    className="w-6 h-6 text-textMuted hover:text-white p-1 rounded hover:bg-black/20 active:scale-90 transition-transform disabled:cursor-wait flex items-center justify-center"
                                    title={t('common.invite', 'пригласить')}
                                  >
                                    {inviteLoadingChannelId === ch.id
                                      ? <div className="w-3.5 h-3.5 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                                      : <UserPlus weight="bold" size={16} />}
                                  </button>
                                )}
                                <button
                                  type="button"
                                  onClick={e => {
                                    e.stopPropagation();
                                    openChannelMembers(ch);
                                  }}
                                  className="text-textMuted hover:text-white p-1 rounded hover:bg-black/20 active:scale-90 transition-transform"
                                  title={t('common.channelMembers', 'участники канала')}
                                >
                                  <Users weight="bold" size={16} />
                                </button>
                              </div>
                            </div>
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
                                  <div key={`${ch.id}-${u.id}`} className="w-[31px] h-[31px] rounded-full border-2 border-panelBg relative shrink-0 overflow-hidden animate-avatar-in" style={{ zIndex: 100 - i }} title={u.displayName}>
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
                          className="text-textMuted hover:text-white transition-all duration-200 hover:scale-110 active:scale-95 w-8 h-8 rounded-lg hover:bg-surface/70 flex items-center justify-center"
                          title={t('modals.addFriend.title', 'добавить друга')}
                        >
                          <Plus weight="bold" size={18} />
                        </button>
                      </div>
                      {store.friends.map(f => (
                        <div key={f.id} onContextMenu={e => handleContextMenu(e, 'friend', f)}
                          onClick={() => { selectChatFriend(f.id); void chatPeer.open(f.id); }}
                          className={`px-3 py-2 rounded-xl mb-1 cursor-pointer flex items-center gap-3 transition-all duration-200 hover:scale-[1.02] active:scale-[0.98] ${selectedChatFriendId === f.id ? 'bg-surfaceHover/80' : 'hover:bg-surfaceHover/80'}`}>
                          <div className="relative w-[47px] h-[47px] shrink-0">
                            <div className="w-full h-full relative">
                              <AvatarImg src={f.avatarBase64} size={47} bgColor={f.avatarColor} />
                            </div>
                            <div className={`absolute -bottom-0.5 -right-0.5 w-4 h-4 rounded-full border-[3px] border-panelBg ${f.isOnline ? 'bg-success' : 'bg-gray-500'}`} />
                          </div>
                          <span className={`font-semibold text-[15px] truncate flex-1 ${f.isOnline ? 'text-white' : 'text-textMuted'}`}>{f.displayName}</span>
                          {unreadChats[f.id] && <span className="w-2 h-2 rounded-full bg-primary shrink-0" />}
                        </div>
                      ))}
                    </div>
                  )}
                </div>

                <div className="bg-white/[0.045] rounded-full mx-4 my-2 p-1 flex relative shrink-0">
                  <button onClick={() => { selectChatFriend(null); setActiveTab('channels'); }} className={`flex-1 py-2.5 rounded-full font-bold text-sm z-10 transition-all duration-200 hover:scale-[1.02] active:scale-[0.98] ${activeTab === 'channels' ? 'text-white' : 'text-textMuted hover:text-white'}`}>{t('main.tabs.channels')}</button>
                  <button onClick={() => setActiveTab('friends')} className={`flex-1 py-2.5 rounded-full font-bold text-sm z-10 transition-all duration-200 hover:scale-[1.02] active:scale-[0.98] flex items-center justify-center gap-2 ${activeTab === 'friends' ? 'text-white' : 'text-textMuted hover:text-white'}`}>
                    <span>{t('main.tabs.friends')}</span>
                    {hasUnreadChats && <span className="w-2 h-2 rounded-full bg-primary shrink-0" />}
                  </button>
                  <div
                    style={{
                      transform: activeTab === 'channels' ? 'translateX(0)' : 'translateX(calc(100% + 4px))',
                      willChange: 'transform'
                    }}
                    className="absolute top-1 bottom-1 left-1 w-[calc(50%-6px)] bg-white/[0.09] rounded-full transition-transform duration-300 ease-out"
                  />
                </div>
              </div>

              <div className={`absolute bottom-[154px] right-9 transition-all duration-500 ${hasInvites && !showInvitesPanel ? 'translate-y-0 opacity-100 pointer-events-auto' : 'translate-y-[150%] opacity-0 pointer-events-none'}`}>
                <button onClick={() => setShowInvitesPanel(true)} className="w-14 h-14 bg-primary/90 backdrop-blur-md border border-white/10 rounded-full flex items-center justify-center hover:scale-105 transition-transform relative">
                  <Mail weight="bold" size={24} color="white" />
                  <div className="absolute top-0 right-0 w-4 h-4 bg-danger rounded-full border-2 border-panelBg animate-pulse" />
                </button>
              </div>

              <div className="glass-slab h-[75px] flex items-center px-4 shrink-0">
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
                    className="group text-textMuted hover:text-white p-2 hover:bg-surface/70 rounded-xl transition-colors"
                  >
                    <div className="transition-transform duration-500 group-hover:rotate-90 group-hover:scale-110 group-active:scale-95">
                      <Settings weight="bold" size={20} />
                    </div>
                  </button>
                </div>
              </div>
            </div>
          )}

          <div className="flex-1 flex flex-col relative">
            {chatVisible && store.currentUser && selectedChatFriend && (!store.currentCallUser || callHidden) && (
              <ChatPanel currentUser={store.currentUser} friend={selectedChatFriend} onCall={async () => {
                setCallView('full');
                const started = await signalRService.startCall(selectedChatFriend.id);
                if (!started) setCallView('chat');
              }} />
            )}
            {callHidden && store.currentCallUser && (
              <ActiveSessionPip
                callUser={store.currentCallUser}
                onOpen={() => setCallView('full')}
              />
            )}

            {store.currentCallUser && (
              <div className={`absolute top-0 left-0 right-0 bottom-[120px] p-3 flex items-center justify-center overflow-hidden ${callHidden ? 'invisible pointer-events-none' : ''}`}>
                {(() => {
                  const callUser = store.currentCallUser!;
                  const currentUser = store.currentUser;
                  const remoteStream = (callUser.isStreaming || !!store.remoteVideoStreams[callUser.id]) ? store.remoteVideoStreams[callUser.id] : null;
                  const localStream = (currentUser?.isStreaming || !!webrtc.localVideoStream) ? webrtc.localVideoStream : null;

                  const hasStreams = Boolean(remoteStream || localStream);

                  if (!hasStreams) {
                    return (
                      <div ref={containerRef} className="w-full h-full p-6 flex items-center justify-center overflow-hidden">
                        <CallUserCard
                          currentCallUser={callUser}
                          callStatus={store.callStatus}
                          cardSize={cardSize}
                          webrtcConnections={store.webrtcConnections}
                          handleContextMenu={handleContextMenu}
                          t={t}
                          isIdle={isIdle}
                        />
                      </div>
                    );
                  }

                  const callUsersList = currentUser ? [callUser, currentUser] : [callUser];
                  const items: any[] = [
                    { type: 'user', id: `calluser-${callUser.id}`, user: callUser }
                  ];

                  if (remoteStream) {
                    items.push({ type: 'stream', id: `stream-${callUser.id}`, user: callUser, stream: remoteStream });
                  }
                  if (localStream && currentUser) {
                    items.push({ type: 'stream', id: `stream-${currentUser.id}`, user: currentUser, stream: localStream });
                  }

                  const activeStream = items.find(item => item.type === 'stream' && store.activeStreamId === item.user.id);

                  if (activeStream && store.isStreamFullscreen) {
                    const normalMaxW = windowSize.width - 40;
                    const normalMaxH = windowSize.height - 140;
                    let normalW = normalMaxW;
                    let normalH = normalW / streamRatio;
                    if (normalH > normalMaxH) {
                      normalH = normalMaxH;
                      normalW = normalH * streamRatio;
                    }

                    const normalTop = 40 + (normalMaxH - normalH) / 2;
                    const normalLeft = (windowSize.width - normalW) / 2;

                    const expMarginX = 8;
                    const expMarginTop = 38;
                    const expMarginBottom = 8;
                    const expMaxW = windowSize.width - expMarginX * 2;
                    const expMaxH = windowSize.height - expMarginTop - expMarginBottom;
                    let expW = expMaxW;
                    let expH = expW / streamRatio;
                    if (expH > expMaxH) {
                      expH = expMaxH;
                      expW = expH * streamRatio;
                    }

                    const expTop = expMarginTop + (expMaxH - expH) / 2;
                    const expLeft = (windowSize.width - expW) / 2;

                    const currentW = showOverlays ? normalW : expW;
                    const currentH = showOverlays ? normalH : expH;
                    const currentTop = showOverlays ? normalTop : expTop;
                    const currentLeft = showOverlays ? normalLeft : expLeft;

                    return (
                      <div className={`fixed inset-0 bg-black z-[9999] ${showOverlays ? '' : 'cursor-none'}`}>
                        <div
                          style={{
                            position: 'absolute',
                            width: `${currentW}px`,
                            height: `${currentH}px`,
                            top: `${currentTop}px`,
                            left: `${currentLeft}px`,
                            transition: 'all 300ms cubic-bezier(0.4, 0, 0.2, 1)'
                          }}
                          className="rounded-xl overflow-hidden border border-[#303035]/70"
                        >
                          <StreamCard
                            user={activeStream.user}
                            stream={activeStream.stream}
                            cardSize={{ w: currentW, h: currentH }}
                            isFocused={true}
                            isFullscreen={true}
                            showOverlays={showOverlays}
                            onClick={() => { }}
                            onContextMenu={e => handleContextMenu(e, 'stream', activeStream.user)}
                            onRatioChange={setStreamRatio}
                          />
                        </div>

                        <div className={`absolute left-6 top-1/2 -translate-y-1/2 flex flex-col gap-3 justify-center items-center transition-all duration-300 z-50 ${showOverlays ? 'translate-x-0 opacity-100' : '-translate-x-20 opacity-0 pointer-events-none'}`}>
                          {callUsersList.map(user => {
                            const isSpeaking = speakingMap[user.id] ?? false;
                            return (
                              <div key={user.id} className="relative group flex items-center">
                                <div
                                  onContextMenu={e => handleContextMenu(e, 'voiceUser', user)}
                                  className={`w-12 h-12 rounded-full border-2 transition-all duration-200 overflow-hidden bg-[#121217] relative cursor-pointer ${isSpeaking ? 'border-[#3BA55C] scale-110' : 'border-[#303035]/70'
                                    }`}
                                >
                                  <AvatarImg src={user.avatarBase64} size={48} bgColor={user.avatarColor} animate={false} />
                                </div>
                                <div className="absolute left-14 bg-[#09090B]/90 backdrop-blur-md border border-[#303035]/80 px-3.5 py-1.5 rounded-full pointer-events-none opacity-0 -translate-x-3 group-hover:opacity-100 group-hover:translate-x-0 transition-all duration-200 ease-out z-50 whitespace-nowrap">
                                  <span className="text-white font-bold text-[13px]">{user.displayName}</span>
                                </div>
                              </div>
                            );
                          })}
                        </div>

                        <div className={`absolute bottom-6 left-1/2 -translate-x-1/2 bg-panelBg/80 backdrop-blur-2xl px-6 py-4 rounded-full flex gap-4 items-center border border-white/[0.07] border-t-white/[0.14] transition-all duration-300 z-50 ${showOverlays ? 'translate-y-0 opacity-100' : 'translate-y-28 opacity-0 pointer-events-none'}`}>
                          <button
                            onClick={() => store.setStreamFullscreen(false)}
                            className="group/mode-button relative w-14 h-14 rounded-full flex items-center justify-center bg-surface/70 hover:bg-surfaceHover/80 text-white transition-colors"
                            aria-label={t('stream.exitFullscreenHint')}
                          >
                            <span
                              role="tooltip"
                              className="absolute bottom-full left-1/2 mb-3 -translate-x-1/2 pointer-events-none whitespace-nowrap opacity-0 group-hover/mode-button:opacity-100 delay-0 group-hover/mode-button:delay-[2000ms] transition-opacity duration-150 bg-[#09090B]/95 border border-[#303035]/70 rounded-md px-2.5 py-1.5 text-[10px] font-bold text-white"
                            >
                              {t('stream.exitFullscreenHint')}
                            </span>
                            <div className="flex items-center justify-center transition-transform duration-200 group-hover/mode-button:scale-110">
                              <CornersIn weight="bold" size={24} />
                            </div>
                          </button>
                          <button
                            onClick={toggleMute}
                            className={`group w-14 h-14 rounded-full flex items-center justify-center relative transition-colors ${(store.currentUser?.isMuted || store.currentUser?.isServerMuted || store.currentUser?.isServerDeafened)
                              ? 'bg-[#2B2D31] text-white'
                              : 'bg-surface/70 hover:bg-surfaceHover/80 text-white'
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
                              : 'bg-surface/70 hover:bg-surfaceHover/80 text-white'
                              }`}
                          >
                            <div className="flex items-center justify-center transition-transform duration-200 group-active:scale-95 group-hover:scale-110">
                              <Headphones weight="bold" size={24} />
                              <div className={`absolute w-[30px] h-[3px] bg-danger rounded-full transition-all duration-300 origin-center ${(store.currentUser?.isDeafened || store.currentUser?.isServerDeafened) ? 'scale-100 opacity-100 rotate-45' : 'scale-0 opacity-0 rotate-45'}`} />
                            </div>
                          </button>
                          {(store.currentUser?.isStreaming || !!webrtc.localVideoStream) && (
                            <button
                              onClick={handleStopStream}
                              className="group w-14 h-14 rounded-full flex items-center justify-center bg-primaryHover text-white hover:bg-primaryActive transition-colors"
                              title={t('stream.stopHint')}
                            >
                              <div className="flex items-center justify-center transition-transform duration-200 group-hover:scale-110">
                                <Desktop weight="bold" size={24} />
                              </div>
                            </button>
                          )}
                          <button onClick={handleEndCall} className="group bg-danger hover:bg-red-600 text-white font-bold py-3.5 px-8 rounded-full flex items-center gap-3 transition-colors text-[15px]">
                            <div className="transition-transform duration-300 group-hover:-rotate-12 group-hover:scale-110">
                              <PhoneOff weight="bold" size={20} />
                            </div>
                            {t('main.voice.endCall')}
                          </button>
                        </div>
                      </div>
                    );
                  }

                  if (activeStream) {
                    const sideItems = items.filter(item => item.id !== activeStream.id);
                    const maxH = containerSize.height - 150;
                    const maxW = containerSize.width;
                    let streamW = maxW;
                    let streamH = streamW / streamRatio;
                    if (streamH > maxH) {
                      streamH = maxH;
                      streamW = streamH * streamRatio;
                    }

                    return (
                      <div ref={containerRef} className="w-full h-full flex flex-col items-center justify-between">
                        <div
                          style={{ width: `${streamW}px`, height: `${streamH}px` }}
                          className="relative overflow-hidden flex items-center justify-center"
                        >
                          <StreamCard
                            user={activeStream.user}
                            stream={activeStream.stream}
                            cardSize={{ w: streamW, h: streamH }}
                            isFocused={true}
                            onClick={() => store.setActiveStreamId(null)}
                            onContextMenu={e => handleContextMenu(e, 'stream', activeStream.user)}
                            onToggleFullscreen={() => store.setStreamFullscreen(true)}
                            onRatioChange={setStreamRatio}
                          />
                        </div>
                        <div className="w-full h-[120px] shrink-0 flex items-center justify-center gap-4 overflow-x-auto mt-4 px-4 pr-1">
                          {sideItems.map(item => {
                            if (item.type === 'user') {
                              return (
                                <CallUserCard
                                  key={`call-${item.id}`}
                                  currentCallUser={item.user}
                                  callStatus={store.callStatus}
                                  cardSize={{ w: 180, h: 101, avatarSize: 40 }}
                                  webrtcConnections={store.webrtcConnections}
                                  handleContextMenu={handleContextMenu}
                                  t={t}
                                  isIdle={isIdle}
                                />
                              );
                            } else {
                              return (
                                <StreamCard
                                  key={item.id}
                                  user={item.user}
                                  stream={item.stream}
                                  cardSize={{ w: 180, h: 101 }}
                                  isFocused={false}
                                  onClick={() => store.setActiveStreamId(item.user.id)}
                                  onContextMenu={e => handleContextMenu(e, 'stream', item.user)}
                                />
                              );
                            }
                          })}
                        </div>
                      </div>
                    );
                  }

                  return (
                    <div ref={containerRef} className="w-full h-full flex flex-wrap items-center justify-center gap-6" style={{ alignContent: 'center' }}>
                      {items.map(item => {
                        if (item.type === 'user') {
                          return (
                            <CallUserCard
                              key={`call-${item.id}`}
                              currentCallUser={item.user}
                              callStatus={store.callStatus}
                              cardSize={cardSize}
                              webrtcConnections={store.webrtcConnections}
                              handleContextMenu={handleContextMenu}
                              t={t}
                              isIdle={isIdle}
                            />
                          );
                        } else {
                          return (
                            <StreamCard
                              key={item.id}
                              user={item.user}
                              stream={item.stream}
                              cardSize={cardSize}
                              isFocused={false}
                              onClick={() => store.setActiveStreamId(item.user.id)}
                              onContextMenu={e => handleContextMenu(e, 'stream', item.user)}
                            />
                          );
                        }
                      })}
                    </div>
                  );
                })()}
              </div>
            )}

            {!store.currentCallUser && !store.currentChannelId && !chatVisible && (
              <div className="flex-1 flex flex-col items-center justify-center px-16">
                <div className="max-w-lg text-center">
                  {joke ? (
                    <>
                      <p className="text-xs text-white/20 mb-3 font-semibold tracking-wider">{t('joke.title', 'шутейка:')}</p>
                      <p className="text-lg text-white/50 font-medium leading-relaxed whitespace-pre-line">
                        {joke === '__NO_JOKE__' ? t('joke.fallback', 'сегодня сервер шутит молча.') : translateJoke(joke, i18n.language)}
                      </p>
                    </>
                  ) : (
                    <div className="w-6 h-6 border-2 border-white/10 border-t-white/30 rounded-full animate-spin mx-auto" />
                  )}
                </div>
              </div>
            )}

            {!store.currentCallUser && store.currentChannelId && activeTab === 'channels' && (
              <div className="absolute top-0 left-0 right-0 bottom-[120px] p-3 flex items-center justify-center overflow-hidden">
                {(() => {
                  const sorted = [...store.voiceUsers].sort((a, b) => {
                    const currentUserId = store.currentUser?.id;
                    if (a.id === currentUserId) return -1;
                    if (b.id === currentUserId) return 1;
                    const nameA = a.displayName.toLowerCase();
                    const nameB = b.displayName.toLowerCase();
                    if (nameA < nameB) return -1;
                    if (nameA > nameB) return 1;
                    return a.id.localeCompare(b.id);
                  });
                  const items: any[] = [];
                  sorted.forEach(user => {
                    items.push({ type: 'user', id: `user-${user.id}`, user });
                    const isStreaming = user.isStreaming || (user.id === store.currentUser?.id && !!webrtc.localVideoStream);
                    if (isStreaming) {
                      const stream = user.id === store.currentUser?.id ? webrtc.localVideoStream : store.remoteVideoStreams[user.id];
                      if (stream) {
                        items.push({ type: 'stream', id: `stream-${user.id}`, user, stream });
                      }
                    }
                  });

                  const activeStream = items.find(item => item.type === 'stream' && store.activeStreamId === item.user.id);

                  if (activeStream && store.isStreamFullscreen) {
                    const normalMaxW = windowSize.width - 40;
                    const normalMaxH = windowSize.height - 140;
                    let normalW = normalMaxW;
                    let normalH = normalW / streamRatio;
                    if (normalH > normalMaxH) {
                      normalH = normalMaxH;
                      normalW = normalH * streamRatio;
                    }

                    const normalTop = 40 + (normalMaxH - normalH) / 2;
                    const normalLeft = (windowSize.width - normalW) / 2;

                    const expMarginX = 8;
                    const expMarginTop = 38;
                    const expMarginBottom = 8;
                    const expMaxW = windowSize.width - expMarginX * 2;
                    const expMaxH = windowSize.height - expMarginTop - expMarginBottom;
                    let expW = expMaxW;
                    let expH = expW / streamRatio;
                    if (expH > expMaxH) {
                      expH = expMaxH;
                      expW = expH * streamRatio;
                    }

                    const expTop = expMarginTop + (expMaxH - expH) / 2;
                    const expLeft = (windowSize.width - expW) / 2;

                    const currentW = showOverlays ? normalW : expW;
                    const currentH = showOverlays ? normalH : expH;
                    const currentTop = showOverlays ? normalTop : expTop;
                    const currentLeft = showOverlays ? normalLeft : expLeft;

                    return (
                      <div className={`fixed inset-0 bg-black z-[9999] ${showOverlays ? '' : 'cursor-none'}`}>
                        <div
                          style={{
                            position: 'absolute',
                            width: `${currentW}px`,
                            height: `${currentH}px`,
                            top: `${currentTop}px`,
                            left: `${currentLeft}px`,
                            transition: 'all 300ms cubic-bezier(0.4, 0, 0.2, 1)'
                          }}
                          className="rounded-xl overflow-hidden border border-[#303035]/70"
                        >
                          <StreamCard
                            user={activeStream.user}
                            stream={activeStream.stream}
                            cardSize={{ w: currentW, h: currentH }}
                            isFocused={true}
                            isFullscreen={true}
                            showOverlays={showOverlays}
                            onClick={() => { }}
                            onContextMenu={e => handleContextMenu(e, 'stream', activeStream.user)}
                            onRatioChange={setStreamRatio}
                          />
                        </div>

                        <div className={`absolute left-6 top-1/2 -translate-y-1/2 flex flex-col gap-3 justify-center items-center transition-all duration-300 z-50 ${showOverlays ? 'translate-x-0 opacity-100' : '-translate-x-20 opacity-0 pointer-events-none'}`}>
                          {sorted.map(user => {
                            const isSpeaking = speakingMap[user.id] ?? false;
                            return (
                              <div key={user.id} className="relative group flex items-center">
                                <div
                                  onContextMenu={e => handleContextMenu(e, 'voiceUser', user)}
                                  className={`w-12 h-12 rounded-full border-2 transition-all duration-200 overflow-hidden bg-[#121217] relative cursor-pointer ${isSpeaking ? 'border-[#3BA55C] scale-110' : 'border-[#303035]/70'
                                    }`}
                                >
                                  <AvatarImg src={user.avatarBase64} size={48} bgColor={user.avatarColor} animate={false} />
                                </div>
                                <div className="absolute left-14 bg-[#09090B]/90 backdrop-blur-md border border-[#303035]/80 px-3.5 py-1.5 rounded-full pointer-events-none opacity-0 -translate-x-3 group-hover:opacity-100 group-hover:translate-x-0 transition-all duration-200 ease-out z-50 whitespace-nowrap">
                                  <span className="text-white font-bold text-[13px]">{user.displayName}</span>
                                </div>
                              </div>
                            );
                          })}
                        </div>

                        <div className={`absolute bottom-6 left-1/2 -translate-x-1/2 bg-panelBg/80 backdrop-blur-2xl px-6 py-4 rounded-full flex gap-4 items-center border border-white/[0.07] border-t-white/[0.14] transition-all duration-300 z-50 ${showOverlays ? 'translate-y-0 opacity-100' : 'translate-y-28 opacity-0 pointer-events-none'}`}>
                          <button
                            onClick={() => store.setStreamFullscreen(false)}
                            className="group/mode-button relative w-14 h-14 rounded-full flex items-center justify-center bg-surface/70 hover:bg-surfaceHover/80 text-white transition-colors"
                            aria-label={t('stream.exitFullscreenHint')}
                          >
                            <span
                              role="tooltip"
                              className="absolute bottom-full left-1/2 mb-3 -translate-x-1/2 pointer-events-none whitespace-nowrap opacity-0 group-hover/mode-button:opacity-100 delay-0 group-hover/mode-button:delay-[2000ms] transition-opacity duration-150 bg-[#09090B]/95 border border-[#303035]/70 rounded-md px-2.5 py-1.5 text-[10px] font-bold text-white"
                            >
                              {t('stream.exitFullscreenHint')}
                            </span>
                            <div className="flex items-center justify-center transition-transform duration-200 group-hover/mode-button:scale-110">
                              <CornersIn weight="bold" size={24} />
                            </div>
                          </button>
                          <button
                            onClick={toggleMute}
                            className={`group w-14 h-14 rounded-full flex items-center justify-center relative transition-colors ${(store.currentUser?.isMuted || store.currentUser?.isServerMuted || store.currentUser?.isServerDeafened)
                              ? 'bg-[#2B2D31] text-white'
                              : 'bg-surface/70 hover:bg-surfaceHover/80 text-white'
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
                              : 'bg-surface/70 hover:bg-surfaceHover/80 text-white'
                              }`}
                          >
                            <div className="flex items-center justify-center transition-transform duration-200 group-active:scale-95 group-hover:scale-110">
                              <Headphones weight="bold" size={24} />
                              <div className={`absolute w-[30px] h-[3px] bg-danger rounded-full transition-all duration-300 origin-center ${(store.currentUser?.isDeafened || store.currentUser?.isServerDeafened) ? 'scale-100 opacity-100 rotate-45' : 'scale-0 opacity-0 rotate-45'}`} />
                            </div>
                          </button>
                          {(store.currentUser?.isStreaming || !!webrtc.localVideoStream) && (
                            <button
                              onClick={handleStopStream}
                              className="group w-14 h-14 rounded-full flex items-center justify-center bg-primaryHover text-white hover:bg-primaryActive transition-colors"
                              title={t('stream.stopHint')}
                            >
                              <div className="flex items-center justify-center transition-transform duration-200 group-hover:scale-110">
                                <Desktop weight="bold" size={24} />
                              </div>
                            </button>
                          )}
                          <button onClick={store.currentCallUser ? handleEndCall : () => signalRService.leaveChannel()} className="group bg-danger hover:bg-red-600 text-white font-bold py-3.5 px-8 rounded-full flex items-center gap-3 transition-colors text-[15px]">
                            <div className="transition-transform duration-300 group-hover:-rotate-12 group-hover:scale-110">
                              <PhoneOff weight="bold" size={20} />
                            </div>
                            {t('main.voice.endCall')}
                          </button>
                        </div>
                      </div>
                    );
                  }

                  if (activeStream) {
                    const sideItems = items.filter(item => item.id !== activeStream.id);
                    const maxH = containerSize.height - 150;
                    const maxW = containerSize.width;
                    let streamW = maxW;
                    let streamH = streamW / streamRatio;
                    if (streamH > maxH) {
                      streamH = maxH;
                      streamW = streamH * streamRatio;
                    }

                    return (
                      <div ref={containerRef} className="w-full h-full flex flex-col items-center justify-between">
                        <div
                          style={{ width: `${streamW}px`, height: `${streamH}px` }}
                          className="relative overflow-hidden flex items-center justify-center"
                        >
                          <StreamCard
                            user={activeStream.user}
                            stream={activeStream.stream}
                            cardSize={{ w: streamW, h: streamH }}
                            isFocused={true}
                            onClick={() => store.setActiveStreamId(null)}
                            onContextMenu={e => handleContextMenu(e, 'stream', activeStream.user)}
                            onToggleFullscreen={() => store.setStreamFullscreen(true)}
                            onRatioChange={setStreamRatio}
                          />
                        </div>
                        <div className="w-full h-[120px] shrink-0 flex items-center justify-center gap-4 overflow-x-auto mt-4 px-4 pr-1">
                          {sideItems.map(item => {
                            if (item.type === 'user') {
                              return (
                                <VoiceUserCard
                                  key={`${store.currentChannelId}-${item.id}`}
                                  user={item.user}
                                  cardSize={{ w: 180, h: 101, avatarSize: 40 }}
                                  isIdle={isIdle}
                                  t={t}
                                  handleContextMenu={handleContextMenu}
                                  webrtcConnections={store.webrtcConnections}
                                  currentUserId={store.currentUser?.id}
                                />
                              );
                            } else {
                              return (
                                <StreamCard
                                  key={item.id}
                                  user={item.user}
                                  stream={item.stream}
                                  cardSize={{ w: 180, h: 101 }}
                                  isFocused={false}
                                  onClick={() => store.setActiveStreamId(item.user.id)}
                                  onContextMenu={e => handleContextMenu(e, 'stream', item.user)}
                                />
                              );
                            }
                          })}
                        </div>
                      </div>
                    );
                  }

                  return (
                    <div ref={containerRef} className="w-full h-full flex flex-wrap items-center justify-center gap-6" style={{ alignContent: 'center' }}>
                      {items.map(item => {
                        if (item.type === 'user') {
                          return (
                            <VoiceUserCard
                              key={`${store.currentChannelId}-${item.id}`}
                              user={item.user}
                              cardSize={cardSize}
                              isIdle={isIdle}
                              t={t}
                              handleContextMenu={handleContextMenu}
                              webrtcConnections={store.webrtcConnections}
                              currentUserId={store.currentUser?.id}
                            />
                          );
                        } else {
                          return (
                            <StreamCard
                              key={item.id}
                              user={item.user}
                              stream={item.stream}
                              cardSize={cardSize}
                              isFocused={false}
                              onClick={() => store.setActiveStreamId(item.user.id)}
                              onContextMenu={e => handleContextMenu(e, 'stream', item.user)}
                            />
                          );
                        }
                      })}
                    </div>
                  );
                })()}
              </div>
            )}

            {store.currentCallUser && !store.isStreamFullscreen && !callHidden && (
              <div className={[
                "absolute bottom-10 left-1/2 -translate-x-1/2 bg-panelBg/70 backdrop-blur-xl px-6 py-4 rounded-full flex gap-4 items-center border border-white/[0.07] border-t-white/[0.14] z-50",
                controlsShake ? "animate-shake" : ""
              ].join(" ")}>
                <button onClick={openCallChat} className="group w-14 h-14 rounded-full flex items-center justify-center bg-surface/70 hover:bg-surfaceHover/80 text-white transition-colors active:scale-95" title={t('chat.open')}><ChatCircle weight="bold" size={24} /></button>
                <button
                  onClick={toggleMute}
                  className={`group w-14 h-14 rounded-full flex items-center justify-center relative transition-colors ${(store.currentUser?.isMuted || store.currentUser?.isServerMuted || store.currentUser?.isServerDeafened)
                    ? 'bg-[#2B2D31] text-white'
                    : 'bg-surface/70 hover:bg-surfaceHover/80 text-white'
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
                    : 'bg-surface/70 hover:bg-surfaceHover/80 text-white'
                    }`}
                >
                  <div className="flex items-center justify-center transition-transform duration-200 group-active:scale-95 group-hover:scale-110">
                    <Headphones weight="bold" size={24} />
                    <div className={`absolute w-[30px] h-[3px] bg-danger rounded-full transition-all duration-300 origin-center ${(store.currentUser?.isDeafened || store.currentUser?.isServerDeafened) ? 'scale-100 opacity-100 rotate-45' : 'scale-0 opacity-0 rotate-45'}`} />
                  </div>
                </button>
                <button
                  onClick={store.currentUser?.isStreaming || !!webrtc.localVideoStream ? handleStopStream : () => setShowStreamPicker(true)}
                  className={`group w-14 h-14 rounded-full flex items-center justify-center relative transition-colors ${store.currentUser?.isStreaming || !!webrtc.localVideoStream
                    ? 'bg-primaryHover text-white hover:bg-primaryActive'
                    : 'bg-surface/70 hover:bg-surfaceHover/80 text-white'
                    }`}
                  title={store.currentUser?.isStreaming || !!webrtc.localVideoStream ? t('stream.stopHint') : t('stream.startHint')}
                >
                  <div className="flex items-center justify-center transition-transform duration-200 group-active:scale-95 group-hover:scale-110">
                    <Desktop weight="bold" size={24} />
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

            {store.currentChannelId && activeTab === 'channels' && !store.currentCallUser && !store.isStreamFullscreen && (
              <div className={[
                "absolute bottom-10 left-1/2 -translate-x-1/2 bg-panelBg/70 backdrop-blur-xl px-6 py-4 rounded-full flex gap-4 items-center border border-white/[0.07] border-t-white/[0.14] z-50",
                controlsShake ? "animate-shake" : ""
              ].join(" ")}>
                <button
                  onClick={toggleMute}
                  className={`group w-14 h-14 rounded-full flex items-center justify-center relative transition-colors ${(store.currentUser?.isMuted || store.currentUser?.isServerMuted || store.currentUser?.isServerDeafened)
                    ? 'bg-[#2B2D31] text-white'
                    : 'bg-surface/70 hover:bg-surfaceHover/80 text-white'
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
                    : 'bg-surface/70 hover:bg-surfaceHover/80 text-white'
                    }`}
                >
                  <div className="flex items-center justify-center transition-transform duration-200 group-active:scale-95 group-hover:scale-110">
                    <Headphones weight="bold" size={24} />
                    <div className={`absolute w-[30px] h-[3px] bg-danger rounded-full transition-all duration-300 origin-center ${(store.currentUser?.isDeafened || store.currentUser?.isServerDeafened) ? 'scale-100 opacity-100 rotate-45' : 'scale-0 opacity-0 rotate-45'}`} />
                  </div>
                </button>
                <button
                  onClick={store.currentUser?.isStreaming || !!webrtc.localVideoStream ? handleStopStream : () => setShowStreamPicker(true)}
                  className={`group w-14 h-14 rounded-full flex items-center justify-center relative transition-colors ${store.currentUser?.isStreaming || !!webrtc.localVideoStream
                    ? 'bg-primaryHover text-white hover:bg-primaryActive'
                    : 'bg-surface/70 hover:bg-surfaceHover/80 text-white'
                    }`}
                  title={store.currentUser?.isStreaming || !!webrtc.localVideoStream ? t('stream.stopHint') : t('stream.startHint')}
                >
                  <div className="flex items-center justify-center transition-transform duration-200 group-active:scale-95 group-hover:scale-110">
                    <Desktop weight="bold" size={24} />
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
              <div className="w-10 h-10 rounded-full bg-surface/70 flex items-center justify-center cursor-pointer hover:bg-surfaceHover/80 transition-colors" style={{ color: getPingColor() }}>
                {ping < 0 ? <WifiOff weight="bold" size={18} /> : <Wifi weight="bold" size={18} />}
              </div>
              {showPingTooltip && (
                <div className="absolute bottom-12 left-0 bg-surface/70 border border-[#303035]/70 rounded-xl px-4 py-2 whitespace-nowrap">
                  <div className="text-xs text-textMuted mb-1 font-bold tracking-wider">{t('main.voice.ping')}</div>
                  <div className="font-bold" style={{ color: getPingColor() }}>{ping < 0 ? t('main.voice.offline') : t('main.voice.pingValue', { ping, defaultValue: `${ping} мс` })}</div>
                </div>
              )}
            </div>
          </div>

        </div>
      </div>

      {renderModal('createChannel',
        <div className="glass-modal p-8 w-[400px]">
          <h2 className="text-xl font-bold mb-6 text-white">{t('modals.createChannel.title')}</h2>
          <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">{t('modals.createChannel.label')}</label>
          <input type="text" value={newChannelName} onChange={e => setNewChannelName(e.target.value)} maxLength={25} onKeyDown={e => e.key === 'Enter' && handleCreateChannel()} placeholder={t('modals.createChannel.placeholder')} className="w-full glass-field text-white rounded-xl p-3 mb-6 outline-none focus:ring-2 focus:ring-primary" />
          {error && <p className="text-danger text-sm mb-4 font-medium">{error}</p>}
          <div className="flex gap-4">
            <button onClick={closeAndResetModals} className="flex-1 bg-surface/70 text-white py-3 rounded-xl font-bold hover:bg-surfaceHover/80 transition-colors">{t('common.cancel')}</button>
            <button onClick={handleCreateChannel} className="flex-1 bg-primary/90 text-white py-3 rounded-xl font-bold hover:opacity-90 transition-opacity">{t('modals.createChannel.submit')}</button>
          </div>
        </div>
      )}

      {renderModal('channelEdit',
        <div className="glass-modal p-8 w-[400px]">
          <h2 className="text-xl font-bold mb-6 text-white">{t('modals.renameChannel.title')}</h2>
          <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">{t('modals.renameChannel.label')}</label>
          <input type="text" value={editChannelName} onChange={e => setEditChannelName(e.target.value)} maxLength={25} onKeyDown={e => e.key === 'Enter' && saveChannelEdit()} className="w-full glass-field text-white rounded-xl p-3 mb-6 outline-none focus:ring-2 focus:ring-primary" />
          {error && <p className="text-danger text-sm mb-4 font-medium">{error}</p>}
          <div className="flex gap-4">
            <button onClick={closeAndResetModals} className="flex-1 bg-surface/70 text-white py-3 rounded-xl font-bold hover:bg-surfaceHover/80 transition-colors">{t('common.cancel')}</button>
            <button onClick={saveChannelEdit} className="flex-1 bg-primary/90 text-white py-3 rounded-xl font-bold hover:opacity-90 transition-opacity">{t('modals.renameChannel.submit')}</button>
          </div>
        </div>
      )}

      {renderModal('addFriend',
        <div className="glass-modal p-8 w-[400px]">
          <h2 className="text-xl font-bold mb-2 text-white">{t('modals.addFriend.title')}</h2>
          <p className="text-textMuted text-sm mb-6 font-medium">{t('modals.addFriend.desc')}</p>
          <input
            type="text"
            value={friendName}
            onChange={e => { setFriendName(e.target.value); if (friendRequestStatus === 'notfound' || friendRequestStatus === 'alreadyfriend') setFriendRequestStatus('idle'); }}
            maxLength={25}
            onKeyDown={e => e.key === 'Enter' && handleAddFriend()}
            placeholder={t('modals.addFriend.placeholder')}
            className={`w-full glass-field text-white rounded-xl p-3 outline-none focus:ring-2 ${friendRequestStatus === 'notfound' ? 'glass-field-danger focus:ring-danger' :
              friendRequestStatus === 'alreadyfriend' ? 'glass-field-warning focus:ring-yellow-400' :
                'focus:ring-primary'
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
              className="flex-1 bg-surface/70 text-white py-3 rounded-xl font-bold hover:bg-surfaceHover/80 transition-colors"
            >{t('common.cancel')}</button>
            <button
              onClick={handleAddFriend}
              disabled={friendRequestStatus === 'loading' || friendRequestStatus === 'sent'}
              className={`flex-1 py-3 rounded-xl font-bold transition-all ${friendRequestStatus === 'sent'
                ? 'bg-green-600 text-white cursor-default scale-105'
                : friendRequestStatus === 'loading'
                  ? 'bg-primary/60 text-white cursor-wait'
                  : 'bg-primary/90 text-white hover:opacity-90'
                }`}
            >
              {friendRequestStatus === 'sent' ? `✓ ${t('modals.addFriend.sent')}` : friendRequestStatus === 'loading' ? '...' : t('modals.addFriend.submit')}
            </button>
          </div>
        </div>
      )}

      {renderModal('settings',
        <div className="relative transition-[margin] duration-[420ms] ease-[cubic-bezier(0.16,1,0.3,1)]" style={{ marginBottom: micTestPanelReserve }}>
          <MicTestPanel isEnabled={noiseSuppression} isActive={settingsTab === 'audio'} onHeightChange={setMicTestPanelHeight} />
          <div
            className="relative z-10 glass-modal w-[500px] flex flex-col overflow-hidden transition-[max-height] duration-[420ms] ease-[cubic-bezier(0.16,1,0.3,1)]"
            style={{ maxHeight: `min(90vh, calc(100vh - 4.5rem - ${micTestPanelReserve}px))` }}
          >
          <div className="flex items-center justify-between p-6 pb-0">
            <h2 className="text-xl font-bold text-white">{t('settings.title', 'настройки')}</h2>
            <button onClick={closeAndResetModals} className="group text-textMuted hover:text-white transition-colors duration-200 p-1.5 rounded-lg hover:bg-surface/70"><X weight="bold" size={24} /></button>
          </div>
          <div className="flex gap-2 px-6 pt-4 pb-4 border-b border-[#303035]/30">
            <button onClick={() => setSettingsTab('general')} className={`px-4 py-2 rounded-xl font-bold text-sm transition-colors ${settingsTab === 'general' ? 'bg-primary/90 text-white' : 'bg-surface/70 text-textMuted hover:text-white'}`}>{t('settings.tabs.general')}</button>
            <button onClick={() => setSettingsTab('audio')} className={`px-4 py-2 rounded-xl font-bold text-sm transition-colors ${settingsTab === 'audio' ? 'bg-primary/90 text-white' : 'bg-surface/70 text-textMuted hover:text-white'}`}>{t('settings.tabs.audio')}</button>
            <button onClick={() => setSettingsTab('privacy')} className={`px-4 py-2 rounded-xl font-bold text-sm transition-colors ${settingsTab === 'privacy' ? 'bg-primary/90 text-white' : 'bg-surface/70 text-textMuted hover:text-white'}`}>{t('settings.tabs.privacy')}</button>
          </div>
          <div className="p-6 overflow-y-auto flex-1" style={{ scrollbarGutter: 'stable' }}>
            {settingsTab === 'general' && (
              <div className="space-y-6">
                <div>
                  <label className="text-xs font-bold text-textMuted mb-3 block tracking-wider">{t('settings.general.system')}</label>

                  <div className="flex items-center justify-between glass-row p-4 rounded-xl mb-3">
                    <div className="mr-4">
                      <span className="font-semibold text-white text-[15px]">{t('settings.general.language')}</span>
                      <p className="text-xs text-textMuted mt-1">{t('settings.general.languageDesc')}</p>
                    </div>
                    <GlassSelect
                      value={language}
                      onChange={(newLang) => {
                        setLanguage(newLang);
                        i18n.changeLanguage(newLang);
                      }}
                      options={[
                        { value: 'ru', label: 'русский' },
                        { value: 'en', label: 'english' }
                      ]}
                      compact
                    />
                  </div>

                  <div className="flex items-center justify-between glass-row p-4 rounded-xl">
                    <div className="mr-4">
                      <span className="font-semibold text-white text-[15px]">{t('settings.general.autoLaunch')}</span>
                      <p className="text-xs text-textMuted mt-1">{t('settings.general.autoLaunchDesc')}</p>
                    </div>
                    <Md3Switch checked={autoLaunch} onChange={handleAutoLaunchToggle} />
                  </div>

                  <div className="flex items-center justify-between glass-row p-4 rounded-xl mt-3">
                    <div className="mr-4">
                      <span className="font-semibold text-white text-[15px]">{t('settings.general.minimizeToTray')}</span>
                      <p className="text-xs text-textMuted mt-1">{t('settings.general.minimizeToTrayDesc')}</p>
                    </div>
                    <Md3Switch checked={minimizeToTray} onChange={(v) => {
                      setMinimizeToTray(v);
                      window.windowControls.setMinimizeToTray(v).catch(() => { });
                    }} />
                  </div>

                  <div className="flex items-center justify-between glass-row p-4 rounded-xl mt-3">
                    <div className="mr-4 flex-1">
                      <span className="font-semibold text-white text-[15px]">{t('settings.general.updates', 'обновления приложения')}</span>
                      <p className="text-xs text-textMuted mt-1">
                        {manualUpdateStatus || (appVersion ? `${t('settings.general.currentVersion', 'текущая версия')}: v${appVersion}` : '')}
                      </p>
                    </div>
                    <button
                      onClick={handleManualCheckUpdates}
                      disabled={manualUpdateChecking}
                      className="px-3.5 py-2 rounded-xl text-xs font-bold text-white bg-primary/90 hover:opacity-90 transition-all flex items-center gap-1.5 shrink-0 active:scale-[0.98] disabled:opacity-50"
                    >
                      <Sparkle weight="bold" size={14} />
                      <span>{manualUpdateChecking ? t('settings.general.checkingUpdates', 'проверка...') : t('settings.general.checkUpdates', 'проверить')}</span>
                    </button>
                  </div>
                </div>
              </div>
            )}
            {settingsTab === 'audio' && (
              <div className="space-y-6">
                <div>
                  <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">{t('settings.audio.inputDevice')}</label>
                  <GlassSelect
                    value={selectedInput}
                    onChange={(val) => {
                      setSelectedInput(val);
                      webrtc.updateSettings(val, noiseSuppression);
                    }}
                    options={[
                      { value: 'default', label: t('settings.audio.default') },
                      ...(audioDevices.inputs.length === 0 && selectedInput !== 'default'
                        ? [{ value: selectedInput, label: t('common.loading') }]
                        : []),
                      ...audioDevices.inputs.map((d) => ({
                        value: d.deviceId,
                        label: d.label || t('settings.audio.micFallback')
                      }))
                    ]}
                  />
                </div>
                <div>
                  <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">{t('settings.audio.outputDevice')}</label>
                  <GlassSelect
                    value={selectedOutput}
                    onChange={(val) => {
                      setSelectedOutput(val);
                      webrtc.setOutputDevice(val);
                    }}
                    options={[
                      { value: 'default', label: t('settings.audio.default') },
                      ...(audioDevices.outputs.length === 0 && selectedOutput !== 'default'
                        ? [{ value: selectedOutput, label: t('common.loading') }]
                        : []),
                      ...audioDevices.outputs.map((d) => ({
                        value: d.deviceId,
                        label: d.label || t('settings.audio.speakerFallback')
                      }))
                    ]}
                  />
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
                <NoiseSuppressionSettings
                  isEnabled={noiseSuppression}
                  onEnabledChange={v => {
                    setNoiseSuppression(v);
                    webrtc.setNoiseSuppression(v);
                  }}
                  mode={micThresholdMode === 'auto' ? 'smart' : 'manual'}
                  onModeChange={m => {
                    const nextMode = m === 'smart' ? 'auto' : 'manual';
                    setMicThresholdMode(nextMode);
                    void webrtc.setNoiseSuppressionMode(nextMode);
                    settingsRef.current = { ...settingsRef.current, micThresholdMode: nextMode };
                  }}
                  manualThreshold={manualThresholdValue}
                  onManualThresholdChange={v => {
                    setManualThresholdValue(v);
                    webrtc.setMicThresholdParams('manual', v);
                    settingsRef.current = { ...settingsRef.current, manualThresholdValue: v };
                  }}
                  smartModel={smartNoiseModel}
                  onSmartModelChange={m => {
                    setSmartNoiseModel(m);
                    webrtc.setSmartNoiseModel(m);
                  }}
                  speechAnalyzer={speechAnalyzerEnabled}
                  onSpeechAnalyzerChange={v => {
                    setSpeechAnalyzerEnabled(v);
                    webrtc.setSpeechAnalyzerEnabled(v);
                  }}
                  echoCancellation={echoCancellationEnabled}
                  onEchoCancellationChange={v => {
                    setEchoCancellationEnabled(v);
                    webrtc.setEchoCancellationEnabled(v);
                  }}
                  suppressionStrength={suppressionStrength}
                  onSuppressionStrengthChange={v => {
                    setSuppressionStrength(v);
                    webrtc.setSuppressionStrength(v);
                  }}
                  onStartCalibration={handleManualCalibration}
                  isCalibrating={isCalibrating}
                  calibrationCountdown={calibrationCountdown}
                  calibrationPhase={calibrationPhase}
                  calibrationSuccess={calibrationSuccess}
                />
              </div>
            )}
            {settingsTab === 'privacy' && (
              <div className="space-y-6">
                <div>
                  <label className="text-xs font-bold text-textMuted mb-3 block tracking-wider">{t('settings.privacy.network', 'сеть')}</label>
                  <div className="flex items-center justify-between glass-row p-4 rounded-xl">
                    <div className="mr-4">
                      <span className="font-semibold text-white text-[15px]">{t('settings.privacy.hideIp', 'скрывать мой IP-адрес')}</span>
                      <p className="text-xs text-textMuted mt-1">{t('settings.privacy.hideIpDesc', 'весь голос идёт через сервер ретрансляции, собеседники не видят ваш IP. задержка чуть выше. применяется к новым подключениям.')}</p>
                    </div>
                    <Md3Switch checked={relayOnlyIce} onChange={(v) => {
                      setRelayOnlyIce(v);
                      webrtc.setRelayOnlyIce(v);
                    }} />
                  </div>
                </div>
                <div>
                  <button
                    onClick={() => store.setModal('privacy', true)}
                    className="group w-full bg-primary/90 hover:opacity-90 text-white py-3 rounded-xl font-bold flex items-center justify-center gap-2 transition-all duration-200 active:scale-98"
                  >
                    <Key weight="bold" size={18} />
                    {t('settings.privacy.changePasswordTitle', 'сменить пароль')}
                  </button>
                </div>
                <button onClick={handleLogout} className="group w-full bg-danger hover:bg-red-600 text-white py-3 rounded-xl font-bold flex items-center justify-center gap-2 transition-colors">
                  <div className="transition-transform duration-200 group-hover:-translate-x-1">
                    <LogOut weight="bold" size={18} />
                  </div>
                  {t('settings.privacy.logout')}
                </button>
                <button
                  onClick={() => {
                    setDeleteConfirmText('');
                    setDeleteAccountError('');
                    setShowDeleteAccountModal(true);
                  }}
                  className="group w-full bg-danger/15 hover:bg-danger/25 text-danger border border-danger/20 py-3 rounded-xl font-bold flex items-center justify-center gap-2 transition-all duration-200 active:scale-98"
                >
                  <Trash weight="bold" size={18} />
                  {t('settings.privacy.deleteAccount', 'удалить аккаунт')}
                </button>
              </div>
            )}
          </div>
          </div>
        </div>
      )}

      {store.pendingChannelSwitch && (
        <div className="fixed inset-0 z-[999] bg-black/70 backdrop-blur-md flex items-center justify-center p-4">
          <div className="glass-modal p-8 w-[400px] text-center">
            <div className="flex items-center justify-between mb-4">
              <h2 className="text-xl font-bold text-white">{t('modals.switchChannel.title', 'сменить канал?')}</h2>
              <button
                onClick={cancelChannelSwitch}
                className="group text-textMuted hover:text-white transition-colors duration-200 p-1.5 rounded-lg hover:bg-surface/70"
              >
                <X weight="bold" size={24} />
              </button>
            </div>

            <p className="text-textMuted mb-8 font-medium">
              {t('modals.switchChannel.desc', 'вы покинете текущий канал и перейдёте в другой.')}
            </p>

            <div className="flex gap-4">
              <button
                onClick={cancelChannelSwitch}
                className="flex-1 bg-surface/70 text-white py-3 rounded-xl font-bold hover:bg-surfaceHover/80 transition-colors"
              >
                {t('modals.switchChannel.stay', 'остаться')}
              </button>
              <button
                onClick={confirmChannelSwitch}
                disabled={isSwitchingChannel}
                className="flex-1 bg-primary/90 text-white py-3 rounded-xl font-bold hover:opacity-90 transition-opacity"
              >
                {isSwitchingChannel ? t('modals.switchChannel.switching', 'переход...') : t('modals.switchChannel.switch', 'перейти')}
              </button>
            </div>
          </div>
        </div>
      )}

      {renderModal('inviteToChannel',
        <div className="glass-modal p-8 w-[400px]">
          <h2 className="text-xl font-bold mb-2 text-white">{t('modals.inviteToChannel.title', 'пригласить в канал')}</h2>
          <p className="text-textMuted text-sm mb-6">{store.selectedChannelForInvite?.name}</p>
          <input type="text" value={inviteFriendSearch} onChange={e => setInviteFriendSearch(e.target.value)} placeholder={t('modals.inviteToChannel.searchPlaceholder', 'поиск среди друзей...')} className="w-full glass-field text-white rounded-xl p-3 mb-4 outline-none focus:ring-2 focus:ring-primary" />
          <div className="max-h-[300px] overflow-y-auto space-y-2">
            {invitableFriends.map(f => (
              <div key={f.id} className="flex items-center gap-3 p-3 glass-row glass-row-hover rounded-xl transition-colors">
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
                  {sentInvites.has(f.id) ? t('modals.inviteToChannel.sent', '✓ отправлено') : t('common.invite', 'пригласить')}
                </button>
              </div>
            ))}
            {invitableFriends.length === 0 && (
              <p className="text-textMuted text-center py-4 font-medium">{t('modals.inviteToChannel.noFriends', 'друзья не найдены')}</p>
            )}
          </div>
          <button onClick={closeAndResetModals} className="w-full mt-4 bg-surface/70 text-white py-3 rounded-xl font-bold hover:bg-surfaceHover/80 transition-colors">{t('common.close', 'закрыть')}</button>
        </div>
      )}

      {renderModal('channelMembers',
        <div className="glass-modal p-8 w-[420px]">
          <div className="flex items-center justify-between mb-2">
            <h2 className="text-xl font-bold text-white flex items-center gap-3"><Users weight="bold" size={24} /> {t('modals.members.title', 'участники')}</h2>
            <button onClick={closeAndResetModals} className="group text-textMuted hover:text-white transition-colors duration-200 p-1.5 rounded-lg hover:bg-surface/70"><X weight="bold" size={24} /></button>
          </div>
          <p className="text-textMuted text-sm mb-6 truncate">{store.selectedChannelForMembers?.name}</p>
          <div className="max-h-[350px] overflow-y-auto space-y-2 pr-2">
            {isChannelMembersLoading && store.channelMembers.length === 0 && (
              <div className="flex justify-center py-8">
                <div className="w-6 h-6 border-2 border-white/30 border-t-white rounded-full animate-spin" />
              </div>
            )}
            {store.channelMembers.map(m => (
              <div key={m.id} onContextMenu={e => handleContextMenu(e, 'channelMember', m)} className="flex items-center gap-3 p-3 glass-row glass-row-hover rounded-xl transition-colors cursor-pointer animate-fade-in">
                <div className="relative w-[47px] h-[47px] shrink-0">
                  <AvatarImg src={m.avatarBase64} size={47} bgColor={m.avatarColor} />
                  <div className={`absolute -bottom-0.5 -right-0.5 w-3.5 h-3.5 rounded-full border-[3px] border-surface ${m.isOnline ? 'bg-success' : 'bg-gray-500'}`} />
                </div>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="font-semibold text-white truncate">{m.displayName}</span>
                    {store.selectedChannelForMembers?.ownerId === m.id && (
                      <span className="text-[10px] font-bold bg-yellow-500/20 text-yellow-500 px-2 py-0.5 rounded-md flex items-center gap-1 shrink-0"><Crown weight="bold" size={12} /> {t('modals.members.creator', 'создатель')}</span>
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
        <div className="glass-modal p-8 w-[400px] text-center">
          <div className="w-16 h-16 bg-danger/20 rounded-full flex items-center justify-center mx-auto mb-4"><UserX weight="bold" size={32} className="text-danger" /></div>
          <h2 className="text-xl font-bold mb-2 text-white">{t('modals.kick.title')}</h2>
          <p className="text-textMuted mb-8">
            <Trans i18nKey="modals.kick.desc" values={{ name: store.userToKick?.displayName }}>
              Вы уверены, что хотите лишить пользователя <span className="text-white font-bold">{store.userToKick?.displayName}</span> доступа к каналу?
            </Trans>
          </p>
          <div className="flex gap-4">
            <button onClick={() => { store.setModal('kickConfirm', false); store.setUserToKick(null); }} className="flex-1 bg-surface/70 text-white py-3 rounded-xl font-bold hover:bg-surfaceHover/80 transition-colors">{t('common.cancel')}</button>
            <button onClick={handleKickConfirm} className="flex-1 bg-danger text-white py-3 rounded-xl font-bold hover:bg-red-600 transition-colors">{t('modals.kick.submit')}</button>
          </div>
        </div>
      )}

      {renderModal('channelFull',
        <div className="glass-modal p-8 w-[400px] text-center border-danger/30">
          <div className="w-20 h-20 bg-danger/20 rounded-full flex items-center justify-center mx-auto mb-4"><Users weight="bold" size={40} className="text-danger" /></div>
          <h2 className="text-xl font-bold mb-4 text-white">{t('modals.channelFull.title', 'канал переполнен')}</h2>
          <p className="text-textMuted mb-8">{t('modals.channelFull.desc', 'максимальное количество участников в канале — 10 человек. подождите, пока кто-то выйдет.')}</p>
          <button onClick={closeAndResetModals} className="w-full bg-surface/70 text-white py-3 rounded-xl font-bold hover:bg-surfaceHover/80 transition-colors">{t('modals.channelFull.gotIt', 'понятно')}</button>
        </div>
      )}

      {renderModal('userVolume',
        <div className="glass-modal p-8 w-[400px]">
          <h2 className="text-xl font-bold mb-2 text-white">
            {volumeType === 'stream' ? t('modals.streamVolume.title', 'громкость трансляции') : t('modals.userVolume.title', 'громкость пользователя')}
          </h2>
          <p className="text-textMuted text-sm mb-6 font-medium">
            {volumeType === 'stream' ? t('stream.streamLabel', 'трансляция {{name}}', { name: volumeUser?.displayName }) : volumeUser?.displayName}
          </p>
          <div>
            <Md3Slider
              min={0}
              max={200}
              step={5}
              value={volumeUserValue}
              label={t('modals.userVolume.label', 'громкость')}
              showPercentage
              showMuteButton
              onChange={v => {
                if (volumeUser) {
                  if (volumeType === 'stream') {
                    webrtc.setStreamVolumeRealtime(volumeUser.id, v)
                  } else {
                    webrtc.setUserVolumeRealtime(volumeUser.id, v)
                  }
                }
              }}
              onChangeEnd={v => {
                setVolumeUserValue(v)
                if (volumeUser) {
                  if (volumeType === 'stream') {
                    store.setStreamVolume(volumeUser.id, v)
                    webrtc.updateRemoteStreamVolume(volumeUser.id)
                  } else {
                    webrtc.setUserVolume(volumeUser.id, v)
                  }
                }
              }}
            />
          </div>
          <button onClick={closeAndResetModals} className="w-full mt-6 bg-surface/70 text-white py-3 rounded-xl font-bold hover:bg-surfaceHover/80 transition-colors">{t('common.close', 'закрыть')}</button>
        </div>
      )}

      {renderModal('incomingCall',
        <div className="glass-modal p-8 w-[350px] text-center">
          <div className="w-[87px] h-[87px] mx-auto mb-4 relative">
            <AvatarImg src={store.incomingCall?.callerAvatarBase64 || null} size={87} bgColor={store.incomingCall?.callerAvatarColor} />
          </div>
          <h2 className="text-xl font-bold mb-2 text-white">{store.incomingCall?.callerName}</h2>
          <p className="text-textMuted mb-8 font-medium">{t('toasts.incomingCall', 'входящий звонок...')}</p>
          <div className="flex gap-4">
            <button onClick={handleDeclineCall} className="flex-1 bg-danger text-white py-3 rounded-xl font-bold hover:bg-red-600 active:scale-95 transition-all flex items-center justify-center gap-2"><PhoneOff weight="bold" size={18} /> {t('main.notifications.decline', 'отклонить')}</button>
            <button onClick={handleAcceptCall} className="flex-1 bg-success text-white py-3 rounded-xl font-bold hover:bg-green-600 active:scale-95 transition-all flex items-center justify-center gap-2"><Phone weight="bold" size={18} /> {t('main.notifications.accept', 'принять')}</button>
          </div>
        </div>
      )}

      {renderModal('incomingChannelInvite',
        <div className="glass-modal p-8 w-[350px] text-center">
          {(() => {
            const invite = store.incomingChannelInvite;
            if (!invite) return null;
            const users = store.channelUsersMap[invite.channelId] || [];
            const displayUsers = users.length > 0 ? users : [{ id: invite.senderId, displayName: invite.senderName, avatarBase64: null, avatarColor: '#C81E70', username: '', isOnline: true, isMuted: false, isDeafened: false, isSpeaking: false }];
            return (
              <>
                <div className="flex justify-center mb-4">
                  {displayUsers.slice(0, 3).map((u, i) => (
                    <div key={u.id} className="w-[87px] h-[87px] rounded-full border-[4px] border-panelBg relative shrink-0" style={{ marginLeft: i === 0 ? 0 : '-1.5rem', zIndex: 10 - i }}>
                      <AvatarImg src={u.avatarBase64 || null} size={87} bgColor={u.avatarColor} />
                    </div>
                  ))}
                </div>
                <h2 className="text-xl font-bold mb-2 text-white truncate px-2">{invite.channelName}</h2>
                <p className="text-textMuted mb-8 font-medium">{t('toasts.incomingChannelInvite', 'вас зовут в канал')}</p>
                <div className="flex gap-4">
                  <button onClick={() => { store.setModal('incomingChannelInvite', false); store.setIncomingChannelInvite(null); signalRService.stopRingtone(); signalRService.declineChannelInvite(invite.channelId); }} className="flex-1 bg-danger text-white py-3 rounded-xl font-bold hover:bg-red-600 active:scale-95 transition-all flex items-center justify-center gap-2"><PhoneOff weight="bold" size={18} /> {t('common.dismiss', 'сбросить')}</button>
                  <button onClick={() => { handleAcceptChannelInvite(invite.channelId); store.setModal('incomingChannelInvite', false); store.setIncomingChannelInvite(null); signalRService.stopRingtone(); store.setChannelInvites(store.channelInvites.filter(i => i.channelId !== invite.channelId)); }} className="flex-1 bg-success text-white py-3 rounded-xl font-bold hover:bg-green-600 transition-colors flex items-center justify-center gap-2"><Phone weight="bold" size={18} /> {t('main.notifications.join', 'войти')}</button>
                </div>
              </>
            );
          })()}
        </div>
      )}

      {store.modals.privacy && (
        <div className="fixed inset-0 z-[100000] bg-black/70 backdrop-blur-md flex items-center justify-center p-3 pt-[3.75rem]">
          <div className="glass-modal p-8 w-[400px]">
            <div className="flex items-center justify-between mb-6">
              <h2 className="text-xl font-bold text-white">{t('settings.privacy.changePasswordTitle', 'сменить пароль')}</h2>
              <button onClick={closeChangePasswordModal} className="group text-textMuted hover:text-white transition-colors duration-200 p-1.5 rounded-lg hover:bg-surface/70"><X weight="bold" size={24} /></button>
            </div>
            <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">{t('settings.privacy.newPassword')}</label>
            <div className="relative mb-6">
              <input type={showPrivacyPass ? 'text' : 'password'} value={newPassword} onChange={e => { setNewPassword(e.target.value); setPrivacyError(''); }} maxLength={25} onKeyDown={e => e.key === 'Enter' && changePassword()} placeholder={t('settings.privacy.passwordHint')} className="w-full glass-field text-white rounded-xl p-3 outline-none pr-10 focus:ring-2 focus:ring-primary" />
              <button onClick={() => setShowPrivacyPass(!showPrivacyPass)} className="absolute right-3 top-3 text-textMuted hover:text-white transition-colors">{showPrivacyPass ? <EyeOff weight="bold" size={20} /> : <Eye weight="bold" size={20} />}</button>
            </div>
            {privacyError && <p className="text-danger text-sm mb-4 font-medium">{privacyError}</p>}
            <div className="flex gap-4">
              <button onClick={closeChangePasswordModal} className="flex-1 bg-surface/70 text-white py-3 rounded-xl font-bold hover:bg-surfaceHover/80 transition-colors">{t('common.cancel')}</button>
              <button onClick={changePassword} className="flex-1 bg-primary/90 text-white py-3 rounded-xl font-bold hover:opacity-90 transition-opacity">{t('settings.privacy.changePassword')}</button>
            </div>
          </div>
        </div>
      )}

      {showDeleteAccountModal && (
        <div className="fixed inset-0 z-[100000] bg-black/70 backdrop-blur-md flex items-center justify-center p-3 pt-[3.75rem]">
          <div className="glass-modal p-8 w-[420px]">
            <div className="flex items-center justify-between mb-4">
              <h2 className="text-xl font-bold text-white">{t('settings.privacy.deleteAccountTitle', 'удаление аккаунта')}</h2>
              <button
                onClick={() => { setShowDeleteAccountModal(false); setDeleteConfirmText(''); setDeleteAccountError(''); }}
                className="group text-textMuted hover:text-white transition-colors duration-200 p-1.5 rounded-lg hover:bg-surface/70"
              >
                <X weight="bold" size={24} />
              </button>
            </div>
            <p className="text-sm text-textMuted mb-4 leading-relaxed">
              {t('settings.privacy.deleteAccountWarning', 'это действие необратимо. все ваши данные, достижения, список друзей и созданные каналы будут удалены навсегда.')}
            </p>
            <label className="text-xs font-bold text-textMuted mb-2 block tracking-wider">
              {t('settings.privacy.deleteAccountPrompt', 'для подтверждения введите «подтверждаю»:')}
            </label>
            <div className="mb-6">
              <input
                type="text"
                value={deleteConfirmText}
                onChange={e => { setDeleteConfirmText(e.target.value); setDeleteAccountError(''); }}
                placeholder={t('settings.privacy.deleteAccountConfirmWord', 'подтверждаю')}
                autoFocus
                className="w-full glass-field text-white rounded-xl p-3 outline-none focus:ring-2 focus:ring-danger"
              />
            </div>
            {deleteAccountError && <p className="text-danger text-sm mb-4 font-medium">{deleteAccountError}</p>}
            <div className="flex gap-4">
              <button
                onClick={() => { setShowDeleteAccountModal(false); setDeleteConfirmText(''); setDeleteAccountError(''); }}
                className="flex-1 bg-surface/70 text-white py-3 rounded-xl font-bold hover:bg-surfaceHover/80 transition-colors active:scale-98"
              >
                {t('common.cancel', 'отмена')}
              </button>
              <button
                disabled={
                  isDeletingAccount ||
                  (deleteConfirmText.trim().toLowerCase() !== t('settings.privacy.deleteAccountConfirmWord', 'подтверждаю').trim().toLowerCase() &&
                   deleteConfirmText.trim().toLowerCase() !== 'подтверждаю' &&
                   deleteConfirmText.trim().toLowerCase() !== 'confirm')
                }
                onClick={handleDeleteAccount}
                className="flex-1 bg-danger hover:bg-red-600 disabled:opacity-40 disabled:cursor-not-allowed text-white py-3 rounded-xl font-bold transition-all active:scale-98"
              >
                {isDeletingAccount ? t('auth.loading', 'загрузка...') : t('settings.privacy.deleteAccountConfirmBtn', 'удалить навсегда')}
              </button>
            </div>
          </div>
        </div>
      )}

      {store.modals.profile && store.selectedProfileUser && (
        <div className="fixed inset-0 z-[100000] bg-black/70 backdrop-blur-md flex items-center justify-center p-3 pt-[3.75rem]">
          <div className="glass-modal w-[400px] overflow-hidden relative">
            <div
              className="h-32 w-full relative transition-colors duration-500"
              style={{ backgroundColor: editProfileAvatarBase64 ? editProfileAvatarColor : store.selectedProfileUser?.avatarColor }}
            >
              <button
                onClick={() => store.closeProfileOnly()}
                className="absolute top-4 right-4 text-white/70 hover:text-white bg-black/20 hover:bg-black/40 p-2 rounded-full backdrop-blur-md transition-colors duration-200"
              >
                <X weight="bold" size={20} />
              </button>

              {store.selectedProfileUser?.id === store.currentUser?.id && !isEditingProfile && (
                <button
                  onClick={() => setIsEditingProfile(true)}
                  className="absolute top-4 right-14 text-white/70 hover:text-white bg-black/20 hover:bg-black/40 p-2 rounded-full backdrop-blur-md transition-all"
                  title={t('profile.editTitle', 'редактировать профиль')}
                >
                  <Edit2 weight="bold" size={20} />
                </button>
              )}
            </div>

            <div className="px-8 pb-8 relative mt-[-56px]">
              <div className="flex items-start gap-6 mb-6 relative z-10">
                <div className="relative group shrink-0">
                  <div className="w-[112px] h-[112px] rounded-full border-[6px] border-panelBg bg-panelBg relative overflow-hidden">
                    <AvatarImg
                      src={isEditingProfile ? (editProfileAvatarBase64 || store.selectedProfileUser?.avatarBase64) : store.selectedProfileUser?.avatarBase64}
                      size={100}
                      bgColor={isEditingProfile ? (editProfileAvatarBase64 ? editProfileAvatarColor : store.selectedProfileUser?.avatarColor) : store.selectedProfileUser?.avatarColor}
                    />
                    {isEditingProfile && (
                      <div
                        className="absolute inset-0 bg-black/60 rounded-full opacity-0 group-hover:opacity-100 transition-opacity flex items-center justify-center cursor-pointer backdrop-blur-sm"
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

                  {!isEditingProfile && (
                    <div className={`absolute bottom-1.5 right-1.5 w-7 h-7 rounded-full border-[4px] border-panelBg ${store.selectedProfileUser?.id === store.currentUser?.id
                      ? (serverConnected ? 'bg-success' : 'bg-gray-500')
                      : (store.selectedProfileUser?.isOnline ? 'bg-success' : 'bg-gray-500')
                      }`} />
                  )}
                </div>

                {!isEditingProfile && store.selectedProfileUser?.aboutMe && (
                  <div className="flex-1 mt-10 relative animate-fade-in">
                    <div className="absolute w-1.5 h-1.5 rounded-full bg-[#303035] left-[-18px] top-[-12px] opacity-90" />
                    <div className="absolute w-2.5 h-2.5 rounded-full bg-[#303035] left-[-8px] top-[-5px] opacity-90" />

                    <div className="bg-[#2B2D31] border border-[#303035]/70 p-3 rounded-2xl min-h-[60px] flex items-center justify-center">
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
                      <label className="text-[10px] font-bold text-textMuted mb-2 block tracking-[0.14em]">{t('profile.displayName', 'отображаемое имя')}</label>
                      <input
                        type="text"
                        value={editProfileDisplayName}
                        onChange={e => {
                          setEditProfileDisplayName(e.target.value);
                          setError('');
                        }}
                        maxLength={20}
                        className="glass-field w-full p-3 rounded-xl text-white font-bold text-base outline-none focus:ring-2 focus:ring-primary transition-shadow"
                      />
                      {error && <p className="text-danger text-xs mt-2 font-medium">{error}</p>}
                    </div>
                    <div>
                      <label className="text-[10px] font-bold text-textMuted mb-2 block tracking-[0.14em]">{t('profile.aboutMe', 'о себе')}</label>
                      <textarea
                        value={editProfileAboutMe}
                        onChange={e => setEditProfileAboutMe(e.target.value)}
                        maxLength={150}
                        rows={3}
                        placeholder={t('profile.aboutMePlaceholder', 'напишите немного о себе...')}
                        className="glass-field w-full p-3 rounded-xl text-white text-sm outline-none focus:ring-2 focus:ring-primary resize-none transition-shadow"
                      />
                    </div>
                  </div>
                ) : (
                  <div className="animate-fade-in text-left">
                    <h2 className="text-3xl font-black text-white tracking-tight break-words">{store.selectedProfileUser?.displayName}</h2>
                    <p
                      className={`text-base mt-1.5 font-bold cursor-pointer transition-opacity inline-block ${isLoginCopied ? 'text-success' : 'text-primaryText hover:underline hover:opacity-80'}`}
                      title={isLoginCopied ? "" : t('profile.copyLogin', 'скопировать логин')}
                      onClick={() => {
                        if (store.selectedProfileUser && !isLoginCopied) {
                          navigator.clipboard.writeText(store.selectedProfileUser.username);
                          setIsLoginCopied(true);
                          setTimeout(() => setIsLoginCopied(false), 2000);
                        }
                      }}
                    >
                      {isLoginCopied ? t('profile.loginCopied', 'скопировано!') : `@${store.selectedProfileUser?.username}`}
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
                            setOfflineToast(t('profile.userOffline', 'пользователь не в сети'));
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
                        ? 'bg-success/20 text-success cursor-default animate-invite-pulse'
                        : 'bg-success text-white hover:opacity-90 active:scale-[0.98]'
                        }`}
                    >
                      <Phone weight="bold" size={18} /> {store.selectedProfileUser && sentInvites.has(store.selectedProfileUser.id) ? t('profile.inviting', 'зовём...') : t('profile.inviteToChannel', 'позвать в канал')}
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
                      className="w-full bg-surface/70 text-danger py-3 rounded-xl font-bold hover:bg-[#2B2D31] transition-colors"
                    >
                      {t('profile.kick', 'исключить из канала')}
                    </button>
                  )}
                </div>
              )}

              {isEditingProfile ? (
                <div className="flex gap-3 pt-4 border-t border-[#303035]/70">
                  <button
                    onClick={() => {
                      setIsEditingProfile(false);
                      setError('');
                      setEditProfileDisplayName(store.currentUser!.displayName);
                      setEditProfileAboutMe(store.currentUser!.aboutMe || '');
                      setEditProfileAvatarBase64(null);
                    }}
                    className="flex-1 bg-surface/70 text-white py-3.5 rounded-xl font-bold hover:bg-surfaceHover/80 transition-colors"
                  >
                    {t('common.cancel', 'отмена')}
                  </button>
                  <button
                    onClick={saveProfileChanges}
                    className="flex-1 bg-primary/90 text-white py-3.5 rounded-xl font-bold active:scale-95 transition-all"
                  >
                    {t('common.save', 'сохранить')}
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
                      className="w-16 h-16 rounded-2xl bg-surface/70 border border-[#303035]/70 flex items-center justify-center text-primaryText hover:bg-primary/10 hover:border-primary/40 hover:scale-105 active:scale-95 transition-all"
                      title={t('achievements.title', 'достижения')}
                    >
                      <Trophy weight="bold" size={28} />
                    </button>
                  ) : store.friends.some(f => f.id === store.selectedProfileUser?.id) ? (
                    <div className="flex justify-center items-center gap-4 w-full">
                      <button
                        onClick={() => {
                          if (store.selectedProfileUser) signalRService.removeFriend(store.selectedProfileUser.id);
                          store.closeProfileOnly();
                        }}
                        className="w-16 h-16 rounded-2xl bg-surface/70 border border-[#303035]/70 flex items-center justify-center text-textMuted hover:text-danger hover:bg-danger/10 hover:border-danger/40 hover:scale-105 active:scale-95 transition-all"
                        title={t('profile.removeFriend', 'удалить из друзей')}
                      >
                        <UserMinus weight="bold" size={28} />
                      </button>

                      <button
                        onClick={() => {
                          if (store.selectedProfileUser) openUserAchievements(store.selectedProfileUser.id);
                          store.closeProfileOnly();
                        }}
                        className="w-16 h-16 rounded-2xl bg-surface/70 border border-[#303035]/70 flex items-center justify-center text-primaryText hover:bg-primary/10 hover:border-primary/40 hover:scale-105 active:scale-95 transition-all"
                        title={t('achievements.title', 'достижения')}
                      >
                        <Trophy weight="bold" size={28} />
                      </button>

                      <button
                        onClick={async () => {
                          if (store.selectedProfileUser && store.selectedProfileUser.isOnline) {
                            const ok = await signalRService.startCall(store.selectedProfileUser.id);
                            if (!ok) {
                              setOfflineToast(t('profile.userOffline', 'пользователь не в сети'));
                              setTimeout(() => setOfflineToast(null), 3000);
                            }
                            store.closeProfileOnly();
                          }
                        }}
                        disabled={!store.selectedProfileUser?.isOnline}
                        className={`w-16 h-16 rounded-2xl bg-surface/70 border border-[#303035]/70 flex items-center justify-center transition-all
                          ${store.selectedProfileUser?.isOnline
                            ? 'text-success hover:bg-success/10 hover:border-success/40 hover:scale-105 active:scale-95'
                            : 'text-textMuted/40 cursor-not-allowed opacity-50'}`}
                        title={store.selectedProfileUser?.isOnline ? t('profile.call', 'позвонить') : t('profile.userOffline', 'пользователь не в сети')}
                      >
                        <Phone weight="bold" size={28} />
                      </button>
                    </div>
                  ) : (
                    <div className="flex justify-center items-center gap-4 w-full">
                      <button
                        onClick={() => {
                          if (store.selectedProfileUser) openUserAchievements(store.selectedProfileUser.id);
                          store.closeProfileOnly();
                        }}
                        className="w-16 h-16 rounded-2xl bg-surface/70 border border-[#303035]/70 flex items-center justify-center text-primaryText hover:bg-primary/10 hover:border-primary/40 hover:scale-105 active:scale-95 transition-all"
                        title={t('achievements.title', 'достижения')}
                      >
                        <Trophy weight="bold" size={28} />
                      </button>

                      {(() => {
                        const hasOutgoingRequest = (
                          store.selectedProfileUser?.friendRequestsReceived ||
                          (store.selectedProfileUser as any)?.FriendRequestsReceived
                        )?.includes(store.currentUser?.id || '');
                        const isRequestSent = profileFriendRequestStatus === 'sent' || hasOutgoingRequest;
                        const isBtnDisabled = profileFriendRequestStatus === 'loading' || isRequestSent;

                        return (
                          <button
                            onClick={async () => {
                              if (!store.selectedProfileUser || isBtnDisabled) return;
                              setProfileFriendRequestStatus('loading');
                              const success = await signalRService.sendFriendRequest(store.selectedProfileUser.username);
                              if (success) {
                                setProfileFriendRequestStatus('sent');
                              } else {
                                setProfileFriendRequestStatus('idle');
                              }
                            }}
                            disabled={isBtnDisabled}
                            className={`w-16 h-16 rounded-2xl bg-surface/70 border border-[#303035]/70 flex items-center justify-center text-success transition-all ${isBtnDisabled
                              ? 'opacity-50 cursor-default'
                              : 'hover:bg-success/10 hover:border-success/40 hover:scale-105 active:scale-95'
                              }`}
                            title={isRequestSent ? t('modals.addFriend.sent', 'отправлено') : t('modals.addFriend.title', 'добавить друга')}
                          >
                            <div className="relative w-7 h-7 flex items-center justify-center">
                              <UserPlus
                                weight="bold"
                                size={28}
                                className={`absolute inset-0 transition-opacity duration-300 ease-in-out ${isRequestSent ? 'opacity-0' : 'opacity-100'
                                  }`}
                              />
                              <UserCheck
                                weight="bold"
                                size={28}
                                className={`absolute inset-0 transition-opacity duration-300 ease-in-out ${isRequestSent ? 'opacity-100' : 'opacity-0'
                                  }`}
                              />
                            </div>
                          </button>
                        );
                      })()}
                    </div>
                  )}
                </div>
              )}
            </div>
          </div>
        </div>
      )}
      {store.achievementToast && createPortal((() => {
        const isHiding = store.achievementToast.startsWith('__hiding__');
        const achId = isHiding ? store.achievementToast.replace('__hiding__', '') : store.achievementToast;
        const def = getAchievementDef(achId);
        if (!def) return null;
        return (
          <div className={`fixed top-14 left-1/2 z-[1000000] ${isHiding ? 'animate-toast-top-out' : 'animate-toast-top-in'}`}>
            <div className="bg-[#09090B]/85 backdrop-blur-2xl border border-primary/50 rounded-modal px-8 py-6 flex items-center gap-6">
              <div className="w-14 h-14 rounded-2xl bg-primary/20 flex items-center justify-center shrink-0 border border-primary/30 overflow-hidden relative group">
                <div className="absolute inset-0 bg-gradient-to-br from-primary/20 to-transparent animate-pulse" />
                <span className="text-4xl relative z-10">{def.icon}</span>
              </div>
              <div className="min-w-0">
                <p className="text-primaryText font-black text-xs tracking-[0.2em] mb-1.5 opacity-90">{t('achievements.toastTitle', 'достижение получено')}</p>
                <p className="text-white font-black text-xl tracking-tight leading-none">{t(`achievements.${def.id}.title`, def.title)}</p>
                <p className="text-textMuted font-bold text-sm mt-2 line-clamp-1 opacity-80">{t(`achievements.${def.id}.description`, def.description)}</p>
              </div>
            </div>
          </div>
        );
      })(), document.body)}

      {renderModal('achievements',
        <div className="glass-modal w-[500px] max-h-[min(90vh,100vh-4.5rem)] flex flex-col overflow-hidden">
          <div className="flex items-center justify-between p-6 pb-4">
            <h2 className="text-xl font-bold text-white flex items-center gap-3">
              <Trophy weight="bold" size={24} />
              {store.achievementsViewUserId ? t('achievements.title', 'достижения') : t('achievements.myTitle', 'мои достижения')}
            </h2>
            <button onClick={closeAndResetModals} className="group text-textMuted hover:text-white transition-colors duration-200 p-1.5 rounded-lg hover:bg-surface/70"><X weight="bold" size={24} /></button>
          </div>
          <div className="px-6 overflow-y-auto flex-1 space-y-3 pb-6">
            {(() => {
              const data = store.achievementsData;
              if (!data) return <p className="text-textMuted text-center py-8">{t('common.loading', 'загрузка...')}</p>;
              const isOwnProfile = !store.achievementsViewUserId;
              const stats = data.stats || {};
              const unlocked = data.unlockedIds || [];

              const unlockedHiddenItems: AchievementDef[] = [];
              for (const id of unlocked) {
                const def = getAchievementDef(id);
                if (def && def.category === 'hidden') {
                  unlockedHiddenItems.push(def);
                }
              }

              const categoryOrder: Record<string, number> = { voice: 0, calls: 1, social: 2, hidden: 3 };

              const bandOf = (a: AchievementDef) =>
                a.hidden ? (unlocked.includes(a.id) ? 0 : 2) : 1;

              const knownItems = [...ACHIEVEMENTS, ...unlockedHiddenItems];

              const filtered = knownItems
                .sort((a, b) => {
                  const bandDiff = bandOf(a) - bandOf(b);
                  if (bandDiff !== 0) return bandDiff;

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

              const totalHidden = data.totalHiddenCount ?? 10;
              const lockedHiddenCount = isOwnProfile ? Math.max(0, totalHidden - unlockedHiddenItems.length) : 0;

              if (filtered.length === 0 && lockedHiddenCount === 0) return <p className="text-textMuted text-center py-8 font-medium">{t('achievements.empty', 'нет достижений')}</p>;

              return (
                <>
                  {filtered.map(a => {
                    const isUnlocked = unlocked.includes(a.id);
                    const statVal = stats[a.statKey] ?? 0;
                    const effectiveStatVal = isUnlocked ? Math.max(statVal, a.maxValue) : statVal;
                    const progress = getProgressPercent(effectiveStatVal, a.maxValue, a.unit);
                    return (
                      <div key={a.id} className={`p-4 rounded-xl border transition-colors ${isUnlocked ? 'bg-primary/10 border-primary/30' : 'glass-row border-transparent'}`}>
                        <div className="flex items-center gap-4">
                          <span className="text-3xl">{a.icon}</span>
                          <div className="flex-1 min-w-0">
                            <div className="flex items-center gap-2 mb-1">
                              <span className="font-bold text-white truncate">{t(`achievements.${a.id}.title`, a.title)}</span>
                              {a.hidden && (
                                <span className="text-[10px] font-bold bg-white/10 text-white/80 px-2 py-0.5 rounded-md shrink-0">{t('achievements.hiddenBadge', 'скрытое')}</span>
                              )}
                              {isUnlocked && <span className="text-[10px] font-bold bg-primary/20 text-primaryText px-2 py-0.5 rounded-md shrink-0">{t('achievements.unlocked', '✓ получено')}</span>}
                            </div>
                            <p className="text-textMuted text-sm truncate">{t(`achievements.${a.id}.description`, a.description)}</p>
                            <div className="mt-2 flex items-center gap-3">
                              <div className="flex-1 h-1.5 bg-black/30 rounded-full overflow-hidden">
                                <div className="h-full rounded-full transition-all duration-500" style={{ width: `${progress * 100}%`, backgroundColor: isUnlocked ? '#C81E70' : '#555' }} />
                              </div>
                              <span className="text-xs text-textMuted font-mono shrink-0">{formatProgress(effectiveStatVal, a.maxValue, a.unit)}</span>
                            </div>
                          </div>
                        </div>
                      </div>
                    );
                  })}
                  {Array.from({ length: lockedHiddenCount }).map((_, idx) => (
                    <div key={`locked_hidden_${idx}`} className="p-4 rounded-xl border transition-colors glass-row border-transparent">
                      <div className="flex items-center gap-4">
                        <span className="text-3xl blur-sm">❓</span>
                        <div className="flex-1 min-w-0">
                          <div className="flex items-center gap-2 mb-1">
                            <span className="font-bold text-white truncate">{t('achievements.hiddenTitle', 'скрытое достижение')}</span>
                            <span className="text-[10px] font-bold bg-white/10 text-white/80 px-2 py-0.5 rounded-md shrink-0">{t('achievements.hiddenBadge', 'скрытое')}</span>
                          </div>
                          <p className="text-textMuted text-sm truncate">{t('achievements.hiddenDesc', '???')}</p>
                        </div>
                      </div>
                    </div>
                  ))}
                </>
              );
            })()}
          </div>
          <div className="p-4 pt-0 text-center">
            <span className="text-xs text-textMuted">{t('achievements.summary', { unlocked: store.achievementsData?.unlockedIds?.length ?? 0, total: ACHIEVEMENTS.length + (store.achievementsData?.totalHiddenCount ?? 10), defaultValue: '{{unlocked}} / {{total}} получено' })}</span>
          </div>
        </div>
      )}

      {renderModal('update',
        <UpdateModal onClose={() => store.setModal('update', false)} />
      )}

      {offlineToast && createPortal(
        <div className="fixed bottom-10 left-1/2 -translate-x-1/2 z-[1000000] animate-toast-in">
          <div className="bg-[#09090B]/85 backdrop-blur-2xl border border-danger/40 rounded-modal px-8 py-5 flex items-center gap-4">
            <div className="w-10 h-10 rounded-full bg-danger/20 flex items-center justify-center shrink-0">
              {offlineToast === t('toasts.noAnswer', 'не отвечает') ? (
                <PhoneOff weight="bold" size={20} className="text-danger" />
              ) : (
                <UserCircleMinus weight="bold" size={20} className="text-danger" />
              )}
            </div>
            <div>
              <p className="text-white font-bold text-base leading-tight">{t('toasts.notification', 'уведомление')}</p>
              <p className="text-danger/90 font-medium text-sm mt-0.5">{offlineToast}</p>
            </div>
          </div>
        </div>,
        document.body
      )}

      {store.systemToast && createPortal(
        <div className="fixed bottom-10 left-1/2 -translate-x-1/2 z-[1000000] animate-toast-in">
          <div className="bg-[#09090B]/85 backdrop-blur-2xl border border-warning/40 rounded-modal px-8 py-5 flex items-center gap-4">
            <div className="w-10 h-10 rounded-full bg-warning/20 flex items-center justify-center shrink-0">
              <MicOff weight="bold" size={20} className="text-warning" />
            </div>
            <div>
              <p className="text-white font-bold text-base leading-tight">{t('toasts.notification', 'уведомление')}</p>
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
            <div className="bg-[#09090B]/85 backdrop-blur-2xl border border-danger/50 rounded-modal px-8 py-6 flex items-center gap-5">
              <div className="w-12 h-12 rounded-2xl bg-danger/20 flex items-center justify-center shrink-0">
                <MicOff weight="bold" size={24} className="text-danger" />
              </div>
              <div className="min-w-0">
                <p className="text-white font-black text-lg tracking-tight leading-none mb-1">{t('toasts.accessRestricted', 'доступ ограничен')}</p>
                <p className="text-textMuted font-bold text-sm truncate opacity-90">{typeof adminBlockToast === 'string' && adminBlockToast !== '__hiding__' ? adminBlockToast : t('toasts.adminRestricted', 'администратор запретил это действие')}</p>
              </div>
            </div>
          </div>
        );
      })(), document.body)}

      {renderCropper()}

      {contextMenu?.visible && (
        <div
          ref={contextMenuRef}
          className="fixed z-[999999] glass-sheet py-2 w-48 animate-fade-in select-none"
          style={{ top: contextMenu.y, left: contextMenu.x }}
          onClick={e => e.stopPropagation()}
          onContextMenu={e => e.stopPropagation()}
        >
          {contextMenu.type === 'channel' ? (
            <>
              {contextMenu.item.ownerId === store.currentUser?.id && (
                <button onClick={() => { setEditChannelId(contextMenu.item.id); setEditChannelName(contextMenu.item.name); store.setModal('channelEdit', true); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-white hover:bg-surfaceHover/80 flex items-center gap-3 font-medium"><Edit2 weight="bold" size={16} /> {t('contextMenu.rename', 'переименовать')}</button>
              )}
              <button onClick={() => { signalRService.quitAccessChannel(contextMenu.item.id); setContextMenu(null); }} className="group w-full text-left px-4 py-2 text-danger hover:bg-surfaceHover/80 flex items-center gap-3 font-medium mt-1">
                <div className="transition-transform duration-200 group-hover:translate-x-1">
                  <LeaveIcon weight="bold" size={16} />
                </div>
                {t('contextMenu.leaveChannel', 'выйти из канала')}</button>
            </>
          ) : contextMenu.type === 'channelMember' ? (
            <>
              <button onClick={() => { store.setSelectedProfileUser(contextMenu.item, 'channelMembers'); store.setModal('profile', true); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-white hover:bg-surfaceHover/80 flex items-center gap-3 font-medium"><Settings weight="bold" size={16} /> {t('contextMenu.profile', 'профиль')}</button>
              {contextMenu.item.id !== store.currentUser?.id && store.currentChannelId === store.selectedChannelForMembers?.id && (
                <button onClick={() => {
                  if (sentInvites.has(contextMenu.item.id)) return;
                  if (!contextMenu.item.isOnline) {
                    setOfflineToast(t('profile.userOffline', 'пользователь не в сети'));
                    setTimeout(() => setOfflineToast(null), 3000);
                  } else if (store.selectedChannelForMembers) {
                    signalRService.callToChannel(contextMenu.item.id, store.selectedChannelForMembers.id, store.selectedChannelForMembers.name);
                    addSentInvite(contextMenu.item.id);
                  }
                  setContextMenu(null);
                }}
                  disabled={sentInvites.has(contextMenu.item.id)}
                  className={`w-full text-left px-4 py-2 flex items-center gap-3 font-medium mt-1 ${sentInvites.has(contextMenu.item.id) ? 'text-success cursor-default animate-invite-pulse' : 'text-white hover:bg-surfaceHover/80'
                    }`}>
                  <Phone weight="bold" size={16} /> {sentInvites.has(contextMenu.item.id) ? t('contextMenu.inviting', 'зовём...') : t('contextMenu.invite', 'позвать в канал')}
                </button>
              )}
              {store.selectedChannelForMembers?.ownerId === store.currentUser?.id && contextMenu.item.id !== store.currentUser?.id && (
                <button onClick={() => { store.setUserToKick(contextMenu.item); store.setModal('kickConfirm', true); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-danger hover:bg-surfaceHover/80 flex items-center gap-3 font-medium mt-1"><UserX weight="bold" size={16} /> {t('contextMenu.kick', 'исключить')}</button>
              )}
            </>
          ) : contextMenu.type === 'voiceUser' ? (
            <>
              <button onClick={() => { setVolumeUser(contextMenu.item); setVolumeType('voice'); setVolumeUserValue(store.userVolumes[contextMenu.item.id] ?? 100); store.setModal('userVolume', true); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-white hover:bg-surfaceHover/80 flex items-center gap-3 font-medium"><Volume2 weight="bold" size={16} /> {t('contextMenu.volume', 'громкость')}</button>
              <button onClick={() => { store.setSelectedProfileUser(contextMenu.item, 'voiceUsers'); store.setModal('profile', true); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-white hover:bg-surfaceHover/80 flex items-center gap-3 font-medium mt-1"><Settings weight="bold" size={16} /> {t('contextMenu.profile', 'профиль')}</button>
            </>
          ) : contextMenu.type === 'stream' ? (
            <>
              <button onClick={() => { setVolumeUser(contextMenu.item); setVolumeType('stream'); setVolumeUserValue(store.streamVolumes[contextMenu.item.id] ?? 100); store.setModal('userVolume', true); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-white hover:bg-surfaceHover/80 flex items-center gap-3 font-medium"><Volume2 weight="bold" size={16} /> {t('contextMenu.streamVolume', 'громкость трансляции')}</button>
            </>
          ) : (
            <>
              <button onClick={() => { store.setSelectedProfileUser(contextMenu.item, 'friends'); store.setModal('profile', true); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-white hover:bg-surfaceHover/80 flex items-center gap-3 font-medium"><Settings weight="bold" size={16} /> {t('contextMenu.profile', 'профиль')}</button>
              <button onClick={() => { signalRService.removeFriend(contextMenu.item.id); setContextMenu(null); }} className="w-full text-left px-4 py-2 text-danger hover:bg-surfaceHover/80 flex items-center gap-3 font-medium mt-1"><UserMinus weight="bold" size={16} /> {t('contextMenu.remove', 'удалить')}</button>
            </>
          )}
        </div>
      )}

      {showStreamPicker && (
        <div className="fixed inset-0 z-[100000] bg-black/70 backdrop-blur-md flex items-center justify-center p-3 pt-[3.75rem]">
          <StreamPicker
            onClose={() => setShowStreamPicker(false)}
            onSelect={handleStartStream}
          />
        </div>
      )}
    </>
  );
}
