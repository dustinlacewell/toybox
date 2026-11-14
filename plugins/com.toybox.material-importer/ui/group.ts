/**
 * Folder listing -> material drafts. Pure: takes source-relative image paths,
 * groups files that share a directory and a base name into one draft each, and
 * attaches stray files by the bare-stem / single-group / single-image rules.
 */

import type { DraftFile, MaterialDraft } from "./draft.js";
import { detectUse } from "./detect.js";

const IMAGE_EXT = /\.(png|jpe?g)$/i;

interface Group {
  key: string;
  draft: MaterialDraft;
}

interface Item {
  file: DraftFile;
  dir: string;
  base: string;
  matched: boolean;
}

export function groupMaterials(relPaths: string[]): MaterialDraft[] {
  const items = relPaths.filter((p) => IMAGE_EXT.test(p)).map(toItem);
  const groups = new Map<string, Group>();

  for (const item of items.filter((i) => i.matched)) addTo(groups, item);
  const perDir = countByDir(groups);
  const loneGroup = (dir: string) => [...groups.values()].find((g) => g.draft.dir === dir);
  const single = new Map([...perDir].filter(([, n]) => n === 1).map(([d]) => [d, loneGroup(d)!]));

  for (const item of items.filter((i) => !i.matched)) {
    const same = groups.get(keyOf(item.dir, nameOf(item)));
    if (same) {
      same.draft.files.push(withUse(item.file, hasAlbedo(same.draft) ? "unassigned" : "albedo"));
    } else if (single.has(item.dir)) {
      single.get(item.dir)!.draft.files.push(item.file);
    } else {
      addTo(groups, { ...item, file: withUse(item.file, "albedo") });
    }
  }
  return finish([...groups.values()].map((g) => g.draft));
}

function toItem(relPath: string): Item {
  const i = relPath.lastIndexOf("/");
  const dir = i < 0 ? "" : relPath.slice(0, i);
  const fileName = i < 0 ? relPath : relPath.slice(i + 1);
  const { base, use } = detectUse(fileName);
  return { file: { relPath, fileName, use }, dir, base, matched: use.use !== "unassigned" };
}

/** The material name an item belongs under: its base, or the folder's name. */
function nameOf(item: Item): string {
  if (item.base) return item.base;
  const last = item.dir.split("/").filter(Boolean).pop();
  return last ?? "material";
}

function keyOf(dir: string, name: string): string {
  return `${dir}\0${name.toLowerCase()}`;
}

function addTo(groups: Map<string, Group>, item: Item): void {
  const name = nameOf(item);
  const key = keyOf(item.dir, name);
  const group = groups.get(key) ?? { key, draft: { name, dir: item.dir, files: [] } };
  group.draft.files.push(item.file);
  groups.set(key, group);
}

function countByDir(groups: Map<string, Group>): Map<string, number> {
  const counts = new Map<string, number>();
  for (const g of groups.values()) counts.set(g.draft.dir, (counts.get(g.draft.dir) ?? 0) + 1);
  return counts;
}

function hasAlbedo(draft: MaterialDraft): boolean {
  return draft.files.some((f) => f.use.use === "albedo");
}

function withUse(file: DraftFile, use: "albedo" | "unassigned"): DraftFile {
  return { ...file, use: { use } };
}

function compare(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

function finish(drafts: MaterialDraft[]): MaterialDraft[] {
  for (const d of drafts) d.files.sort((a, b) => compare(a.fileName, b.fileName));
  return drafts.sort((a, b) => compare(a.dir, b.dir) || compare(a.name, b.name));
}
