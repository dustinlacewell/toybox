/**
 * Filename -> use detection. Pure: reads a texture's file name and guesses what
 * it is (albedo, normal, a packed channel map, ...) from its suffix token, and
 * what material it belongs to from the part of the name before that suffix.
 */

import type { Channels, FileUse } from "./draft.js";

/** A trailing size token like `4K` or `2048`; ignored when finding the suffix. */
const RESOLUTION = /^(1k|2k|4k|8k|512|1024|2048|4096|8192)$/i;

/** One row per use: every lower-case suffix that means it. Two-token suffixes are
 *  written joined with `_`; both the joined and the run-together spellings are
 *  listed. */
const SUFFIX_TABLE: { suffixes: string[]; use: () => FileUse }[] = [
  { suffixes: ["albedo", "alb", "basecolor", "base_color", "color", "col", "diffuse", "diff"], use: () => ({ use: "albedo" }) },
  { suffixes: ["normal", "nrm", "nor", "norm", "normalgl", "normal_gl", "nor_gl"], use: () => ({ use: "normal", directX: false }) },
  { suffixes: ["normaldx", "normal_dx", "nor_dx"], use: () => ({ use: "normal", directX: true }) },
  { suffixes: ["emissive", "emission", "emit"], use: () => ({ use: "emissive" }) },
  { suffixes: ["height", "displacement", "disp", "bump"], use: () => ({ use: "height" }) },
  { suffixes: ["roughness", "rough", "rgh"], use: () => channels({ r: "roughness" }) },
  { suffixes: ["smoothness", "smooth", "gloss", "glossiness"], use: () => channels({ r: "smoothness" }) },
  { suffixes: ["metallic", "metalness", "metal", "mtl"], use: () => channels({ r: "metallic" }) },
  { suffixes: ["ao", "ambientocclusion", "ambient_occlusion", "occlusion"], use: () => channels({ r: "ao" }) },
  { suffixes: ["opacity", "alpha"], use: () => channels({ r: "opacity" }) },
  { suffixes: ["orm", "arm"], use: () => channels({ r: "ao", g: "roughness", b: "metallic" }) },
  { suffixes: ["rma"], use: () => channels({ r: "roughness", g: "metallic", b: "ao" }) },
  { suffixes: ["maskmap", "mask_map", "mask"], use: () => channels({ r: "metallic", g: "ao", a: "smoothness" }) },
];

const BY_SUFFIX: Map<string, () => FileUse> = new Map(
  SUFFIX_TABLE.flatMap((row) => row.suffixes.map((s) => [s, row.use] as const)),
);

function channels(c: Channels): FileUse {
  return { use: "channels", channels: c };
}

interface Token {
  text: string;
  /** Index in the stem where the token starts. */
  start: number;
  /** Index in the stem just past the token. */
  end: number;
}

function tokenize(stem: string): Token[] {
  return [...stem.matchAll(/[^_\-. ]+/g)].map((m) => ({
    text: m[0],
    start: m.index!,
    end: m.index! + m[0].length,
  }));
}

function stripExtension(fileName: string): string {
  return fileName.replace(/\.[^.]+$/, "");
}

/** The use for the last one or two tokens, preferring the two-token spelling. */
function matchSuffix(tokens: Token[]): { use: FileUse; width: number } | null {
  const last = tokens[tokens.length - 1].text.toLowerCase();
  if (tokens.length >= 2) {
    const two = `${tokens[tokens.length - 2].text.toLowerCase()}_${last}`;
    const make = BY_SUFFIX.get(two);
    if (make) return { use: make(), width: 2 };
  }
  const make = BY_SUFFIX.get(last);
  return make ? { use: make(), width: 1 } : null;
}

/** What `fileName` is used for, and the `base` name it shares with its siblings. */
export function detectUse(fileName: string): { base: string; use: FileUse } {
  const stem = stripExtension(fileName);
  const tokens = tokenize(stem);
  while (tokens.length > 0 && RESOLUTION.test(tokens[tokens.length - 1].text)) tokens.pop();
  if (tokens.length === 0) return { base: stem, use: { use: "unassigned" } };

  const hit = matchSuffix(tokens);
  if (!hit) {
    return { base: stem.slice(0, tokens[tokens.length - 1].end), use: { use: "unassigned" } };
  }
  const suffixStart = tokens[tokens.length - hit.width].start;
  return { base: stem.slice(0, suffixStart).replace(/[_\-. ]+$/, ""), use: hit.use };
}
