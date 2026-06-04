/**
 * Sequential, resumable thumbnail generation. The work list comes from
 * `listPendingThumbs` (disk is the source of truth, so a crash/restart simply
 * resumes). One asset is rendered at a time — responsiveness beats throughput
 * for this one-time job — yielding to the event loop between items so the UI
 * stays interactive. Per-asset state is pushed to Rust (catalog) and surfaced
 * via the progress callback.
 */

import type { Asset } from "../domain/catalog";
import { saveThumb, setThumbState } from "./tauriApi";
import { ThumbRenderer } from "./thumbRenderer";

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

  /** Begin (or resume) processing. Safe to call again after pause. */
  async start(): Promise<void> {
    if (this.running) return;
    this.running = true;
    this.cancelled = false;
    this.renderer ??= new ThumbRenderer();
    this.emit();

    try {
      for (const id of this.pendingIds.slice(this.done)) {
        if (this.cancelled) break;
        await this.renderOne(id);
        this.done++;
        this.emit();
        await yieldToEventLoop();
      }
    } finally {
      this.running = false;
      this.currentName = null;
      this.emit();
      if (this.done >= this.total) this.teardown();
    }
  }

  /** Pause after the current item; rendering can be resumed with start(). */
  pause(): void {
    this.cancelled = true;
  }

  /** Stop and release the GL context. */
  teardown(): void {
    this.cancelled = true;
    this.renderer?.dispose();
    this.renderer = null;
  }

  private async renderOne(id: string): Promise<void> {
    const asset = this.assetsById.get(id);
    if (!asset) return;
    this.currentName = asset.name;
    this.cb.onAssetState(id, "rendering");
    this.emit();

    try {
      const png = await this.renderer!.render(asset);
      await saveThumb(id, png); // also marks `ready` in the catalog (Rust)
      this.cb.onAssetState(id, "ready");
    } catch (e) {
      const msg = String(e);
      await setThumbState(id, "error", msg);
      this.cb.onAssetState(id, "error", msg);
    }
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
    const ric = (window as unknown as { requestIdleCallback?: (cb: () => void) => void })
      .requestIdleCallback;
    if (ric) ric(() => resolve());
    else setTimeout(resolve, 0);
  });
}
