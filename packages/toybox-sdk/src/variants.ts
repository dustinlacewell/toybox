import type { AssetKind, SeedEntryInput } from "./index.js";

/** Only explicit detail/size tokens identify variants. Colour names are untouched. */
export function variantInfo(name: string, kind: AssetKind): { base: string; label: string; rank: number; explicit: boolean } {
  const stem = name.replace(/\.(gltf|glb|fbx)$/i, "");
  if (kind === "animation" || kind === "audio") return { base: stem, label: "Original", rank: 0, explicit: false };
  if (kind === "model") {
    const match = stem.match(/(?:[_ .-])LOD(\d+)$/i);
    const level = match ? Number(match[1]) : 0;
    return { base: match ? stem.slice(0, match.index) : stem, label: ["Full", "Medium", "Low"][level] ?? `Level ${level}`, rank: level, explicit: !!match };
  }
  const match = stem.match(/(?:^|[_ .-])(512|1024|2048|4096|8192|1k|2k|4k|8k)(?=$|[_ .-])/i);
  const size = match ? (/k$/i.test(match[1]) ? parseInt(match[1]) * 1024 : Number(match[1])) : 0;
  const base = match ? `${stem.slice(0, match.index)}_${stem.slice(match.index! + match[0].length)}`.replace(/^[_ .-]+|[_ .-]+$/g, "") : stem;
  return { base, label: size ? String(size) : "Original", rank: -size, explicit: !!match };
}

export function groupSeedVariants(entries: SeedEntryInput[]): SeedEntryInput[] {
  const groups = new Map<string, SeedEntryInput[]>();
  for (const entry of entries) {
    const kind = entry.kind ?? "model";
    const name = entry.kind === "material" ? entry.name : entry.file;
    const info = variantInfo(name, kind);
    const key = `${entry.pack}\0${entry.category}\0${kind}\0${info.base.toLowerCase()}`;
    const group = groups.get(key) ?? [];
    group.push(entry); groups.set(key, group);
  }
  return [...groups.values()].map((group) => {
    if (group[0].kind === "animation" || group[0].kind === "audio") return group[0];
    group.sort((a, b) => variantInfo(a.kind === "material" ? a.name : a.file, a.kind ?? "model").rank - variantInfo(b.kind === "material" ? b.name : b.file, b.kind ?? "model").rank);
    const [best, ...rest] = group;
    const info = variantInfo(best.kind === "material" ? best.name : best.file, best.kind ?? "model");
    const id = best.kind === "material" ? `uid://import_${best.pack}_material_${info.base}` : `uid://import_${best.pack}_${best.category}_${info.base}`;
    return { ...best, id, ...(rest.length ? { variants: rest } : {}) } as SeedEntryInput;
  });
}
