

import { create } from 'zustand';

export interface User {
  id: string;
  username: string;
  displayName: string;
  avatarBase64: string | null;
  avatarColor: string;
  aboutMe?: string;
  isOnline: boolean;

  
  isMuted: boolean;
  isDeafened: boolean;

  
  isServerMuted?: boolean;
  isServerDeafened?: boolean;

  isSpeaking: boolean;
  currentChannelId?: string | null;
  currentCallUserId?: string | null;
  lastSeen?: string;
}

export interface VoiceChannel {
  id: string;
  name: string;
  ownerId: string;
  maxUsers?: number;
  createdAt?: string;
}

export interface ChannelInvite {
  senderId: string;
  senderName: string;
  channelId: string;
  channelName: string;
}

export interface UserStateUpdate {
  userId: string;
  isMuted?: boolean;
  isDeafened?: boolean;
  isSpeaking?: boolean;
}

export interface ChannelUpdate {
  channel: VoiceChannel;
  users: User[];
}

export interface IncomingCall {
  callerId: string;
  callerName: string;
  callerAvatarColor: string;
  callerAvatarBase64?: string | null;
}

interface AppState {
  currentUser: User | null;
  channels: VoiceChannel[];
  friends: User[];
  friendRequests: User[];
  channelInvites: ChannelInvite[];
  voiceUsers: User[];
  currentChannelId: string | null;

  channelUsersMap: Record<string, User[]>;

  channelMembers: User[];
  channelMembersCache: Record<string, User[]>;
  selectedChannelForMembers: VoiceChannel | null;
  userToKick: User | null;

  incomingCall: IncomingCall | null;
  currentCallUser: User | null;
  callStatus: 'idle' | 'calling' | 'connected';

  isJoiningChannel: boolean;
  userVolumes: Record<string, number>;
  noiseSuppression: boolean;
  setNoiseSuppression: (enabled: boolean) => void;
  webrtcConnections: Record<string, boolean>;
  pendingChannelSwitch: string | null;
  setPendingChannelSwitch: (channelId: string | null) => void;

  achievementToast: string | null;
  achievementsData: { stats: Record<string, number>; unlockedIds: string[] } | null;
  achievementsViewUserId: string | null;
  setAchievementToast: (id: string | null) => void;
  setAchievementsData: (data: { stats: Record<string, number>; unlockedIds: string[] } | null) => void;
  setAchievementsViewUserId: (id: string | null) => void;

  setWebRTCConnectionStatus: (userId: string, isConnected: boolean) => void;

  setCurrentUser: (user: User | null) => void;
  setChannels: (channels: VoiceChannel[]) => void;
  setFriendRequests: (reqs: User[]) => void;
  setChannelInvites: (invites: ChannelInvite[]) => void;
  setVoiceUsers: (users: User[]) => void;
  setCurrentChannelId: (id: string | null) => void;
  setIsJoiningChannel: (isJoining: boolean) => void;
  
  systemToast: string | null;
  setSystemToast: (msg: string | null) => void;

  setFriends: (friends: User[]) => void;

  setChannelUsers: (channelId: string, users: User[]) => void;
  setFullChannelState: (stateMap: Record<string, User[]>) => void;
  addUserToChannelMap: (channelId: string, user: User) => void;
  removeUserFromChannelMap: (channelId: string, userId: string) => void;

  setChannelMembers: (users: User[]) => void;
  setChannelMembersCache: (channelId: string, users: User[]) => void;
  setSelectedChannelForMembers: (ch: VoiceChannel | null) => void;
  setUserToKick: (u: User | null) => void;

  setIncomingCall: (call: IncomingCall | null) => void;
  setCurrentCallUser: (user: User | null) => void;
  setCallStatus: (status: 'idle' | 'calling' | 'connected') => void;
  setUserVolume: (userId: string, volume: number) => void;

  updateUserStatus: (userId: string, updates: Partial<User>) => void;
  setSpeakingStatus: (userId: string, isSpeaking: boolean) => void;

  modals: {
    settings: boolean;
    privacy: boolean;
    addFriend: boolean;
    createChannel: boolean;
    profile: boolean;
    inviteToChannel: boolean;
    channelEdit: boolean;
    userVolume: boolean;
    incomingCall: boolean;
    channelFull: boolean;
    channelMembers: boolean;
    kickConfirm: boolean;
    channelSwitch: boolean;
    achievements: boolean;
    adminConsole: boolean;
    adminUserSettings: boolean;
    incomingChannelInvite: boolean;
  };

  setModal: (modalName: keyof AppState['modals'], isOpen: boolean) => void;
  closeAllModals: () => void;
  closeProfileOnly: () => void;

  selectedProfileUser: User | null;
  profileSource: 'friends' | 'channelMembers' | 'voiceUsers' | 'none';
  setSelectedProfileUser: (user: User | null, source?: 'friends' | 'channelMembers' | 'voiceUsers' | 'none') => void;

  selectedChannelForInvite: VoiceChannel | null;
  setSelectedChannelForInvite: (ch: VoiceChannel | null) => void;

  incomingChannelInvite: ChannelInvite | null;
  setIncomingChannelInvite: (invite: ChannelInvite | null) => void;
}

const updateUserInList = (list: User[], userId: string, updates: Partial<User>): User[] => {
  let changed = false;
  const next = list.map(user => {
    if (user.id !== userId) return user;
    changed = true;
    return { ...user, ...updates };
  });
  return changed ? next : list;
};

export const useAppStore = create<AppState>((set) => ({
  currentUser: null,
  channels: [],
  friends: [],
  friendRequests: [],
  channelInvites: [],
  voiceUsers: [],
  currentChannelId: null,

  channelUsersMap: {},

  channelMembers: [],
  channelMembersCache: {},
  selectedChannelForMembers: null,
  userToKick: null,

  incomingCall: null,
  currentCallUser: null,
  callStatus: 'idle',

  isJoiningChannel: false,
  userVolumes: {},
  noiseSuppression: true,
  setNoiseSuppression: (enabled) => set({ noiseSuppression: enabled }),
  webrtcConnections: {},

  pendingChannelSwitch: null,
  setPendingChannelSwitch: (channelId) => set({ pendingChannelSwitch: channelId }),

  achievementToast: null,
  achievementsData: null,
  achievementsViewUserId: null,
  setAchievementToast: (id) => set({ achievementToast: id }),
  setAchievementsData: (data) => set({ achievementsData: data }),
  setAchievementsViewUserId: (id) => set({ achievementsViewUserId: id }),

  systemToast: null,
  setSystemToast: (msg) => set({ systemToast: msg }),

  setCurrentUser: (user) => set({ currentUser: user }),
  setChannels: (channels) => set({ channels }),
  setFriends: (friends) => set({ friends }),
  setFriendRequests: (reqs) => set({ friendRequests: reqs }),
  setChannelInvites: (invites) => set({ channelInvites: invites }),
  setVoiceUsers: (users) => set({ voiceUsers: users }),
  setCurrentChannelId: (id) => set({ currentChannelId: id }),
  setIsJoiningChannel: (isJoining) => set({ isJoiningChannel: isJoining }),

  setChannelUsers: (channelId, users) => set((state) => ({
    channelUsersMap: { ...state.channelUsersMap, [channelId]: [...users] },
    voiceUsers: state.currentChannelId === channelId ? [...users] : state.voiceUsers
  })),

  setFullChannelState: (stateMap) => set((state) => {
    const currentChannelId = state.currentChannelId;
    const currentChannelUsers = currentChannelId ? (stateMap[currentChannelId] || []) : [];
    return {
      channelUsersMap: stateMap,
      voiceUsers: currentChannelId ? currentChannelUsers : state.voiceUsers
    };
  }),

  addUserToChannelMap: (channelId, user) => set((state) => {
    const current = state.channelUsersMap[channelId] || [];
    if (current.some(u => u.id === user.id)) return state;

    const nextChannelUsers = [...current, user];
    return {
      channelUsersMap: {
        ...state.channelUsersMap,
        [channelId]: nextChannelUsers,
      },
      voiceUsers: state.currentChannelId === channelId
        ? state.voiceUsers.some(u => u.id === user.id)
          ? state.voiceUsers
          : [...state.voiceUsers, user]
        : state.voiceUsers
    };
  }),

  removeUserFromChannelMap: (channelId, userId) => set((state) => {
    if (channelId) {
      const list = state.channelUsersMap[channelId];
      if (!list || !list.some(u => u.id === userId)) return state;

      const nextList = list.filter(u => u.id !== userId);

      return {
        channelUsersMap: {
          ...state.channelUsersMap,
          [channelId]: nextList,
        },
        voiceUsers: state.currentChannelId === channelId
          ? state.voiceUsers.filter(u => u.id !== userId)
          : state.voiceUsers
      };
    }

    let changed = false;
    const newMap: Record<string, User[]> = {};
    for (const [key, users] of Object.entries(state.channelUsersMap)) {
      const filtered = users.filter(u => u.id !== userId);
      if (filtered.length !== users.length) changed = true;
      newMap[key] = filtered;
    }

    return changed
      ? {
          channelUsersMap: newMap,
          voiceUsers: state.voiceUsers.filter(u => u.id !== userId)
        }
      : state;
  }),

  setChannelMembers: (users) => set({ channelMembers: users }),
  setChannelMembersCache: (channelId, users) => set((state) => ({
    channelMembersCache: { ...state.channelMembersCache, [channelId]: users }
  })),
  setSelectedChannelForMembers: (ch) => set({ selectedChannelForMembers: ch }),
  setUserToKick: (u) => set({ userToKick: u }),

  setIncomingCall: (call) => set({ incomingCall: call }),
  setCurrentCallUser: (user) => set({ currentCallUser: user }),
  setCallStatus: (status) => set({ callStatus: status }),

  setUserVolume: (userId, volume) => set((state) => ({
    userVolumes: { ...state.userVolumes, [userId]: volume }
  })),

  setWebRTCConnectionStatus: (userId, isConnected) => set((state) => ({
    webrtcConnections: { ...state.webrtcConnections, [userId]: isConnected }
  })),

  setSpeakingStatus: (userId, isSpeaking) => set((state) => {
  // Только обновляем коллекции, где пользователь реально присутствует
  const voiceUsers = updateUserInList(state.voiceUsers, userId, { isSpeaking });

  let currentCallUser = state.currentCallUser;
  if (currentCallUser?.id === userId) {
    currentCallUser = { ...currentCallUser, isSpeaking };
  }

  let currentUser = state.currentUser;
  if (currentUser?.id === userId) {
    currentUser = { ...currentUser, isSpeaking };
  }

  // channelUsersMap — обновляем только текущий канал
  let channelUsersMap = state.channelUsersMap;
  if (state.currentChannelId) {
    const channelUsers = state.channelUsersMap[state.currentChannelId];
    if (channelUsers?.some(u => u.id === userId)) {
      channelUsersMap = {
        ...state.channelUsersMap,
        [state.currentChannelId]: updateUserInList(channelUsers, userId, { isSpeaking })
      };
    }
  }

  return {
    voiceUsers,
    currentCallUser,
    currentUser,
    channelUsersMap
  };
}),

  updateUserStatus: (userId, updates) => set((state) => {
    const voiceUsers = updateUserInList(state.voiceUsers, userId, updates);
    const friends = updateUserInList(state.friends, userId, updates);
    const friendRequests = updateUserInList(state.friendRequests, userId, updates);
    const channelMembers = updateUserInList(state.channelMembers, userId, updates);

    const channelUsersMap = Object.fromEntries(
      Object.entries(state.channelUsersMap).map(([channelId, users]) => [
        channelId,
        updateUserInList(users, userId, updates)
      ])
    );

    const channelMembersCache = Object.fromEntries(
      Object.entries(state.channelMembersCache).map(([channelId, users]) => [
        channelId,
        updateUserInList(users, userId, updates)
      ])
    );

    const currentUser = state.currentUser?.id === userId
      ? { ...state.currentUser, ...updates }
      : state.currentUser;

    const currentCallUser = state.currentCallUser?.id === userId
      ? { ...state.currentCallUser, ...updates }
      : state.currentCallUser;

    const selectedProfileUser = state.selectedProfileUser?.id === userId
      ? { ...state.selectedProfileUser, ...updates }
      : state.selectedProfileUser;

    return {
      voiceUsers,
      friends,
      friendRequests,
      channelMembers,
      channelMembersCache,
      channelUsersMap,
      currentUser,
      currentCallUser,
      selectedProfileUser
    };
  }),

  modals: {
    settings: false,
    privacy: false,
    addFriend: false,
    createChannel: false,
    profile: false,
    inviteToChannel: false,
    channelEdit: false,
    userVolume: false,
    incomingCall: false,
    channelFull: false,
    channelMembers: false,
    kickConfirm: false,
    channelSwitch: false,
    achievements: false,
    adminConsole: false,
    adminUserSettings: false,
    incomingChannelInvite: false,
  },

  setModal: (name, isOpen) => set((state) => ({
    modals: { ...state.modals, [name]: isOpen }
  })),

  closeAllModals: () => set({
    modals: {
      settings: false,
      privacy: false,
      addFriend: false,
      createChannel: false,
      profile: false,
      inviteToChannel: false,
      channelEdit: false,
      userVolume: false,
      incomingCall: false,
      channelFull: false,
      channelMembers: false,
      kickConfirm: false,
      channelSwitch: false,
      achievements: false,
      adminConsole: false,
      adminUserSettings: false,
      incomingChannelInvite: false,
    },
    pendingChannelSwitch: null
  }),

  closeProfileOnly: () => set((state) => ({
    modals: { ...state.modals, profile: false }
  })),

  selectedProfileUser: null,
  profileSource: 'none',
  setSelectedProfileUser: (user, source = 'none') => set({ selectedProfileUser: user, profileSource: source }),

  selectedChannelForInvite: null,
  setSelectedChannelForInvite: (ch) => set({ selectedChannelForInvite: ch }),

  incomingChannelInvite: null,
  setIncomingChannelInvite: (invite) => set({ incomingChannelInvite: invite })
}));