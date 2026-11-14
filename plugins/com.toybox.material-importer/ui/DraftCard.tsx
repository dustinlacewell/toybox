/**
 * One material in the review list: an include checkbox, an editable name, its
 * problems, and a row per image file.
 */

import { Checkbox, Stack, TextInput } from "@ldlework/toybox-sdk/ui";
import type { ChannelRole, FileUse } from "./draft.js";
import type { ReviewItem } from "./edit.js";
import { FileRow } from "./FileRow.js";

type Channel = "r" | "g" | "b" | "a";

interface Props {
  item: ReviewItem;
  problems: string[];
  onInclude: (included: boolean) => void;
  onRename: (name: string) => void;
  onUse: (fileIndex: number, use: FileUse) => void;
  onChannel: (fileIndex: number, channel: Channel, role: ChannelRole | undefined) => void;
}

export function DraftCard({ item, problems, onInclude, onRename, onUse, onChannel }: Props) {
  return (
    <Stack gap={6} style={{ opacity: item.included ? 1 : 0.5 }}>
      <Stack dir="row" gap={8} align="center">
        <Checkbox checked={item.included} onChange={onInclude} label={null} />
        <TextInput
          value={item.draft.name}
          onChange={(e) => onRename(e.currentTarget.value)}
          style={{ flex: 1 }}
        />
      </Stack>
      {item.included &&
        problems.map((p) => (
          <div className="export__warn" key={p}>
            {p}
          </div>
        ))}
      {item.draft.files.map((f, i) => (
        <FileRow
          key={f.relPath}
          file={f}
          onUse={(use) => onUse(i, use)}
          onChannel={(c, role) => onChannel(i, c, role)}
        />
      ))}
    </Stack>
  );
}
