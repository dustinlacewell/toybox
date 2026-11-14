/**
 * One image file of the open material: a large preview, its name, a Select for
 * what it is used for, and, for a packed file, one Select per channel.
 */

import { Select, Stack } from "@ldlework/toybox-sdk/ui";
import type { ChannelRole, DraftFile, FileUse } from "./draft.js";
import type { Channel } from "./handlers.js";
import { Thumb } from "./Thumb.js";
import type { ThumbLoader } from "./thumbs.js";
import { ROLE_OPTIONS, roleFromKey, USE_OPTIONS, useFromKey, useKey } from "./uses.js";

const CHANNELS: Channel[] = ["r", "g", "b", "a"];

interface Props {
  file: DraftFile;
  loader: ThumbLoader;
  onUse: (use: FileUse) => void;
  onChannel: (channel: Channel, role: ChannelRole | undefined) => void;
}

export function TextureCard({ file, loader, onUse, onChannel }: Props) {
  const { use } = file;
  return (
    <Stack gap={6}>
      <Thumb loader={loader} relPath={file.relPath} box={220} size={440} />
      <span
        title={file.relPath}
        style={{
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          fontSize: "var(--fs-md)",
        }}
      >
        {file.fileName}
      </span>
      <Select value={useKey(use)} options={USE_OPTIONS} onChange={(key) => onUse(useFromKey(key))} />
      {use.use === "channels" && (
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 6 }}>
          {CHANNELS.map((c) => (
            <Stack dir="row" gap={4} align="center" key={c}>
              <span className="export__label">{c.toUpperCase()}</span>
              <div style={{ flex: 1, minWidth: 0 }}>
                <Select
                  value={use.channels[c] ?? ""}
                  options={ROLE_OPTIONS}
                  onChange={(key) => onChannel(c, roleFromKey(key))}
                />
              </div>
            </Stack>
          ))}
        </div>
      )}
    </Stack>
  );
}
