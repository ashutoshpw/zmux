type Entry = { count: number; resetAt: number };

export class FixedWindowRateLimiter {
  private readonly entries = new Map<string, Entry>();

  constructor(
    private readonly limit: number,
    private readonly windowMs: number,
    private readonly maxKeys = 10_000,
  ) {}

  allow(key: string, now = Date.now()): boolean {
    const current = this.entries.get(key);
    if (!current || current.resetAt <= now) {
      if (this.entries.size >= this.maxKeys) this.prune(now);
      if (this.entries.size >= this.maxKeys) this.entries.delete(this.entries.keys().next().value ?? "");
      this.entries.set(key, { count: 1, resetAt: now + this.windowMs });
      return true;
    }
    current.count += 1;
    return current.count <= this.limit;
  }

  private prune(now: number): void {
    for (const [key, entry] of this.entries) {
      if (entry.resetAt <= now) this.entries.delete(key);
    }
  }
}
