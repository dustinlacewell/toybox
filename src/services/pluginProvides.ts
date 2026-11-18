/**
 * The short "provides" line of a plugin in Settings, built from what it ships
 * and, for a plugin that loaded, what its module turned out to provide. Pure:
 * no host imports.
 */

export interface Provided {
  exporter: boolean;
  rigProfiles: boolean;
  /** Extension points the plugin contributes to. */
  extensionPoints: string[];
}

interface ProvidesSource {
  entry?: string;
  assetKinds?: string[];
  assetTypes?: { id?: unknown; label?: unknown }[];
  dependencies?: Record<string, string>;
  fields?: unknown[];
  ui?: { importPanel?: string; exportPanel?: string };
}

/** "A", "A and B", "A, B and C". */
export function naturalList(items: string[]): string {
  return items.length > 1 ? `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}` : items[0] ?? "";
}

/** `typeLabel` names a type id; `dependencyNames` the plugins this one needs.
 *  `provided` is absent for a plugin that is off or failed to load, then only
 *  the manifest speaks. */
export function providesText(manifest: ProvidesSource, typeLabel: (id: string) => string, dependencyNames: string[], provided?: Provided): string {
  const declared = (manifest.assetTypes ?? []).filter((type) => typeof type?.id === "string");
  const declaredIds = declared.map((type) => type.id as string);
  const labels = declared.map((type) => (typeof type.label === "string" && type.label) || typeLabel(type.id as string));
  const parts: string[] = [];
  if (labels.length) parts.push(naturalList(labels));
  if (manifest.ui?.importPanel) {
    const imports = (manifest.assetKinds ?? []).filter((kind) => !declaredIds.includes(kind)).map((kind) => typeLabel(kind).toLowerCase());
    if (!labels.length) parts.push(`Imports ${naturalList(imports.length ? imports : [typeLabel("model").toLowerCase()])}`);
  }
  if (provided ? provided.exporter : !!(manifest.ui?.exportPanel || manifest.fields?.length)) parts.push("Exporter");
  if (provided?.rigProfiles) parts.push("Rig profiles");
  if (dependencyNames.length && (provided ? provided.extensionPoints.length > 0 : !!manifest.entry)) parts.push(`Extends ${naturalList(dependencyNames)}`);
  return parts.join(", ") || "Plugin";
}
