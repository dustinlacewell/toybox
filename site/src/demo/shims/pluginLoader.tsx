import type { ComponentType } from "react";
import type { SlotComponentProps } from "@ldlework/toybox-sdk";
const icons=import.meta.glob("../../../../plugins/*/icons/*.svg",{eager:true,query:"?url",import:"default"}) as Record<string,string>;
export function pluginUrl(id:string,path:string){return icons[`../../../../plugins/${id}/${path}`]??"";}
export async function loadPlugin(){throw Error("External plugins can be installed in Toybox desktop.");}
export async function loadSlotComponent():Promise<ComponentType<SlotComponentProps<unknown>>> {
  return function DesktopImportPanel(){return <section className="demo-desktop-panel"><h2>Bring your own assets into Toybox</h2><p>Use the desktop app to import files from your computer.</p><a href="https://github.com/dustinlacewell/toybox/releases/latest" target="_blank" rel="noreferrer">Download Toybox</a></section>;};
}
