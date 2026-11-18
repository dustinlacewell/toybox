/**
 * The tags of one asset for the properties panel.
 *
 * A tag belongs to the asset and lives in the library where the asset's file is.
 * An asset that exists in several libraries shows the union of its copies' tags,
 * and adding or removing a tag changes every copy that can be written (one call
 * per library). What could not be saved is returned in plain words.
 */

import { useCallback, useMemo } from "react";

import type { Asset } from "../domain/catalog";
import {
  anyWritable, copiesOf, describeResult, leftoverTags, planIsEmpty, planTagChange, unionTags,
  type TagChange, type TagResult,
} from "../domain/tagEditing";
import { runTagPlan, libraryName, writeReason } from "../services/tagEdits";
import { useAlsoIn } from "../state/duplicates";
import { useAssets } from "../state/assetCache";
import { useStore } from "../state/store";

export interface AssetTagState {
  /** The asset's tags: the union over all its copies. */
  tags: string[];
  /** Tags held only on this computer (left by older versions where the library could not take them): shown, not editable. */
  leftover: string[];
  /** Libraries holding a copy, the shown one first. */
  libraries: string[];
  /** Whether at least one copy can be written. */
  editable: boolean;
  /** Add or remove tags on every copy. Never rejects: failures come back in the result. */
  change: (change: TagChange) => Promise<TagResult>;
}

export function useLibraryTags(asset: Asset): AssetTagState {
  const libraries = useStore((state) => state.libraries);
  const alsoIn = useAlsoIn(asset);
  // The other copies are fetched by id: the window holds no catalog.
  const copyIds = useMemo(() => alsoIn.map((copy) => copy.id), [alsoIn]);
  const found = useAssets(copyIds.length ? copyIds : undefined).assets;
  const copies = useMemo(() => {
    const byId = new Map<string, Asset>(found.map((candidate) => [candidate.id, candidate]));
    return copiesOf(asset, alsoIn, (id) => byId.get(id));
  }, [asset, found, alsoIn]);
  const tags = useMemo(() => unionTags(copies), [copies]);
  const leftover = useMemo(() => leftoverTags(copies, tags), [copies, tags]);
  // `libraries` is read so the state follows a library going offline or read-only.
  const editable = useMemo(() => anyWritable(copies, writeReason), [copies, libraries]);

  const change = useCallback(async (edit: TagChange): Promise<TagResult> => {
    const plan = planTagChange([copies], edit, writeReason);
    if (planIsEmpty(plan)) return { message: "Nothing to change.", failed: false };
    const outcomes = await runTagPlan(plan);
    return describeResult(outcomes, plan.blocked, libraryName);
  }, [copies]);

  return { tags, leftover, libraries: copies.map((copy) => copy.library), editable, change };
}
