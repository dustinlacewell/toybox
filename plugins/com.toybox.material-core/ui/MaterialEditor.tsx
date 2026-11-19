/**
 * The open material as a narrow column: its editable name, its problems, and one
 * compact row per image file.
 */

import { Stack, TextInput } from "@ldlework/toybox-sdk/ui";
import type { ChannelRole, FileUse } from "../lib/draft.js";
import type { ReviewItem } from "../lib/edit.js";
import type { Channel } from "./handlers.js";
import { TextureRow } from "./TextureRow.js";
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
    <Stack gap={14}>
      <Stack gap={6}>
        <TextInput
          value={item.draft.name}
          onChange={(e) => onRename(e.currentTarget.value)}
          style={{ width: "100%" }}
        />
        {item.included &&
          problems.map((p) => (
            <div className="export__warn" key={p} style={{ marginTop: 0 }}>
              {p}
            </div>
          ))}
      </Stack>
      {item.draft.files.map((f, i) => (
        <TextureRow
          key={f.relPath}
          file={f}
          loader={loader}
          onUse={(use) => onUse(i, use)}
          onChannel={(c, role) => onChannel(i, c, role)}
        />
      ))}
    </Stack>
  );
}
