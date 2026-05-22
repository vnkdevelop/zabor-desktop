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
}

export const ACHIEVEMENTS: AchievementDef[] = [

  {
    id: 'first_channel',
    title: 'Первооткрыватель',
    description: 'Создать свой первый канал',
    icon: '🔭',
    maxValue: 1,
    statKey: 'channelsCreated',
    category: 'voice'
  },
  {
    id: 'soul_1',
    title: 'Болтун',
    description: 'Провести 10 часов в каналах',
    icon: '🗣️',
    maxValue: 600,
    statKey: 'totalVoiceMinutes',
    category: 'voice',
    unit: 'min'
  },
  {
    id: 'soul_2',
    title: 'Душа компании',
    description: 'Провести 50 часов в каналах',
    icon: '😜',
    maxValue: 3000,
    statKey: 'totalVoiceMinutes',
    category: 'voice',
    unit: 'min'
  },
  {
    id: 'soul_3',
    title: 'Оратор',
    description: 'Провести 100 часов в каналах',
    icon: '🎙️',
    maxValue: 6000,
    statKey: 'totalVoiceMinutes',
    category: 'voice',
    unit: 'min'
  },
  {
    id: 'crowd',
    title: 'Массовка',
    description: 'Быть в канале c 10 участниками',
    icon: '👥',
    maxValue: 10,
    statKey: 'maxUsersInChannel',
    category: 'voice'
  },
  {
    id: 'collector',
    title: 'Коллекционер',
    description: 'Побывать в 10 разных каналах',
    icon: '📚',
    maxValue: 10,
    statKey: 'uniqueChannels',
    category: 'voice'
  },
  {
    id: 'marathon',
    title: 'Тюлень',
    description: 'Сидеть в канале 5 часов',
    icon: '🦭',
    maxValue: 300,
    statKey: 'longestSessionMinutes',
    category: 'voice',
    unit: 'min'
  },
  {
    id: 'modnik',
    title: 'Модник',
    description: 'Загрузить GIF-аватарку',
    icon: '🪩',
    maxValue: 1,
    statKey: 'gifAvatarUploaded',
    category: 'voice'
  },
  {
    id: 'victim',
    title: 'Жертва',
    description: 'Быть кикнутым из канала',
    icon: '🥲',
    maxValue: 1,
    statKey: 'timesKicked',
    category: 'voice'
  },


  {
    id: 'first_call',
    title: 'Первый звонок',
    description: 'Совершить первый звонок',
    icon: '📞',
    maxValue: 1,
    statKey: 'totalCalls',
    category: 'calls'
  },
  {
    id: 'same_wave',
    title: 'Переговорщики',
    description: 'Звонок длительностью 2+ часа',
    icon: '🫂',
    maxValue: 120,
    statKey: 'longestCallMinutes',
    category: 'calls',
    unit: 'min'
  },
  {
    id: 'gossip',
    title: 'Мошенник',
    description: 'Совершить 50 звонков',
    icon: '🗿',
    maxValue: 50,
    statKey: 'totalCalls',
    category: 'calls'
  },
  {
    id: 'busy',
    title: 'Занят',
    description: 'Отклонить более 5 звонков',
    icon: '🚫',
    maxValue: 5,
    statKey: 'declinedCalls',
    category: 'calls'
  },


  {
    id: 'first_friend',
    title: 'Первый друг',
    description: 'Добавить первого друга',
    icon: '❤️',
    maxValue: 1,
    statKey: 'friendsCount',
    category: 'social'
  },
  {
    id: 'magnet',
    title: 'Магнит',
    description: 'Добавить 20 друзей',
    icon: '🧲',
    maxValue: 20,
    statKey: 'friendsCount',
    category: 'social'
  },
  {
    id: 'popular',
    title: 'Глава захолустья',
    description: 'Твой профиль просмотрели 100 раз',
    icon: '⭐',
    maxValue: 100,
    statKey: 'profileViews',
    category: 'social'
  },


  {
    id: 'night_owl',
    title: 'Ночная сова',
    description: 'Начать звонок после 2:00 ночи',
    icon: '🦉',
    maxValue: 1,
    statKey: 'nightCalls',
    category: 'hidden',
    hidden: true
  },
  {
    id: 'radio_silence',
    title: 'Тихоня',
    description: '30 минут в канале с выключенным микрофоном',
    icon: '🔇',
    maxValue: 1,
    statKey: 'mutedSessions',
    category: 'hidden',
    hidden: true
  },
  {
    id: 'early_bird',
    title: 'Ранняя пташка',
    description: 'Зайти в канал до 7 утра',
    icon: '🌅',
    maxValue: 1,
    statKey: 'earlyJoins',
    category: 'hidden',
    hidden: true
  },
  {
    id: 'introvert',
    title: 'Интроверт',
    description: 'Просидеть в канале 2 часа в полном одиночестве',
    icon: '🕯️',
    maxValue: 120,
    statKey: 'soloVoiceMinutes',
    category: 'hidden',
    hidden: true,
    unit: 'min'
  },
  {
    id: 'rostelecom',
    title: 'Ростелеком',
    description: 'Потерять соединение с сервером 5 раз за час',
    icon: '📡',
    maxValue: 5,
    statKey: 'disconnectsPerHour',
    category: 'hidden',
    hidden: true
  },
  {
    id: 'annoy_admin',
    title: 'Душнее душнилы',
    description: 'Задолбать даже админа',
    icon: '☠️',
    maxValue: 1,
    statKey: 'annoyAdmin',
    category: 'hidden',
    hidden: true
  }
];

export const getAchievementDef = (id: string) => ACHIEVEMENTS.find(a => a.id === id);

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