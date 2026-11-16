import { useCallback, useEffect, useRef, useState } from "react";
import type { Asset, AssetKind, ThumbMeta } from "../domain/catalog";
import { listPendingThumbs } from "../services/tauriApi";
import type { ThumbProgress } from "../services/thumbQueue";
import { ThumbnailJobs } from "../services/thumbnailJobs";
import { useStore } from "../state/store";
import { usePreviewPreferences } from "../state/previewPreferences";
import { registerUpdateParticipant } from "../services/updateLifecycle";

export function useThumbGeneration(assets: Asset[]) {
  const patchAsset = useStore(state => state.patchAsset);
  const libraryRoot = useStore(state => state.catalog?.libraryRoot);
  const materialStyle = usePreviewPreferences(state => state.materialStyle);
  const assetsRef = useRef(assets); assetsRef.current = assets;
  const jobsRef = useRef<ThumbnailJobs | null>(null);
  const attempted = useRef(new WeakSet<ThumbMeta>());
  const [progress, setProgress] = useState<ThumbProgress | null>(null);
  const [error, setError] = useState<string>();
  const previousStyle = useRef(materialStyle);
  const assetIds = assets.map(asset => asset.id).join("\n");

  useEffect(() => {
    if (!libraryRoot) return;
    setProgress(null); setError(undefined);
    attempted.current = new WeakSet();
    const jobs = new ThumbnailJobs({
      onProgress: value => { if (jobsRef.current === jobs) setProgress(value); },
      onAssetState: (id, state, failure) => {
        if (jobsRef.current !== jobs) return;
        const current = useStore.getState().catalog?.assets.find(asset => asset.id === id);
        if (!current) return;
        patchAsset(id, { thumb: {
          state, error: failure,
          available: state === "ready" || current.thumb.state === "ready" || current.thumb.available,
          revision: state === "ready" ? (current.thumb.revision ?? 0) + 1 : current.thumb.revision,
        } });
      },
    }, failure => { if (jobsRef.current === jobs) setError(String(failure)); });
    jobs.updateAssets(assetsRef.current);
    jobsRef.current = jobs;
    let resumeAfterUpdate=false;
    const unregister=registerUpdateParticipant({pause:()=>{resumeAfterUpdate=!jobs.isPaused();return jobs.suspend();},resume:()=>{if(resumeAfterUpdate)jobs.enqueue([],true);}});
    return () => { unregister();jobsRef.current = null; void jobs.stop(); };
  }, [libraryRoot, patchAsset]);

  useEffect(() => {
    const jobs = jobsRef.current;
    if (!jobs) return;
    jobs.updateAssets(assets);
    const missing = assets.filter(asset => ["missing", "queued"].includes(asset.thumb.state) && !attempted.current.has(asset.thumb));
    for (const asset of missing) attempted.current.add(asset.thumb);
    jobs.enqueue(missing.map(asset => asset.id));
  }, [assets, libraryRoot]);

  // Disk also records refreshes interrupted by closing Toybox.
  useEffect(() => {
    const jobs = jobsRef.current;
    if (!jobs || !assetIds) return;
    let active = true;
    const refreshable = new Set(assetsRef.current.filter(asset => !["missing","queued","rendering"].includes(asset.thumb.state)).map(asset => asset.id));
    void listPendingThumbs().then(ids => {
      if (!active) return;
      // Missing/queued entries were already submitted by the effect above.
      jobs.enqueue(ids.filter(id => refreshable.has(id)));
    }).catch(failure => { if (active) setError(String(failure)); });
    return () => { active = false; };
  }, [assetIds, libraryRoot]);

  const regenerate = useCallback(async (kind?: AssetKind) => {
    setError(undefined);
    await jobsRef.current?.regenerate(assetsRef.current.filter(asset => !kind || asset.kind === kind).map(asset => asset.id));
  }, []);

  useEffect(() => {
    if (previousStyle.current === materialStyle) return;
    previousStyle.current = materialStyle;
    void regenerate("material");
  }, [materialStyle, regenerate]);

  const start = useCallback(async () => {
    setError(undefined);
    try {
      const ids=await listPendingThumbs();
      jobsRef.current?.updateAssets(useStore.getState().catalog?.assets ?? []);
      jobsRef.current?.enqueue(ids, true);
    }
    catch (failure) { setError(String(failure)); }
  }, []);
  const pause = useCallback(() => jobsRef.current?.pause(), []);
  const stop = useCallback(async () => { await jobsRef.current?.suspend(); setProgress(null); }, []);
  return { progress, error, start, pause, stop, regenerate };
}
