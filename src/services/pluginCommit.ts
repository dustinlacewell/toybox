/**
 * What an asset panel's or an asset viewer's `ctx.commit` does: the entries go into the item's own library,
 * stamped with the plugin's id, exactly as an import; a plugin that reuses the qualified `asset.id` replaces
 * that item in place.
 */

import type { SeedEntryInput } from "@ldlework/toybox-sdk";

import { libraryOf, parseQualified, qualify, unqualifyEntry } from "../domain/libraryId";
import { withOrigin } from "../domain/origin";
import { useStore } from "../state/store";
import { invalidateThumb, mergeSeedEntries } from "./tauriApi";

export async function commitToLibrary(library: string, pluginId: string, entries: SeedEntryInput[]): Promise<void> {
  const raw = entries.map((entry) => unqualifyEntry(library, withOrigin(entry, pluginId)));
  await mergeSeedEntries(library, raw);
  // An item replaced in place has new files behind its old picture; the thumbnail jobs make the new one.
  const changed = raw.map((entry) => qualify(library, entry.id));
  await Promise.all(changed.map((id) => invalidateThumb(id).catch(() => undefined)));
  await invalidateParentThumbs(raw.map((entry) => ({ ...entry, id: qualify(library, entry.id) })));
  useStore.getState().bumpData();
}

/** A picture can show the items made from the item (a substance shows its default material): when items that
 *  name a parent in `origin.derivedFrom` are added, replaced or deleted, the parents' thumbnails are made again.
 *  `items` carry qualified ids; a `derivedFrom` may be raw (in the item's library) or qualified. */
export async function invalidateParentThumbs(items: readonly { id: string; origin?: { derivedFrom?: string } }[]): Promise<void> {
  const parents = new Set<string>();
  for (const item of items) {
    const from = item.origin?.derivedFrom;
    if (from) parents.add(parseQualified(from) ? from : qualify(libraryOf(item.id), from));
  }
  await Promise.all([...parents].map((id) => invalidateThumb(id).catch(() => undefined)));
}
