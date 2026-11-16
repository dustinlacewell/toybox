/**
 * Bounded, resumable thumbnail generation. The work list comes from
 * `listPendingThumbs` (disk is the source of truth, so a crash/restart simply
 * resumes). Prepare up to three assets concurrently while a single renderer
 * captures and saves in order. Pause/teardown drain and release unused scenes.
 */

import type { Asset } from "../domain/catalog";
import { saveThumb, setThumbState } from "./tauriApi";
import { ThumbRenderer } from "./thumbRenderer";

type Prepared = { subject: Awaited<ReturnType<ThumbRenderer["prepare"]>> } | { error: unknown };
const PREPARE_LIMIT = 3;

export interface ThumbProgress {
  done: number;
  total: number;
  running: boolean;
  currentName: string | null;
}

export interface ThumbQueueCallbacks {
  onProgress: (p: ThumbProgress) => void;
  /** Reflect a single asset's new thumb state into the in-memory catalog. */
  onAssetState: (id: string, state: "rendering" | "ready" | "error", error?: string) => void;
}

export class ThumbQueue {
  private renderer: ThumbRenderer | null = null;
  private running = false;
  private cancelled = false;
  private done = 0;
  private total = 0;
  private currentName: string | null = null;
  private activeRun: Promise<void> | null = null;
  private disposeRequested = false;

  constructor(
    private readonly assetsById: Map<string, Asset>,
    private readonly pendingIds: string[],
    private readonly cb: ThumbQueueCallbacks,
  ) {
    this.total = pendingIds.length;
  }

  isRunning(): boolean {
    return this.running;
  }

  remainingIds(): string[] { return this.pendingIds.slice(this.done); }

  /** Begin (or resume) processing. Safe to call again after pause. */
  start(): Promise<void> {
    if (this.activeRun) return this.activeRun;
    this.activeRun = this.run().finally(() => { this.activeRun = null; });
    return this.activeRun;
  }

  private async run(): Promise<void> {
    this.running = true;
    this.cancelled = false;
    this.disposeRequested = false;
    this.renderer ??= new ThumbRenderer();
    const renderer = this.renderer;
    const prepared = new Map<number, Promise<Prepared>>();
    const fill = () => {
      for (let i = this.done; i < Math.min(this.done + PREPARE_LIMIT, this.total); i++) {
        if (prepared.has(i)) continue;
        const asset = this.assetsById.get(this.pendingIds[i]);
        if (asset) prepared.set(i, Promise.resolve().then(() => renderer.prepare(asset))
          .then(subject => ({ subject }), error => ({ error })));
      }
    };
    this.emit();

    try {
      while (this.done < this.total) {
        if (this.cancelled) break;
        fill();
        const item = prepared.get(this.done);
        prepared.delete(this.done);
        if (item && !await this.renderOne(this.pendingIds[this.done], item, renderer)) break;
        this.done++;
        this.emit();
        await yieldToEventLoop();
      }
    } finally {
      // In-flight reads cannot be cancelled, but their scenes must not leak or
      // outlive the renderer used by material preparation.
      await Promise.all([...prepared.values()].map(async item => {
        const result = await item;
        if ("subject" in result) renderer.discard(result.subject);
      }));
      this.running = false;
      this.currentName = null;
      this.emit();
      if (this.disposeRequested || this.done >= this.total) this.releaseRenderer();
    }
  }

  /** Pause after the current item; rendering can be resumed with start(). */
  pause(): void {
    this.cancelled = true;
  }

  /** Stop and release the GL context. */
  async teardown(): Promise<void> {
    this.cancelled = true;
    this.disposeRequested = true;
    await this.activeRun;
    this.releaseRenderer();
  }

  private releaseRenderer(): void {
    this.renderer?.dispose();
    this.renderer = null;
  }

  private async renderOne(id: string, prepared: Promise<Prepared>, renderer: ThumbRenderer): Promise<boolean> {
    const asset = this.assetsById.get(id);
    if (!asset) return true;
    this.currentName = asset.name;
    this.emit();
    const result = await prepared;
    if (this.cancelled) {
      if ("subject" in result) renderer.discard(result.subject);
      return false;
    }
    this.cb.onAssetState(id, "rendering");
    this.emit();

    try {
      if ("error" in result) throw result.error;
      const png = await renderer.renderPrepared(result.subject);
      await saveThumb(id, png); // Publishes the complete PNG before notifying cards.
      this.cb.onAssetState(id, "ready");
    } catch (e) {
      await this.fail(id, String(e));
    }
    return true;
  }

  private async fail(id: string, msg: string): Promise<void> {
    await setThumbState(id, "error", msg);
    this.cb.onAssetState(id, "error", msg);
  }

  private emit(): void {
    this.cb.onProgress({
      done: this.done,
      total: this.total,
      running: this.running,
      currentName: this.currentName,
    });
  }
}

function yieldToEventLoop(): Promise<void> {
  return new Promise((resolve) => {
    const ric = (window as unknown as { requestIdleCallback?: (cb: () => void, options: { timeout: number }) => void })
      .requestIdleCallback;
    if (ric) ric(() => resolve(), { timeout: 50 });
    else setTimeout(resolve, 0);
  });
}
