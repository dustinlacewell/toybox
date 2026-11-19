/**
 * One image file of the open material, as a compact row: a small preview beside
 * its name over a Select for what it is used for. A packed file adds one Select
 * per channel under that, in a 2x2 grid.
 */

import { Select } from "@ldlework/toybox-sdk/ui";
import type { ChannelRole, DraftFile, FileUse } from "../lib/draft.js";
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

export function TextureRow({ file, loader, onUse, onChannel }: Props) {
  const { use } = file;
  return (
    <div style={{ display: "flex", gap: 10, alignItems: "flex-start" }}>
      <Thumb loader={loader} relPath={file.relPath} box={56} size={128} />
      <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 6 }}>
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
        <Select
          value={useKey(use)}
          options={USE_OPTIONS}
          onChange={(key) => onUse(useFromKey(key))}
        />
        {use.use === "channels" && (
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 6 }}>
            {CHANNELS.map((c) => (
              <div key={c} style={{ display: "flex", gap: 4, alignItems: "center", minWidth: 0 }}>
                <span className="export__label">{c.toUpperCase()}</span>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <Select
                    value={use.channels[c] ?? ""}
                    options={ROLE_OPTIONS}
                    onChange={(key) => onChannel(c, roleFromKey(key))}
                  />
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
