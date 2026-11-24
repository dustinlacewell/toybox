import { useCallback, useEffect, useRef, useState } from "react";
import type { AssetKind } from "../domain/catalog";
import { NO_RULES, toBrowseFilter } from "../domain/browseQuery";
import { emptyFilter } from "../domain/facets";
import { getAssets, pendingThumbs, queryIds } from "../services/browse";
import type { ThumbProgress } from "../services/thumbQueue";
import { ThumbnailJobs, type ThumbnailSource } from "../services/thumbnailJobs";
import { cacheAssets, cachedAsset, patchCachedAsset } from "../state/assetCache";
import { useStore } from "../state/store";
import { usePreviewPreferences } from "../state/previewPreferences";
import { registerUpdateParticipant } from "../services/updateLifecycle";
import { assetTypesLoaded, useAssetTypes } from "../state/assetTypes";

/** Where the thumbnail version of a plugin type, as last drawn in a library on this computer, is kept. */
const versionKey = (library: string, type: string) => `toybox.thumbnailVersion.${library}.${type}`;
function drawnVersion(library: string, type: string): number {
  try { return Number(localStorage.getItem(versionKey(library, type))) || 0; } catch { return 0; }
}
function rememberVersion(library: string, type: string, version: number) {
  try { localStorage.setItem(versionKey(library, type), String(version)); } catch { /* the pictures are made again next time */ }
}

/** The plugin types that draw their own thumbnails (with the host's material picture, perhaps). */
const pluginThumbTypes = () => useAssetTypes.getState().types.filter(entry => entry.implementation?.renderThumbnail);

/** Pictures still to make, library by library, a page of ids at a time: nothing holds them all. */
function pendingSource(): ThumbnailSource {
  const libraries = useStore.getState().libraries.filter(library => library.available).map(library => library.id);
  let index = 0;
  let after = "";
  return {
    async next() {
      while (index < libraries.length) {
        const page = await pendingThumbs(libraries[index], after, 200).catch(() => ({ ids: [] as string[], next: null }));
        // `next` is where the library goes on (a raw id), not a qualified one.
        if (page.next === null) { index++; after = ""; } else after = page.next;
        if (page.ids.length) return page.ids;
      }
      return [];
    },
    async resolve(ids) {
      const assets = await getAssets(ids);
      cacheAssets(assets);
      return assets;
    },
  };
}

export function useThumbGeneration() {
  // Jobs belong to the set of libraries; adding or removing one restarts them.
  const libraryRoot = useStore(state => state.libraries.map(library => library.id).join(","));
  const dataVersion = useStore(state => state.dataVersion);
  const materialStyle = usePreviewPreferences(state => state.materialStyle);
  const jobsRef = useRef<ThumbnailJobs | null>(null);
  const [progress, setProgress] = useState<ThumbProgress | null>(null);
  const [error, setError] = useState<string>();
  const previousStyle = useRef(materialStyle);

  useEffect(() => {
    if (!libraryRoot) return;
    setProgress(null); setError(undefined);
    const jobs = new ThumbnailJobs({
      onProgress: value => { if (jobsRef.current === jobs) setProgress(value); },
      onAssetState: (id, state, failure) => {
        if (jobsRef.current !== jobs) return;
        const current = cachedAsset(id);
        if (!current) return;
        patchCachedAsset(id, { thumb: {
          state, error: failure,
          available: state === "ready" || current.thumb.state === "ready" || current.thumb.available,
          revision: state === "ready" ? (current.thumb.revision ?? 0) + 1 : current.thumb.revision,
        } });
      },
    }, failure => { if (jobsRef.current === jobs) setError(String(failure)); });
    jobsRef.current = jobs;
    jobs.useSource(pendingSource());
    let resumeAfterUpdate=false;
    const unregister=registerUpdateParticipant({pause:()=>{resumeAfterUpdate=!jobs.isPaused();return jobs.suspend();},resume:()=>{if(resumeAfterUpdate)jobs.enqueue([],true);}});
    return () => { unregister();jobsRef.current = null; void jobs.stop(); };
  }, [libraryRoot]);

  // Items added or replaced have no picture: look again, once the changes settle. The disk is the source of
  // truth, so this also finds refreshes interrupted by closing Toybox.
  useEffect(() => {
    const timer = window.setTimeout(() => jobsRef.current?.restartSource(pendingSource()), 1000);
    return () => window.clearTimeout(timer);
  }, [dataVersion, libraryRoot]);

  /** Make the pictures of one type again (its look changed): every item of the type, a page of ids at a time. */
  const regenerate = useCallback(async (kind: AssetKind, library?: string) => {
    setError(undefined);
    const wanted = emptyFilter(kind);
    if (library) wanted.libraries = new Set([library]);
    const filter = toBrowseFilter(wanted, null, NO_RULES);
    for (let offset = 0; ; offset += 5000) {
      const page = await queryIds(filter, offset, 5000);
      await jobsRef.current?.regenerate(page.ids);
      if (!page.truncated) break;
    }
  }, []);

  useEffect(() => {
    if (previousStyle.current === materialStyle) return;
    previousStyle.current = materialStyle;
    void regenerate("material").then(async () => {
      // A plugin type may show a material in its picture (a substance does), on the same shape.
      for (const entry of pluginThumbTypes()) await regenerate(entry.id);
    });
  }, [materialStyle, regenerate]);

  // A plugin type whose pictures changed look (a higher `thumbnailVersion`) has them made again, once per library.
  useEffect(() => {
    if (!libraryRoot) return;
    let cancelled = false;
    void assetTypesLoaded().then(async () => {
      const libraries = useStore.getState().libraries.filter(library => library.available).map(library => library.id);
      for (const entry of pluginThumbTypes()) {
        const version = entry.implementation?.thumbnailVersion ?? 0;
        for (const library of libraries) {
          if (cancelled || drawnVersion(library, entry.id) >= version) continue;
          await regenerate(entry.id, library);
          if (!cancelled) rememberVersion(library, entry.id, version);
        }
      }
    }).catch(failure => setError(String(failure)));
    return () => { cancelled = true; };
  }, [libraryRoot, regenerate]);

  const start = useCallback(async () => {
    setError(undefined);
    try {
      jobsRef.current?.restartSource(pendingSource());
      jobsRef.current?.enqueue([], true);
    }
    catch (failure) { setError(String(failure)); }
  }, []);
  const pause = useCallback(() => jobsRef.current?.pause(), []);
  const stop = useCallback(async () => { await jobsRef.current?.suspend(); setProgress(null); }, []);
  return { progress, error, start, pause, stop, regenerate };
}
