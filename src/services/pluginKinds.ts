/**
 * Which library types a plugin works with. Pure — no host imports — so the
 * registry, the icon lookup and the tests share one rule.
 */

interface KindsSource {
  assetKinds?: string[];
  assetTypes?: { id?: unknown }[];
}

/** A plugin's types: the ones it lists in `assetKinds`, then any type it
 *  declares that the list leaves out. With neither, models — the default from
 *  before manifests named their types. */
export function pluginKinds(manifest: KindsSource): string[] {
  const declared = (manifest.assetTypes ?? []).map((type) => type.id).filter((id): id is string => typeof id === "string" && !!id);
  const listed = manifest.assetKinds ?? [];
  const all = [...new Set([...listed, ...declared])];
  return all.length ? all : ["model"];
}

/** Whether a plugin handles assets of `kind`. `"*"` accepts every type. */
export function acceptsKind(manifest: KindsSource, kind: string): boolean {
  const kinds = pluginKinds(manifest);
  return kinds.includes("*") || kinds.includes(kind);
}
