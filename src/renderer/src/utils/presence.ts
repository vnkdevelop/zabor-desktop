type Translate = (key: string, options?: Record<string, unknown>) => string;

export const formatLastOnline = (
  iso: string | null | undefined,
  t: Translate,
  language: string
): string => {
  if (!iso) return t('presence.offline');

  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return t('presence.offline');

  const now = Date.now();
  const diff = now - then;

  if (diff < 60_000) return t('presence.justNow');

  const minutes = Math.floor(diff / 60_000);
  if (minutes < 60) return t('presence.minutesAgo', { count: minutes });

  const startOfDay = (ms: number): number => {
    const d = new Date(ms);
    d.setHours(0, 0, 0, 0);
    return d.getTime();
  };
  const dayDiff = Math.round((startOfDay(now) - startOfDay(then)) / 86_400_000);

  if (dayDiff <= 0) {
    const hours = Math.floor(diff / 3_600_000);
    return t('presence.hoursAgo', { count: hours });
  }

  if (dayDiff === 1) return t('presence.yesterday');

  const thenDate = new Date(then);
  const sameYear = thenDate.getFullYear() === new Date(now).getFullYear();
  const date = thenDate.toLocaleDateString(language, {
    day: 'numeric',
    month: 'long',
    year: sameYear ? undefined : 'numeric'
  });
  return t('presence.lastSeenDate', { date });
};
