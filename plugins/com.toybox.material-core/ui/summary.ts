/**
 * What a review list shows about a draft at a glance: the file to preview it by,
 * the roles its files supply, and which included items need attention. Pure.
 */

import type { ChannelRole, DraftFile, MaterialDraft } from "../lib/draft.js";
import { problemsFor, type ReviewItem } from "../lib/edit.js";
import { ROLE_OPTIONS, USE_OPTIONS } from "./uses.js";

type Role = "albedo" | "normal" | "roughness" | "metallic" | "ao" | "height" | "emissive" | "opacity";

const ROLE_ORDER: Role[] = [
  "albedo",
  "normal",
  "roughness",
  "metallic",
  "ao",
  "height",
  "emissive",
  "opacity",
];
const CHANNELS = ["r", "g", "b", "a"] as const;

/** The file that best shows the material in a list: the albedo, else the first
 *  file that is used for something, else the first file. */
export function previewFile(draft: MaterialDraft): DraftFile | undefined {
  const { files } = draft;
  return (
    files.find((f) => f.use.use === "albedo") ??
    files.find((f) => f.use.use !== "unassigned") ??
    files[0]
  );
}

/** The short label of every role the draft's files supply, in a fixed order. */
export function roleLabels(draft: MaterialDraft): string[] {
  const present = new Set(draft.files.flatMap(rolesOf));
  return ROLE_ORDER.filter((r) => present.has(r)).map(labelOf);
}

/** The roles one file supplies. A smoothness channel supplies roughness. */
function rolesOf(file: DraftFile): Role[] {
  const { use } = file;
  switch (use.use) {
    case "unassigned":
      return [];
    case "channels":
      return CHANNELS.flatMap((c) => (use.channels[c] ? [channelRole(use.channels[c])] : []));
    default:
      return [use.use];
  }
}

function channelRole(role: ChannelRole): Role {
  return role === "smoothness" ? "roughness" : role;
}

function labelOf(role: Role): string {
  return [...USE_OPTIONS, ...ROLE_OPTIONS].find((o) => o.value === role)?.label ?? role;
}

/** The indexes of included items that have problems. */
export function attentionIndexes(items: ReviewItem[], pack: string, category: string): number[] {
  return items.flatMap((item, i) =>
    item.included && problemsFor(items, i, pack, category).length > 0 ? [i] : [],
  );
}
