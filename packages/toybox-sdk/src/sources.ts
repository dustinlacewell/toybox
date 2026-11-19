import type { FsApi } from "./index.js";

/** Something an importer reads from: a folder to search, or files named one by one. */
export interface ImportSource {
  /** The folder reads are jailed to (`fs.readDir(root, ...)`, `fs.readBytes(root, ...)`). */
  root: string;
  /** Root-relative, forward-slashed paths of the named files. Absent: search the whole folder. */
  files?: string[];
}

function splitPath(path: string): { dir: string; name: string } {
  const at = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  if (at < 0) return { dir: ".", name: path };
  let dir = path.slice(0, at);
  if (dir === "" || /^[A-Za-z]:$/.test(dir)) dir += path[at];
  return { dir, name: path.slice(at + 1) };
}

/**
 * Turn the absolute files and folders an importer is given on the command line
 * (`ImportRun.paths`) into sources. A folder becomes a source to search. The
 * jailed `fs` reads under a source root, so files become one source per parent
 * folder, with the root-relative names of the files. Rejects with a clear
 * message for a path that does not exist.
 */
export async function resolveSources(fs: Pick<FsApi, "readDir">, paths: string[]): Promise<ImportSource[]> {
  const folders: ImportSource[] = [];
  const named = new Map<string, string[]>();
  for (const path of paths) {
    try {
      await fs.readDir(path, ".");
      folders.push({ root: path });
      continue;
    } catch { /* not a readable folder: it should be a file */ }
    const { dir, name } = splitPath(path);
    let listing;
    try { listing = await fs.readDir(dir, "."); } catch { throw new Error(`${path} does not exist`); }
    const entry = listing.find((e) => !e.isDir && e.name === name) ?? listing.find((e) => !e.isDir && e.name.toLowerCase() === name.toLowerCase());
    if (!entry) throw new Error(`${path} does not exist or is not a file`);
    const files = named.get(dir) ?? [];
    const rel = entry.relPath.replace(/\\/g, "/");
    if (!files.includes(rel)) files.push(rel);
    named.set(dir, files);
  }
  return [...folders, ...[...named].map(([root, files]) => ({ root, files }))];
}
