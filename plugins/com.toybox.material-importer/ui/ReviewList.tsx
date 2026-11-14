/**
 * The scrollable list of material cards under review.
 */

import { Stack } from "@ldlework/toybox-sdk/ui";
import type { ChannelRole, FileUse } from "./draft.js";
import { problemsFor, type ReviewItem } from "./edit.js";
import { DraftCard } from "./DraftCard.js";

type Channel = "r" | "g" | "b" | "a";

interface Props {
  items: ReviewItem[];
  pack: string;
  category: string;
  onInclude: (index: number, included: boolean) => void;
  onRename: (index: number, name: string) => void;
  onUse: (index: number, fileIndex: number, use: FileUse) => void;
  onChannel: (
    index: number,
    fileIndex: number,
    channel: Channel,
    role: ChannelRole | undefined,
  ) => void;
}

export function ReviewList({ items, pack, category, onInclude, onRename, onUse, onChannel }: Props) {
  return (
    <div style={{ maxHeight: "50vh", overflowY: "auto" }}>
      <Stack gap={16}>
        {items.map((item, i) => (
          <DraftCard
            // Order is fixed while reviewing, so the index is a stable key.
            key={i}
            item={item}
            problems={problemsFor(items, i, pack, category)}
            onInclude={(included) => onInclude(i, included)}
            onRename={(name) => onRename(i, name)}
            onUse={(fileIndex, use) => onUse(i, fileIndex, use)}
            onChannel={(fileIndex, c, role) => onChannel(i, fileIndex, c, role)}
          />
        ))}
      </Stack>
    </div>
  );
}
