/**
 * Animation playback state for the viewer. Owns a three AnimationMixer over the
 * loaded scene and the currently-selected clip's action, and exposes a transport
 * API for the AnimationBar plus a `tick(delta)` the in-Canvas driver calls each
 * frame. Returns a no-op shape when the asset has no clips.
 *
 * The mixer is rebuilt whenever the scene or clip set changes (e.g. an in-place
 * reload), and the previous mixer's actions are released.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AnimationMixer, type AnimationClip, type Group, LoopRepeat } from "three";
import { rootMotion } from "../../domain/animationMotion";

export interface AnimationControls {
  clips: AnimationClip[];
  /** Index of the active clip. */
  currentIndex: number;
  playing: boolean;
  /** Current play head, seconds. */
  time: number;
  /** Active clip duration, seconds. */
  duration: number;
  play: () => void;
  pause: () => void;
  toggle: () => void;
  seek: (seconds: number) => void;
  selectClip: (index: number) => void;
  /** Advance the mixer; called from the r3f frame loop. */
  tick: (delta: number) => void;
}

export function useAnimation(scene: Group | null, clips: AnimationClip[], { inPlace = false, speed = 1 }: { inPlace?: boolean; speed?: number } = {}): AnimationControls {
  const mixerRef = useRef<AnimationMixer | null>(null);
  const [currentIndex, setCurrentIndex] = useState(0);
  const [playing, setPlaying] = useState(true);
  const [time, setTime] = useState(0);

  const hasClips = clips.length > 0;
  const clampedIndex = Math.min(currentIndex, Math.max(0, clips.length - 1));
  const rawDuration = hasClips ? clips[clampedIndex]?.duration : 0;
  const duration =
    rawDuration != null && Number.isFinite(rawDuration) && rawDuration > 0 ? rawDuration : 0;
  const motionRef=useRef<ReturnType<typeof rootMotion>>(null);
  const applyRoot = useCallback(() => {
    const motion=motionRef.current;
    if (!motion) return;
    const action = mixerRef.current?.existingAction(clips[clampedIndex]);
    if (action) motion.apply(action.time,inPlace);
  }, [inPlace, clips, clampedIndex]);

  // Reset selection + play head when the clip set changes (different asset).
  useEffect(() => {
    setCurrentIndex(0);
    setTime(0);
    setPlaying(true);
  }, [clips]);

  // (Re)build the mixer when the scene or clip set changes.
  useEffect(() => {
    if (!scene || !hasClips) {
      mixerRef.current = null;
      return;
    }
    const mixer = new AnimationMixer(scene);
    mixerRef.current = mixer;
    setTime(0);
    return () => {
      mixer.stopAllAction();
      mixer.uncacheRoot(scene);
      mixerRef.current = null;
    };
  }, [scene, clips, hasClips]);

  // Recreate an action only when the clip changes; pausing preserves its time.
  useEffect(() => {
    const mixer = mixerRef.current;
    if (!mixer || !hasClips) return;
    const clip = clips[clampedIndex];
    // The prior mixer has restored the reused model's rest pose by this point.
    // Measuring during render would instead capture the previous clip's pose.
    motionRef.current=scene?rootMotion(scene,clip):null;
    const action = mixer.clipAction(clip);
    action.reset();
    action.setLoop(LoopRepeat, Infinity);
    action.paused = !playing;
    action.play();
    return () => {
      action.stop();
      motionRef.current=null;
    };
  }, [scene, clips, clampedIndex, hasClips]);

  useEffect(() => {
    const mixer = mixerRef.current;
    if (mixer && hasClips) mixer.clipAction(clips[clampedIndex]).paused = !playing;
  }, [playing, clips, clampedIndex, hasClips]);
  useEffect(() => { mixerRef.current?.update(0); applyRoot(); }, [applyRoot]);

  // Throttle scrubber state updates: the mixer advances every frame, but we only
  // push `time` to React ~10×/sec so playback doesn't re-render the viewer at
  // 60fps. The 3D pose still updates every frame (mixer.update runs each tick).
  const sinceUiUpdate = useRef(0);
  const tick = useCallback(
    (delta: number) => {
      const mixer = mixerRef.current;
      if (!mixer || !playing) return;
      mixer.update(delta * speed);
      applyRoot();
      sinceUiUpdate.current += delta;
      if (sinceUiUpdate.current >= 0.1) {
        sinceUiUpdate.current = 0;
        setTime(mixer.existingAction(clips[clampedIndex])?.time ?? 0);
      }
    },
    [playing, clips, clampedIndex, speed, applyRoot],
  );

  const seek = useCallback(
    (seconds: number) => {
      const mixer = mixerRef.current;
      if (!mixer || !hasClips) return;
      const action = mixer.clipAction(clips[clampedIndex]);
      action.time = Math.max(0, Math.min(seconds, duration));
      mixer.update(0); // apply the pose at the new time without advancing
      applyRoot();
      setTime(action.time);
    },
    [clips, clampedIndex, duration, hasClips, applyRoot],
  );

  const selectClip = useCallback((index: number) => {
    setCurrentIndex(index);
    setTime(0);
    setPlaying(true);
  }, []);

  return useMemo(
    () => ({
      clips,
      currentIndex: clampedIndex,
      playing,
      time,
      duration,
      play: () => setPlaying(true),
      pause: () => setPlaying(false),
      toggle: () => setPlaying((p) => !p),
      seek,
      selectClip,
      tick,
    }),
    [clips, clampedIndex, playing, time, duration, seek, selectClip, tick],
  );
}
