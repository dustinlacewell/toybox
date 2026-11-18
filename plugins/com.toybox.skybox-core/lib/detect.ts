/**
 * Recognising skies in a folder of images. Pure: no host, DOM or three imports,
 * so the rules are unit-tested directly.
 *
 * A sky is either one image (a panorama, or a cube map unfolded into a cross
 * or strip) or six images that name their cube faces.
 */

export type Face = "px" | "nx" | "py" | "ny" | "pz" | "nz";
/** The order WebGL and three.js expect cube faces in. */
export const FACES: readonly Face[] = ["px", "nx", "py", "ny", "pz", "nz"];
export const FACE_LABELS: Record<Face, string> = { px: "Right (+X)", nx: "Left (−X)", py: "Top (+Y)", ny: "Bottom (−Y)", pz: "Front (+Z)", nz: "Back (−Z)" };

export type Layout = "equirect" | "cubemap" | "cross-horizontal" | "cross-vertical" | "strip-horizontal" | "strip-vertical";
/** The layouts one image can have. */
export type ImageLayout = Exclude<Layout, "cubemap">;
export const LAYOUT_LABELS: Record<Layout, string> = {
  equirect: "Panorama (equirectangular)",
  cubemap: "Cube map (6 images)",
  "cross-horizontal": "Cube map (horizontal cross)",
  "cross-vertical": "Cube map (vertical cross)",
  "strip-horizontal": "Cube map (horizontal strip)",
  "strip-vertical": "Cube map (vertical strip)",
};

export type Encoding = "hdr" | "exr" | "ldr";
const EXTENSIONS: Record<string, Encoding> = { hdr: "hdr", exr: "exr", png: "ldr", jpg: "ldr", jpeg: "ldr", webp: "ldr" };

/** Lower-case extension without the dot, or "". */
export const extensionOf = (name: string): string => /\.([^./\\]+)$/.exec(name)?.[1].toLowerCase() ?? "";
/** How a file's pixels are stored, or undefined when it is not a sky image. */
export const encodingOf = (name: string): Encoding | undefined => EXTENSIONS[extensionOf(name)];
export const isSkyImage = (name: string): boolean => encodingOf(name) !== undefined;
/** High-dynamic-range encodings carry real light levels. */
export const isHdr = (encoding: Encoding): boolean => encoding !== "ldr";
export const formatLabel = (name: string): string => { const e = extensionOf(name); return e === "jpeg" ? "JPG" : e.toUpperCase(); };

const baseName = (path: string) => path.slice(path.lastIndexOf("/") + 1);
const dirName = (path: string) => (path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "");
const stemOf = (name: string) => name.replace(/\.[^.]+$/, "");

/** Face tokens in common use. Named sides follow the usual engine convention:
 *  front is +Z and back is −Z. */
const FACE_TOKENS: Record<string, Face> = {
  px: "px", nx: "nx", py: "py", ny: "ny", pz: "pz", nz: "nz",
  posx: "px", negx: "nx", posy: "py", negy: "ny", posz: "pz", negz: "nz",
  "+x": "px", "-x": "nx", "+y": "py", "-y": "ny", "+z": "pz", "-z": "nz",
  right: "px", left: "nx", top: "py", up: "py", bottom: "ny", down: "ny", front: "pz", back: "nz",
  rt: "px", lf: "nx", dn: "ny", ft: "pz", bk: "nz",
};
// Longest first, so `posx` is not read as a name ending in `x`.
const TOKEN_PATTERN = Object.keys(FACE_TOKENS).sort((a, b) => b.length - a.length).map((token) => token.replace(/[+]/g, "\\+")).join("|");
const FACE_SUFFIX = new RegExp(`^(?:(.*?)[_\\-. ])?(${TOKEN_PATTERN})$`, "i");

/** The cube face a file names and the shared name before it, if any. A token
 *  must be the whole stem or follow a separator: `sky_px`, `Left`, `night-up`. */
export function faceOf(fileName: string): { face: Face; prefix: string } | undefined {
  const stem = stemOf(baseName(fileName));
  const match = FACE_SUFFIX.exec(stem);
  if (match) return { face: FACE_TOKENS[match[2].toLowerCase()], prefix: match[1] ?? "" };
  // Signed axes are their own separator: `sky+x`, `sky-z`.
  const signed = /^(.+?)([+-][xyz])$/i.exec(stem);
  return signed ? { face: FACE_TOKENS[signed[2].toLowerCase()], prefix: signed[1].replace(/[_\-. ]+$/, "") } : undefined;
}

export interface SkyCandidate {
  /** Display and library name. */
  name: string;
  /** Source-relative paths. One for an image; six, in `FACES` order, for a cube map. */
  files: string[];
  /** `cubemap` for six face files; otherwise decided from the image's size. */
  layout: "cubemap" | "image";
  encoding: Encoding;
  format: string;
  /** Face → source-relative path, for a cube map. */
  faces?: Record<Face, string>;
  /** Whether to tick it by default: HDR images, cube maps, and images whose
   *  name says they are a sky. Other images are listed but left for the user. */
  likely: boolean;
}

const SKY_WORDS = /(^|[^a-z])(sky|skies|skybox|skydome|pano|panorama|hdri|hdr|equirect|equirectangular|env|envmap|environment|cubemap|cube|cross|latlong)([^a-z]|$)/i;

/** Group a folder's files into skies. Six files that share a name, extension
 *  and folder and each name a different cube face become one cube map; every
 *  other image is its own candidate. */
export function findSkies(paths: string[]): SkyCandidate[] {
  const images = paths.map((path) => path.replace(/\\/g, "/")).filter(isSkyImage).sort((a, b) => a.localeCompare(b));
  const groups = new Map<string, { prefix: string; faces: Partial<Record<Face, string>>; files: string[] }>();
  for (const path of images) {
    const named = faceOf(path);
    if (!named) continue;
    const key = `${dirName(path)}\0${named.prefix.toLowerCase()}\0${extensionOf(path)}`;
    const group = groups.get(key) ?? { prefix: named.prefix, faces: {}, files: [] };
    // Two files claiming one face (`sky_up` and `sky_top`) is not a cube map.
    group.faces[named.face] = group.faces[named.face] === undefined ? path : "";
    group.files.push(path);
    groups.set(key, group);
  }
  const used = new Set<string>();
  const result: SkyCandidate[] = [];
  for (const group of groups.values()) {
    if (group.files.length !== 6 || !FACES.every((face) => group.faces[face])) continue;
    const faces = group.faces as Record<Face, string>;
    const files = FACES.map((face) => faces[face]);
    for (const file of files) used.add(file);
    const folder = baseName(dirName(files[0]));
    result.push({
      name: cleanName(group.prefix || folder || "Cube map"),
      files, faces, layout: "cubemap",
      encoding: encodingOf(files[0])!, format: formatLabel(files[0]), likely: true,
    });
  }
  for (const path of images) {
    if (used.has(path)) continue;
    const encoding = encodingOf(path)!;
    result.push({
      name: cleanName(stemOf(baseName(path))), files: [path], layout: "image", encoding, format: formatLabel(path),
      likely: isHdr(encoding) || SKY_WORDS.test(path),
    });
  }
  return uniqueNames(result).sort((a, b) => a.name.localeCompare(b.name));
}

/** A name that is one safe path segment on every platform. */
export function cleanName(name: string): string {
  const cleaned = name.replace(/[<>:"/\\|?*\x00-\x1f]/g, "_").replace(/\s+/g, " ").replace(/^[. _-]+|[. _-]+$/g, "");
  return cleaned || "Sky";
}

/** Names are folders in the library, so they must differ (ignoring case). */
function uniqueNames(candidates: SkyCandidate[]): SkyCandidate[] {
  const seen = new Set<string>();
  return candidates.map((candidate) => {
    let name = candidate.name;
    for (let index = 2; seen.has(name.toLowerCase()); index++) name = `${candidate.name} ${index}`;
    seen.add(name.toLowerCase());
    return name === candidate.name ? candidate : { ...candidate, name };
  });
}

/** What a single image holds, from its shape: a 2:1 image is a panorama; 4:3
 *  and 3:4 are crosses; 6:1 and 1:6 are strips. Anything else is treated as a
 *  panorama, and `certain` is false so the user can be told. */
export function layoutFromSize(width: number, height: number): { layout: ImageLayout; certain: boolean } {
  if (!(width > 0 && height > 0)) return { layout: "equirect", certain: false };
  if (width === height * 6) return { layout: "strip-horizontal", certain: true };
  if (height === width * 6) return { layout: "strip-vertical", certain: true };
  if (width * 3 === height * 4) return { layout: "cross-horizontal", certain: true };
  if (width * 4 === height * 3) return { layout: "cross-vertical", certain: true };
  return { layout: "equirect", certain: width === height * 2 };
}

/** Where each face sits in an unfolded cube image, as a cell (column, row) of
 *  a `columns` × `rows` grid. `turn` rotates that face by 180°. */
export interface FaceCell { face: Face; column: number; row: number; turn?: boolean; }
export function faceCells(layout: ImageLayout): { columns: number; rows: number; cells: FaceCell[] } | undefined {
  switch (layout) {
    case "cross-horizontal": return { columns: 4, rows: 3, cells: [
      { face: "px", column: 2, row: 1 }, { face: "nx", column: 0, row: 1 }, { face: "py", column: 1, row: 0 },
      { face: "ny", column: 1, row: 2 }, { face: "pz", column: 1, row: 1 }, { face: "nz", column: 3, row: 1 }] };
    // In a vertical cross the back face hangs below the bottom, upside down.
    case "cross-vertical": return { columns: 3, rows: 4, cells: [
      { face: "px", column: 2, row: 1 }, { face: "nx", column: 0, row: 1 }, { face: "py", column: 1, row: 0 },
      { face: "ny", column: 1, row: 2 }, { face: "pz", column: 1, row: 1 }, { face: "nz", column: 1, row: 3, turn: true }] };
    case "strip-horizontal": return { columns: 6, rows: 1, cells: FACES.map((face, column) => ({ face, column, row: 0 })) };
    case "strip-vertical": return { columns: 1, rows: 6, cells: FACES.map((face, row) => ({ face, column: 0, row })) };
    default: return undefined;
  }
}

/** The metadata a skybox asset carries in the library. */
export interface SkyMeta {
  layout: Layout;
  encoding: Encoding;
  /** File format label, e.g. `HDR`. */
  format: string;
  /** Pixel size of the image (of one face for a six-image cube map). */
  width: number;
  height: number;
  /** Face → file name, for a six-image cube map. */
  faces?: Record<Face, string>;
}

/** Read stored metadata defensively: a library edited by hand, or written by a
 *  newer plugin, must not break the viewer. */
export function readMeta(meta: unknown, files: string[]): SkyMeta {
  const value = (meta && typeof meta === "object" ? meta : {}) as Partial<SkyMeta>;
  const first = baseName(files[0] ?? "");
  const encoding = (["hdr", "exr", "ldr"] as const).find((item) => item === value.encoding) ?? encodingOf(first) ?? "ldr";
  const width = Number.isFinite(value.width) ? Number(value.width) : 0;
  const height = Number.isFinite(value.height) ? Number(value.height) : 0;
  const names = files.map(baseName);
  let faces: Record<Face, string> | undefined;
  if (value.faces && typeof value.faces === "object" && FACES.every((face) => names.includes((value.faces as Record<string, string>)[face]))) {
    faces = Object.fromEntries(FACES.map((face) => [face, (value.faces as Record<string, string>)[face]])) as Record<Face, string>;
  } else if (files.length === 6) {
    const named = Object.fromEntries(names.map((name) => [faceOf(name)?.face, name]));
    if (FACES.every((face) => named[face])) faces = Object.fromEntries(FACES.map((face) => [face, named[face]])) as Record<Face, string>;
  }
  const known = value.layout && value.layout in LAYOUT_LABELS ? value.layout : undefined;
  const layout: Layout = faces ? "cubemap" : known && known !== "cubemap" ? known : layoutFromSize(width, height).layout;
  return { layout, encoding, format: typeof value.format === "string" && value.format ? value.format : formatLabel(first), width, height, ...(faces ? { faces } : {}) };
}

/** The pixel size of one cube face, or of the panorama. */
export function describeSize(meta: SkyMeta): string {
  if (!meta.width || !meta.height) return "Unknown";
  const cells = meta.layout === "cubemap" || meta.layout === "equirect" ? undefined : faceCells(meta.layout);
  if (cells) return `${Math.round(meta.width / cells.columns)} × ${Math.round(meta.height / cells.rows)} per face`;
  return `${meta.width} × ${meta.height}${meta.layout === "cubemap" ? " per face" : ""}`;
}

/** A stable id: the same source folder and name always produce the same id, so
 *  importing a folder again updates its skies instead of duplicating them. */
export function skyId(pack: string, name: string): string {
  let hash = 0xcbf29ce484222325n;
  for (const char of `${pack}\0${name.toLowerCase()}`) hash = BigInt.asUintN(64, (hash ^ BigInt(char.codePointAt(0)!)) * 0x100000001b3n);
  return `uid://skybox_${hash.toString(16)}`;
}
