export type DiagnosticRecord = Record<string, string | number | boolean> & { event: string };

const events = new Set(['capture', 'connection', 'media', 'recovery', 'sfu', 'control', 'call', 'chat']);
const numbers = new Set(['connection', 'epoch', 'frames', 'bytes', 'packets', 'lost', 'rttMs', 'concealed', 'targetMs', 'width', 'height', 'attempt', 'generation', 'count']);
const strings: Record<string, Set<string>> = {
  route: new Set(['p2p', 'sfu', 'turn', 'signalr', 'unknown']),
  networkRoute: new Set(['p2p', 'turn', 'unknown']),
  transport: new Set(['udp', 'tcp', 'unknown']),
  state: new Set(['starting', 'connecting', 'connected', 'disconnected', 'failed', 'closed', 'stable', 'active', 'waiting', 'stalled', 'resuming', 'resumed', 'ended', 'cancelled']),
  reason: new Set(['first-frame-timeout', 'video-stall', 'ice-failed', 'disconnected', 'manual', 'ended', 'busy', 'denied', 'unavailable', 'native-exit', 'keyframe']),
  sourceKind: new Set(['screen', 'window', 'camera']),
  mediaKind: new Set(['video', 'audio'])
};

export function sanitizeDiagnostic(value: unknown): DiagnosticRecord | null {
  if (!value || typeof value !== 'object') return null;
  const input = value as Record<string, unknown>;
  if (typeof input.event !== 'string' || !events.has(input.event)) return null;
  const result: DiagnosticRecord = { event: input.event };
  for (const [key, value] of Object.entries(input)) {
    if (numbers.has(key) && typeof value === 'number' && Number.isFinite(value) && value >= 0 && value <= Number.MAX_SAFE_INTEGER) result[key] = Math.round(value * 1000) / 1000;
    else if (typeof value === 'string' && strings[key]?.has(value)) result[key] = value;
    else if (['muted', 'deafened', 'ultra', 'streaming'].includes(key) && typeof value === 'boolean') result[key] = value;
  }
  return result;
}
