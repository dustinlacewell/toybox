import type { Asset } from "../domain/catalog";
import { clearThumbs } from "./tauriApi";
import { ThumbQueue, type ThumbQueueCallbacks } from "./thumbQueue";

/** Items rendered per queue: their assets are fetched together and the queue reports progress over them. */
const BATCH = 100;

/** Where the work comes from when the window does not hold the library: ids to render (empty: no more) and the
 *  assets for ids it is about to render. */
export interface ThumbnailSource {
  /** The next ids still to render, a few hundred at most; empty when there are no more. */
  next(): Promise<string[]>;
  /** The assets with these ids (those that are gone are left out). */
  resolve(ids: string[]): Promise<Asset[]>;
}

/** One background worker shared by automatic generation, refresh and pause. */
export class ThumbnailJobs {
  private assets = new Map<string, Asset>();
  private pending = new Set<string>();
  private queue?: ThumbQueue;
  private worker?: Promise<void>;
  private paused = false;
  private closed = false;
  private transitions: Promise<void> = Promise.resolve();
  private source?: ThumbnailSource;
  private exhausted = true;
  private finished = 0;

  constructor(private callbacks: ThumbQueueCallbacks, private onError: (error: unknown) => void) {
    // Progress over the batch in hand counts on from the ones already done.
    const report = callbacks.onProgress;
    this.callbacks = { ...callbacks, onProgress: (progress) => report({ ...progress, done: progress.done + this.finished, total: progress.total + this.finished + this.pending.size }) };
  }

  /** Work from a library that is paged, not held: ids come from `source`, assets are fetched a batch at a time. */
  useSource(source: ThumbnailSource) {
    this.source = source;
    this.exhausted = false;
    this.pump();
  }

  /** Ask the source from the beginning again (items were added, or the work was asked for again). */
  restartSource(source?: ThumbnailSource) {
    if (source) this.source = source;
    this.exhausted = !this.source;
    this.finished = 0;
    this.pump();
  }

  updateAssets(assets: Asset[]) {
    this.assets = new Map(assets.map(asset => [asset.id, asset]));
    for (const id of this.pending) if (!this.assets.has(id)) this.pending.delete(id);
  }

  enqueue(ids: string[], resume = false) {
    if (this.closed) return;
    for (const id of ids) if (this.source || this.assets.has(id)) this.pending.add(id);
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
        for (const id of ids) if (this.source || this.assets.has(id)) this.pending.add(id);
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
    if (this.worker || this.closed || this.paused || (!this.pending.size && this.exhausted)) return;
    this.worker = Promise.resolve().then(async () => {
      while (!this.closed && !this.paused) {
        if (!this.pending.size && !this.exhausted && this.source) {
          const more = await this.source.next();
          if (!more.length) this.exhausted = true;
          for (const id of more) this.pending.add(id);
        }
        if (!this.pending.size) break;
        // One batch at a time: only its assets are fetched, and the queue's progress is over them.
        const ids = [...this.pending].slice(0, this.source ? BATCH : this.pending.size);
        for (const id of ids) this.pending.delete(id);
        if (this.source) {
          const missing = ids.filter(id => !this.assets.has(id));
          if (missing.length) for (const asset of await this.source.resolve(missing)) this.assets.set(asset.id, asset);
        }
        const known = ids.filter(id => this.assets.has(id));
        if (!known.length) continue;
        const queue = new ThumbQueue(this.assets, known, this.callbacks);
        this.queue = queue;
        try { await queue.start(); }
        finally {
          const left = queue.remainingIds();
          for (const id of left) this.pending.add(id);
          this.finished += known.length - left.length;
          this.queue = undefined;
          if (this.source) for (const id of known) if (!left.includes(id)) this.assets.delete(id);
        }
      }
    }).catch(error => { this.paused = true; this.onError(error); }).finally(() => {
      this.worker = undefined;
      this.pump();
    });
  }
}
