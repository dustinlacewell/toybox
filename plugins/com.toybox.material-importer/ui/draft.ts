/**
 * The importer's working model: a folder of images grouped into material drafts,
 * each image tagged with what it is used for. Plain data, shared by detection,
 * grouping, entry building and (later) the panel.
 */

export type ChannelRole = "ao" | "roughness" | "smoothness" | "metallic" | "opacity";
export type Channels = { r?: ChannelRole; g?: ChannelRole; b?: ChannelRole; a?: ChannelRole };

/** What one image file is used for. A scalar map is always `channels`, even a
 *  plain grayscale one. */
export type FileUse =
  | { use: "unassigned" }
  | { use: "albedo" }
  | { use: "normal"; directX: boolean }
  | { use: "emissive" }
  | { use: "height" }
  | { use: "channels"; channels: Channels };

export interface DraftFile {
  /** Source-root-relative path, forward-slashed. */
  relPath: string;
  fileName: string;
  use: FileUse;
}

export interface MaterialDraft {
  name: string;
  /** Source-root-relative directory ("" at the root). */
  dir: string;
  files: DraftFile[];
}
