/**
 * One image file in a material card: its name, a Select for what it is used for,
 * and, for a packed file, one small Select per channel.
 */

import { Select, Stack } from "@ldlework/toybox-sdk/ui";
import type { ChannelRole, DraftFile, FileUse } from "./draft.js";
import { ROLE_OPTIONS, roleFromKey, USE_OPTIONS, useFromKey, useKey } from "./uses.js";

type Channel = "r" | "g" | "b" | "a";
const CHANNELS: Channel[] = ["r", "g", "b", "a"];

interface Props {
  file: DraftFile;
  onUse: (use: FileUse) => void;
  onChannel: (channel: Channel, role: ChannelRole | undefined) => void;
}

export function FileRow({ file, onUse, onChannel }: Props) {
  const { use } = file;
  return (
    <Stack gap={4}>
      <Stack dir="row" gap={8} align="center" justify="space-between">
        <span style={{ overflow: "hidden", textOverflow: "ellipsis" }} title={file.relPath}>
          {file.fileName}
        </span>
        <Select
          value={useKey(use)}
          options={USE_OPTIONS}
          onChange={(key) => onUse(useFromKey(key))}
        />
      </Stack>
      {use.use === "channels" && (
        <Stack dir="row" gap={8} align="center" style={{ paddingLeft: 16 }}>
          {CHANNELS.map((c) => (
            <Stack dir="row" gap={4} align="center" key={c}>
              <span className="export__label">{c.toUpperCase()}</span>
              <Select
                value={use.channels[c] ?? ""}
                options={ROLE_OPTIONS}
                onChange={(key) => onChannel(c, roleFromKey(key))}
              />
            </Stack>
          ))}
        </Stack>
      )}
    </Stack>
  );
}
