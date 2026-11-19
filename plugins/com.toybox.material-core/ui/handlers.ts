/**
 * The edit callbacks the review screens pass down, one per kind of change.
 * Each names the item by its index in the review list. Types only.
 */

import type { ChannelRole, FileUse } from "../lib/draft.js";

export type Channel = "r" | "g" | "b" | "a";

export interface ReviewHandlers {
  onInclude: (index: number, included: boolean) => void;
  onIncludeAll: (indexes: number[], included: boolean) => void;
  onRename: (index: number, name: string) => void;
  onUse: (index: number, fileIndex: number, use: FileUse) => void;
  onChannel: (
    index: number,
    fileIndex: number,
    channel: Channel,
    role: ChannelRole | undefined,
  ) => void;
}
