/**
 * Webhook deliveries already acted on, by ID, for a while: per instance and bounded, so at
 * worst a retry acts twice, which is harmless. A delivery whose handling failed is
 * forgotten, so Hypha's retry acts on it.
 */
export class Deliveries {
  #until = new Map<string, number>();
  readonly #keepMs: number;
  readonly #max: number;
  readonly #now: () => number;

  constructor(keepMs = 10 * 60 * 1000, max = 5_000, now = () => Date.now()) {
    this.#keepMs = keepMs;
    this.#max = max;
    this.#now = now;
  }

  /** Runs `act` unless `delivery` was acted on recently (then `undefined`). */
  async once<T>(delivery: string | null, act: () => Promise<T>): Promise<T | undefined> {
    if (!delivery) return act();
    const now = this.#now();
    if (this.#until.size > this.#max) {
      for (const [key, until] of this.#until) if (until <= now) this.#until.delete(key);
    }
    const until = this.#until.get(delivery);
    if (until && until > now) return undefined;
    this.#until.set(delivery, now + this.#keepMs);
    try {
      return await act();
    } catch (error) {
      this.#until.delete(delivery);
      throw error;
    }
  }
}
