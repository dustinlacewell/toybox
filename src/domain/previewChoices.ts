/** Catalog suggestions are derived from already loaded rig data. The native
 * composer independently rechecks the selected files before playback. */
import type { AnimationAsset, Catalog, ModelAsset, RigDefinition, SceneAsset } from "./catalog";

export interface PreviewChoice { assetId: string; rigId: string; exact: boolean; }
const registryIndexes = new WeakMap<NonNullable<Catalog["rigs"]>, Map<string, RigDefinition>>();
const nodeIndexes = new WeakMap<RigDefinition, Map<string, RigDefinition["nodes"][number]>>();
function nodes(rig: RigDefinition) {
  let index = nodeIndexes.get(rig);
  if (!index) { index = new Map(rig.nodes.map(node => [node.key, node])); nodeIndexes.set(rig, index); }
  return index;
}
function sameNode(a: RigDefinition["nodes"][number], b: RigDefinition["nodes"][number]) {
  return a.parent === b.parent && a.rest.length === b.rest.length && a.rest.every((value, i) => Math.abs(value - b.rest[i]) <= .00001);
}
export function directRigMatch(animation: AnimationAsset, model: ModelAsset, catalog: Catalog): Omit<PreviewChoice, "assetId"> | undefined {
  const tracks = animation.clip.tracks;
  if (!tracks?.length || tracks.some(track => !["translation", "rotation", "scale"].includes(track.path))) return;
  const registry = catalog.rigs ?? [];
  let index = registryIndexes.get(registry);
  if (!index) { index = new Map(registry.map(rig => [rig.id, rig])); registryIndexes.set(registry, index); }
  for (const source of animation.rigs ?? []) {
    const a = index.get(source.rigId);
    if (!a?.unambiguous) continue;
    for (const target of model.rigs ?? []) {
      const b = index.get(target.rigId);
      if (!b?.unambiguous || a.kind !== b.kind) continue;
      const sourceKeys = new Map(Object.entries(source.nodes).map(([key, node]) => [node, key]));
      const an = nodes(a), bn = nodes(b), checked = new Set<string>();
      const valid = tracks.every(track => {
        let key: string | null | undefined = sourceKeys.get(track.node);
        if (key === undefined || target.nodes[key] === undefined) return false;
        while (key !== null) {
          if (checked.has(key)) break;
          const left = an.get(key), right = bn.get(key);
          if (!left || !right || !sameNode(left, right)) return false;
          checked.add(key); key = left.parent;
        }
        return true;
      });
      if (valid) return { rigId: b.id, exact: a.nodes.length === b.nodes.length && a.nodes.every(node => { const other = bn.get(node.key); return !!other && sameNode(node, other); }) };
    }
  }
}
export function directPreviewChoices(asset: SceneAsset, catalog: Catalog): PreviewChoice[] {
  if(!asset.rigs?.length)return [];
  return catalog.assets.flatMap(other => {
    const match = asset.kind === "animation" && other.kind === "model" ? directRigMatch(asset, other, catalog)
      : asset.kind === "model" && other.kind === "animation" ? directRigMatch(other, asset, catalog) : undefined;
    return match ? [{ assetId: other.id, ...match }] : [];
  });
}
