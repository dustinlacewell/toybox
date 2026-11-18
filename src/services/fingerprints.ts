/**
 * Content fingerprints arriving in the background. The backend fingerprints the
 * items of local libraries that have none yet, one library at a time, after the
 * catalogs are shown (`start_fingerprinting`); this module starts it and keeps the
 * store in step: fingerprints patch the merged catalog in place (no rescan, which
 * re-groups duplicates as they come), and the progress of a first-time pass is
 * kept for the one quiet line in Settings -> Library.
 */

import { useStore } from "../state/store";
import { fingerprintStatus, onFingerprints, startFingerprinting } from "./tauriApi";

let following: Promise<() => void> | undefined;

/** Listen for the backend's fingerprint events, once for the life of the window. */
function follow(): Promise<() => void> {
  following ??= onFingerprints({
    progress: (progress) => useStore.getState().setFingerprinting(progress.library, progress.running ? { done: progress.done, total: progress.total } : null),
    hashes: (library, hashes) => useStore.getState().applyContentHashes(library, hashes),
  }).catch(() => () => {});
  return following;
}

/** Start the background pass (or tell a running one that libraries changed) and pick
 *  up where it already is. Never throws and never waits for the pass. */
export function beginFingerprinting(): void {
  void follow()
    .then(() => startFingerprinting())
    .then(() => fingerprintStatus())
    .then((running) => running.forEach((progress) => useStore.getState().setFingerprinting(progress.library, { done: progress.done, total: progress.total })))
    .catch(() => {});
}
