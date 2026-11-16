import type { Asset } from "../domain/catalog";
import { clearThumbs } from "./tauriApi";
import { ThumbQueue, type ThumbQueueCallbacks } from "./thumbQueue";

/** One background worker shared by automatic generation, refresh and pause. */
export class ThumbnailJobs {
  private assets = new Map<string, Asset>();
  private pending = new Set<string>();
  private queue?: ThumbQueue;
  private worker?: Promise<void>;
  private paused = false;
  private closed = false;
  private transitions: Promise<void> = Promise.resolve();

  constructor(private callbacks: ThumbQueueCallbacks, private onError: (error: unknown) => void) {}

  updateAssets(assets: Asset[]) {
    this.assets = new Map(assets.map(asset => [asset.id, asset]));
    for (const id of this.pending) if (!this.assets.has(id)) this.pending.delete(id);
  }

  enqueue(ids: string[], resume = false) {
    if (this.closed) return;
    for (const id of ids) if (this.assets.has(id)) this.pending.add(id);
    if (resume) this.paused = false;
    this.pump();
  }

  pause() { this.paused = true; this.queue?.pause(); }
  isPaused() { return this.paused; }

  async suspend() {
    this.paused = true;
    await this.transitions;
    this.paused = true;
    await this.queue?.teardown().catch(() => {}); // The worker reports any failure.
    await this.worker;
  }

  /** Serialize preference changes so an older render cannot overwrite a newer one. */
  regenerate(ids: string[]): Promise<void> {
    const operation = this.transitions.then(async () => {
      if (this.closed || !ids.length) return;
      this.paused = true;
      await this.queue?.teardown().catch(() => {});
      await this.worker;
      try {
        await clearThumbs(ids, true);
        for (const id of ids) if (this.assets.has(id)) this.pending.add(id);
      } finally {
        if (!this.closed) { this.paused = false; this.pump(); }
      }
    });
    this.transitions = operation.catch(error => this.onError(error));
    return this.transitions;
  }

  async stop() {
    this.closed = true;
    this.paused = true;
    await this.queue?.teardown().catch(() => {});
    await this.worker;
    await this.transitions;
    this.pending.clear();
  }

  private pump() {
    if (this.worker || this.closed || this.paused || !this.pending.size) return;
    this.worker = Promise.resolve().then(async () => {
      while (!this.closed && !this.paused && this.pending.size) {
        const ids = [...this.pending].filter(id => this.assets.has(id));
        this.pending.clear();
        if (!ids.length) break;
        const queue = new ThumbQueue(this.assets, ids, this.callbacks);
        this.queue = queue;
        try { await queue.start(); }
        finally {
          for (const id of queue.remainingIds()) this.pending.add(id);
          this.queue = undefined;
        }
      }
    }).catch(error => { this.paused = true; this.onError(error); }).finally(() => {
      this.worker = undefined;
      this.pump();
    });
  }
}
