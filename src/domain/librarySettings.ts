/**
 * What a library's compact row in Settings shows: the muted second line, the
 * short tags and the item count. Pure: no imports at run time, so the tests
 * load this file on its own.
 */

/** The parts of `LibraryInfo` this module reads (the full type is in `catalog.ts`). */
export interface LibraryRowFacts {
  kind: string;
  root: string;
  available?: boolean;
  isDefault?: boolean;
  offline?: boolean;
  remote?: { role: "read" | "write" | "admin"; url: string; serverName?: string; offline?: boolean };
}

/** Keeps the start and the end of a long path and drops the middle ("C:\Art\...\Props"). */
export function shortenPath(path: string, max = 48): string {
  if (max < 8 || path.length <= max) return path;
  const keepEnd = Math.ceil((max - 1) * 0.6);
  const keepStart = max - 1 - keepEnd;
  return `${path.slice(0, keepStart)}…${path.slice(path.length - keepEnd)}`;
}

/** The host of a server address, or the address itself when it does not parse. */
export function hostOf(url: string): string {
  try { return new URL(url).host || url; } catch { return url; }
}

/** The muted line under the name: a local library's folder, a remote one's host. */
export function librarySecondary(library: LibraryRowFacts): string {
  return library.remote ? hostOf(library.remote.url) : library.root;
}

/** A role in the short form a tag uses. */
export function roleTag(role: "read" | "write" | "admin"): string {
  return role === "read" ? "read-only" : role === "write" ? "can edit" : "admin";
}

/** A role in a sentence, for the expanded panel. */
export function accessSentence(role: "read" | "write" | "admin"): string {
  return role === "read" ? "Read-only: you can browse and download, not change anything."
    : role === "write" ? "Can edit: you can also import, delete and copy items in."
    : "Admin: you can also manage access tokens.";
}

export interface LibraryTag { text: string; tone?: "default" | "warning" }

/** The tags after a library's name: only the ones that apply. */
export function libraryTags(library: LibraryRowFacts): LibraryTag[] {
  const tags: LibraryTag[] = [];
  if (library.isDefault) tags.push({ text: "Default" });
  if (library.remote) {
    tags.push({ text: roleTag(library.remote.role) });
    if (library.offline || library.remote.offline) tags.push({ text: "offline", tone: "warning" });
  } else if (library.available === false) tags.push({ text: "unavailable", tone: "warning" });
  return tags;
}

/** "1 item", "12 items". */
export function itemCount(count: number): string {
  return `${count} ${count === 1 ? "item" : "items"}`;
}
