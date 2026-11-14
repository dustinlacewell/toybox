/**
 * The choices the review list offers for a file's use and a channel's role, and
 * the mapping between a Select's string value and the draft's typed value. Pure.
 */

import type { SelectOption } from "@ldlework/toybox-sdk/ui";
import type { ChannelRole, FileUse } from "./draft.js";

export const USE_OPTIONS: SelectOption[] = [
  { value: "unassigned", label: "Unassigned" },
  { value: "albedo", label: "Albedo" },
  { value: "normal", label: "Normal" },
  { value: "normalDx", label: "Normal (DirectX)" },
  { value: "emissive", label: "Emissive" },
  { value: "height", label: "Height" },
  { value: "channels", label: "Channels" },
];

export const ROLE_OPTIONS: SelectOption[] = [
  { value: "", label: "—" },
  { value: "ao", label: "AO" },
  { value: "roughness", label: "Roughness" },
  { value: "smoothness", label: "Smoothness" },
  { value: "metallic", label: "Metallic" },
  { value: "opacity", label: "Opacity" },
];

/** The Select value for a use. */
export function useKey(use: FileUse): string {
  if (use.use === "normal") return use.directX ? "normalDx" : "normal";
  return use.use;
}

/** The use a Select value names. Choosing `channels` starts with all four empty. */
export function useFromKey(key: string): FileUse {
  switch (key) {
    case "albedo":
    case "emissive":
    case "height":
      return { use: key };
    case "normal":
      return { use: "normal", directX: false };
    case "normalDx":
      return { use: "normal", directX: true };
    case "channels":
      return { use: "channels", channels: {} };
    default:
      return { use: "unassigned" };
  }
}

/** The role a role Select value names; the empty value is no role. */
export function roleFromKey(key: string): ChannelRole | undefined {
  return key === "" ? undefined : (key as ChannelRole);
}
