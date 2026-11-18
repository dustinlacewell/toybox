/**
 * Loading and changing the set of libraries. The window holds no catalog: this module makes each library
 * ready to be read (`openLibrary`), keeps the library list, errors, packs and rigs in the store, and says
 * `bumpData` when contents changed so the list and the counts ask again.
 */

import type { LibraryInfo, RigDefinition } from "../domain/catalog";
import { clearAssetCache } from "../state/assetCache";
import { useStore } from "../state/store";
import { listRigs, openLibrary } from "./browse";
import { beginFingerprinting } from "./fingerprints";
import { migratePersonalTagsOfRemoteLibraries } from "./tagEdits";
import { listLibraries, loadPacks } from "./tauriApi";

const reason = (failure: unknown) => String(failure).replace(/^Error: /, "");

/** Make one library ready to be read: the cache when it is current, otherwise a scan. For a remote library both
 *  mean asking the server, and falling back to what was cached when the server cannot be reached. */
async function readLibrary(library: LibraryInfo, force: boolean): Promise<void> {
  if (!library.available) throw new Error(library.remote ? `The server has not been reachable yet and nothing is saved from it. ${library.offlineReason ?? ""}`.trim() : `The folder cannot be read: ${library.root}`);
  useStore.getState().setLibraryCount(library.id, (await openLibrary(library.id, force)).count);
}

/** The rigs of every readable library, each once (a rig id is a hash of its contents). */
async function readRigs(libraries: readonly LibraryInfo[], errors: Record<string, string>): Promise<RigDefinition[]> {
  const rigs = new Map<string, RigDefinition>();
  for (const library of libraries) {
    if (!library.available || errors[library.id]) continue;
    for (const rig of await listRigs(library.id).catch(() => [])) if (!rigs.has(rig.id)) rigs.set(rig.id, rig);
  }
  return [...rigs.values()];
}

/** Make every library ready, in parallel (they may be on different drives). A library that cannot be read is
 *  reported in `errors`; the rest still show. */
export async function readAllLibraries(force: boolean): Promise<{ libraries: LibraryInfo[]; errors: Record<string, string> }> {
  const libraries = await listLibraries();
  const errors: Record<string, string> = {};
  await Promise.all(libraries.map(async (library) => {
    try { await readLibrary(library, force); }
    catch (failure) { errors[library.id] = reason(failure); }
  }));
  // Reading a remote library learns whether its server answered: report that state, not the one from before.
  return { libraries: libraries.some((library) => library.remote) ? await listLibraries() : libraries, errors };
}

/** Make all libraries ready (forced, rescan them) and show them. Rejects only when libraries exist and none could be read. */
export async function loadAllLibraries(force: boolean): Promise<void> {
  const state = useStore.getState();
  const { libraries, errors } = await readAllLibraries(force);
  state.setLibraries(libraries);
  state.setLibraryErrors(errors);
  state.setPacks(await loadPacks().catch(() => []));
  if (libraries.length && Object.keys(errors).length === libraries.length) throw new Error(Object.values(errors).join("\n"));
  state.setRigs(await readRigs(libraries, errors));
  state.bumpData();
  // The lists are on screen; only now do the local libraries start fingerprinting their items.
  beginFingerprinting();
  // Personal tags left by older versions move into the remote libraries that can take them (best effort, in the background).
  void migratePersonalTagsOfRemoteLibraries();
}

/** Re-read the library list into the store (after a rename, default or add/remove). */
export async function refreshLibraryList(): Promise<LibraryInfo[]> {
  const libraries = await listLibraries();
  useStore.getState().setLibraries(libraries);
  return libraries;
}

/** Make one library ready again (after an import, a transfer, a sync) and refresh what is shown of it. */
export async function syncLibrary(id: string, force: boolean): Promise<void> {
  const libraries = await refreshLibraryList();
  const state = useStore.getState();
  const info = libraries.find((library) => library.id === id);
  if (!info) return;
  const { [id]: _previous, ...errors } = state.libraryErrors;
  try {
    await readLibrary(info, force);
    if (info.remote) await refreshLibraryList();
    const current = useStore.getState();
    current.setLibraryErrors(errors);
    current.setPacks(await loadPacks().catch(() => []));
    current.setRigs(await readRigs(useStore.getState().libraries, errors));
    current.bumpData();
    beginFingerprinting();
  } catch (failure) {
    state.setLibraryErrors({ ...errors, [id]: reason(failure) });
    state.bumpData();
  }
}

/** A library was removed from the registry: its items leave the view. */
export async function forgetLibrary(id: string): Promise<void> {
  await refreshLibraryList();
  const state = useStore.getState();
  const { [id]: _gone, ...errors } = state.libraryErrors;
  state.setLibraryErrors(errors);
  clearAssetCache();
  const kept = (itemId: string) => !itemId.startsWith(`${id}::`);
  state.setSelection([...state.selection].filter(kept));
  if (state.previewId && !kept(state.previewId)) state.setPreview(null);
  state.setFilterLibraries((libraries) => new Set([...libraries].filter((library) => library !== id)));
  state.setRigs(await readRigs(useStore.getState().libraries, errors));
  state.bumpData();
}
