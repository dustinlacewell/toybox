/**
 * The open material: its editable name, its problems, and a grid of its image
 * files as texture cards.
 */

import { Stack, TextInput } from "@ldlework/toybox-sdk/ui";
import type { ChannelRole, FileUse } from "./draft.js";
import type { ReviewItem } from "./edit.js";
import type { Channel } from "./handlers.js";
import { TextureCard } from "./TextureCard.js";
import type { ThumbLoader } from "./thumbs.js";

interface Props {
  item: ReviewItem;
  problems: string[];
  loader: ThumbLoader;
  onRename: (name: string) => void;
  onUse: (fileIndex: number, use: FileUse) => void;
  onChannel: (fileIndex: number, channel: Channel, role: ChannelRole | undefined) => void;
}

export function MaterialEditor({ item, problems, loader, onRename, onUse, onChannel }: Props) {
  return (
    <Stack gap={16}>
      <Stack gap={6}>
        <TextInput
          value={item.draft.name}
          onChange={(e) => onRename(e.currentTarget.value)}
          style={{ width: "100%", maxWidth: 480 }}
        />
        {item.included &&
          problems.map((p) => (
            <div className="export__warn" key={p} style={{ marginTop: 0 }}>
              {p}
            </div>
          ))}
      </Stack>
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fill, minmax(220px, 1fr))",
          gap: 16,
        }}
      >
        {item.draft.files.map((f, i) => (
          <TextureCard
            key={f.relPath}
            file={f}
            loader={loader}
            onUse={(use) => onUse(i, use)}
            onChannel={(c, role) => onChannel(i, c, role)}
          />
        ))}
      </div>
    </Stack>
  );
}
