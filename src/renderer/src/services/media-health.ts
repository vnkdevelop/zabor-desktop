export type MediaHealthAction = 'none' | 'keyframe' | 'recover';
type Sample = { frames: number; since: number; requested: boolean; recoveredAt: number };

export class MediaHealthTracker {
  private samples = new Map<string, Sample>();
  public observe(key: string, frames: number, now: number): MediaHealthAction {
    const previous = this.samples.get(key);
    if (!previous || previous.frames !== frames) {
      if (!previous && this.samples.size >= 256) this.samples.delete(this.samples.keys().next().value!);
      this.samples.set(key, { frames, since: now, requested: false, recoveredAt: previous?.recoveredAt ?? -15000 });
      return 'none';
    }
    const elapsed = now - previous.since;
    if (elapsed >= 12000 && previous.requested && now - previous.recoveredAt >= 15000) {
      previous.recoveredAt = now;
      previous.since = now;
      previous.requested = false;
      return 'recover';
    }
    if (elapsed >= 5000 && !previous.requested) {
      previous.requested = true;
      return 'keyframe';
    }
    return 'none';
  }
  public reset(): void { this.samples.clear(); }
}
