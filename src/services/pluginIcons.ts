import type { AssetKind } from "../domain/catalog";
import type { LoadedImporter } from "./pluginRegistry";
import { pluginKinds } from "./pluginKinds";
import { pluginUrl } from "./pluginLoader";

/** Artwork stays inside its plugin, served through the existing jailed protocol. */
const safe=(path:unknown):path is string=>typeof path==="string"&&!!path&&!path.includes(":")&&!path.includes("\\")&&!path.startsWith("/")&&!path.split("/").some(part=>part===".."||part===".");

/** Navigation artwork per type. A plugin that declares a type supplies that
 *  type's icon; any type left without one takes a stable importer choice. */
export function pluginTypeIcons(importers:LoadedImporter[],types:{id:string;pluginId?:string;icon?:string}[]=[]):Partial<Record<AssetKind,string>> {
  const icons:Partial<Record<AssetKind,string>>={};
  for(const type of types)if(type.pluginId&&safe(type.icon))icons[type.id]=pluginUrl(type.pluginId,type.icon);
  for(const {manifest} of [...importers].sort((a,b)=>a.manifest.id.localeCompare(b.manifest.id))) {
    for(const kind of pluginKinds(manifest)) {
      const path=manifest.assetIcons?.[kind]??manifest.icon;
      if(!icons[kind]&&safe(path))icons[kind]=pluginUrl(manifest.id,path);
    }
  }
  return icons;
}
