import i18n from './i18n';

export interface AchievementDef {
  id: string;
  title: string;
  description: string;
  icon: string;
  maxValue: number;
  statKey: string;
  category: 'voice' | 'calls' | 'social' | 'hidden';
  hidden?: boolean;
  unit?: 'min';
}

export interface AchievementsPayload {
  stats: Record<string, number>;
  unlockedIds: string[];
  visitedChannelIds: string[];
  totalHiddenCount?: number;
  unlockedHiddenDefs?: HiddenAchievementPayload[];
}

export const ACHIEVEMENTS: AchievementDef[] = [

  {
    id: 'first_channel',
    title: 'первооткрыватель',
    description: 'создать свой первый канал',
    icon: '🔭',
    maxValue: 1,
    statKey: 'channelsCreated',
    category: 'voice'
  },
  {
    id: 'soul_1',
    title: 'болтун',
    description: 'провести 10 часов в каналах',
    icon: '🗣️',
    maxValue: 600,
    statKey: 'totalVoiceMinutes',
    category: 'voice',
    unit: 'min'
  },
  {
    id: 'soul_2',
    title: 'душа компании',
    description: 'провести 50 часов в каналах',
    icon: '😜',
    maxValue: 3000,
    statKey: 'totalVoiceMinutes',
    category: 'voice',
    unit: 'min'
  },
  {
    id: 'soul_3',
    title: 'оратор',
    description: 'провести 100 часов в каналах',
    icon: '🎙️',
    maxValue: 6000,
    statKey: 'totalVoiceMinutes',
    category: 'voice',
    unit: 'min'
  },
  {
    id: 'crowd',
    title: 'массовка',
    description: 'быть в канале c 8 участниками',
    icon: '👥',
    maxValue: 8,
    statKey: 'maxUsersInChannel',
    category: 'voice'
  },
  {
    id: 'collector',
    title: 'коллекционер',
    description: 'побывать в 10 разных каналах',
    icon: '📚',
    maxValue: 10,
    statKey: 'uniqueChannels',
    category: 'voice'
  },

  {
    id: 'modnik',
    title: 'модник',
    description: 'загрузить GIF-аватарку',
    icon: '🪩',
    maxValue: 1,
    statKey: 'gifAvatarUploaded',
    category: 'voice'
  },
  {
    id: 'victim',
    title: 'жертва',
    description: 'быть кикнутым из канала',
    icon: '🥲',
    maxValue: 1,
    statKey: 'timesKicked',
    category: 'voice'
  },


  {
    id: 'first_call',
    title: 'первый звонок',
    description: 'совершить первый звонок',
    icon: '📞',
    maxValue: 1,
    statKey: 'totalCalls',
    category: 'calls'
  },
  {
    id: 'same_wave',
    title: 'переговорщики',
    description: 'звонок длительностью 5+ часов',
    icon: '🫂',
    maxValue: 300,
    statKey: 'longestCallMinutes',
    category: 'calls',
    unit: 'min'
  },
  {
    id: 'gossip',
    title: 'мошенник',
    description: 'совершить 50 звонков',
    icon: '🗿',
    maxValue: 50,
    statKey: 'totalCalls',
    category: 'calls'
  },
  {
    id: 'busy',
    title: 'занят',
    description: 'отклонить 5 звонков',
    icon: '🚫',
    maxValue: 5,
    statKey: 'declinedCalls',
    category: 'calls'
  },


  {
    id: 'first_friend',
    title: 'первый друг',
    description: 'добавить первого друга',
    icon: '❤️',
    maxValue: 1,
    statKey: 'friendsCount',
    category: 'social'
  },
  {
    id: 'magnet',
    title: 'магнит',
    description: 'добавить 20 друзей',
    icon: '🧲',
    maxValue: 20,
    statKey: 'friendsCount',
    category: 'social'
  },
  {
    id: 'popular',
    title: 'глава захолустья',
    description: 'твой профиль просмотрели 100 раз',
    icon: '⭐',
    maxValue: 100,
    statKey: 'profileViews',
    category: 'social'
  },


  // не не, читерить запрещено, иди отдыхай
];

export interface HiddenAchievementPayload {
  id: string;
  icon: string;
  maxValue: number;
  statKey: string;
  unit?: 'min';
  category?: 'voice' | 'calls' | 'social' | 'hidden';
  titleRu: string;
  descriptionRu: string;
  titleEn: string;
  descriptionEn: string;
}

const STORAGE_KEY = 'zabor_unlocked_hidden_achievements_v1';
const hiddenAchievementsCache = new Map<string, HiddenAchievementPayload>();

export const registerHiddenAchievementI18n = (def: HiddenAchievementPayload) => {
  if (!def || !def.id) return;
  i18n.addResourceBundle('ru', 'translation', {
    achievements: {
      [def.id]: {
        title: def.titleRu,
        description: def.descriptionRu
      }
    }
  }, true, true);

  i18n.addResourceBundle('en', 'translation', {
    achievements: {
      [def.id]: {
        title: def.titleEn,
        description: def.descriptionEn
      }
    }
  }, true, true);
};

export const registerHiddenAchievement = (def: HiddenAchievementPayload, persist: boolean = true) => {
  if (!def || !def.id) return;
  hiddenAchievementsCache.set(def.id, def);
  registerHiddenAchievementI18n(def);

  if (persist) {
    try {
      const stored = localStorage.getItem(STORAGE_KEY);
      const map: Record<string, HiddenAchievementPayload> = stored ? JSON.parse(stored) : {};
      map[def.id] = def;
      localStorage.setItem(STORAGE_KEY, JSON.stringify(map));
    } catch { }
  }
};

export const initHiddenAchievementsCache = () => {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (!stored) return;
    const map: Record<string, HiddenAchievementPayload> = JSON.parse(stored);
    for (const id of Object.keys(map)) {
      const def = map[id];
      if (def && def.id) {
        hiddenAchievementsCache.set(def.id, def);
        registerHiddenAchievementI18n(def);
      }
    }
  } catch { }
};

// Immediately hydrate cache from localStorage on module load
initHiddenAchievementsCache();

export const getAchievementDef = (id: string): AchievementDef | undefined => {
  const standard = ACHIEVEMENTS.find(a => a.id === id);
  if (standard) return standard;

  const hidden = hiddenAchievementsCache.get(id);
  if (hidden) {
    const isEn = i18n.language === 'en';
    return {
      id: hidden.id,
      title: isEn ? hidden.titleEn : hidden.titleRu,
      description: isEn ? hidden.descriptionEn : hidden.descriptionRu,
      icon: hidden.icon,
      maxValue: hidden.maxValue,
      statKey: hidden.statKey,
      category: 'hidden',
      hidden: true,
      unit: hidden.unit
    };
  }

  return undefined;
};

export const getCachedHiddenAchievement = (id: string): HiddenAchievementPayload | undefined => {
  return hiddenAchievementsCache.get(id);
};

export const formatProgress = (value: number, max: number, unit?: string): string => {
  const safeValue = Math.min(value ?? 0, max);

  if (unit === 'min') {
    const valH = Math.floor(safeValue / 60);
    const maxH = Math.floor(max / 60);
    return `${valH} / ${maxH}${i18n.t('achievements.hoursUnit', ' ч')}`;
  }

  return `${safeValue} / ${max}`;
};

export const getProgressPercent = (value: number, max: number, unit?: string): number => {
  const safeValue = Math.min(value ?? 0, max);

  if (unit === 'min') {
    const valH = Math.floor(safeValue / 60);
    const maxH = Math.floor(max / 60);
    if (maxH === 0) return 0;
    return Math.min(valH / maxH, 1);
  }

  if (max === 0) return 0;
  return Math.min(safeValue / max, 1);
};