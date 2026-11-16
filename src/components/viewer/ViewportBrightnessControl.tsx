import { IconButton } from "@ldlework/toybox-sdk/ui";
import { Sun, SunDim, Moon } from "lucide-react";
import type { PreviewBrightness } from "../../domain/previewBrightness";
import "./ViewerToolbar.css";

/** Controlled lighting levels, shared by every full 3D viewport. */
export function ViewportBrightnessControl({ value, onChange }: { value: PreviewBrightness; onChange: (value: PreviewBrightness) => void }) {
  return <div className="vtoolbar vtoolbar--brightness" role="group" aria-label="Preview brightness">
    {([{value:"normal",label:"Current brightness",icon:Sun},{value:"dim",label:"Dim lighting",icon:SunDim},{value:"veryDim",label:"Very dim lighting",icon:Moon}] as const).map(option =>
      <IconButton key={option.value} icon={option.icon} label={option.label} active={value===option.value} aria-pressed={value===option.value} onClick={()=>onChange(option.value)}/>)}
  </div>;
}
